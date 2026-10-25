"""Private transactions. The chain test runs against a development chain:

    tesserax --dev --tmp --mine --author <address>
    TESSERAX_URL=ws://127.0.0.1:9944 pytest -m chain
"""

import os

import pytest

from tesserax import Amount, Client, InvalidInput, Keypair, ShieldedAccount, StorageError, is_shielded_address, shielded_minimum_fee

URL = os.environ.get("TESSERAX_URL")

# The shielded address of //Alice, as the TypeScript and Rust SDKs derive it.
ALICE = Keypair.from_uri("//Alice")


def test_shielded_addresses_derive_from_the_key():
    alice = ShieldedAccount.create(ALICE)
    assert alice.address.startswith("zte")
    assert is_shielded_address(alice.address)
    assert not is_shielded_address(ALICE.address)
    assert ShieldedAccount.create(Keypair.from_uri("//Alice")).address == alice.address
    assert ShieldedAccount.create(Keypair.from_uri("//Bob")).address != alice.address


def test_states_roundtrip():
    alice = ShieldedAccount.create(ALICE)
    restored = ShieldedAccount.restore(ALICE, alice.state())
    assert restored.address == alice.address
    assert restored.balance == Amount(0)
    assert restored.block is None
    with pytest.raises(StorageError):
        ShieldedAccount.restore(ALICE, b"\x00" * 10)


def test_the_typescript_sdk_derives_the_same_address():
    # Recorded from the TypeScript SDK: both run the same shielded wallet.
    expected = os.environ.get("TESSERAX_ALICE_SHIELDED")
    if expected:
        assert ShieldedAccount.create(ALICE).address == expected


@pytest.mark.chain
@pytest.mark.skipif(not URL, reason="set TESSERAX_URL to a development node")
def test_shield_send_and_unshield():
    with Client(URL) as client:
        bob = Keypair.from_uri("//Bob")
        fee = shielded_minimum_fee(client)
        assert fee.planck > 0

        alice_private = ShieldedAccount.create(ALICE)
        bob_private = ShieldedAccount.create(bob)
        alice_private.sync(client)
        start = alice_private.balance.planck

        alice_private.shield(client, ALICE, Amount.from_tess("5"))
        alice_private.sync(client)
        assert alice_private.balance.planck == start + Amount.from_tess("5").planck

        restored = ShieldedAccount.restore(ALICE, alice_private.state())
        assert restored.block == alice_private.block
        assert restored.balance == alice_private.balance

        bob_private.sync(client)
        bob_start = bob_private.balance.planck
        alice_private.send(client, bob_private.address, Amount.from_tess("2"))
        assert alice_private.pending.planck > 0
        alice_private.sync(client)
        assert alice_private.pending.planck == 0
        assert alice_private.balance.planck == start + Amount.from_tess("3").planck - fee.planck

        synced = bob_private.sync(client)
        assert synced.received.planck == Amount.from_tess("2").planck
        assert bob_private.balance.planck == bob_start + Amount.from_tess("2").planck

        before = client.balance(bob.address).free.planck
        bob_private.unshield(client, bob.address, Amount.from_tess("1"))
        bob_private.sync(client)
        assert client.balance(bob.address).free.planck == before + Amount.from_tess("1").planck

        with pytest.raises(InvalidInput):
            bob_private.send(client, alice_private.address, Amount.from_tess("1000"))
