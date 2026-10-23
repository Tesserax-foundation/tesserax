"""Smart contracts. The chain test deploys sdk/fixtures/contracts/Token.sol:

    tesserax --dev --tmp --mine --author <address>
    TESSERAX_URL=ws://127.0.0.1:9944 pytest -m chain
"""

import json
import os
import threading
from pathlib import Path

import pytest

from tesserax import Client, ContractReverted, Keypair, revert_reason, to_h160
from tesserax.contracts import _compact

URL = os.environ.get("TESSERAX_URL")
FIXTURES = Path(__file__).parents[2] / "sdk" / "fixtures" / "contracts"
ABI = json.loads((FIXTURES / "Token.abi.json").read_text())


def test_to_h160_maps_ethereum_derived_accounts_back():
    derived = bytes(range(1, 21)) + b"\xee" * 12
    assert to_h160(derived).lower() == "0x" + bytes(range(1, 21)).hex()


def test_compact_encoding():
    assert _compact(0) == b"\x00"
    assert _compact(63) == b"\xfc"
    assert _compact(64) == b"\x01\x01"
    assert _compact(16383) == b"\xfd\xff"
    assert _compact(16384) == b"\x02\x00\x01\x00"
    assert _compact(1 << 30) == b"\x03\x00\x00\x00\x40"


def test_revert_reasons():
    error = bytes.fromhex(
        "08c379a0"
        "0000000000000000000000000000000000000000000000000000000000000020"
        "0000000000000000000000000000000000000000000000000000000000000014"
        "6275726e20657863656564732062616c616e6365000000000000000000000000"
    )
    assert revert_reason(ABI, error) == "burn exceeds balance"
    panic = bytes.fromhex("4e487b71" + "11".rjust(64, "0"))
    assert revert_reason(None, panic) == "panic 0x11 (arithmetic overflow or underflow)"
    assert revert_reason(None, b"") == "reverted without a reason"


@pytest.mark.chain
@pytest.mark.skipif(not URL, reason="set TESSERAX_URL to a development node")
def test_token_on_a_development_chain():
    with Client(URL) as client:
        alice, bob = Keypair.from_uri("//Alice"), Keypair.from_uri("//Bob")
        mapped = client.substrate.rpc_request(
            "state_call", ["ReviveApi_address", "0x" + alice.account_id.hex(), client.substrate.get_chain_head()]
        )["result"]
        assert to_h160(alice.address).lower() == mapped

        code = (FIXTURES / "Token.polkavm").read_bytes()
        token = client.deploy_contract(alice, code, ABI, args=["Tesserax Coin", 1_000_000])
        minted = token.deployment.contract_events[0]
        assert minted.name == "Transfer"
        assert minted.args["to"] == to_h160(alice.address)
        assert minted.args["value"] == 1_000_000

        assert token.read("name") == "Tesserax Coin"
        assert token.read("balanceOf", [to_h160(alice.address)]) == 1_000_000

        watched = []
        watcher = threading.Thread(
            target=lambda: watched.append(next(Client(URL).contract(token.address, ABI).watch_events(poll_seconds=1))),
            daemon=True,
        )
        watcher.start()
        sent = token.write(alice, "transfer", [to_h160(bob.address), 250])
        assert sent.contract_events[0].args == {"from": to_h160(alice.address), "to": to_h160(bob.address), "value": 250}
        assert token.read("balanceOf", [to_h160(bob.address)]) == 250
        watcher.join(timeout=180)
        assert watched and watched[0].name == "Transfer" and watched[0].block_number == sent.block_number

        with pytest.raises(ContractReverted) as reverted:
            token.write(bob, "transfer", [to_h160(alice.address), 1_000])
        assert reverted.value.reason == "InsufficientBalance(available: 250, required: 1000)"
        with pytest.raises(ContractReverted) as reverted:
            token.write(bob, "burn", [1_000])
        assert reverted.value.reason == "burn exceeds balance"

        events = token.events(token.deployment.block_number, sent.block_number)
        assert [e.args["value"] for e in events if e.name == "Transfer"] == [1_000_000, 250]

        assert client.contract(token.address, ABI).read("totalSupply") == 1_000_000
