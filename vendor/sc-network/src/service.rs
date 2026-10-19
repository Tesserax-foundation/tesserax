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

//! Network service types shared by the network backend.

use crate::{config::TransportConfig, error::Error};

pub use crate::request_responses::{InboundFailure, OutboundFailure, ResponseFailure};
pub use metrics::NotificationMetrics;

pub(crate) mod metrics;
pub(crate) mod out_events;

pub mod signature;
pub mod traits;

pub(crate) fn ensure_addresses_consistent_with_transport<'a>(
	addresses: impl Iterator<Item = &'a sc_network_types::multiaddr::Multiaddr>,
	transport: &TransportConfig,
) -> Result<(), Error> {
	use sc_network_types::multiaddr::Protocol;

	if matches!(transport, TransportConfig::MemoryOnly) {
		let addresses: Vec<_> = addresses
			.filter(|x| x.iter().any(|y| !matches!(y, Protocol::Memory(_))))
			.cloned()
			.collect();

		if !addresses.is_empty() {
			return Err(Error::AddressesForAnotherTransport {
				transport: transport.clone(),
				addresses,
			});
		}
	} else {
		let addresses: Vec<_> = addresses
			.filter(|x| x.iter().any(|y| matches!(y, Protocol::Memory(_))))
			.cloned()
			.collect();

		if !addresses.is_empty() {
			return Err(Error::AddressesForAnotherTransport {
				transport: transport.clone(),
				addresses,
			});
		}
	}

	Ok(())
}
