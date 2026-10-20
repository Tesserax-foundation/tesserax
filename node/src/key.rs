//! Account key utilities.
//!
//! Tesserax accounts are ML-DSA-65 keys. New keys come from a 24 word BIP39 phrase, i.e. 256 bits
//! of entropy, so that a quantum search over the phrase space stays out of reach.

use sc_cli::{GenerateNodeKeyCmd, InspectNodeKeyCmd, SubstrateCli};
use sp_core::crypto::{Pair as _, Ss58Codec};
use tesserax_crypto::Pair;

/// Key management utilities.
#[derive(Debug, clap::Subcommand)]
pub enum KeySubcommand {
	/// Generate a new ML-DSA account key.
	Generate(GenerateCmd),

	/// Show the account of a secret phrase, seed or URI.
	Inspect(InspectCmd),

	/// Generate a random node key (ML-DSA-65 network identity) and print its peer id.
	GenerateNodeKey(GenerateNodeKeyCmd),

	/// Print the peer id of a node key.
	InspectNodeKey(InspectNodeKeyCmd),
}

impl KeySubcommand {
	pub fn run<C: SubstrateCli>(&self, cli: &C) -> sc_cli::Result<()> {
		match self {
			Self::Generate(cmd) => cmd.run(),
			Self::Inspect(cmd) => cmd.run(),
			Self::GenerateNodeKey(cmd) => {
				let chain_spec = cli.load_spec(cmd.chain.as_deref().unwrap_or(""))?;
				cmd.run(chain_spec.id(), &C::executable_name())
			},
			Self::InspectNodeKey(cmd) => cmd.run(),
		}
	}
}

#[derive(Debug, Clone, clap::Args)]
pub struct GenerateCmd {
	/// Print the result as JSON, including the full public key.
	#[arg(long)]
	pub json: bool,
}

impl GenerateCmd {
	pub fn run(&self) -> sc_cli::Result<()> {
		let mut entropy = [0u8; 32];
		getrandom::fill(&mut entropy).map_err(|e| format!("No randomness available: {e}"))?;
		let phrase = bip39::Mnemonic::from_entropy(&entropy)
			.map_err(|e| format!("Phrase generation failed: {e}"))?
			.to_string();
		let (pair, seed) = Pair::from_phrase(&phrase, None)
			.map_err(|e| format!("Key generation failed: {e:?}"))?;
		print_key(&pair, Some(&phrase), &seed, self.json);
		Ok(())
	}
}

#[derive(Debug, Clone, clap::Args)]
pub struct InspectCmd {
	/// A secret phrase, a 0x-prefixed hex seed or a secret URI such as `//Alice`.
	pub uri: String,

	/// Print the result as JSON, including the full public key.
	#[arg(long)]
	pub json: bool,
}

impl InspectCmd {
	pub fn run(&self) -> sc_cli::Result<()> {
		let (pair, seed) = Pair::from_string_with_seed(&self.uri, None)
			.map_err(|e| format!("Invalid secret URI: {e:?}"))?;
		let seed = seed.unwrap_or_else(|| pair.seed());
		print_key(&pair, None, &seed, self.json);
		Ok(())
	}
}

fn print_key(pair: &Pair, phrase: Option<&str>, seed: &[u8; 32], json: bool) {
	let account = pair.account_id();
	let address = account.to_ss58check();
	if json {
		let mut value = serde_json::json!({
			"secretSeed": format!("0x{}", hex::encode(seed)),
			"accountId": format!("0x{}", hex::encode(account.as_ref() as &[u8])),
			"address": address,
			"publicKey": format!("0x{}", hex::encode(pair.public().0)),
			"scheme": "ML-DSA-65",
		});
		if let Some(phrase) = phrase {
			value["secretPhrase"] = phrase.into();
		}
		println!("{}", serde_json::to_string_pretty(&value).expect("valid JSON; qed"));
		return;
	}
	if let Some(phrase) = phrase {
		println!("Secret phrase:  {phrase}");
	}
	println!("Secret seed:    0x{}", hex::encode(seed));
	println!("Scheme:         ML-DSA-65");
	println!("Account ID:     0x{}", hex::encode(account.as_ref() as &[u8]));
	println!("Address:        {address}");
}

