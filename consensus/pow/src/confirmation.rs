//! Confirmation depth.
//!
//! Proof of work alone never makes a block final. The node finalizes every block once it is
//! buried [`CONFIRMATION_DEPTH`] blocks deep in the best chain. The client then refuses any chain
//! that does not contain the finalized block, which caps reorganizations at that depth regardless
//! of the work behind the competing chain.
//!
//! A node cut off from the network for longer than the confirmation depth may finalize a minority
//! chain and must then be resynchronized manually.

use futures::StreamExt;
use sc_client_api::{Backend, BlockchainEvents, Finalizer};
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::{Block as BlockT, Saturating};
use std::sync::Arc;

use crate::LOG_TARGET;

/// Depth at which blocks are finalized: 60 blocks, 18 minutes at the 18 second target.
pub const CONFIRMATION_DEPTH: u32 = 60;

/// Finalize blocks buried `depth` blocks deep in the best chain. Runs until the client stops.
pub async fn finalize_at_depth<B, BE, C>(client: Arc<C>, depth: u32)
where
	B: BlockT,
	BE: Backend<B>,
	C: BlockchainEvents<B> + HeaderBackend<B> + Finalizer<B, BE>,
{
	let mut imports = client.every_import_notification_stream();
	while let Some(notification) = imports.next().await {
		if !notification.is_new_best {
			continue;
		}

		let info = client.info();
		let target = info.best_number.saturating_sub(depth.into());
		if target <= info.finalized_number {
			continue;
		}

		let hash = match client.hash(target) {
			Ok(Some(hash)) => hash,
			Ok(None) => continue,
			Err(e) => {
				log::warn!(target: LOG_TARGET, "Failed to look up block #{target}: {e}");
				continue;
			},
		};

		if let Err(e) = client.finalize_block(hash, None, true) {
			log::warn!(target: LOG_TARGET, "Failed to finalize block #{target} ({hash}): {e}");
		}
	}
}
