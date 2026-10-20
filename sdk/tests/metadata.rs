//! The metadata the SDK is generated from must be the metadata of the runtime. After a runtime
//! change, update it with:
//!
//! ```text
//! TESSERAX_UPDATE_METADATA=1 cargo test -p tesserax-sdk --test metadata
//! ```

#[test]
fn metadata_matches_the_runtime() {
	let runtime = tesserax_runtime::Runtime::metadata_at_version(16).expect("metadata V16 is supported");
	let sdk = include_bytes!("../metadata/tesserax.scale");
	if runtime.as_slice() != sdk.as_slice() {
		let path = concat!(env!("CARGO_MANIFEST_DIR"), "/metadata/tesserax.scale");
		if std::env::var("TESSERAX_UPDATE_METADATA").is_ok() {
			std::fs::write(path, runtime.as_slice()).unwrap();
			return;
		}
		panic!("{path} is out of date: run this test with TESSERAX_UPDATE_METADATA=1 to update it");
	}
}
