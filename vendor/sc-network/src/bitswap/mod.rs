// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Substrate.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate. If not, see <https://www.gnu.org/licenses/>.

//! Bitswap server for Substrate.
//!
//! Supports querying indexed transactions by hash over the standard bitswap protocol (v1.2.0).
//! CIDs must reference a supported 256-bit transaction hash.

use cid::Version as CidVersion;
use unsigned_varint::encode as varint_encode;

/// Bitswap client.
mod client;
pub(crate) mod schema;

pub use cid::Cid;
pub use client::{
	request_bitswap_blocks, request_bitswap_blocks_unverified, BitswapError, FetchOutcome,
	BLAKE2B_256_MULTIHASH_CODE, KECCAK_256_MULTIHASH_CODE, SHA2_256_MULTIHASH_CODE,
};

pub(crate) use schema::bitswap::Message as BitswapProtoMessage;

pub(crate) const LOG_TARGET: &str = "sub-libp2p::bitswap";

/// Max number of blocks per wantlist.
pub const MAX_WANTED_BLOCKS: usize = 16;

/// Bitswap protocol name.
pub(crate) const PROTOCOL_NAME: &str = "/ipfs/bitswap/1.2.0";

/// IPFS raw multicodec used for indexed transaction payload bytes.
pub const RAW_CODEC: u64 = 0x55;

/// Check if a CID is supported by the bitswap protocol — CIDv1, 32-byte digest, with a
/// supported multihash code (Blake2b-256, SHA2-256, or Keccak-256).
pub fn is_cid_supported(cid: &Cid) -> bool {
	cid.version() != CidVersion::V0 &&
		cid.hash().size() == 32 &&
		is_supported_multihash_code(cid.hash().code())
}

/// Return `true` if `code` is a supported multihash code.
pub(crate) fn is_supported_multihash_code(code: u64) -> bool {
	matches!(code, BLAKE2B_256_MULTIHASH_CODE | SHA2_256_MULTIHASH_CODE | KECCAK_256_MULTIHASH_CODE)
}

/// CID metadata without the actual content bytes.
#[derive(PartialEq, Eq, Clone, Debug)]
pub(crate) struct Prefix {
	/// The version of CID.
	pub version: CidVersion,
	/// The codec of CID.
	pub codec: u64,
	/// The multihash type of CID.
	pub mh_type: u64,
	/// The multihash length of CID.
	pub mh_len: u8,
}

impl From<&Cid> for Prefix {
	fn from(cid: &Cid) -> Self {
		Self {
			version: cid.version(),
			codec: cid.codec(),
			mh_type: cid.hash().code(),
			mh_len: cid.hash().size(),
		}
	}
}

impl Prefix {
	/// Convert the prefix to encoded bytes.
	pub(crate) fn to_bytes(&self) -> Vec<u8> {
		let mut res = Vec::with_capacity(4);
		let mut buf = varint_encode::u64_buffer();
		let version = varint_encode::u64(self.version.into(), &mut buf);
		res.extend_from_slice(version);
		let mut buf = varint_encode::u64_buffer();
		let codec = varint_encode::u64(self.codec, &mut buf);
		res.extend_from_slice(codec);
		let mut buf = varint_encode::u64_buffer();
		let mh_type = varint_encode::u64(self.mh_type, &mut buf);
		res.extend_from_slice(mh_type);
		let mut buf = varint_encode::u64_buffer();
		let mh_len = varint_encode::u64(self.mh_len as u64, &mut buf);
		res.extend_from_slice(mh_len);
		res
	}
}
