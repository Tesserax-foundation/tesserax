//! The SDKs (Rust, Python, TypeScript) derive keys exactly as the node: `vectors/keys.json` holds
//! keys derived by `tesserax key inspect`.

use tesserax_sdk::Keypair;

#[test]
fn keys_match_the_node() {
	let vectors: serde_json::Value =
		serde_json::from_str(include_str!("../vectors/keys.json")).expect("valid JSON");
	for vector in vectors["vectors"].as_array().expect("a list of vectors") {
		let uri = vector["uri"].as_str().unwrap();
		let key = Keypair::from_uri(uri).unwrap();
		assert_eq!(key.address(), vector["address"].as_str().unwrap(), "{uri}");
		let account: String = key.account_id().0.iter().map(|b| format!("{b:02x}")).collect();
		assert_eq!(format!("0x{account}"), vector["account_id"].as_str().unwrap(), "{uri}");
	}
}
