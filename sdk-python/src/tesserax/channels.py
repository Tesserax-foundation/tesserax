"""Micropayments: payment channels, vouchers and the HTTP 402 protocol. See
`docs/micropayments.md`."""

from __future__ import annotations

import base64
import json
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Optional

from .amount import Amount
from .errors import InvalidInput, TesseraxError
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


class ChannelPayer:
    """Pays services over payment channels, one per payee, opened and topped up on demand. With
    `owner`, channels are opened for the owner through the agent's delegation."""

    def __init__(self, client: "Client", key: Keypair, deposit: Amount, max_price: Amount, owner: Optional[str] = None):
        self.client, self.key, self.deposit, self.max_price, self.owner = client, key, deposit, max_price, owner
        self.channels: dict[str, PayerChannel] = {}
        self._lock = threading.Lock()
        self._genesis: Optional[bytes] = None

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

            amount = channel.spent + requirement.price
            if channel.deposit < amount:
                top_up = Amount(max(self.deposit.planck, amount.planck - channel.deposit.planck))
                self.client.top_up_channel(self.key, channel.id, top_up, self.owner)
                channel = PayerChannel(channel.id, channel.deposit + top_up, channel.spent)

            voucher = Voucher.sign(self.key, self._genesis, channel.id, amount)
            self.channels[payee] = PayerChannel(channel.id, channel.deposit, amount)
            return voucher

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
    info: ChannelInfo
    fetched: float
    accepted: Amount
    unclaimed: Optional[Voucher] = None


class ChannelReceiver:
    """Verifies the vouchers paying a service and claims them."""

    def __init__(self, client: "Client", key: Keypair):
        self.client, self.key = client, key
        self._channels: dict[int, _Received] = {}
        self._lock = threading.Lock()
        self._genesis: Optional[bytes] = None

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
            if channel is None or time.monotonic() - channel.fetched > _REFRESH_SECONDS or channel.info.deposit < voucher.amount:
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
            channel.accepted = voucher.amount
            channel.unclaimed = voucher
            return voucher.amount

    def claim_all(self) -> list["TxOutcome"]:
        """Claim the latest voucher of every channel."""
        with self._lock:
            pending = [(c, c.unclaimed) for c in self._channels.values() if c.unclaimed]
            for channel, _ in pending:
                channel.unclaimed = None
        outcomes = []
        for channel, voucher in pending:
            try:
                outcomes.append(self.client.claim_channel(self.key, voucher))
            except TesseraxError:
                with self._lock:
                    if channel.unclaimed is None or channel.unclaimed.amount < voucher.amount:
                        channel.unclaimed = voucher
                raise
        return outcomes

    def unclaimed(self) -> Amount:
        """Amount accepted and not claimed yet."""
        with self._lock:
            return Amount(
                sum(c.unclaimed.amount.planck - c.info.claimed.planck for c in self._channels.values() if c.unclaimed)
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
