use super::*;
use std::collections::HashSet;

const TESS: u64 = 1_000_000_000_000;
const FEE: u64 = TESS / 100;

/// The rules of the runtime, without the runtime: a proof that verifies, a known anchor, unspent
/// nullifiers and a pool that never goes negative.
#[derive(Default)]
struct Chain {
	tree: Tree,
	roots: Vec<Digest>,
	nullifiers: HashSet<Digest>,
	pool: u64,
	events: Vec<Transacted>,
}

impl Chain {
	fn new() -> Self {
		let mut chain = Self::default();
		chain.roots.push(chain.tree.root());
		chain
	}

	fn apply(&mut self, transaction: &ShieldedTransaction, source: Option<[u8; 32]>) -> Result<(), &'static str> {
		if !transaction.verify(source.as_ref()) {
			return Err("invalid proof");
		}
		if !self.roots.contains(&transaction.anchor) {
			return Err("unknown anchor");
		}
		if transaction.nullifiers[0] == transaction.nullifiers[1]
			|| transaction.nullifiers.iter().any(|n| self.nullifiers.contains(n))
		{
			return Err("nullifier spent");
		}
		self.pool = (self.pool + transaction.value_in)
			.checked_sub(transaction.value_out + transaction.fee)
			.ok_or("pool underflow")?;
		self.nullifiers.extend(transaction.nullifiers);
		let position = self.tree.append(transaction.commitments[0]);
		self.tree.append(transaction.commitments[1]);
		self.roots.push(self.tree.root());
		self.events.push(Transacted {
			position,
			nullifiers: transaction.nullifiers,
			commitments: transaction.commitments,
			notes: transaction.notes.map(|n| n.to_vec()),
		});
		Ok(())
	}

	fn sync(&self, wallet: &mut Wallet) {
		let from = self.events.iter().position(|e| e.position == wallet.position()).unwrap_or(self.events.len());
		for event in &self.events[from..] {
			wallet.ingest(event).unwrap();
		}
	}
}

#[test]
fn shield_send_and_unshield() {
	let mut chain = Chain::new();
	let mut alice = Wallet::new(spending_key(&[1; 32]));
	let mut bob = Wallet::new(spending_key(&[2; 32]));
	let (alice_account, bob_account) = ([9; 32], [7; 32]);

	// Alice moves 10 TESS of her transparent account into the pool.
	let shield = alice.shield(alice_account, 10 * TESS).unwrap();
	assert!(!shield.verify(Some(&bob_account)), "the proof binds the account paying value_in");
	chain.apply(&shield, Some(alice_account)).unwrap();
	chain.sync(&mut alice);
	chain.sync(&mut bob);
	assert_eq!((alice.balance(), bob.balance()), (10 * TESS, 0));

	// She sends 4 TESS to Bob's shielded address, privately.
	let address = decode_address(&encode_address(bob.address())).unwrap();
	let send = alice.send(&address, 4 * TESS, FEE).unwrap();
	alice.mark_pending(&send);
	assert_eq!((alice.balance(), alice.pending()), (0, 10 * TESS));
	chain.apply(&send, None).unwrap();
	chain.sync(&mut alice);
	chain.sync(&mut bob);
	assert_eq!((alice.balance(), alice.pending()), (6 * TESS - FEE, 0));
	assert_eq!(bob.balance(), 4 * TESS);

	// The same notes cannot be spent twice.
	assert_eq!(chain.apply(&send, None), Err("nullifier spent"));

	// Bob moves 3 TESS out of the pool to his transparent account.
	let unshield = bob.unshield(bob_account, 3 * TESS, FEE).unwrap();
	assert_eq!(unshield.recipient, Some(bob_account));
	chain.apply(&unshield, None).unwrap();
	chain.sync(&mut alice);
	chain.sync(&mut bob);
	assert_eq!(bob.balance(), TESS - FEE);
	assert_eq!(chain.pool, 10 * TESS - 3 * TESS - 2 * FEE);
	assert_eq!(alice.balance() + bob.balance(), chain.pool);

	// A wallet restored from its state has the same notes and the same tree.
	let restored = Wallet::restore(spending_key(&[1; 32]), &alice.state()).unwrap();
	assert_eq!((restored.balance(), restored.root()), (alice.balance(), chain.tree.root()));
	assert!(Wallet::restore(spending_key(&[1; 32]), &[2]).is_err());
}

#[test]
fn spending_more_than_two_notes_hold_is_refused() {
	let mut chain = Chain::new();
	let mut wallet = Wallet::new(spending_key(&[3; 32]));
	for _ in 0..3 {
		let shield = wallet.shield([9; 32], TESS).unwrap();
		chain.apply(&shield, Some([9; 32])).unwrap();
		chain.sync(&mut wallet);
	}
	assert_eq!(wallet.balance(), 3 * TESS);
	let other = spending_key(&[4; 32]).address();
	assert!(matches!(wallet.send(&other, 2 * TESS + TESS / 2, FEE), Err(Error::TooManyNotes { .. })));
	assert!(matches!(wallet.send(&other, 4 * TESS, FEE), Err(Error::InsufficientFunds { .. })));
	// Two notes are enough for 1.5 TESS and the fee.
	let send = wallet.send(&other, TESS + TESS / 2, FEE).unwrap();
	chain.apply(&send, None).unwrap();
}

#[test]
fn events_are_ingested_in_order() {
	let mut chain = Chain::new();
	let mut wallet = Wallet::new(spending_key(&[5; 32]));
	let shield = wallet.shield([9; 32], TESS).unwrap();
	chain.apply(&shield, Some([9; 32])).unwrap();
	chain.apply(&wallet.shield([9; 32], TESS).unwrap(), Some([9; 32])).unwrap();
	assert_eq!(wallet.ingest(&chain.events[1]).unwrap_err(), Error::OutOfOrder { expected: 0, got: 2 });
}

#[test]
fn addresses_are_checked() {
	let address = encode_address(&spending_key(&[6; 32]).address());
	assert!(address.starts_with(ADDRESS_PREFIX));
	let mut corrupted = address.clone().into_bytes();
	let last = corrupted.len() - 1;
	corrupted[last] = if corrupted[last] == b'1' { b'2' } else { b'1' };
	assert_eq!(decode_address(&String::from_utf8(corrupted).unwrap()), Err(Error::InvalidAddress));
	assert_eq!(decode_address("te5abc"), Err(Error::InvalidAddress));
}

#[test]
fn transact_is_a_general_transaction() {
	let mut chain = Chain::new();
	let mut wallet = Wallet::new(spending_key(&[8; 32]));
	chain.apply(&wallet.shield([9; 32], TESS).unwrap(), Some([9; 32])).unwrap();
	chain.sync(&mut wallet);
	let send = wallet.send(&spending_key(&[1; 32]).address(), TESS / 2, FEE).unwrap();
	let extrinsic = transact_extrinsic(42, 0, &send);
	let mut input = &extrinsic[..];
	let length = codec::Compact::<u32>::decode(&mut input).unwrap().0 as usize;
	assert_eq!(length, input.len());
	assert_eq!(&input[..8], &[0x45, 0, 0, 0, 0, 0, 42, 0]);
	assert_eq!(ShieldedTransaction::decode(&mut &input[8..]).unwrap(), send);
}
