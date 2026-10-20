frame_benchmarking::define_benchmarks!(
	[frame_benchmarking, BaselineBench::<Runtime>]
	[frame_system, SystemBench::<Runtime>]
	[frame_system_extensions, SystemExtensionsBench::<Runtime>]
	[pallet_balances, Balances]
	[pallet_timestamp, Timestamp]
	[pallet_sudo, Sudo]
	[pallet_shielded, Shielded]
	[pallet_emission, Emission]
	[pallet_agents, Agents]
	[pallet_agent_registry, AgentRegistry]
);
