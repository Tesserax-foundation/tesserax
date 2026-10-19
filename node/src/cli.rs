#[derive(Debug, clap::Parser)]
pub struct Cli {
	#[command(subcommand)]
	pub subcommand: Option<Subcommand>,

	#[clap(flatten)]
	pub run: sc_cli::RunCmd,

	#[clap(flatten)]
	pub mining: MiningParams,
}

/// Built-in miner options.
#[derive(Debug, Clone, clap::Args)]
pub struct MiningParams {
	/// Mine blocks with the built-in CPU miner.
	#[arg(long, requires = "author")]
	pub mine: bool,

	/// Account credited with the rewards of mined blocks (SS58 address or 0x-prefixed hex).
	#[arg(long, value_name = "ACCOUNT")]
	pub author: Option<String>,

	/// Number of mining threads. Defaults to the number of CPU cores.
	#[arg(long, value_name = "COUNT")]
	pub mining_threads: Option<usize>,

	/// Mine with the 256 MiB light mode cache instead of the 2 GiB dataset. About ten times
	/// slower, for machines with little memory.
	#[arg(long)]
	pub mining_light: bool,
}

#[derive(Debug, clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Subcommand {
	/// Key management cli utilities
	#[command(subcommand)]
	Key(sc_cli::KeySubcommand),

	/// Export the chain specification.
	ExportChainSpec(sc_cli::ExportChainSpecCmd),

	/// Validate blocks.
	CheckBlock(sc_cli::CheckBlockCmd),

	/// Export blocks.
	ExportBlocks(sc_cli::ExportBlocksCmd),

	/// Export the state of a given block into a chain spec.
	ExportState(sc_cli::ExportStateCmd),

	/// Import blocks.
	ImportBlocks(sc_cli::ImportBlocksCmd),

	/// Remove the whole chain.
	PurgeChain(sc_cli::PurgeChainCmd),

	/// Revert the chain to a previous state.
	Revert(sc_cli::RevertCmd),

	/// Sub-commands concerned with benchmarking.
	#[command(subcommand)]
	Benchmark(frame_benchmarking_cli::BenchmarkCmd),

	/// Db meta columns information.
	ChainInfo(sc_cli::ChainInfoCmd),
}
