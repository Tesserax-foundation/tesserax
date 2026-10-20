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

/// `vectors/voucher.json`: a voucher of 1.5 TESS on channel 7 for the genesis hash `[1; 32]`,
/// signed by `//Alice`. Regenerate with `cargo test -p tesserax-sdk --test vectors -- --ignored`.
#[test]
#[ignore]
fn voucher_vector() {
	let key = Keypair::from_uri("//Alice").unwrap();
	let genesis = [1u8; 32];
	let amount = tesserax_sdk::Amount::from_tess("1.5").unwrap();
	let voucher = tesserax_sdk::Voucher::sign(&key, &genesis, 7, amount);
	let hex = |b: &[u8]| format!("0x{}", b.iter().map(|b| format!("{b:02x}")).collect::<String>());
	let vector = serde_json::json!({
		"description": "A voucher signed by the Rust SDK: every SDK must build the same message and accept the signature.",
		"signer": "//Alice",
		"genesis": hex(&genesis),
		"channel": 7,
		"amount": "1.5",
		"message": hex(&tesserax_sdk::channels::voucher_message(&genesis, 7, amount.0)),
		"header": voucher.to_header(),
	});
	let path = concat!(env!("CARGO_MANIFEST_DIR"), "/vectors/voucher.json");
	std::fs::write(path, serde_json::to_string_pretty(&vector).unwrap()).unwrap();
}

#[test]
fn voucher_vector_verifies() {
	let vector: serde_json::Value =
		serde_json::from_str(include_str!("../vectors/voucher.json")).expect("valid JSON");
	let key = Keypair::from_uri(vector["signer"].as_str().unwrap()).unwrap();
	let voucher = tesserax_sdk::Voucher::from_header(vector["header"].as_str().unwrap()).unwrap();
	assert!(voucher.verify(&[1u8; 32], &key.public_key()));
	assert!(!voucher.verify(&[2u8; 32], &key.public_key()));
}
