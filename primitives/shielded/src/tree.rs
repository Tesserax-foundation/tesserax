//! Incremental Merkle tree of note commitments.

use crate::hash::{domain, hash, Digest};
use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Depth of the note tree: up to `2^32` notes.
pub const TREE_DEPTH: usize = 32;

/// Merkle tree node: `H(left, right, NODE)`.
pub fn node(left: &Digest, right: &Digest) -> Digest {
	hash(left, right, domain::NODE)
}

/// Roots of empty subtrees: `empty_roots()[h]` is the root of an empty tree of height `h`.
pub fn empty_roots() -> [Digest; TREE_DEPTH + 1] {
	let mut roots = [Digest::ZERO; TREE_DEPTH + 1];
	for height in 1..=TREE_DEPTH {
		roots[height] = node(&roots[height - 1], &roots[height - 1]);
	}
	roots
}

/// Authentication path of a leaf: siblings from the leaf level up, and the leaf position.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, TypeInfo)]
pub struct MerklePath {
	pub position: u64,
	pub siblings: Vec<Digest>,
}

impl MerklePath {
	/// Root obtained by hashing `leaf` along this path.
	pub fn root(&self, leaf: &Digest) -> Digest {
		let mut current = *leaf;
		for (height, sibling) in self.siblings.iter().enumerate() {
			current = if (self.position >> height) & 1 == 0 {
				node(&current, sibling)
			} else {
				node(sibling, &current)
			};
		}
		current
	}
}

/// Append-only Merkle tree keeping only its right frontier: what the chain stores.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub struct Frontier {
	/// Number of leaves.
	pub size: u64,
	/// `left[h]` is the last complete left subtree root at height `h`, if any.
	pub left: [Digest; TREE_DEPTH],
}

impl Default for Frontier {
	fn default() -> Self {
		Frontier { size: 0, left: [Digest::ZERO; TREE_DEPTH] }
	}
}

impl Frontier {
	/// Whether the tree is full.
	pub fn is_full(&self) -> bool {
		self.size >= 1u64 << TREE_DEPTH
	}

	/// Append `leaf`. Returns its position, or `None` if the tree is full.
	pub fn append(&mut self, leaf: Digest) -> Option<u64> {
		if self.is_full() {
			return None;
		}
		let position = self.size;
		let mut current = leaf;
		for height in 0..TREE_DEPTH {
			if (position >> height) & 1 == 0 {
				self.left[height] = current;
				break;
			}
			current = node(&self.left[height], &current);
		}
		self.size += 1;
		Some(position)
	}

	/// Root of the tree.
	pub fn root(&self) -> Digest {
		self.root_with(&empty_roots())
	}

	/// Root of the tree, given the precomputed [`empty_roots`].
	pub fn root_with(&self, empty: &[Digest; TREE_DEPTH + 1]) -> Digest {
		let mut current = empty[0];
		let mut filled = false;
		for height in 0..TREE_DEPTH {
			let bit = (self.size >> height) & 1;
			current = match (bit, filled) {
				(1, false) => node(&self.left[height], &empty[height]),
				(1, true) => node(&self.left[height], &current),
				(_, false) => empty[height + 1],
				(_, true) => node(&current, &empty[height]),
			};
			filled |= bit == 1;
		}
		current
	}
}

/// Complete tree kept by wallets to build authentication paths.
#[cfg(feature = "std")]
#[derive(Clone, Debug, Default)]
pub struct Tree {
	leaves: Vec<Digest>,
}

#[cfg(feature = "std")]
impl Tree {
	/// Append `leaf` and return its position.
	pub fn append(&mut self, leaf: Digest) -> u64 {
		self.leaves.push(leaf);
		self.leaves.len() as u64 - 1
	}

	/// Number of leaves.
	pub fn len(&self) -> u64 {
		self.leaves.len() as u64
	}

	/// Whether the tree is empty.
	pub fn is_empty(&self) -> bool {
		self.leaves.is_empty()
	}

	/// Leaf at `position`.
	pub fn leaf(&self, position: u64) -> Option<Digest> {
		self.leaves.get(position as usize).copied()
	}

	/// Root of the tree.
	pub fn root(&self) -> Digest {
		self.levels().last().map(|level| level[0]).unwrap_or(empty_roots()[TREE_DEPTH])
	}

	/// Authentication path of the leaf at `position`.
	pub fn path(&self, position: u64) -> Option<MerklePath> {
		if position >= self.len() {
			return None;
		}
		let levels = self.levels();
		let empty = empty_roots();
		let siblings = (0..TREE_DEPTH)
			.map(|height| {
				let index = (position >> height) ^ 1;
				levels[height].get(index as usize).copied().unwrap_or(empty[height])
			})
			.collect();
		Some(MerklePath { position, siblings })
	}

	/// Nodes of every level, from the leaves to the root, without the empty right part.
	fn levels(&self) -> Vec<Vec<Digest>> {
		let empty = empty_roots();
		let mut levels = Vec::with_capacity(TREE_DEPTH + 1);
		levels.push(self.leaves.clone());
		for height in 0..TREE_DEPTH {
			let current = &levels[height];
			let next = current
				.chunks(2)
				.map(|pair| node(&pair[0], pair.get(1).unwrap_or(&empty[height])))
				.collect::<Vec<_>>();
			levels.push(if next.is_empty() { Vec::new() } else { next });
		}
		if levels[TREE_DEPTH].is_empty() {
			levels[TREE_DEPTH].push(empty[TREE_DEPTH]);
		}
		levels
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn leaf(i: u64) -> Digest {
		Digest([i, i + 1, i + 2, i + 3])
	}

	#[test]
	fn empty_frontier_root() {
		assert_eq!(Frontier::default().root(), empty_roots()[TREE_DEPTH]);
		assert_eq!(Tree::default().root(), empty_roots()[TREE_DEPTH]);
	}

	#[test]
	fn frontier_matches_full_tree() {
		let mut frontier = Frontier::default();
		let mut tree = Tree::default();
		for i in 0..37 {
			assert_eq!(frontier.append(leaf(i)), Some(i));
			tree.append(leaf(i));
			assert_eq!(frontier.root(), tree.root(), "after {} leaves", i + 1);
		}
	}

	#[test]
	fn paths_lead_to_root() {
		let mut tree = Tree::default();
		for i in 0..21 {
			tree.append(leaf(i));
		}
		let root = tree.root();
		for i in 0..21 {
			let path = tree.path(i).unwrap();
			assert_eq!(path.siblings.len(), TREE_DEPTH);
			assert_eq!(path.root(&leaf(i)), root);
			assert_ne!(path.root(&leaf(i + 100)), root);
		}
		assert!(tree.path(21).is_none());
	}
}
