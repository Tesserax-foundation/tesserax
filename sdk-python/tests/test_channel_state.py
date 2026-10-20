"""State files of the channel payer and receiver, against a chain simulated in memory."""

import json
import os
import stat

import pytest

from tesserax import (
    Amount,
    ChannelInfo,
    ChannelPayer,
    ChannelReceiver,
    Keypair,
    PaymentRejected,
    PaymentRequired,
    StorageError,
)
from tesserax import channels as channels_module

PAYER = Keypair.from_uri("//Alice")
SERVICE = Keypair.from_uri("//Bob")
PRICE = Amount.from_tess("0.1")


def tess(text: str) -> Amount:
    return Amount.from_tess(text)


class FakeChain:
    """The channel calls of `Client`, on a chain held in memory."""

    def __init__(self, genesis: bytes = bytes([7]) * 32):
        self.genesis = genesis
        self.channels: dict[int, ChannelInfo] = {}
        self.opened = 0
        self.claims = 0

    def genesis_hash(self) -> bytes:
        return self.genesis

    def channel(self, channel_id: int):
        return self.channels.get(channel_id)

    def open_channel(self, payer: Keypair, payee: str, deposit: Amount) -> int:
        channel_id = len(self.channels)
        self.channels[channel_id] = ChannelInfo(
            channel_id, payer.address, payee, payer.address, payer.public_key, deposit, Amount(0), None
        )
        self.opened += 1
        return channel_id

    def top_up_channel(self, signer: Keypair, channel_id: int, amount: Amount, owner=None):
        self.channels[channel_id].deposit = self.channels[channel_id].deposit + amount

    def claim_channel(self, payee: Keypair, voucher):
        channel = self.channels[voucher.channel]
        assert voucher.verify(self.genesis, channel.signer_public)
        assert channel.claimed < voucher.amount, "claimed twice"
        channel.claimed = voucher.amount
        self.claims += 1
        return "claimed"


def pay(payer: ChannelPayer, receiver: ChannelReceiver):
    return payer.pay(PaymentRequired.from_header(receiver.payment_required(PRICE)))


def test_a_payer_resumes_its_channels_after_a_restart(tmp_path):
    chain, path = FakeChain(), tmp_path / "payer.json"
    receiver = ChannelReceiver(chain, SERVICE)
    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"), state_file=path)
    pay(payer, receiver)
    second = pay(payer, receiver)

    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"), state_file=path)
    third = pay(payer, receiver)
    assert chain.opened == 1
    assert third.channel == second.channel
    assert third.amount == tess("0.3")

    state = json.loads(path.read_text())
    assert state["kind"] == "payer" and state["version"] == 1
    assert state["channels"][SERVICE.address] == {"id": third.channel, "deposit": "2", "spent": "0.3"}
    if os.name == "posix":
        assert stat.S_IMODE(path.stat().st_mode) == 0o600


def test_a_payer_records_a_voucher_before_returning_it(tmp_path, monkeypatch):
    chain, path = FakeChain(), tmp_path / "payer.json"
    receiver = ChannelReceiver(chain, SERVICE)
    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"), state_file=path)
    pay(payer, receiver)

    def fail(*_):
        raise StorageError("disk full")

    monkeypatch.setattr(channels_module._store, "save", fail)
    with pytest.raises(StorageError):
        pay(payer, receiver)
    assert payer.channels[SERVICE.address].spent == tess("0.1")


def test_a_service_claims_after_a_restart_and_refuses_replays(tmp_path):
    chain, path = FakeChain(), tmp_path / "service.json"
    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"))
    receiver = ChannelReceiver(chain, SERVICE, state_file=path)
    first = pay(payer, receiver)
    assert receiver.verify(first.to_header(), PRICE) == tess("0.1")
    second = pay(payer, receiver)
    assert receiver.verify(second.to_header(), PRICE) == tess("0.2")

    receiver = ChannelReceiver(chain, SERVICE, state_file=path)
    assert receiver.unclaimed() == tess("0.2")
    with pytest.raises(PaymentRejected) as rejected:
        receiver.verify(second.to_header(), PRICE)
    assert rejected.value.rejection.reason == "the voucher does not pay the price"
    assert rejected.value.rejection.accepted == tess("0.2")

    third = pay(payer, receiver)
    assert receiver.verify(third.to_header(), PRICE) == tess("0.3")
    assert receiver.claim_all() == ["claimed"]
    assert chain.channels[third.channel].claimed == tess("0.3")

    receiver = ChannelReceiver(chain, SERVICE, state_file=path)
    assert receiver.unclaimed() == Amount(0)
    assert receiver.claim_all() == []


def test_a_claim_interrupted_by_a_crash_is_not_repeated(tmp_path):
    chain, path = FakeChain(), tmp_path / "service.json"
    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"))
    receiver = ChannelReceiver(chain, SERVICE, state_file=path)
    voucher = pay(payer, receiver)
    receiver.verify(voucher.to_header(), PRICE)
    # The claim went through, then the service stopped before recording it.
    chain.claim_channel(SERVICE, voucher)

    receiver = ChannelReceiver(chain, SERVICE, state_file=path)
    assert receiver.claim_all() == []
    assert chain.claims == 1
    assert receiver.unclaimed() == Amount(0)


def test_vouchers_of_closed_channels_are_dropped(tmp_path):
    chain, path = FakeChain(), tmp_path / "service.json"
    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"))
    receiver = ChannelReceiver(chain, SERVICE, state_file=path)
    voucher = pay(payer, receiver)
    receiver.verify(voucher.to_header(), PRICE)
    del chain.channels[voucher.channel]
    assert receiver.claim_all() == []
    assert json.loads(path.read_text())["channels"] == {}


def test_state_files_belong_to_one_chain_and_key(tmp_path):
    chain, path = FakeChain(), tmp_path / "state.json"
    payer = ChannelPayer(chain, PAYER, tess("2"), tess("1"), state_file=path)
    pay(payer, ChannelReceiver(chain, SERVICE))

    with pytest.raises(StorageError, match="another key"):
        ChannelPayer(chain, SERVICE, tess("2"), tess("1"), state_file=path)
    with pytest.raises(StorageError, match="another owner"):
        ChannelPayer(chain, PAYER, tess("2"), tess("1"), owner=SERVICE.address, state_file=path)
    with pytest.raises(StorageError, match="another chain"):
        ChannelPayer(FakeChain(bytes(32)), PAYER, tess("2"), tess("1"), state_file=path)
    with pytest.raises(StorageError, match="not a receiver state file"):
        ChannelReceiver(chain, SERVICE, state_file=path)

    path.write_text("{not json")
    with pytest.raises(StorageError, match="not a valid state file"):
        ChannelPayer(chain, PAYER, tess("2"), tess("1"), state_file=path)
