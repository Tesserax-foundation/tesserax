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
