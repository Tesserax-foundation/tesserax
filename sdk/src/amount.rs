//! Amounts of TESS, with 12 decimals.

use crate::{Error, Result};
use core::fmt;

/// Number of decimals of TESS.
pub const DECIMALS: u32 = 12;

/// Planck in one TESS.
pub const PLANCK_PER_TESS: u128 = 10u128.pow(DECIMALS);

/// An amount of TESS, in planck. Serialized as a decimal string of TESS, such as `"1.5"`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Amount(pub u128);

impl serde::Serialize for Amount {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.to_string())
	}
}

impl<'de> serde::Deserialize<'de> for Amount {
	fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
		let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
		Amount::from_tess(&text).map_err(serde::de::Error::custom)
	}
}

impl Amount {
	/// Parse a decimal amount of TESS such as `"1.5"` or `"0.000001"`.
	pub fn from_tess(text: &str) -> Result<Self> {
		let invalid = || Error::InvalidAmount(text.to_string());
		let text = text.trim();
		let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
		if whole.is_empty() && fraction.is_empty() ||
			!whole.chars().all(|c| c.is_ascii_digit()) ||
			!fraction.chars().all(|c| c.is_ascii_digit()) ||
			fraction.len() > DECIMALS as usize
		{
			return Err(invalid());
		}
		let whole: u128 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| invalid())? };
		let mut fraction_planck: u128 = 0;
		if !fraction.is_empty() {
			fraction_planck = fraction.parse::<u128>().map_err(|_| invalid())? *
				10u128.pow(DECIMALS - fraction.len() as u32);
		}
		whole
			.checked_mul(PLANCK_PER_TESS)
			.and_then(|w| w.checked_add(fraction_planck))
			.map(Amount)
			.ok_or_else(invalid)
	}

	/// The amount in planck.
	pub fn planck(self) -> u128 {
		self.0
	}
}

impl fmt::Display for Amount {
	/// Decimal TESS without trailing zeros, such as `1.5`.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let whole = self.0 / PLANCK_PER_TESS;
		let fraction = self.0 % PLANCK_PER_TESS;
		if fraction == 0 {
			return write!(f, "{whole}");
		}
		let fraction = format!("{fraction:012}");
		write!(f, "{whole}.{}", fraction.trim_end_matches('0'))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_and_formats() {
		assert_eq!(Amount::from_tess("1").unwrap().0, PLANCK_PER_TESS);
		assert_eq!(Amount::from_tess("1.5").unwrap().0, 1_500_000_000_000);
		assert_eq!(Amount::from_tess("0.000000000001").unwrap().0, 1);
		assert_eq!(Amount::from_tess(".25").unwrap().0, 250_000_000_000);
		assert_eq!(Amount(1_500_000_000_000).to_string(), "1.5");
		assert_eq!(Amount(1).to_string(), "0.000000000001");
		assert_eq!(Amount(14_000_000 * PLANCK_PER_TESS).to_string(), "14000000");
		assert_eq!(serde_json::to_string(&Amount(1_500_000_000_000)).unwrap(), "\"1.5\"");
		assert_eq!(serde_json::from_str::<Amount>("\"0.25\"").unwrap(), Amount(250_000_000_000));
		for bad in ["", ".", "1.0000000000001", "-1", "1e3", "1,5", "abc"] {
			assert!(Amount::from_tess(bad).is_err(), "{bad}");
		}
	}
}
