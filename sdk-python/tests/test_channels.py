import json
from pathlib import Path

import pytest

from tesserax import Amount, InvalidInput, Keypair, PaymentRequired, Voucher, voucher_message

VECTOR = json.loads((Path(__file__).parents[2] / "sdk" / "vectors" / "voucher.json").read_text())


def test_vouchers_match_the_rust_sdk():
    genesis = bytes.fromhex(VECTOR["genesis"][2:])
    message = voucher_message(genesis, VECTOR["channel"], Amount.from_tess(VECTOR["amount"]))
    assert "0x" + message.hex() == VECTOR["message"]
    voucher = Voucher.from_header(VECTOR["header"])
    signer = Keypair.from_uri(VECTOR["signer"])
    assert voucher.verify(genesis, signer.public_key)
    assert not voucher.verify(bytes(32), signer.public_key)


def test_vouchers_roundtrip_and_bind_the_amount():
    key = Keypair.from_uri("//Alice")
    genesis = bytes([3]) * 32
    voucher = Voucher.sign(key, genesis, 12, Amount.from_tess("0.037"))
    parsed = Voucher.from_header(voucher.to_header())
    assert parsed.verify(genesis, key.public_key)
    assert not Voucher(parsed.channel, Amount.from_tess("0.038"), parsed.signature).verify(genesis, key.public_key)
    with pytest.raises(InvalidInput):
        Voucher.from_header('{"channel":-1,"amount":"1","signature":""}')


def test_payment_requirements_roundtrip():
    header = PaymentRequired("te1", Amount.from_tess("0.001"), "0x00", accepted=Amount.from_tess("0.5")).to_header()
    parsed = PaymentRequired.from_header(header)
    assert str(parsed.price) == "0.001" and str(parsed.accepted) == "0.5" and parsed.error is None
