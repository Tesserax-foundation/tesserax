import json
from pathlib import Path

import pytest

from tesserax import Amount, InvalidAddress, InvalidAmount, InvalidKey, Keypair, format_address, parse_address, verify

VECTORS = json.loads((Path(__file__).parents[2] / "sdk" / "vectors" / "keys.json").read_text())["vectors"]


@pytest.mark.parametrize("vector", VECTORS, ids=[v["uri"][:40] for v in VECTORS])
def test_keys_match_the_node(vector):
    key = Keypair.from_uri(vector["uri"])
    assert "0x" + key.public_key.hex() == vector["public_key"]
    assert "0x" + key.account_id.hex() == vector["account_id"]
    assert key.address == vector["address"]


def test_generated_keys_roundtrip():
    key, phrase = Keypair.generate()
    assert len(phrase.split()) == 24
    assert Keypair.from_uri(phrase).address == key.address
    assert Keypair.from_uri(phrase + "//agent").address != key.address


def test_invalid_keys():
    for uri in ["not a phrase", "//Alice/soft", "0x1234"]:
        with pytest.raises(InvalidKey):
            Keypair.from_uri(uri)


def test_signatures():
    key = Keypair.from_uri("//Alice")
    signature = key.sign(b"message")
    assert len(signature) == 4627
    assert verify(key.public_key, b"message", signature)
    assert not verify(key.public_key, b"other", signature)
    assert not verify(key.public_key, b"message", signature, context=b"other")
    # Hedged: two signatures of the same message differ.
    assert key.sign(b"message") != signature


def test_addresses():
    key = Keypair.from_uri("//Alice")
    assert parse_address(key.address) == key.account_id
    assert parse_address("0x" + key.account_id.hex()) == key.account_id
    assert format_address(key.account_id) == key.address
    polkadot = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5"
    for bad in [polkadot, "0x1234", "garbage"]:
        with pytest.raises(InvalidAddress):
            parse_address(bad)


def test_amounts():
    assert Amount.from_tess("1").planck == 10**12
    assert Amount.from_tess("1.5").planck == 1_500_000_000_000
    assert Amount.from_tess("0.000000000001").planck == 1
    assert str(Amount(1_500_000_000_000)) == "1.5"
    assert str(Amount(14_000_000 * 10**12)) == "14000000"
    assert str(Amount(1)) == "0.000000000001"
    for bad in ["", "-1", "1.0000000000001", "abc", 1.5]:
        with pytest.raises(InvalidAmount):
            Amount.from_tess(bad)
