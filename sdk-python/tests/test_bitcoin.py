"""The Bitcoin side of swaps. The transactions built here are checked by Bitcoin Core in the tests
of the Rust SDK (`sdk/src/swaps/testdata/bitcoin-py.json`); WRITE_VECTORS=1 writes them again.
"""

import hashlib
import json
import os
from pathlib import Path

import pytest

pytest.importorskip("embit")

from embit import ec  # noqa: E402
from embit.transaction import Transaction  # noqa: E402

from tesserax import Keypair  # noqa: E402
from tesserax.bitcoin import (  # noqa: E402
    ANNOUNCEMENT_LEN,
    ANNOUNCEMENT_WITH_REFUND_LEN,
    NOTICE_SATS,
    Announcement,
    Htlc,
    Utxo,
    address_of_key,
    claim_transaction,
    find_lock,
    lock_transaction,
    outpoint_bytes,
    refund_key,
    refund_transaction,
)

ROOT = Path(__file__).resolve().parents[2]
TS_VECTORS = ROOT / "sdk/src/swaps/testdata/bitcoin-ts.json"
PY_VECTORS = ROOT / "sdk/src/swaps/testdata/bitcoin-py.json"


def key(byte: int) -> ec.PrivateKey:
    return ec.PrivateKey(bytes([byte]) * 32)


def test_announcements_round_trip():
    taker = Keypair.from_uri("//Alice").address
    plain = Announcement(offer=2**64 - 2, taker=taker, hash=bytes([6]) * 32, timelock=1_900_000_000)
    assert len(plain.encode()) == ANNOUNCEMENT_LEN and plain.encode()[:2] == b"TX"
    assert Announcement.decode(plain.encode()) == plain
    with_refund = Announcement(4, taker, bytes([6]) * 32, 1_900_000_000, key(4).get_public_key().sec())
    assert len(with_refund.encode()) == ANNOUNCEMENT_WITH_REFUND_LEN and with_refund.encode()[:2] == b"TR"
    assert Announcement.decode(with_refund.encode()) == with_refund
    assert Announcement.decode(plain.encode()[1:]) is None


def test_the_payments_of_the_typescript_sdk_are_read_the_same():
    v = json.loads(TS_VECTORS.read_text())
    lock = find_lock(Transaction.from_string(v["lock"]), bytes.fromhex(v["maker"]))
    assert (lock.offer, lock.vout, lock.amount) == (4, 0, 40_000)
    assert lock.htlc.script().hex() == v["script"]
    # The refund key the web wallet derives from the seed of an account.
    refund = refund_key(bytes([9]) * 32, hashlib.sha256(bytes([7]) * 32).digest())
    assert refund.get_public_key().sec().hex() == v["refund"]
    assert len(outpoint_bytes(lock.txid, lock.vout)) == 36


def test_locks_claims_and_refunds():
    maker, payer = key(1), key(5)
    secret = bytes([7]) * 32
    hash_ = hashlib.sha256(secret).digest()
    refund = refund_key(bytes([9]) * 32, hash_)
    htlc = Htlc(hash_, maker.get_public_key().sec(), refund.get_public_key().sec(), 1_800_000_000)
    taker = Keypair.from_uri("//Alice").address
    announcement = Announcement(4, taker, hash_, htlc.timelock, htlc.taker)
    funding = Utxo("11" * 32, 1, 100_000)

    with pytest.raises(Exception):
        lock_transaction(payer, [funding], htlc, Announcement(4, taker, hash_, htlc.timelock), 40_000, 2, "regtest")
    lock_tx = lock_transaction(payer, [funding], htlc, announcement, 40_000, 2, "regtest")
    assert lock_tx.vout[1].value == NOTICE_SATS
    lock = find_lock(lock_tx, htlc.maker)
    assert lock and lock.htlc == htlc and lock.taker == taker and lock.amount == 40_000
    assert find_lock(lock_tx, key(3).get_public_key().sec()) is None

    claim = claim_transaction(lock, secret, maker, 3)
    payer_address = address_of_key(payer.get_public_key().sec(), "regtest")
    assert payer_address.startswith("bcrt1q")
    back = refund_transaction(lock, refund, payer_address, 3, "regtest")
    assert back.locktime == htlc.timelock

    vectors = {
        "maker": htlc.maker.hex(),
        "payer_script": "0014" + hashlib.new("ripemd160", hashlib.sha256(payer.get_public_key().sec()).digest()).hexdigest(),
        "funding_amount": funding.amount,
        "lock": lock_tx.serialize().hex(),
        "claim": claim.serialize().hex(),
        "refund": back.serialize().hex(),
    }
    if os.environ.get("WRITE_VECTORS"):
        PY_VECTORS.write_text(json.dumps(vectors, indent=2) + "\n")
    assert json.loads(PY_VECTORS.read_text()) == vectors
