"""Micropayments: payment channels, vouchers and the HTTP 402 protocol. See
`docs/micropayments.md`."""

from __future__ import annotations

import base64
import json
import os
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Optional

from . import _store
from .amount import Amount
from .errors import InvalidInput, StorageError, TesseraxError
from .keys import Keypair, account_id, format_address, parse_address, verify

if TYPE_CHECKING:
    from .client import Client, TxOutcome

#: FIPS 204 context of voucher signatures.
VOUCHER_CONTEXT = b"tesserax-channel"
#: Response header of a service asking for payment, with status 402.
PAYMENT_HEADER = "Tesserax-Payment"
#: Request header carrying a voucher.
VOUCHER_HEADER = "Tesserax-Voucher"
#: Blocks before a channel closes under which a service stops accepting its vouchers.
CLOSE_SAFETY_BLOCKS = 100

_REFRESH_SECONDS = 60
#: Version of the channel state files.
_STATE_VERSION = 1


def voucher_message(genesis: bytes, channel: int, amount: Amount) -> bytes:
    """The message a voucher signs: SCALE encoding of `(genesis_hash, channel, amount)`."""
    return genesis + channel.to_bytes(8, "little") + amount.planck.to_bytes(16, "little")


@dataclass
class Voucher:
    """A voucher: the payer owes `amount` in total on `channel`."""

    channel: int
    amount: Amount
    signature: bytes

    @classmethod
    def sign(cls, key: Keypair, genesis: bytes, channel: int, amount: Amount) -> "Voucher":
        return cls(channel, amount, key.sign(voucher_message(genesis, channel, amount), VOUCHER_CONTEXT))

    def verify(self, genesis: bytes, signer: bytes) -> bool:
        """Whether the voucher is signed by the ML-DSA-87 public key `signer`."""
        try:
            return verify(signer, voucher_message(genesis, self.channel, self.amount), self.signature, VOUCHER_CONTEXT)
        except Exception:
            return False

    def to_header(self) -> str:
        return json.dumps(
            {"channel": self.channel, "amount": str(self.amount), "signature": base64.b64encode(self.signature).decode()},
            separators=(",", ":"),
        )

    def to_json(self) -> dict:
        return json.loads(self.to_header())

    @classmethod
    def from_header(cls, value: str) -> "Voucher":
        try:
            v = json.loads(value)
            channel = v["channel"]
            if not isinstance(channel, int) or channel < 0 or not isinstance(v["amount"], str):
                raise ValueError
            return cls(channel, Amount.from_tess(v["amount"]), base64.b64decode(v["signature"], validate=True))
        except Exception:
            raise InvalidInput("malformed voucher") from None


@dataclass
class PaymentRequired:
    """What a service asks for, in the `Tesserax-Payment` header of a 402 response."""

    payee: str
    price: Amount
    genesis: str
    accepted: Optional[Amount] = None
    error: Optional[str] = None

    def to_header(self) -> str:
        value = {"payee": self.payee, "price": str(self.price), "genesis": self.genesis}
        if self.accepted is not None:
            value["accepted"] = str(self.accepted)
        if self.error is not None:
            value["error"] = self.error
        return json.dumps(value, separators=(",", ":"))

    @classmethod
    def from_header(cls, value: str) -> "PaymentRequired":
        try:
            r = json.loads(value)
            accepted = Amount.from_tess(r["accepted"]) if "accepted" in r else None
            return cls(r["payee"], Amount.from_tess(r["price"]), r["genesis"], accepted, r.get("error"))
        except Exception:
            raise InvalidInput("malformed payment requirement") from None


@dataclass
class ChannelInfo:
    id: int
    payer: str
    payee: str
    #: Account of the key signing the vouchers.
    signer: str
    signer_public: bytes = field(repr=False)
    deposit: Amount
    claimed: Amount
    #: Block from which the close can be finalized, if the payer requested it.
    closes_at: Optional[int]


@dataclass
class PayerChannel:
    id: int
    deposit: Amount
    #: Cumulative amount of the latest voucher signed.
    spent: Amount


def _check_state(state: dict, kind: str, path: str, genesis: bytes, expected: dict[str, tuple[Optional[str], str]]) -> None:
    """Refuse a state file of another kind, version or chain, or whose `field` differs from
    `value`, for each `field: (value, what)` of `expected`."""
    if not isinstance(state, dict) or state.get("version") != _STATE_VERSION or state.get("kind") != kind:
        raise StorageError(f"{path} is not a {kind} state file of version {_STATE_VERSION}")
    if state.get("genesis") != "0x" + genesis.hex():
        raise StorageError(f"{path} was written for another chain")
    for field_name, (value, what) in expected.items():
        if state.get(field_name) != value:
            raise StorageError(f"{path} was written for another {what}")


class ChannelPayer:
    """Pays services over payment channels, one per payee, opened and topped up on demand. With
    `owner`, channels are opened for the owner through the agent's delegation.

    With `state_file`, every channel opened, every top-up and every voucher is written to disk
    before the voucher is returned: after a restart the payer resumes its channels instead of
    opening new ones, and never signs a voucher below one it already gave. A state file must be
    used by one payer at a time; it is refused if written for another chain, key or owner."""

    def __init__(
        self,
        client: "Client",
        key: Keypair,
        deposit: Amount,
        max_price: Amount,
        owner: Optional[str] = None,
        state_file: Optional[str | os.PathLike] = None,
    ):
        self.client, self.key, self.deposit, self.max_price, self.owner = client, key, deposit, max_price, owner
        self.channels: dict[str, PayerChannel] = {}
        self._lock = threading.Lock()
        self._genesis: Optional[bytes] = None
        self._file = os.fspath(state_file) if state_file is not None else None
        self._owner_address = format_address(parse_address(owner)) if owner else None
        if self._file is not None:
            self._genesis = client.genesis_hash()
            state = _store.load(self._file)
            if state is not None:
                _check_state(
                    state,
                    "payer",
                    self._file,
                    self._genesis,
                    {"signer": (key.address, "key"), "owner": (self._owner_address, "owner")},
                )
                self.channels = {
                    payee: PayerChannel(c["id"], Amount.from_tess(c["deposit"]), Amount.from_tess(c["spent"]))
                    for payee, c in state["channels"].items()
                }

    def _persist(self) -> None:
        if self._file is None:
            return
        _store.save(
            self._file,
            {
                "version": _STATE_VERSION,
                "kind": "payer",
                "genesis": "0x" + self._genesis.hex(),
                "signer": self.key.address,
                "owner": self._owner_address,
                "channels": {
                    payee: {"id": c.id, "deposit": str(c.deposit), "spent": str(c.spent)}
                    for payee, c in self.channels.items()
                },
            },
        )

    def pay(self, requirement: PaymentRequired, max_price: Optional[Amount] = None) -> Voucher:
        """A voucher paying `requirement`, opening or topping up the channel if needed."""
        max_price = max_price or self.max_price
        with self._lock:  # One payment at a time: vouchers are cumulative.
            if self._genesis is None:
                self._genesis = self.client.genesis_hash()
            if requirement.genesis.lower() != "0x" + self._genesis.hex():
                raise InvalidInput("the service is on another chain")
            if max_price < requirement.price:
                raise InvalidInput(f"the service asks {requirement.price} TESS, more than the maximum of {max_price}")
            if requirement.price.planck == 0:
                raise InvalidInput("the service asks a price of zero")
            payee = format_address(parse_address(requirement.payee))

            channel = self.channels.get(payee)
            if channel and requirement.accepted and channel.spent < requirement.accepted:
                raise InvalidInput(
                    f"the service says {requirement.accepted} TESS were paid on channel {channel.id}, "
                    f"but only {channel.spent} were"
                )
            on_chain = self.client.channel(channel.id) if channel else None
            if channel is None or on_chain is None or on_chain.closes_at is not None:
                deposit = max(self.deposit, requirement.price, key=lambda a: a.planck)
                if self.owner:
                    channel_id = self.client.agent_open_channel(self.key, self.owner, payee, deposit)
                else:
                    channel_id = self.client.open_channel(self.key, payee, deposit)
                channel = PayerChannel(channel_id, deposit, Amount(0))
                self.channels[payee] = channel
                self._persist()

            amount = channel.spent + requirement.price
            if channel.deposit < amount:
                top_up = Amount(max(self.deposit.planck, amount.planck - channel.deposit.planck))
                self.client.top_up_channel(self.key, channel.id, top_up, self.owner)
                channel = PayerChannel(channel.id, channel.deposit + top_up, channel.spent)
                self.channels[payee] = channel
                self._persist()

            # Recorded before the voucher leaves the payer.
            self.channels[payee] = PayerChannel(channel.id, channel.deposit, amount)
            try:
                self._persist()
            except StorageError:
                self.channels[payee] = channel
                raise
            return Voucher.sign(self.key, self._genesis, channel.id, amount)

    def request(
        self,
        url: str,
        method: str = "GET",
        body: Optional[bytes] = None,
        headers: Optional[dict] = None,
        max_price: Optional[Amount] = None,
    ) -> tuple[int, dict, bytes]:
        """An HTTP request, paying the service if it answers 402 with a `Tesserax-Payment`
        header. Returns the status, headers and body of the response."""
        status, response_headers, data = _http(url, method, body, headers or {})
        requirement = _header(response_headers, PAYMENT_HEADER)
        if status != 402 or requirement is None:
            return status, response_headers, data
        voucher = self.pay(PaymentRequired.from_header(requirement), max_price)
        return _http(url, method, body, {**(headers or {}), VOUCHER_HEADER: voucher.to_header()})


def _header(headers: dict, name: str) -> Optional[str]:
    return next((v for k, v in headers.items() if k.lower() == name.lower()), None)


def _http(url: str, method: str, body: Optional[bytes], headers: dict) -> tuple[int, dict, bytes]:
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, dict(response.headers), response.read()
    except urllib.error.HTTPError as error:
        return error.code, dict(error.headers), error.read()


@dataclass
class Rejection:
    """Why a service refused a voucher."""

    reason: str
    accepted: Optional[Amount] = None


@dataclass
class _Received:
    #: State of the channel on chain, read at `fetched`; unknown after a restart.
    info: Optional[ChannelInfo]
    fetched: float
    accepted: Amount
    unclaimed: Optional[Voucher] = None


class ChannelReceiver:
    """Verifies the vouchers paying a service and claims them.

    With `state_file`, the amount accepted on each channel and the latest voucher not claimed yet
    are written to disk before a voucher is accepted: after a restart the service still claims
    what it was paid, and refuses vouchers it already accepted. A state file must be used by one
    receiver at a time; it is refused if written for another chain or service."""

    def __init__(self, client: "Client", key: Keypair, state_file: Optional[str | os.PathLike] = None):
        self.client, self.key = client, key
        self._channels: dict[int, _Received] = {}
        self._lock = threading.Lock()
        self._genesis: Optional[bytes] = None
        self._file = os.fspath(state_file) if state_file is not None else None
        if self._file is not None:
            genesis = self._genesis_hash()
            state = _store.load(self._file)
            if state is not None:
                _check_state(state, "receiver", self._file, genesis, {"payee": (key.address, "service")})
                for channel_id, c in state["channels"].items():
                    unclaimed = Voucher.from_header(json.dumps(c["unclaimed"])) if c.get("unclaimed") else None
                    self._channels[int(channel_id)] = _Received(None, 0.0, Amount.from_tess(c["accepted"]), unclaimed)

    def _persist(self) -> None:
        if self._file is None:
            return
        _store.save(
            self._file,
            {
                "version": _STATE_VERSION,
                "kind": "receiver",
                "genesis": "0x" + self._genesis_hash().hex(),
                "payee": self.key.address,
                "channels": {
                    str(channel_id): {
                        "accepted": str(c.accepted),
                        "unclaimed": c.unclaimed.to_json() if c.unclaimed else None,
                    }
                    for channel_id, c in self._channels.items()
                },
            },
        )

    def _genesis_hash(self) -> bytes:
        if self._genesis is None:
            self._genesis = self.client.genesis_hash()
        return self._genesis

    def payment_required(self, price: Amount, rejection: Optional[Rejection] = None) -> str:
        """The `Tesserax-Payment` header asking for `price`, after `rejection` if any."""
        return PaymentRequired(
            payee=self.key.address,
            price=price,
            genesis="0x" + self._genesis_hash().hex(),
            accepted=rejection.accepted if rejection else None,
            error=rejection.reason if rejection else None,
        ).to_header()

    def verify(self, header: str, price: Amount) -> Amount:
        """Check the `Tesserax-Voucher` header of a request paying `price`. Returns the amount of
        the voucher, or raises `PaymentRejected`."""
        genesis = self._genesis_hash()
        try:
            voucher = Voucher.from_header(header)
        except InvalidInput as error:
            raise PaymentRejected(Rejection(str(error))) from None
        with self._lock:
            channel = self._channels.get(voucher.channel)
            if (
                channel is None
                or channel.info is None
                or time.monotonic() - channel.fetched > _REFRESH_SECONDS
                or channel.info.deposit < voucher.amount
            ):
                info = self.client.channel(voucher.channel)
                if info is None:
                    raise PaymentRejected(Rejection("unknown channel"))
                accepted = channel.accepted if channel and info.claimed < channel.accepted else info.claimed
                channel = _Received(info, time.monotonic(), accepted, channel.unclaimed if channel else None)
                self._channels[voucher.channel] = channel

            def reject(reason: str) -> PaymentRejected:
                return PaymentRejected(Rejection(reason, channel.accepted))

            if channel.info.payee != self.key.address:
                raise PaymentRejected(Rejection("the channel pays another payee"))
            if channel.info.closes_at is not None:
                if channel.info.closes_at - self.client.substrate.get_block_number(None) < CLOSE_SAFETY_BLOCKS:
                    raise reject("the channel is closing")
            if channel.info.deposit < voucher.amount:
                raise reject("the voucher exceeds the deposit")
            if voucher.amount < channel.accepted + price:
                raise reject("the voucher does not pay the price")
            if not voucher.verify(genesis, channel.info.signer_public):
                raise reject("invalid voucher signature")

            # Recorded before the request is served.
            previous = (channel.accepted, channel.unclaimed)
            channel.accepted, channel.unclaimed = voucher.amount, voucher
            try:
                self._persist()
            except StorageError:
                channel.accepted, channel.unclaimed = previous
                raise PaymentRejected(Rejection("the service cannot record the voucher")) from None
            return voucher.amount

    def claim_all(self) -> list["TxOutcome"]:
        """Claim the latest voucher of every channel. A voucher stays recorded until its claim
        succeeds; vouchers of channels closed in the meantime, or already claimed, are dropped."""
        with self._lock:
            pending = [c.unclaimed for c in self._channels.values() if c.unclaimed]
        outcomes = []
        for voucher in pending:
            info = self.client.channel(voucher.channel)
            if info is not None and info.claimed < voucher.amount:
                outcomes.append(self.client.claim_channel(self.key, voucher))
            with self._lock:
                if info is None:
                    # Closed: nothing can be claimed any more, and its vouchers are refused.
                    self._channels.pop(voucher.channel, None)
                elif (channel := self._channels.get(voucher.channel)) is not None:
                    # A newer voucher may have arrived during the claim.
                    if channel.unclaimed is not None and not voucher.amount < channel.unclaimed.amount:
                        channel.unclaimed = None
                    if channel.info is not None and channel.info.claimed < voucher.amount:
                        channel.info.claimed = voucher.amount
                self._persist()
        return outcomes

    def unclaimed(self) -> Amount:
        """Amount accepted and not claimed yet."""
        with self._lock:
            return Amount(
                sum(
                    c.unclaimed.amount.planck - (c.info.claimed.planck if c.info else 0)
                    for c in self._channels.values()
                    if c.unclaimed
                )
            )


class PaymentRejected(TesseraxError):
    """A voucher was refused; `rejection` goes in the 402 answer."""

    def __init__(self, rejection: Rejection):
        super().__init__(rejection.reason)
        self.rejection = rejection


def channel_info(channel_id: int, c: dict) -> ChannelInfo:
    signer = c["signer"]
    if isinstance(signer, (list, tuple)):
        signer = signer[0]
    signer_public = bytes.fromhex(signer[2:]) if isinstance(signer, str) else bytes(signer)
    return ChannelInfo(
        id=channel_id,
        payer=format_address(parse_address(c["payer"])),
        payee=format_address(parse_address(c["payee"])),
        signer=format_address(account_id(signer_public)),
        signer_public=signer_public,
        deposit=Amount(c["deposit"]),
        claimed=Amount(c["claimed"]),
        closes_at=c["closes_at"],
    )


__all__ = [
    "CLOSE_SAFETY_BLOCKS",
    "PAYMENT_HEADER",
    "VOUCHER_CONTEXT",
    "VOUCHER_HEADER",
    "ChannelInfo",
    "ChannelPayer",
    "ChannelReceiver",
    "PayerChannel",
    "PaymentRejected",
    "PaymentRequired",
    "Rejection",
    "Voucher",
    "voucher_message",
]
