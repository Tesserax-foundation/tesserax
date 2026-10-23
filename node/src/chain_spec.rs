use sc_service::{ChainType, Properties};
use tesserax_runtime::{DECIMALS, SS58_PREFIX, WASM_BINARY};

/// Specialized `ChainSpec`. This is a specialization of the general Substrate ChainSpec type.
pub type ChainSpec = sc_service::GenericChainSpec;

fn properties() -> Properties {
	let mut properties = Properties::new();
	properties.insert("tokenSymbol".into(), "TESS".into());
	properties.insert("tokenDecimals".into(), DECIMALS.into());
	properties.insert("ss58Format".into(), SS58_PREFIX.into());
	properties
}

pub fn development_chain_spec() -> Result<ChainSpec, String> {
	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
		None,
	)
	.with_name("Tesserax Development")
	.with_id("tesserax_dev")
	.with_chain_type(ChainType::Development)
	.with_protocol_id("tess")
	.with_properties(properties())
	.with_genesis_config_preset_name(sp_genesis_builder::DEV_RUNTIME_PRESET)
	.build())
}

pub fn local_chain_spec() -> Result<ChainSpec, String> {
	Ok(ChainSpec::builder(
		WASM_BINARY.ok_or_else(|| "Development wasm not available".to_string())?,
		None,
	)
	.with_name("Tesserax Local Testnet")
	.with_id("tesserax_local")
	.with_chain_type(ChainType::Local)
	.with_protocol_id("tess")
	.with_properties(properties())
	.with_genesis_config_preset_name(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET)
	.build())
}

/// The public testnet, built from its runtime preset. Its specification is generated once and
/// frozen in `chain-specs/testnet.json`, genesis included: nodes load that file, whatever the
/// runtime they are built with.
pub fn testnet_genesis_chain_spec() -> Result<ChainSpec, String> {
	Ok(ChainSpec::builder(WASM_BINARY.ok_or_else(|| "Testnet wasm not available".to_string())?, None)
		.with_name("Tesserax Testnet")
		.with_id("tesserax_testnet")
		.with_chain_type(ChainType::Live)
		.with_protocol_id("tess-testnet")
		.with_properties(properties())
		.with_genesis_config_preset_name(tesserax_runtime::genesis_config_presets::TESTNET_RUNTIME_PRESET)
		.build())
}

/// The public testnet, from its frozen specification.
pub fn testnet_chain_spec() -> Result<ChainSpec, String> {
	ChainSpec::from_json_bytes(&include_bytes!("../../chain-specs/testnet.json")[..])
}

