//! The books of the pool, in SQLite: shares, blocks found, credits, balances and payments.
//!
//! Rewards follow PPLNS ("pay per last N shares"): when a rig finds a block, the shares of the
//! last `N` units of difficulty (a multiple of the block difficulty) split its reward, in
//! proportion to their difficulty. The split is recorded at once; the reward is credited when the
//! block is final, 60 blocks deep, and dropped if the block left the chain.
//!
//! Amounts are in planck (10^-12 TESS), stored as decimal text: SQLite integers stop at 2^63.

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

pub type Result<T> = std::result::Result<T, rusqlite::Error>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS shares (
	id INTEGER PRIMARY KEY,
	address TEXT NOT NULL,
	worker TEXT NOT NULL,
	difficulty INTEGER NOT NULL,
	created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS shares_by_time ON shares (created_at);
CREATE TABLE IF NOT EXISTS blocks (
	nonce TEXT PRIMARY KEY,
	height INTEGER NOT NULL,
	hash TEXT,
	finder TEXT NOT NULL,
	worker TEXT NOT NULL,
	network_difficulty TEXT NOT NULL,
	effort REAL NOT NULL,
	found_at INTEGER NOT NULL,
	status TEXT NOT NULL,
	reward TEXT,
	fee TEXT
);
CREATE INDEX IF NOT EXISTS blocks_by_height ON blocks (height);
CREATE TABLE IF NOT EXISTS block_shares (
	block TEXT NOT NULL REFERENCES blocks (nonce),
	address TEXT NOT NULL,
	weight TEXT NOT NULL,
	PRIMARY KEY (block, address)
);
CREATE TABLE IF NOT EXISTS credits (
	block TEXT NOT NULL REFERENCES blocks (nonce),
	address TEXT NOT NULL,
	amount TEXT NOT NULL,
	PRIMARY KEY (block, address)
);
CREATE TABLE IF NOT EXISTS balances (
	address TEXT PRIMARY KEY,
	balance TEXT NOT NULL,
	paid TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS payments (
	id INTEGER PRIMARY KEY,
	address TEXT NOT NULL,
	amount TEXT NOT NULL,
	status TEXT NOT NULL,
	tx_hash TEXT,
	block_hash TEXT,
	error TEXT,
	created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS payments_by_address ON payments (address, id);
CREATE TABLE IF NOT EXISTS meta (
	key TEXT PRIMARY KEY,
	value TEXT NOT NULL
);
";

/// A share accepted from a rig.
#[derive(Clone, Debug)]
pub struct Share {
	pub address: String,
	pub worker: String,
	pub difficulty: u64,
	/// Milliseconds since the Unix epoch.
	pub created_at: i64,
}

/// A block found by a rig, as submitted.
#[derive(Clone, Debug)]
pub struct Found {
	pub height: u64,
	/// The 11-byte nonce of the seal, hexadecimal: it identifies the block among its competitors.
	pub nonce: String,
	pub finder: String,
	pub worker: String,
	pub network_difficulty: u128,
	pub found_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlockRow {
	pub height: u64,
	pub nonce: String,
	pub hash: Option<String>,
	pub finder: String,
	pub worker: String,
	pub network_difficulty: String,
	/// Shares of the round over the block difficulty, in percent.
	pub effort: f64,
	pub found_at: i64,
	/// `pending`, `confirmed` or `orphaned`.
	pub status: String,
	pub reward: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaymentRow {
	pub id: i64,
	pub address: String,
	pub amount: String,
	/// `sending`, `confirmed`, `failed`, or `unknown` after an interruption.
	pub status: String,
	pub tx_hash: Option<String>,
	pub block_hash: Option<String>,
	pub created_at: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Account {
	pub balance: String,
	pub paid: String,
	/// Estimated share of the blocks not final yet.
	pub immature: String,
}

pub struct Ledger {
	conn: Mutex<Connection>,
}

fn amount(text: Option<String>) -> u128 {
	text.and_then(|t| t.parse().ok()).unwrap_or(0)
}

impl Ledger {
	pub fn open(path: &Path) -> Result<Self> {
		let conn = Connection::open(path)?;
		conn.pragma_update(None, "journal_mode", "WAL")?;
		conn.pragma_update(None, "synchronous", "FULL")?;
		conn.pragma_update(None, "foreign_keys", "ON")?;
		conn.execute_batch(SCHEMA)?;
		Ok(Ledger { conn: Mutex::new(conn) })
	}

	pub fn add_shares(&self, shares: &[Share]) -> Result<()> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		{
			let mut insert = tx.prepare("INSERT INTO shares (address, worker, difficulty, created_at) VALUES (?1, ?2, ?3, ?4)")?;
			for share in shares {
				let difficulty = i64::try_from(share.difficulty).unwrap_or(i64::MAX);
				insert.execute(params![share.address, share.worker, difficulty, share.created_at])?;
			}
		}
		tx.commit()
	}

	/// Record a block found, with the PPLNS split of its reward over the last `window` units of
	/// difficulty, and the effort of the round it ends.
	pub fn record_block(&self, found: &Found, window: u128) -> Result<()> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		let mut weights: BTreeMap<String, u128> = BTreeMap::new();
		{
			let mut newest = tx.prepare("SELECT address, difficulty FROM shares ORDER BY id DESC")?;
			let mut rows = newest.query([])?;
			let mut total = 0u128;
			while let Some(row) = rows.next()? {
				if total >= window {
					break;
				}
				let difficulty = row.get::<_, i64>(1)?.max(0) as u128;
				// The oldest share counts for the part of it inside the window.
				let weight = difficulty.min(window - total);
				total += weight;
				*weights.entry(row.get(0)?).or_default() += weight;
			}
		}
		let round_start: i64 = tx
			.query_row("SELECT value FROM meta WHERE key = 'round_start'", [], |r| r.get::<_, String>(0))
			.optional()?
			.and_then(|v| v.parse().ok())
			.unwrap_or(0);
		let (round, last): (i64, Option<i64>) = tx.query_row(
			"SELECT COALESCE(SUM(difficulty), 0), MAX(id) FROM shares WHERE id > ?1",
			[round_start],
			|r| Ok((r.get(0)?, r.get(1)?)),
		)?;
		let effort = if found.network_difficulty == 0 { 0.0 } else { round as f64 / found.network_difficulty as f64 * 100.0 };
		tx.execute(
			"INSERT OR IGNORE INTO blocks (nonce, height, finder, worker, network_difficulty, effort, found_at, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
			params![found.nonce, found.height as i64, found.finder, found.worker, found.network_difficulty.to_string(), effort, found.found_at],
		)?;
		for (address, weight) in &weights {
			tx.execute(
				"INSERT OR REPLACE INTO block_shares (block, address, weight) VALUES (?1, ?2, ?3)",
				params![found.nonce, address, weight.to_string()],
			)?;
		}
		if let Some(last) = last {
			tx.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('round_start', ?1)", [last.to_string()])?;
		}
		tx.commit()
	}

	/// Blocks waiting to become final.
	pub fn pending_blocks(&self) -> Result<Vec<BlockRow>> {
		self.blocks_where("status = 'pending'", 1000)
	}

	pub fn set_block_hash(&self, nonce: &str, hash: &str) -> Result<()> {
		self.conn.lock().execute("UPDATE blocks SET hash = ?2 WHERE nonce = ?1", params![nonce, hash])?;
		Ok(())
	}

	/// The block is final: split `reward` between the shares of its window, after the pool fee
	/// (`fee_bps` hundredths of a percent). The rounding remainder goes to the fee.
	pub fn confirm_block(&self, nonce: &str, hash: &str, reward: u128, fee_bps: u32) -> Result<()> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		let weights: Vec<(String, u128)> = tx
			.prepare("SELECT address, weight FROM block_shares WHERE block = ?1")?
			.query_map([nonce], |r| Ok((r.get::<_, String>(0)?, amount(r.get(1)?))))?
			.collect::<Result<_>>()?;
		let total: u128 = weights.iter().map(|(_, w)| w).sum();
		let mut fee = reward * u128::from(fee_bps) / 10_000;
		let distributable = reward - fee;
		let mut credited = 0u128;
		for (address, weight) in &weights {
			let share = if total == 0 { 0 } else { distributable * weight / total };
			if share == 0 {
				continue;
			}
			credited += share;
			tx.execute("INSERT OR REPLACE INTO credits (block, address, amount) VALUES (?1, ?2, ?3)", params![nonce, address, share.to_string()])?;
			Self::add_balance(&tx, address, share as i128)?;
		}
		fee += distributable - credited;
		tx.execute(
			"UPDATE blocks SET status = 'confirmed', hash = ?2, reward = ?3, fee = ?4 WHERE nonce = ?1",
			params![nonce, hash, reward.to_string(), fee.to_string()],
		)?;
		tx.commit()
	}

	/// The block left the chain: it pays nothing.
	pub fn orphan_block(&self, nonce: &str) -> Result<()> {
		self.conn.lock().execute("UPDATE blocks SET status = 'orphaned' WHERE nonce = ?1", [nonce])?;
		Ok(())
	}

	fn add_balance(tx: &rusqlite::Transaction<'_>, address: &str, delta: i128) -> Result<()> {
		let (balance, paid): (u128, u128) = tx
			.query_row("SELECT balance, paid FROM balances WHERE address = ?1", [address], |r| Ok((amount(r.get(0)?), amount(r.get(1)?))))
			.optional()?
			.unwrap_or((0, 0));
		let balance = (balance as i128 + delta).max(0) as u128;
		tx.execute(
			"INSERT OR REPLACE INTO balances (address, balance, paid) VALUES (?1, ?2, ?3)",
			params![address, balance.to_string(), paid.to_string()],
		)?;
		Ok(())
	}

	/// Balances of at least `minimum`, to pay.
	pub fn balances_due(&self, minimum: u128) -> Result<Vec<(String, u128)>> {
		let conn = self.conn.lock();
		let mut statement = conn.prepare("SELECT address, balance FROM balances ORDER BY address")?;
		let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, amount(r.get(1)?))))?;
		Ok(rows.collect::<Result<Vec<_>>>()?.into_iter().filter(|(_, b)| *b >= minimum).collect())
	}

	/// Take `amount` off the balance of `address` for a payment about to be sent.
	pub fn start_payment(&self, address: &str, amount: u128, now: i64) -> Result<i64> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		Self::add_balance(&tx, address, -(amount as i128))?;
		tx.execute(
			"INSERT INTO payments (address, amount, status, created_at) VALUES (?1, ?2, 'sending', ?3)",
			params![address, amount.to_string(), now],
		)?;
		let id = tx.last_insert_rowid();
		tx.commit()?;
		Ok(id)
	}

	pub fn payment_sent(&self, id: i64, tx_hash: &str, block_hash: &str) -> Result<()> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		let (address, sent): (String, u128) =
			tx.query_row("SELECT address, amount FROM payments WHERE id = ?1", [id], |r| Ok((r.get(0)?, amount(r.get(1)?))))?;
		tx.execute(
			"UPDATE payments SET status = 'confirmed', tx_hash = ?2, block_hash = ?3 WHERE id = ?1",
			params![id, tx_hash, block_hash],
		)?;
		let paid: u128 = tx.query_row("SELECT paid FROM balances WHERE address = ?1", [&address], |r| Ok(amount(r.get(0)?)))?;
		tx.execute("UPDATE balances SET paid = ?2 WHERE address = ?1", params![address, (paid + sent).to_string()])?;
		tx.commit()
	}

	/// The payment did not happen: its amount returns to the balance.
	pub fn payment_failed(&self, id: i64, error: &str) -> Result<()> {
		let mut conn = self.conn.lock();
		let tx = conn.transaction()?;
		let (address, sent): (String, u128) =
			tx.query_row("SELECT address, amount FROM payments WHERE id = ?1", [id], |r| Ok((r.get(0)?, amount(r.get(1)?))))?;
		tx.execute("UPDATE payments SET status = 'failed', error = ?2 WHERE id = ?1", params![id, error])?;
		Self::add_balance(&tx, &address, sent as i128)?;
		tx.commit()
	}

	/// Whether the payment happened is not known, as after an interruption: the amount stays off
	/// the balance, so that it is never paid twice, until the operator checks the chain.
	pub fn payment_unknown(&self, id: i64, error: &str) -> Result<()> {
		self.conn.lock().execute("UPDATE payments SET status = 'unknown', error = ?2 WHERE id = ?1", params![id, error])?;
		Ok(())
	}

	/// At start: payments interrupted while sending.
	pub fn interrupted_payments(&self) -> Result<usize> {
		Ok(self.conn.lock().execute(
			"UPDATE payments SET status = 'unknown', error = 'interrupted while sending' WHERE status = 'sending'",
			[],
		)?)
	}

	/// Delete shares older than `before` that no PPLNS window can still use: those past the last
	/// `keep` units of difficulty.
	pub fn prune_shares(&self, keep: u128, before: i64) -> Result<usize> {
		let conn = self.conn.lock();
		let mut statement = conn.prepare("SELECT id, difficulty FROM shares ORDER BY id DESC")?;
		let mut rows = statement.query([])?;
		let mut total = 0u128;
		let mut boundary = None;
		while let Some(row) = rows.next()? {
			total += row.get::<_, i64>(1)?.max(0) as u128;
			if total >= keep {
				boundary = Some(row.get::<_, i64>(0)?);
				break;
			}
		}
		drop(rows);
		drop(statement);
		match boundary {
			Some(id) => Ok(conn.execute("DELETE FROM shares WHERE id < ?1 AND created_at < ?2", params![id, before])?),
			None => Ok(0),
		}
	}

	fn blocks_where(&self, condition: &str, limit: u32) -> Result<Vec<BlockRow>> {
		let conn = self.conn.lock();
		let mut statement = conn.prepare(&format!(
			"SELECT height, nonce, hash, finder, worker, network_difficulty, effort, found_at, status, reward FROM blocks WHERE {condition} ORDER BY height DESC, found_at DESC LIMIT {limit}"
		))?;
		let rows = statement.query_map([], |r| {
			Ok(BlockRow {
				height: r.get::<_, i64>(0)? as u64,
				nonce: r.get(1)?,
				hash: r.get(2)?,
				finder: r.get(3)?,
				worker: r.get(4)?,
				network_difficulty: r.get(5)?,
				effort: r.get(6)?,
				found_at: r.get(7)?,
				status: r.get(8)?,
				reward: r.get(9)?,
			})
		})?;
		rows.collect()
	}

	pub fn blocks(&self, limit: u32) -> Result<Vec<BlockRow>> {
		self.blocks_where("1", limit)
	}

	pub fn payments(&self, address: Option<&str>, limit: u32) -> Result<Vec<PaymentRow>> {
		let conn = self.conn.lock();
		let mut statement = conn.prepare(
			"SELECT id, address, amount, status, tx_hash, block_hash, created_at FROM payments WHERE ?1 IS NULL OR address = ?1 ORDER BY id DESC LIMIT ?2",
		)?;
		let rows = statement.query_map(params![address, limit], |r| {
			Ok(PaymentRow {
				id: r.get(0)?,
				address: r.get(1)?,
				amount: r.get(2)?,
				status: r.get(3)?,
				tx_hash: r.get(4)?,
				block_hash: r.get(5)?,
				created_at: r.get(6)?,
			})
		})?;
		rows.collect()
	}

	/// What the pool owes `address`, and an estimate of its share of the blocks not final yet
	/// (`fee_bps` taken, the reward of a pending block taken as `expected_reward`).
	pub fn account(&self, address: &str, fee_bps: u32, expected_reward: u128) -> Result<Account> {
		let conn = self.conn.lock();
		let (balance, paid) = conn
			.query_row("SELECT balance, paid FROM balances WHERE address = ?1", [address], |r| Ok((amount(r.get(0)?), amount(r.get(1)?))))
			.optional()?
			.unwrap_or((0, 0));
		let mut statement = conn.prepare(
			"SELECT bs.weight, (SELECT group_concat(weight, ',') FROM block_shares WHERE block = b.nonce) FROM blocks b JOIN block_shares bs ON bs.block = b.nonce WHERE b.status = 'pending' AND bs.address = ?1",
		)?;
		let rows = statement.query_map([address], |r| Ok((amount(r.get(0)?), r.get::<_, Option<String>>(1)?)))?;
		let mut immature = 0u128;
		let distributable = expected_reward - expected_reward * u128::from(fee_bps) / 10_000;
		for row in rows {
			let (weight, all) = row?;
			let total: u128 = all.unwrap_or_default().split(',').filter_map(|w| w.parse::<u128>().ok()).sum();
			if total > 0 {
				immature += distributable * weight / total;
			}
		}
		Ok(Account { balance: balance.to_string(), paid: paid.to_string(), immature: immature.to_string() })
	}

	/// Blocks found, and when the last one was.
	pub fn block_counts(&self) -> Result<(u64, Option<i64>)> {
		self.conn.lock().query_row(
			"SELECT COUNT(*), MAX(found_at) FROM blocks WHERE status != 'orphaned'",
			[],
			|r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)),
		)
	}

	/// Reward of the last block credited, for the estimates of the pending ones.
	pub fn last_reward(&self) -> Result<Option<u128>> {
		Ok(self
			.conn
			.lock()
			.query_row("SELECT reward FROM blocks WHERE status = 'confirmed' ORDER BY height DESC LIMIT 1", [], |r| r.get::<_, Option<String>>(0))
			.optional()?
			.flatten()
			.and_then(|r| r.parse().ok()))
	}

	/// Difficulty of the shares of the current round, since the last block found.
	pub fn round_difficulty(&self) -> Result<u128> {
		let conn = self.conn.lock();
		let start: i64 = conn
			.query_row("SELECT value FROM meta WHERE key = 'round_start'", [], |r| r.get::<_, String>(0))
			.optional()?
			.and_then(|v| v.parse().ok())
			.unwrap_or(0);
		conn.query_row("SELECT COALESCE(SUM(difficulty), 0) FROM shares WHERE id > ?1", [start], |r| Ok(r.get::<_, i64>(0)? as u128))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn share(address: &str, difficulty: u64) -> Share {
		Share { address: address.into(), worker: "rig".into(), difficulty, created_at: 0 }
	}

	fn found(nonce: &str, network_difficulty: u128) -> Found {
		Found { height: 10, nonce: nonce.into(), finder: "a".into(), worker: "rig".into(), network_difficulty, found_at: 1 }
	}

	fn ledger() -> (tempfile::TempDir, Ledger) {
		let dir = tempfile::tempdir().unwrap();
		let ledger = Ledger::open(&dir.path().join("pool.sqlite")).unwrap();
		(dir, ledger)
	}

	#[test]
	fn pplns_splits_the_last_window() {
		let (_dir, ledger) = ledger();
		// Oldest first: c falls out of the window, b is cut to fit it.
		ledger.add_shares(&[share("c", 500), share("b", 400), share("a", 300), share("a", 300)]).unwrap();
		ledger.record_block(&found("n1", 400), 800).unwrap();
		ledger.confirm_block("n1", "0xh1", 1_000_000, 100).unwrap();
		// Fee 1%: 10 000. a: 600/800 of 990 000, b: 200/800.
		assert_eq!(ledger.balances_due(1).unwrap(), vec![("a".into(), 742_500), ("b".into(), 247_500)]);
		let block = &ledger.blocks(10).unwrap()[0];
		assert_eq!(block.status, "confirmed");
		assert_eq!(block.reward.as_deref(), Some("1000000"));
		// The round had 1500 of difficulty for a block of 400.
		assert_eq!(block.effort, 375.0);
	}

	#[test]
	fn the_rounding_remainder_goes_to_the_fee() {
		let (_dir, ledger) = ledger();
		ledger.add_shares(&[share("a", 1), share("b", 1), share("c", 1)]).unwrap();
		ledger.record_block(&found("n", 3), 3).unwrap();
		ledger.confirm_block("n", "0xh", 100, 0).unwrap();
		let paid: u128 = ledger.balances_due(1).unwrap().iter().map(|(_, b)| b).sum();
		assert_eq!(paid, 99);
		let conn = ledger.conn.lock();
		let fee: String = conn.query_row("SELECT fee FROM blocks", [], |r| r.get(0)).unwrap();
		assert_eq!(fee, "1");
	}

	#[test]
	fn orphaned_blocks_pay_nothing_and_rounds_restart() {
		let (_dir, ledger) = ledger();
		ledger.add_shares(&[share("a", 10)]).unwrap();
		ledger.record_block(&found("n", 100), 1000).unwrap();
		assert_eq!(ledger.round_difficulty().unwrap(), 0);
		ledger.add_shares(&[share("a", 7)]).unwrap();
		assert_eq!(ledger.round_difficulty().unwrap(), 7);
		ledger.orphan_block("n").unwrap();
		assert!(ledger.pending_blocks().unwrap().is_empty());
		assert!(ledger.balances_due(1).unwrap().is_empty());
	}

	#[test]
	fn payments_move_balances_once() {
		let (_dir, ledger) = ledger();
		ledger.add_shares(&[share("a", 1)]).unwrap();
		ledger.record_block(&found("n", 1), 1).unwrap();
		assert_eq!(ledger.account("a", 0, 1000).unwrap().immature, "1000");
		ledger.confirm_block("n", "0xh", 5000, 0).unwrap();

		let failed = ledger.start_payment("a", 2000, 1).unwrap();
		assert_eq!(ledger.account("a", 0, 0).unwrap().balance, "3000");
		ledger.payment_failed(failed, "dropped").unwrap();
		assert_eq!(ledger.account("a", 0, 0).unwrap().balance, "5000");

		let sent = ledger.start_payment("a", 5000, 2).unwrap();
		ledger.payment_sent(sent, "0xtx", "0xb").unwrap();
		let account = ledger.account("a", 0, 0).unwrap();
		assert_eq!((account.balance.as_str(), account.paid.as_str()), ("0", "5000"));

		// Interrupted: never paid twice.
		ledger.add_shares(&[share("a", 1)]).unwrap();
		ledger.record_block(&found("m", 1), 1).unwrap();
		ledger.confirm_block("m", "0xh2", 300, 0).unwrap();
		ledger.start_payment("a", 300, 3).unwrap();
		assert_eq!(ledger.interrupted_payments().unwrap(), 1);
		assert_eq!(ledger.account("a", 0, 0).unwrap().balance, "0");
		let statuses: Vec<String> = ledger.payments(Some("a"), 10).unwrap().into_iter().map(|p| p.status).collect();
		assert_eq!(statuses, ["unknown", "confirmed", "failed"]);
	}

	#[test]
	fn pruning_keeps_the_window() {
		let (_dir, ledger) = ledger();
		ledger.add_shares(&[share("a", 5), share("a", 5), share("a", 5), share("a", 5)]).unwrap();
		assert_eq!(ledger.prune_shares(10, 1).unwrap(), 2);
		ledger.record_block(&found("n", 10), 100).unwrap();
		ledger.confirm_block("n", "0xh", 100, 0).unwrap();
		assert_eq!(ledger.balances_due(1).unwrap(), vec![("a".into(), 100)]);
	}
}
