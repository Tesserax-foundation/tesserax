//! JSON state files, written atomically: a crash leaves either the previous or the new content,
//! never a partial file.

use crate::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::{
	fs::{self, File, OpenOptions},
	io::Write,
	path::Path,
};

/// Read the JSON file at `path`, if it exists.
pub(crate) fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
	match fs::read(path) {
		Ok(bytes) => serde_json::from_slice(&bytes)
			.map(Some)
			.map_err(|e| Error::Storage(format!("{} is not a valid state file: {e}", path.display()))),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(Error::Storage(format!("cannot read {}: {e}", path.display()))),
	}
}

/// Replace the file at `path` with `value` as JSON, readable by the user only.
///
/// The content is written to a temporary file in the same directory, flushed to disk, then
/// renamed over `path`.
pub(crate) fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
	let fail = |e: std::io::Error| Error::Storage(format!("cannot write {}: {e}", path.display()));
	let json = serde_json::to_vec_pretty(value).expect("state files serialize; qed");
	let name = path.file_name().ok_or_else(|| Error::Storage(format!("{} is not a file path", path.display())))?;
	let temporary = path.with_file_name(format!(".{}.tmp", name.to_string_lossy()));

	let mut options = OpenOptions::new();
	options.write(true).create(true).truncate(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
	let mut file = options.open(&temporary).map_err(fail)?;
	file.write_all(&json).map_err(fail)?;
	file.sync_all().map_err(fail)?;
	drop(file);
	fs::rename(&temporary, path).map_err(fail)?;

	// Make the rename itself durable.
	#[cfg(unix)]
	if let Some(directory) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
		if let Ok(directory) = File::open(directory) {
			let _ = directory.sync_all();
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::BTreeMap;

	fn temporary_dir() -> std::path::PathBuf {
		let mut suffix = [0u8; 8];
		getrandom::fill(&mut suffix).unwrap();
		let dir = std::env::temp_dir().join(format!("tesserax-store-{}", u64::from_le_bytes(suffix)));
		fs::create_dir_all(&dir).unwrap();
		dir
	}

	#[test]
	fn roundtrip_and_missing_file() {
		let dir = temporary_dir();
		let path = dir.join("state.json");
		assert!(load::<BTreeMap<String, u32>>(&path).unwrap().is_none());

		let value = BTreeMap::from([("a".to_string(), 1u32)]);
		save(&path, &value).unwrap();
		assert_eq!(load::<BTreeMap<String, u32>>(&path).unwrap(), Some(value));
		// No temporary file is left behind.
		assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
		}

		fs::write(&path, b"{not json").unwrap();
		assert!(matches!(load::<BTreeMap<String, u32>>(&path), Err(Error::Storage(_))));
		fs::remove_dir_all(dir).unwrap();
	}
}
