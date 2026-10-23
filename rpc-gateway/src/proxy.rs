//! Accepts TLS connections and relays each one, byte for byte, to the RPC of the node: HTTP
//! requests and websockets alike, the node answering both on one port.

use std::{
	io,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	time::Duration,
};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::Semaphore,
};
use tokio_rustls::TlsAcceptor;

pub struct Proxy {
	pub acceptor: TlsAcceptor,
	/// `host:port` of the RPC of the node.
	pub upstream: String,
	pub connections: Arc<Semaphore>,
	pub handshake_timeout: Duration,
	pub stats: Arc<Stats>,
}

#[derive(Default)]
pub struct Stats {
	pub accepted: AtomicU64,
	pub refused: AtomicU64,
	pub handshake_failures: AtomicU64,
	pub upstream_failures: AtomicU64,
}

impl Proxy {
	pub async fn run(self: Arc<Self>, listener: TcpListener) -> io::Result<()> {
		loop {
			let (socket, _) = match listener.accept().await {
				Ok(accepted) => accepted,
				// Out of file descriptors and the like: wait for connections to close.
				Err(e) => {
					tracing::warn!("Accepting a connection: {e}");
					tokio::time::sleep(Duration::from_millis(100)).await;
					continue;
				},
			};
			let Ok(permit) = self.connections.clone().try_acquire_owned() else {
				self.stats.refused.fetch_add(1, Ordering::Relaxed);
				continue;
			};
			let proxy = self.clone();
			tokio::spawn(async move {
				proxy.relay(socket).await;
				drop(permit);
			});
		}
	}

	async fn relay(&self, socket: TcpStream) {
		let _ = socket.set_nodelay(true);
		let mut tls = match tokio::time::timeout(self.handshake_timeout, self.acceptor.accept(socket)).await {
			Ok(Ok(tls)) => tls,
			Ok(Err(e)) => {
				self.stats.handshake_failures.fetch_add(1, Ordering::Relaxed);
				tracing::debug!("TLS handshake: {e}");
				return;
			},
			Err(_) => {
				self.stats.handshake_failures.fetch_add(1, Ordering::Relaxed);
				return;
			},
		};
		self.stats.accepted.fetch_add(1, Ordering::Relaxed);
		let mut node = match tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(&self.upstream)).await {
			Ok(Ok(node)) => node,
			Ok(Err(e)) => {
				self.stats.upstream_failures.fetch_add(1, Ordering::Relaxed);
				tracing::warn!("Connecting to the node at {}: {e}", self.upstream);
				return;
			},
			Err(_) => {
				self.stats.upstream_failures.fetch_add(1, Ordering::Relaxed);
				tracing::warn!("Connecting to the node at {}: timed out", self.upstream);
				return;
			},
		};
		let _ = node.set_nodelay(true);
		if let Err(e) = tokio::io::copy_bidirectional(&mut tls, &mut node).await {
			tracing::debug!("Connection closed: {e}");
		}
	}
}
