// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Configuration of the networking layer.
//!
//! The [`Params`] struct is the struct that must be passed in order to initialize the networking.
//! See the documentation of [`Params`].

pub use crate::{
	peer_store::PeerStoreProvider,
	request_responses::{
		IncomingRequest, OutgoingResponse, ProtocolConfig as RequestResponseConfig,
	},
	service::{
		metrics::NotificationMetrics,
		traits::{NotificationConfig, NotificationService, PeerStore},
	},
	types::ProtocolName,
};

pub use litep2p::crypto::ml_dsa;
pub use sc_network_types::build_multiaddr;
use sc_network_types::{
	multiaddr::{self, Multiaddr},
	PeerId,
};

use crate::service::{ensure_addresses_consistent_with_transport, traits::NetworkBackend};
use codec::Encode;
use prometheus_endpoint::Registry;
use zeroize::Zeroize;

pub use sc_network_common::{
	role::{Role, Roles},
	sync::SyncMode,
	ExHashT,
};

use sp_runtime::traits::Block as BlockT;

use std::{
	error::Error,
	fmt, fs,
	future::Future,
	io::{self, Write},
	iter,
	net::Ipv4Addr,
	num::NonZeroUsize,
	path::{Path, PathBuf},
	pin::Pin,
	str::{self, FromStr},
	sync::Arc,
	time::Duration,
};

/// Default value for Kademlia replication factor which determines to how many closest peers a
/// record is replicated to.
pub const DEFAULT_KADEMLIA_REPLICATION_FACTOR: usize = 20;

/// Configuration of a notification protocol.
pub type NonDefaultSetConfig = crate::litep2p::shim::notification::config::NotificationProtocolConfig;

/// Default timeout for idle connections of 10 seconds is good enough for most networks.
/// It doesn't make sense to expose it as a CLI parameter on individual nodes, but customizations
/// are possible in custom nodes through [`NetworkConfiguration`].
pub const DEFAULT_IDLE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum number of locally kept Kademlia provider keys.
///
/// 10000 keys is enough for a testnet with fast runtime (1-minute epoch) and 13 parachains.
pub const KADEMLIA_MAX_PROVIDER_KEYS: usize = 10000;

/// Time to keep Kademlia content provider records.
///
/// 10 h is enough time to keep the parachain bootnode record for two 4-hour epochs.
pub const KADEMLIA_PROVIDER_RECORD_TTL: Duration = Duration::from_secs(10 * 3600);

/// Interval of republishing Kademlia provider records.
///
/// 3.5 h means we refresh next epoch provider record 30 minutes before next 4-hour epoch comes.
pub const KADEMLIA_PROVIDER_REPUBLISH_INTERVAL: Duration = Duration::from_secs(12600);

/// Protocol name prefix, transmitted on the wire for legacy protocol names.
/// I.e., `dot` in `/dot/sync/2`. Should be unique for each chain. Always UTF-8.
/// Deprecated in favour of genesis hash & fork ID based protocol names.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProtocolId(smallvec::SmallVec<[u8; 6]>);

impl<'a> From<&'a str> for ProtocolId {
	fn from(bytes: &'a str) -> ProtocolId {
		Self(bytes.as_bytes().into())
	}
}

impl AsRef<str> for ProtocolId {
	fn as_ref(&self) -> &str {
		str::from_utf8(&self.0[..])
			.expect("the only way to build a ProtocolId is through a UTF-8 String; qed")
	}
}

impl fmt::Debug for ProtocolId {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		fmt::Debug::fmt(self.as_ref(), f)
	}
}

/// Parses a string address and splits it into Multiaddress and PeerId, if
/// valid.
///
/// # Example
///
/// ```
/// # use sc_network_types::{multiaddr::Multiaddr, PeerId};
/// use sc_network::config::parse_str_addr;
/// let (peer_id, addr) = parse_str_addr(
/// 	"/ip4/198.51.100.19/tcp/30333/p2p/QmSk5HQbn6LhUwDiNMseVUjuRYhEtYj4aUZ6WfWoGURpdV"
/// ).unwrap();
/// assert_eq!(peer_id, "QmSk5HQbn6LhUwDiNMseVUjuRYhEtYj4aUZ6WfWoGURpdV".parse::<PeerId>().unwrap().into());
/// assert_eq!(addr, "/ip4/198.51.100.19/tcp/30333".parse::<Multiaddr>().unwrap());
/// ```
pub fn parse_str_addr(addr_str: &str) -> Result<(PeerId, Multiaddr), ParseErr> {
	let addr: Multiaddr = addr_str.parse()?;
	parse_addr(addr)
}

/// Splits a Multiaddress into a Multiaddress and PeerId.
pub fn parse_addr(mut addr: Multiaddr) -> Result<(PeerId, Multiaddr), ParseErr> {
	let multihash = match addr.pop() {
		Some(multiaddr::Protocol::P2p(multihash)) => multihash,
		_ => return Err(ParseErr::PeerIdMissing),
	};
	let peer_id = PeerId::from_multihash(multihash).map_err(|_| ParseErr::InvalidPeerId)?;

	Ok((peer_id, addr))
}

/// Address of a node, including its identity.
///
/// This struct represents a decoded version of a multiaddress that ends with `/p2p/<peerid>`.
///
/// # Example
///
/// ```
/// # use sc_network_types::{multiaddr::Multiaddr, PeerId};
/// use sc_network::config::MultiaddrWithPeerId;
/// let addr: MultiaddrWithPeerId =
/// 	"/ip4/198.51.100.19/tcp/30333/p2p/QmSk5HQbn6LhUwDiNMseVUjuRYhEtYj4aUZ6WfWoGURpdV".parse().unwrap();
/// assert_eq!(addr.peer_id.to_base58(), "QmSk5HQbn6LhUwDiNMseVUjuRYhEtYj4aUZ6WfWoGURpdV");
/// assert_eq!(addr.multiaddr.to_string(), "/ip4/198.51.100.19/tcp/30333");
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(try_from = "String", into = "String")]
pub struct MultiaddrWithPeerId {
	/// Address of the node.
	pub multiaddr: Multiaddr,
	/// Its identity.
	pub peer_id: PeerId,
}

impl MultiaddrWithPeerId {
	/// Concatenates the multiaddress and peer ID into one multiaddress containing both.
	pub fn concat(&self) -> Multiaddr {
		let mut addr = self.multiaddr.clone();
		// Ensure that the address not already contains the `p2p` protocol.
		if matches!(addr.iter().last(), Some(multiaddr::Protocol::P2p(_))) {
			addr.pop();
		}
		addr.with(multiaddr::Protocol::P2p(From::from(self.peer_id)))
	}
}

impl fmt::Display for MultiaddrWithPeerId {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		fmt::Display::fmt(&self.concat(), f)
	}
}

impl FromStr for MultiaddrWithPeerId {
	type Err = ParseErr;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let (peer_id, multiaddr) = parse_str_addr(s)?;
		Ok(Self { peer_id, multiaddr })
	}
}

impl From<MultiaddrWithPeerId> for String {
	fn from(ma: MultiaddrWithPeerId) -> String {
		format!("{}", ma)
	}
}

impl TryFrom<String> for MultiaddrWithPeerId {
	type Error = ParseErr;
	fn try_from(string: String) -> Result<Self, Self::Error> {
		string.parse()
	}
}

/// Error that can be generated by `parse_str_addr`.
#[derive(Debug)]
pub enum ParseErr {
	/// Error while parsing the multiaddress.
	MultiaddrParse(multiaddr::ParseError),
	/// Multihash of the peer ID is invalid.
	InvalidPeerId,
	/// The peer ID is missing from the address.
	PeerIdMissing,
}

impl fmt::Display for ParseErr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::MultiaddrParse(err) => write!(f, "{}", err),
			Self::InvalidPeerId => write!(f, "Peer id at the end of the address is invalid"),
			Self::PeerIdMissing => write!(f, "Peer id is missing from the address"),
		}
	}
}

impl std::error::Error for ParseErr {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Self::MultiaddrParse(err) => Some(err),
			Self::InvalidPeerId => None,
			Self::PeerIdMissing => None,
		}
	}
}

impl From<multiaddr::ParseError> for ParseErr {
	fn from(err: multiaddr::ParseError) -> ParseErr {
		Self::MultiaddrParse(err)
	}
}

/// Custom handshake for the notification protocol
#[derive(Debug, Clone)]
pub struct NotificationHandshake(Vec<u8>);

impl NotificationHandshake {
	/// Create new `NotificationHandshake` from an object that implements `Encode`
	pub fn new<H: Encode>(handshake: H) -> Self {
		Self(handshake.encode())
	}

	/// Create new `NotificationHandshake` from raw bytes
	pub fn from_bytes(bytes: Vec<u8>) -> Self {
		Self(bytes)
	}
}

impl std::ops::Deref for NotificationHandshake {
	type Target = Vec<u8>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

/// Configuration for the transport layer.
#[derive(Clone, Debug)]
pub enum TransportConfig {
	/// Normal transport mode.
	Normal {
		/// If true, the network will use mDNS to discover other libp2p nodes on the local network
		/// and connect to them if they support the same chain.
		enable_mdns: bool,

		/// If true, allow connecting to private IPv4/IPv6 addresses (as defined in
		/// [RFC1918](https://tools.ietf.org/html/rfc1918)). Irrelevant for addresses that have
		/// been passed in `::sc_network::config::NetworkConfiguration::boot_nodes`.
		allow_private_ip: bool,
	},

	/// Only allow connections within the same process.
	/// Only addresses of the form `/memory/...` will be supported.
	MemoryOnly,
}

/// The policy for connections to non-reserved peers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NonReservedPeerMode {
	/// Accept them. This is the default.
	Accept,
	/// Deny them.
	Deny,
}

impl NonReservedPeerMode {
	/// Attempt to parse the peer mode from a string.
	pub fn parse(s: &str) -> Option<Self> {
		match s {
			"accept" => Some(Self::Accept),
			"deny" => Some(Self::Deny),
			_ => None,
		}
	}

	/// If we are in "reserved-only" peer mode.
	pub fn is_reserved_only(&self) -> bool {
		matches!(self, NonReservedPeerMode::Deny)
	}
}

/// The configuration of a node's secret key, describing the type of key
/// and how it is obtained. A node's identity keypair is the result of
/// the evaluation of the node key configuration.
#[derive(Clone, Debug)]
pub enum NodeKeyConfig {
	/// The seed of the node's ML-DSA-65 identity key.
	MlDsa(Secret<ml_dsa::SecretKey>),
}

impl Default for NodeKeyConfig {
	fn default() -> NodeKeyConfig {
		Self::MlDsa(Secret::New)
	}
}

/// The options for obtaining an ML-DSA-65 secret key.
pub type MlDsaSecret = Secret<ml_dsa::SecretKey>;

/// The configuration options for obtaining a secret key `K`.
#[derive(Clone)]
pub enum Secret<K> {
	/// Use the given secret key `K`.
	Input(K),
	/// Read the secret key from a file. If the file does not exist,
	/// it is created with a newly generated secret key `K`. The format
	/// of the file is determined by `K`:
	///
	///   * `ml_dsa::SecretKey`: the 32 byte ML-DSA-65 seed, raw or hex encoded.
	File(PathBuf),
	/// Always generate a new secret key `K`.
	New,
}

impl<K> fmt::Debug for Secret<K> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::Input(_) => f.debug_tuple("Secret::Input").finish(),
			Self::File(path) => f.debug_tuple("Secret::File").field(path).finish(),
			Self::New => f.debug_tuple("Secret::New").finish(),
		}
	}
}

impl NodeKeyConfig {
	/// Evaluate a `NodeKeyConfig` to obtain an identity `Keypair`:
	///
	///  * If the secret is configured as input, the corresponding keypair is returned.
	///
	///  * If the secret is configured as a file, it is read from that file, if it exists. Otherwise
	///    a new secret is generated and stored. In either case, the keypair obtained from the
	///    secret is returned.
	///
	///  * If the secret is configured to be new, it is generated and the corresponding keypair is
	///    returned.
	pub fn into_keypair(self) -> io::Result<ml_dsa::Keypair> {
		use NodeKeyConfig::*;
		match self {
			MlDsa(Secret::New) => Ok(ml_dsa::Keypair::generate()),

			MlDsa(Secret::Input(k)) => Ok(ml_dsa::Keypair::from(k)),

			MlDsa(Secret::File(f)) => get_secret(
				f,
				|mut b| {
					let parsed = match String::from_utf8(b.to_vec()).ok().and_then(|s| {
						let s = s.trim();
						if s.len() == 64 {
							array_bytes::hex2bytes(s).ok()
						} else {
							None
						}
					}) {
						Some(s) => ml_dsa::SecretKey::try_from_bytes(s),
						_ => ml_dsa::SecretKey::try_from_bytes(&mut b),
					};
					parsed.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
				},
				ml_dsa::SecretKey::generate,
				|b| b.as_ref().to_vec(),
			)
			.map(ml_dsa::Keypair::from),
		}
	}
}

/// Load a secret key from a file, if it exists, or generate a
/// new secret key and write it to that file. In either case,
/// the secret key is returned.
fn get_secret<P, F, G, E, W, K>(file: P, parse: F, generate: G, serialize: W) -> io::Result<K>
where
	P: AsRef<Path>,
	F: for<'r> FnOnce(&'r mut [u8]) -> Result<K, E>,
	G: FnOnce() -> K,
	E: Error + Send + Sync + 'static,
	W: Fn(&K) -> Vec<u8>,
{
	std::fs::read(&file)
		.and_then(|mut sk_bytes| {
			parse(&mut sk_bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
		})
		.or_else(|e| {
			if e.kind() == io::ErrorKind::NotFound {
				file.as_ref().parent().map_or(Ok(()), fs::create_dir_all)?;
				let sk = generate();
				let mut sk_vec = serialize(&sk);
				write_secret_file(file, &sk_vec)?;
				sk_vec.zeroize();
				Ok(sk)
			} else {
				Err(e)
			}
		})
}

/// Write secret bytes to a file.
pub(super) fn write_secret_file<P>(path: P, sk_bytes: &[u8]) -> io::Result<()>
where
	P: AsRef<Path>,
{
	let mut file = open_secret_file(&path)?;
	file.write_all(sk_bytes)
}

/// Opens a file containing a secret key in write mode.
#[cfg(unix)]
fn open_secret_file<P>(path: P) -> io::Result<fs::File>
where
	P: AsRef<Path>,
{
	use std::os::unix::fs::OpenOptionsExt;
	fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

/// Opens a file containing a secret key in write mode.
#[cfg(not(unix))]
fn open_secret_file<P>(path: P) -> Result<fs::File, io::Error>
where
	P: AsRef<Path>,
{
	fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// Configuration for a set of nodes.
#[derive(Clone, Debug)]
pub struct SetConfig {
	/// Maximum allowed number of incoming substreams related to this set.
	pub in_peers: u32,

	/// Number of outgoing substreams related to this set that we're trying to maintain.
	pub out_peers: u32,

	/// List of reserved node addresses.
	pub reserved_nodes: Vec<MultiaddrWithPeerId>,

	/// Whether nodes that aren't in [`SetConfig::reserved_nodes`] are accepted or automatically
	/// refused.
	pub non_reserved_mode: NonReservedPeerMode,
}

impl Default for SetConfig {
	fn default() -> Self {
		Self {
			in_peers: 25,
			out_peers: 75,
			reserved_nodes: Vec::new(),
			non_reserved_mode: NonReservedPeerMode::Accept,
		}
	}
}

/// Network service configuration.
#[derive(Clone, Debug)]
pub struct NetworkConfiguration {
	/// Directory path to store network-specific configuration. None means nothing will be saved.
	pub net_config_path: Option<PathBuf>,

	/// Multiaddresses to listen for incoming connections.
	pub listen_addresses: Vec<Multiaddr>,

	/// Multiaddresses to advertise. Detected automatically if empty.
	pub public_addresses: Vec<Multiaddr>,

	/// List of initial node addresses
	pub boot_nodes: Vec<MultiaddrWithPeerId>,

	/// The node key configuration, which determines the node's network identity keypair.
	pub node_key: NodeKeyConfig,

	/// Configuration for the default set of nodes used for block syncing and transactions.
	pub default_peers_set: SetConfig,

	/// Number of substreams to reserve for full nodes for block syncing and transactions.
	/// Any other slot will be dedicated to light nodes.
	///
	/// This value is implicitly capped to `default_set.out_peers + default_set.in_peers`.
	pub default_peers_set_num_full: u32,

	/// Client identifier. Sent over the wire for debugging purposes.
	pub client_version: String,

	/// Name of the node. Sent over the wire for debugging purposes.
	pub node_name: String,

	/// Configuration for the transport layer.
	pub transport: TransportConfig,

	/// Idle connection timeout.
	///
	/// Set by default to [`DEFAULT_IDLE_CONNECTION_TIMEOUT`].
	pub idle_connection_timeout: Duration,

	/// Maximum number of peers to ask the same blocks in parallel.
	pub max_parallel_downloads: u32,

	/// Maximum number of blocks per request.
	pub max_blocks_per_request: u32,

	/// Number of peers that need to be connected before warp sync is started.
	pub min_peers_to_start_warp_sync: Option<usize>,

	/// Initial syncing mode.
	pub sync_mode: SyncMode,

	/// True if Kademlia random discovery should be enabled.
	///
	/// If true, the node will automatically randomly walk the DHT in order to find new peers.
	pub enable_dht_random_walk: bool,

	/// Should we insert non-global addresses into the DHT?
	pub allow_non_globals_in_dht: bool,

	/// Require iterative Kademlia DHT queries to use disjoint paths for increased resiliency in
	/// the presence of potentially adversarial nodes.
	pub kademlia_disjoint_query_paths: bool,

	/// Kademlia replication factor determines to how many closest peers a record is replicated to.
	///
	/// Discovery mechanism requires successful replication to all
	/// `kademlia_replication_factor` peers to consider record successfully put.
	pub kademlia_replication_factor: NonZeroUsize,

	/// Enable serving indexed transaction data using IPFS Bitswap protocol.
	pub ipfs_server: bool,

	/// List of IPFS bootstrap nodes to register in IPFS DHT as a provider of indexed transaction
	/// data.
	///
	/// If IPFS bootstrap nodes are not provided, this node will only handle direct Bitswap
	/// requests from peers that already know its address.
	pub ipfs_bootnodes: Vec<MultiaddrWithPeerId>,

	/// Networking backend used for P2P communication.
	pub network_backend: NetworkBackendType,
}

impl NetworkConfiguration {
	/// Create new default configuration
	pub fn new<SN: Into<String>, SV: Into<String>>(
		node_name: SN,
		client_version: SV,
		node_key: NodeKeyConfig,
		net_config_path: Option<PathBuf>,
	) -> Self {
		let default_peers_set = SetConfig::default();
		Self {
			net_config_path,
			listen_addresses: Vec::new(),
			public_addresses: Vec::new(),
			boot_nodes: Vec::new(),
			node_key,
			default_peers_set_num_full: default_peers_set.in_peers + default_peers_set.out_peers,
			default_peers_set,
			client_version: client_version.into(),
			node_name: node_name.into(),
			transport: TransportConfig::Normal { enable_mdns: false, allow_private_ip: true },
			idle_connection_timeout: DEFAULT_IDLE_CONNECTION_TIMEOUT,
			max_parallel_downloads: 5,
			max_blocks_per_request: 64,
			min_peers_to_start_warp_sync: None,
			sync_mode: SyncMode::Full,
			enable_dht_random_walk: true,
			allow_non_globals_in_dht: false,
			kademlia_disjoint_query_paths: false,
			kademlia_replication_factor: NonZeroUsize::new(DEFAULT_KADEMLIA_REPLICATION_FACTOR)
				.expect("value is a constant; constant is non-zero; qed."),
			ipfs_server: false,
			ipfs_bootnodes: Vec::new(),
			network_backend: NetworkBackendType::Litep2p,
		}
	}

	/// Create new default configuration for localhost-only connection with random port (useful for
	/// testing)
	pub fn new_local() -> NetworkConfiguration {
		let mut config =
			NetworkConfiguration::new("test-node", "test-client", Default::default(), None);

		config.listen_addresses =
			vec![iter::once(multiaddr::Protocol::Ip4(Ipv4Addr::new(127, 0, 0, 1)))
				.chain(iter::once(multiaddr::Protocol::Tcp(0)))
				.collect()];

		config.allow_non_globals_in_dht = true;
		config
	}

	/// Create new default configuration for localhost-only connection with random port (useful for
	/// testing)
	pub fn new_memory() -> NetworkConfiguration {
		let mut config =
			NetworkConfiguration::new("test-node", "test-client", Default::default(), None);

		config.listen_addresses =
			vec![iter::once(multiaddr::Protocol::Ip4(Ipv4Addr::new(127, 0, 0, 1)))
				.chain(iter::once(multiaddr::Protocol::Tcp(0)))
				.collect()];

		config.allow_non_globals_in_dht = true;
		config
	}

	/// Drop this node's `webrtc-direct` listen addresses and reject public ones.
	///
	/// WebRTC is not supported: its DTLS handshake relies on elliptic curve cryptography, while
	/// every connection of this network uses post-quantum TLS. Listen addresses added by default
	/// are removed silently; an explicitly configured public WebRTC address is an error.
	pub fn validate_and_complete_webrtc_addresses(&mut self) -> Result<(), crate::error::Error> {
		self.listen_addresses.retain(|address| !is_webrtc_address(address));
		if self.public_addresses.iter().any(is_webrtc_address) {
			return Err(crate::error::Error::WebRtcNotSupportedByBackend);
		}
		Ok(())
	}

	/// Remove every `webrtc-direct` address of this node.
	///
	/// The relay chain side of a collator uses this to drop the WebRTC listeners appended by
	/// default for a full node. The public WebRTC addresses go with the listeners serving them.
	/// Dropping one is warned about, as it can only have been configured explicitly.
	pub fn remove_webrtc_addresses(&mut self) {
		self.listen_addresses.retain(|address| !is_webrtc_address(address));
		self.public_addresses.retain(|address| {
			let keep = !is_webrtc_address(address);
			if !keep {
				log::warn!(
					target: crate::LOG_TARGET,
					"removing public WebRTC address {address}: no WebRTC listener on this node",
				);
			}
			keep
		});
	}
}

/// IPFS server configuration.
pub struct IpfsConfig<Block: BlockT, H: ExHashT, N: NetworkBackend<Block, H>> {
	/// Network-backend-specific Bitswap configuration.
	pub bitswap_config: N::BitswapConfig,
	/// Indexed transactions provider.
	pub block_provider: Box<dyn crate::IpfsBlockProvider>,
	/// IPFS bootstrap nodes.
	pub bootnodes: Vec<MultiaddrWithPeerId>,
}

/// Network initialization parameters.
pub struct Params<Block: BlockT, H: ExHashT, N: NetworkBackend<Block, H>> {
	/// Assigned role for our node (full, light, ...).
	pub role: Role,

	/// How to spawn background tasks.
	pub executor: Box<dyn Fn(Pin<Box<dyn Future<Output = ()> + Send>>) + Send + Sync>,

	/// Network layer configuration.
	pub network_config: FullNetworkConfiguration<Block, H, N>,

	/// Legacy name of the protocol to use on the wire. Should be different for each chain.
	pub protocol_id: ProtocolId,

	/// Genesis hash of the chain
	pub genesis_hash: Block::Hash,

	/// Fork ID to distinguish protocols of different hard forks. Part of the standard protocol
	/// name on the wire.
	pub fork_id: Option<String>,

	/// Registry for recording prometheus metrics to.
	pub metrics_registry: Option<Registry>,

	/// Block announce protocol configuration
	pub block_announce_config: N::NotificationProtocolConfig,

	/// Bitswap configuration, if the server has been enabled.
	pub ipfs_config: Option<IpfsConfig<Block, H, N>>,

	/// Notification metrics.
	pub notification_metrics: NotificationMetrics,
}

/// Full network configuration.
pub struct FullNetworkConfiguration<B: BlockT + 'static, H: ExHashT, N: NetworkBackend<B, H>> {
	/// Installed notification protocols.
	pub(crate) notification_protocols: Vec<N::NotificationProtocolConfig>,

	/// List of request-response protocols that the node supports.
	pub(crate) request_response_protocols: Vec<N::RequestResponseProtocolConfig>,

	/// Network configuration.
	pub network_config: NetworkConfiguration,

	/// [`PeerStore`](crate::peer_store::PeerStore),
	peer_store: Option<N::PeerStore>,

	/// Handle to [`PeerStore`](crate::peer_store::PeerStore).
	peer_store_handle: Arc<dyn PeerStoreProvider>,

	/// Registry for recording prometheus metrics to.
	pub metrics_registry: Option<Registry>,
}

impl<B: BlockT + 'static, H: ExHashT, N: NetworkBackend<B, H>> FullNetworkConfiguration<B, H, N> {
	/// Create new [`FullNetworkConfiguration`].
	pub fn new(network_config: &NetworkConfiguration, metrics_registry: Option<Registry>) -> Self {
		let bootnodes = network_config.boot_nodes.iter().map(|bootnode| bootnode.peer_id).collect();
		let peer_store = N::peer_store(bootnodes, metrics_registry.clone());
		let peer_store_handle = peer_store.handle();

		Self {
			peer_store: Some(peer_store),
			peer_store_handle,
			notification_protocols: Vec::new(),
			request_response_protocols: Vec::new(),
			network_config: network_config.clone(),
			metrics_registry,
		}
	}

	/// Add a notification protocol.
	pub fn add_notification_protocol(&mut self, config: N::NotificationProtocolConfig) {
		self.notification_protocols.push(config);
	}

	/// Get reference to installed notification protocols.
	pub fn notification_protocols(&self) -> &Vec<N::NotificationProtocolConfig> {
		&self.notification_protocols
	}

	/// Add a request-response protocol.
	pub fn add_request_response_protocol(&mut self, config: N::RequestResponseProtocolConfig) {
		self.request_response_protocols.push(config);
	}

	/// Get handle to [`PeerStore`].
	pub fn peer_store_handle(&self) -> Arc<dyn PeerStoreProvider> {
		Arc::clone(&self.peer_store_handle)
	}

	/// Take [`PeerStore`].
	///
	/// `PeerStore` is created when `FullNetworkConfig` is initialized so that `PeerStoreHandle`s
	/// can be passed onto notification protocols. `PeerStore` itself should be started only once
	/// and since technically it's not a libp2p task, it should be started with `SpawnHandle` in
	/// `builder.rs` instead of using the libp2p/litep2p executor in the networking backend. This
	/// function consumes `PeerStore` and starts its event loop in the appropriate place.
	pub fn take_peer_store(&mut self) -> N::PeerStore {
		self.peer_store
			.take()
			.expect("`PeerStore` can only be taken once when it's started; qed")
	}

	/// Verify addresses are consistent with enabled transports.
	pub fn sanity_check_addresses(&self) -> Result<(), crate::error::Error> {
		ensure_addresses_consistent_with_transport(
			self.network_config.listen_addresses.iter(),
			&self.network_config.transport,
		)?;
		ensure_addresses_consistent_with_transport(
			self.network_config.boot_nodes.iter().map(|x| &x.multiaddr),
			&self.network_config.transport,
		)?;
		ensure_addresses_consistent_with_transport(
			self.network_config
				.default_peers_set
				.reserved_nodes
				.iter()
				.map(|x| &x.multiaddr),
			&self.network_config.transport,
		)?;

		for notification_protocol in &self.notification_protocols {
			ensure_addresses_consistent_with_transport(
				notification_protocol.set_config().reserved_nodes.iter().map(|x| &x.multiaddr),
				&self.network_config.transport,
			)?;
		}
		ensure_addresses_consistent_with_transport(
			self.network_config.public_addresses.iter(),
			&self.network_config.transport,
		)?;

		Ok(())
	}

	/// Check for duplicate bootnodes.
	pub fn sanity_check_bootnodes(&self) -> Result<(), crate::error::Error> {
		self.network_config.boot_nodes.iter().try_for_each(|bootnode| {
			if let Some(other) = self
				.network_config
				.boot_nodes
				.iter()
				.filter(|o| o.multiaddr == bootnode.multiaddr)
				.find(|o| o.peer_id != bootnode.peer_id)
			{
				Err(crate::error::Error::DuplicateBootnode {
					address: bootnode.multiaddr.clone().into(),
					first_id: bootnode.peer_id.into(),
					second_id: other.peer_id.into(),
				})
			} else {
				Ok(())
			}
		})
	}

	/// Collect all reserved nodes and bootnodes addresses.
	pub fn known_addresses(&self) -> Vec<(PeerId, Multiaddr)> {
		let mut addresses: Vec<_> = self
			.network_config
			.default_peers_set
			.reserved_nodes
			.iter()
			.map(|reserved| (reserved.peer_id, reserved.multiaddr.clone()))
			.chain(self.notification_protocols.iter().flat_map(|protocol| {
				protocol
					.set_config()
					.reserved_nodes
					.iter()
					.map(|reserved| (reserved.peer_id, reserved.multiaddr.clone()))
			}))
			.chain(
				self.network_config
					.boot_nodes
					.iter()
					.map(|bootnode| (bootnode.peer_id, bootnode.multiaddr.clone())),
			)
			.collect();

		// Remove possible duplicates.
		addresses.sort();
		addresses.dedup();

		addresses
	}
}

/// Network backend type.
#[derive(Debug, Clone, Default, Copy)]
pub enum NetworkBackendType {
	/// Use litep2p for P2P networking, with post-quantum TLS connections.
	#[default]
	Litep2p,
}

/// Whether `address` is a `webrtc-direct` address.
fn is_webrtc_address(address: &Multiaddr) -> bool {
	address.iter().any(|protocol| matches!(protocol, multiaddr::Protocol::WebRTCDirect))
}


#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	fn tempdir_with_prefix(prefix: &str) -> TempDir {
		tempfile::Builder::new().prefix(prefix).tempdir().unwrap()
	}

	#[test]
	fn test_secret_file() {
		let tmp = tempdir_with_prefix("x");
		std::fs::remove_dir(tmp.path()).unwrap(); // should be recreated
		let file = tmp.path().join("x").to_path_buf();
		let kp1 = NodeKeyConfig::MlDsa(Secret::File(file.clone())).into_keypair().unwrap();
		let kp2 = NodeKeyConfig::MlDsa(Secret::File(file.clone())).into_keypair().unwrap();
		assert!(file.is_file() && kp1.public() == kp2.public())
	}

	#[test]
	fn test_secret_input() {
		let sk = ml_dsa::SecretKey::generate();
		let kp1 = NodeKeyConfig::MlDsa(Secret::Input(sk.clone())).into_keypair().unwrap();
		let kp2 = NodeKeyConfig::MlDsa(Secret::Input(sk)).into_keypair().unwrap();
		assert!(kp1.public() == kp2.public());
	}

	#[test]
	fn test_secret_new() {
		let kp1 = NodeKeyConfig::MlDsa(Secret::New).into_keypair().unwrap();
		let kp2 = NodeKeyConfig::MlDsa(Secret::New).into_keypair().unwrap();
		assert!(kp1.public() != kp2.public());
	}
}
