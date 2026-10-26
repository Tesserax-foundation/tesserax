"""Atomic swaps of TESS against ETH, USDC and USDT on EVM chains (`docs/swaps.md` of the chain).

A maker offers TESS; a taker locks its payment on the EVM chain under the SHA-256 hash of a
secret; the maker checks it and fills the offer, which locks the TESS for the taker under the same
hash; the taker claims them with the secret, without fee, and the secret pays the maker on the EVM
chain. `Swaps` (`client.swaps`) is the Tesserax side; `EvmSwaps` the side of the EVM chains, with
web3.py (`pip install "tesserax[swaps]"`).

    from tesserax import Amount, Client, EvmSwaps, evm_networks, new_secret, payment_due

    offer = client.swaps.offers()[0]
    evm = EvmSwaps(next(n for n in evm_networks(testnet=True) if n.chain == offer.payment.chain),
                   private_key=open("taker.key").read().strip())
    secret, hash_ = new_secret()                  # keep the secret until the claim
    lock = evm.lock_payment(offer.payment.asset, "0x" + offer.payout.hex(),
                            payment_due(Amount.from_tess("50").planck, offer.price), hash_,
                            evm.head()[1] + 86_400, offer.id, "te...")
    # Once the maker filled it:
    trade = client.swaps.trade_of_lock(offer.payment.chain, lock)
    client.swaps.claim(trade, secret)             # unsigned, without fee
"""

from __future__ import annotations

import hashlib
import secrets as _secrets
from dataclasses import dataclass
from typing import Any, Optional, Union

from .amount import Amount
from .contracts import _compact
from .errors import InvalidInput, TransactionFailed
from .keys import Keypair, format_address, parse_address

#: Planck in one TESS: prices are for one TESS.
PLANCK_PER_TESS = 10**12

#: The chains a payment is made on, and the assets: BTC on Bitcoin, ETH on the others ("Native").
CHAINS = ("Bitcoin", "Ethereum", "Base", "Arbitrum")
ASSETS = ("Native", "Usdc", "Usdt")
EVM_CHAINS = ("Ethereum", "Base", "Arbitrum")


@dataclass(frozen=True)
class Payment:
    """What an offer is paid in: a chain of `CHAINS` and an asset of `ASSETS`."""

    chain: str
    asset: str

    @property
    def decimals(self) -> int:
        """Decimals of the asset: prices count in its smallest unit."""
        if self.chain == "Bitcoin":
            return 8
        return 18 if self.asset == "Native" else 6

    @property
    def symbol(self) -> str:
        if self.chain == "Bitcoin":
            return "BTC"
        return {"Native": "ETH", "Usdc": "USDC", "Usdt": "USDT"}[self.asset]


def payment_due(amount: int, price: int) -> int:
    """The payment, in the smallest unit of its asset, of `amount` planck at `price` units per TESS,
    rounded up, as the chain computes it."""
    return -(-amount * price // PLANCK_PER_TESS)


def tess_for(payment: int, price: int) -> int:
    """The TESS, in planck, a payment of `payment` units buys at `price`: the most whose payment due
    is at most `payment`."""
    return 0 if price == 0 else payment * PLANCK_PER_TESS // price


def hash_of(secret: bytes) -> bytes:
    """SHA-256 of a secret."""
    return hashlib.sha256(secret).digest()


def new_secret() -> tuple[bytes, bytes]:
    """A new secret of 32 bytes and its SHA-256 hash."""
    secret = _secrets.token_bytes(32)
    return secret, hash_of(secret)


@dataclass
class Offer:
    id: int
    maker: str
    payment: Payment
    #: Smallest units of the asset for one TESS.
    price: int
    available: Amount
    min_fill: Amount
    #: The EVM address the maker is paid to, or its public key on Bitcoin.
    payout: bytes
    fills: int
    completed: int


@dataclass
class Trade:
    """TESS locked for a taker."""

    id: int
    offer: int
    maker: str
    taker: str
    amount: Amount
    price: int
    payment: Payment
    hash: bytes
    #: The lock of the payment on its chain.
    payment_lock: bytes
    #: Last block the taker may claim at.
    expires: int


@dataclass
class Filled:
    trade: Trade


@dataclass
class Claimed:
    """Claimed with `secret`, which releases the payment."""

    trade: int
    taker: str
    amount: Amount
    secret: bytes


@dataclass
class Refunded:
    trade: int
    maker: str
    amount: Amount


SwapEvent = Union[Filled, Claimed, Refunded]


def _bytes(value: Any) -> bytes:
    if isinstance(value, (bytes, bytearray)):
        return bytes(value)
    if isinstance(value, str):
        return bytes.fromhex(value[2:] if value.startswith("0x") else value)
    if isinstance(value, (list, tuple)):
        if len(value) == 1 and isinstance(value[0], (list, tuple, str, bytes)):
            return _bytes(value[0])
        return bytes(value)
    raise InvalidInput(f"not bytes: {value!r}")


def _enum(value: Any) -> str:
    """The variant of a decoded enumeration without data."""
    if isinstance(value, dict):
        return next(iter(value))
    return str(value)


def _account(value: Any) -> str:
    if isinstance(value, str) and value.startswith("0x"):
        return format_address(bytes.fromhex(value[2:]))
    return format_address(parse_address(value))


def _payment(value: dict) -> Payment:
    return Payment(chain=_enum(value["chain"]), asset=_enum(value["asset"]))


def _offer(offer_id: int, o: dict) -> Offer:
    return Offer(
        id=int(offer_id),
        maker=_account(o["maker"]),
        payment=_payment(o["payment"]),
        price=int(o["price"]),
        available=Amount(int(o["available"])),
        min_fill=Amount(int(o["min_fill"])),
        payout=_bytes(o["payout"]),
        fills=int(o["fills"]),
        completed=int(o["completed"]),
    )


def _trade(trade_id: int, t: dict) -> Trade:
    return Trade(
        id=int(trade_id),
        offer=int(t["offer"]),
        maker=_account(t["maker"]),
        taker=_account(t["taker"]),
        amount=Amount(int(t["amount"])),
        price=int(t["price"]),
        payment=_payment(t["payment"]),
        hash=_bytes(t["hash"]),
        payment_lock=_bytes(t["payment_lock"]),
        expires=int(t["expires"]),
    )


def _bounded(value: bytes) -> str:
    if len(value) > 64:
        raise InvalidInput("at most 64 bytes")
    return "0x" + value.hex()


def _fixed32(value: bytes) -> str:
    if len(value) != 32:
        raise InvalidInput("32 bytes expected")
    return "0x" + value.hex()


def _key_value(key: Any) -> int:
    value = getattr(key, "value", key)
    if isinstance(value, (list, tuple)):
        value = value[0]
    return int(value)


class Swaps:
    """The Tesserax side of swaps: offers, trades, and their calls (`client.swaps`)."""

    def __init__(self, client):
        self._client = client

    @property
    def _substrate(self):
        return self._client.substrate

    def offers(self) -> list[Offer]:
        """Open offers."""
        found = [_offer(_key_value(k), v.value) for k, v in self._substrate.query_map("Swaps", "Offers")]
        return sorted(found, key=lambda o: o.id)

    def offer(self, offer_id: int) -> Optional[Offer]:
        """The offer `offer_id`, if open."""
        value = self._substrate.query("Swaps", "Offers", [offer_id]).value
        return _offer(offer_id, value) if value else None

    def trades(self) -> list[Trade]:
        """Trades waiting for their claim or their refund."""
        found = [_trade(_key_value(k), v.value) for k, v in self._substrate.query_map("Swaps", "Trades")]
        return sorted(found, key=lambda t: t.id)

    def trade(self, trade_id: int) -> Optional[Trade]:
        """The trade `trade_id`, if not claimed nor refunded yet."""
        value = self._substrate.query("Swaps", "Trades", [trade_id]).value
        return _trade(trade_id, value) if value else None

    def trade_of_lock(self, chain: str, lock: bytes) -> Optional[int]:
        """The trade the payment lock `lock` on `chain` filled, if any."""
        value = self._substrate.query("Swaps", "FilledLocks", [chain, _bounded(lock)]).value
        return None if value is None else int(value)

    def create_offer(self, maker: Keypair, payment: Payment, price: int, amount: Amount, min_fill: Amount, payout: bytes) -> int:
        """Offer `amount` TESS at `price` units of `payment` for one TESS, paid to `payout`, in trades
        of at least `min_fill`. Returns the id of the offer."""
        outcome = self._client.submit(
            maker,
            "Swaps",
            "offer",
            {
                "payment": {"chain": payment.chain, "asset": payment.asset},
                "price": price,
                "amount": amount.planck,
                "min_fill": min_fill.planck,
                "payout": _bounded(payout),
            },
        )
        return int(_event(outcome, "Offered")["offer"])

    def update_offer(self, maker: Keypair, offer_id: int, price: int, min_fill: Amount, add: Amount = Amount(0), remove: Amount = Amount(0)):
        """Change the price and the smallest fill of offer `offer_id`, adding `add` TESS and
        withdrawing `remove`."""
        params = {"id": offer_id, "price": price, "min_fill": min_fill.planck, "add": add.planck, "remove": remove.planck}
        return self._client.submit(maker, "Swaps", "update_offer", params)

    def cancel_offer(self, maker: Keypair, offer_id: int):
        """Withdraw offer `offer_id`: what remains returns to its maker."""
        return self._client.submit(maker, "Swaps", "cancel_offer", {"id": offer_id})

    def fill(self, maker: Keypair, offer_id: int, taker: str, amount: Amount, hash_: bytes, payment_lock: bytes) -> int:
        """Lock `amount` TESS of offer `offer_id` for `taker` under `hash_`, for the payment locked
        by `payment_lock`, as the maker, once it checked the payment. Returns the id of the trade."""
        outcome = self._client.submit(
            maker,
            "Swaps",
            "fill",
            {
                "id": offer_id,
                "taker": "0x" + parse_address(taker).hex(),
                "amount": amount.planck,
                "hash": _fixed32(hash_),
                "payment_lock": _bounded(payment_lock),
            },
        )
        return int(_event(outcome, "Filled")["trade"])

    def claim(self, trade_id: int, secret: bytes):
        """Claim trade `trade_id` for its taker with `secret`: submitted without signer and without
        fee, so anyone can submit it, a taker without TESS first."""
        call = self._substrate.compose_call("Swaps", "claim", {"id": trade_id, "secret": _fixed32(secret)})
        # Version 5, general transaction; extension version 0; era, nonce, tip, metadata hash mode.
        body = bytes([0b0100_0000 | 5, 0, 0, 0, 0, 0]) + bytes(call.data.data)
        return self._client._submit_unsigned(_compact(len(body)) + body)

    def refund(self, signer: Keypair, trade_id: int):
        """Return trade `trade_id` to its maker once it expired. Anyone can submit it."""
        return self._client.submit(signer, "Swaps", "refund", {"id": trade_id})

    def events(self, block_number: int) -> list[SwapEvent]:
        """What happened to trades in block `block_number` of the best chain."""
        s = self._substrate
        block_hash = s.get_block_hash(block_number)
        if not block_hash:
            return []
        found: list[SwapEvent] = []
        for record in s.get_events(block_hash):
            value = record.value
            event = value.get("event", value)
            if event.get("module_id") != "Swaps":
                continue
            name, data = event.get("event_id"), event.get("attributes")
            if name == "Filled":
                found.append(Filled(_trade(int(data["trade"]), data)))
            elif name == "Claimed":
                found.append(Claimed(int(data["trade"]), _account(data["taker"]), Amount(int(data["amount"])), _bytes(data["secret"])))
            elif name == "Refunded":
                found.append(Refunded(int(data["trade"]), _account(data["maker"]), Amount(int(data["amount"]))))
        return found


def _event(outcome, name: str) -> dict:
    for module, event, attributes in outcome.events:
        if module == "Swaps" and event == name:
            return attributes
    raise TransactionFailed(f"no Swaps.{name} event")


# The EVM side.

#: The swap contract, at the same address on every EVM chain.
SWAP_CONTRACT = "0xeA2f4f08D2A0f1972031B98De63f4B0395c256Fc"

_LOCKED = {
    "type": "event",
    "name": "Locked",
    "anonymous": False,
    "inputs": [
        {"name": "id", "type": "bytes32", "indexed": True},
        {"name": "sender", "type": "address", "indexed": True},
        {"name": "recipient", "type": "address", "indexed": True},
        {"name": "token", "type": "address", "indexed": False},
        {"name": "amount", "type": "uint256", "indexed": False},
        {"name": "hash", "type": "bytes32", "indexed": False},
        {"name": "timelock", "type": "uint64", "indexed": False},
        {"name": "offer", "type": "uint64", "indexed": False},
        {"name": "taker", "type": "bytes32", "indexed": False},
    ],
}

#: ABI of the swap contract.
SWAP_ABI = [
    {
        "type": "function",
        "name": "lock",
        "stateMutability": "payable",
        "inputs": [
            {"name": "recipient", "type": "address"},
            {"name": "token", "type": "address"},
            {"name": "amount", "type": "uint256"},
            {"name": "hash", "type": "bytes32"},
            {"name": "timelock", "type": "uint64"},
            {"name": "offer", "type": "uint64"},
            {"name": "taker", "type": "bytes32"},
        ],
        "outputs": [{"name": "id", "type": "bytes32"}],
    },
    {"type": "function", "name": "claim", "stateMutability": "nonpayable", "inputs": [{"name": "id", "type": "bytes32"}, {"name": "secret", "type": "bytes32"}], "outputs": []},
    {"type": "function", "name": "refund", "stateMutability": "nonpayable", "inputs": [{"name": "id", "type": "bytes32"}], "outputs": []},
    {
        "type": "function",
        "name": "locks",
        "stateMutability": "view",
        "inputs": [{"name": "id", "type": "bytes32"}],
        "outputs": [
            {"name": "sender", "type": "address"},
            {"name": "recipient", "type": "address"},
            {"name": "token", "type": "address"},
            {"name": "amount", "type": "uint256"},
            {"name": "hash", "type": "bytes32"},
            {"name": "timelock", "type": "uint64"},
            {"name": "state", "type": "uint8"},
        ],
    },
    _LOCKED,
    {"type": "event", "name": "Claimed", "anonymous": False, "inputs": [{"name": "id", "type": "bytes32", "indexed": True}, {"name": "secret", "type": "bytes32", "indexed": False}]},
    {"type": "event", "name": "Refunded", "anonymous": False, "inputs": [{"name": "id", "type": "bytes32", "indexed": True}]},
]

_ERC20_ABI = [
    {"type": "function", "name": "approve", "stateMutability": "nonpayable", "inputs": [{"name": "spender", "type": "address"}, {"name": "amount", "type": "uint256"}], "outputs": [{"name": "", "type": "bool"}]},
    {"type": "function", "name": "allowance", "stateMutability": "view", "inputs": [{"name": "owner", "type": "address"}, {"name": "spender", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]},
    {"type": "function", "name": "balanceOf", "stateMutability": "view", "inputs": [{"name": "owner", "type": "address"}], "outputs": [{"name": "", "type": "uint256"}]},
]

ZERO_ADDRESS = "0x0000000000000000000000000000000000000000"


@dataclass(frozen=True)
class EvmNetwork:
    """An EVM chain with its stablecoins."""

    chain: str
    name: str
    chain_id: int
    #: A public RPC; users may give their own.
    rpc_url: str
    usdc: str
    usdt: Optional[str]
    explorer: str


def evm_networks(testnet: bool) -> list[EvmNetwork]:
    """The EVM chains of swaps: the mainnets for the mainnet of Tesserax, their testnets for its
    testnet. Token addresses are those of Circle for USDC and of Tether for USDT (USD₮0 on
    Arbitrum); Base has no native USDT."""
    if testnet:
        return [
            EvmNetwork("Ethereum", "Ethereum Sepolia", 11_155_111, "https://ethereum-sepolia-rpc.publicnode.com", "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238", None, "https://sepolia.etherscan.io"),
            EvmNetwork("Base", "Base Sepolia", 84_532, "https://sepolia.base.org", "0x036CbD53842c5426634e7929541eC2318f3dCF7e", None, "https://sepolia.basescan.org"),
            EvmNetwork("Arbitrum", "Arbitrum Sepolia", 421_614, "https://sepolia-rollup.arbitrum.io/rpc", "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d", None, "https://sepolia.arbiscan.io"),
        ]
    return [
        EvmNetwork("Ethereum", "Ethereum", 1, "https://ethereum-rpc.publicnode.com", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "0xdAC17F958D2ee523a2206206994597C13D831ec7", "https://etherscan.io"),
        EvmNetwork("Base", "Base", 8_453, "https://mainnet.base.org", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", None, "https://basescan.org"),
        EvmNetwork("Arbitrum", "Arbitrum", 42_161, "https://arb1.arbitrum.io/rpc", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831", "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9", "https://arbiscan.io"),
    ]


def token_of(network: EvmNetwork, asset: str) -> Optional[str]:
    """The token of `asset` on `network`, the zero address for ETH, if the chain has it."""
    return {"Native": ZERO_ADDRESS, "Usdc": network.usdc, "Usdt": network.usdt}.get(asset)


def asset_of(network: EvmNetwork, token: str) -> Optional[str]:
    """The asset of `token` on `network`, if it is one of its assets."""
    for asset in ASSETS:
        known = token_of(network, asset)
        if known is not None and known.lower() == token.lower():
            return asset
    return None


_LOCK_STATES = {1: "Locked", 2: "Claimed", 3: "Refunded"}


@dataclass
class LockStatus:
    """A payment lock as the contract stores it. `state` is Locked, Claimed or Refunded."""

    state: str
    sender: str
    recipient: str
    token: str
    amount: int
    hash: bytes
    #: Unix time from which the sender may take it back.
    timelock: int


@dataclass
class PaymentLock(LockStatus):
    """A payment locked for a maker, from its event."""

    id: bytes = b""
    #: The offer it pays, and the Tesserax account of its taker.
    offer: int = 0
    taker: str = ""
    #: Block of its event.
    block: int = 0


#: Blocks `EvmSwaps.locks` reads in one request: public RPCs limit the range of logs.
LOG_RANGE = 2_000


class EvmSwaps:
    """The EVM side of swaps on one chain, through web3.py: reads with its RPC, and sends with
    `private_key`, if given."""

    def __init__(self, network: EvmNetwork, rpc_url: Optional[str] = None, private_key: Optional[str] = None):
        try:
            from web3 import Web3
        except ImportError as error:
            raise ImportError('the EVM side of swaps needs web3.py: pip install "tesserax[swaps]"') from error
        self.network = network
        self.web3 = Web3(Web3.HTTPProvider(rpc_url or network.rpc_url))
        self.contract = self.web3.eth.contract(address=Web3.to_checksum_address(SWAP_CONTRACT), abi=SWAP_ABI)
        self.account = self.web3.eth.account.from_key(private_key) if private_key else None

    def head(self) -> tuple[int, int]:
        """The latest block, and its unix time."""
        block = self.web3.eth.get_block("latest")
        return block["number"], block["timestamp"]

    def lock_status(self, lock_id: bytes) -> Optional[LockStatus]:
        """Lock `lock_id` as the contract stores it, if it exists."""
        sender, recipient, token, amount, hash_, timelock, state = self.contract.functions.locks(lock_id).call()
        if state not in _LOCK_STATES:
            return None
        return LockStatus(_LOCK_STATES[state], sender, recipient, token, amount, bytes(hash_), timelock)

    def locks(self, from_block: int, to_block: int, recipient: Optional[str] = None) -> list[PaymentLock]:
        """Locks created in blocks `from_block` to `to_block` for `recipient`, if given, with their
        current state."""
        found = []
        event = self.contract.events.Locked
        for start in range(from_block, to_block + 1, LOG_RANGE):
            end = min(start + LOG_RANGE - 1, to_block)
            filters = {"recipient": self.web3.to_checksum_address(recipient)} if recipient else None
            for log in event.get_logs(from_block=start, to_block=end, argument_filters=filters):
                a = log["args"]
                status = self.lock_status(a["id"])
                found.append(
                    PaymentLock(
                        state=status.state if status else "Locked",
                        sender=a["sender"],
                        recipient=a["recipient"],
                        token=a["token"],
                        amount=a["amount"],
                        hash=bytes(a["hash"]),
                        timelock=a["timelock"],
                        id=bytes(a["id"]),
                        offer=a["offer"],
                        taker=format_address(bytes(a["taker"])),
                        block=log["blockNumber"],
                    )
                )
        return found

    def amount_of(self, lock: LockStatus, payment: Payment) -> Optional[int]:
        """The amount a lock pays, if it pays in `payment` on this chain."""
        if payment.chain != self.network.chain or asset_of(self.network, lock.token) != payment.asset:
            return None
        return lock.amount

    def balance(self, owner: str, asset: str) -> int:
        """Balance of `owner` in `asset`."""
        token = token_of(self.network, asset)
        if token is None:
            return 0
        owner = self.web3.to_checksum_address(owner)
        if token == ZERO_ADDRESS:
            return self.web3.eth.get_balance(owner)
        return self.web3.eth.contract(address=self.web3.to_checksum_address(token), abi=_ERC20_ABI).functions.balanceOf(owner).call()

    def _send(self, function, value: int = 0):
        if self.account is None:
            raise InvalidInput("no EVM key to send with")
        if self.web3.eth.chain_id != self.network.chain_id:
            raise InvalidInput(f"the RPC is on chain {self.web3.eth.chain_id}, not on {self.network.name}")
        tx = function.build_transaction(
            {"from": self.account.address, "value": value, "nonce": self.web3.eth.get_transaction_count(self.account.address, "pending"), "chainId": self.network.chain_id}
        )
        signed = self.account.sign_transaction(tx)
        tx_hash = self.web3.eth.send_raw_transaction(signed.raw_transaction)
        receipt = self.web3.eth.wait_for_transaction_receipt(tx_hash)
        if receipt["status"] != 1:
            raise TransactionFailed(f"the EVM transaction {tx_hash.hex()} reverted")
        return receipt

    def lock_payment(self, asset: str, recipient: str, amount: int, hash_: bytes, timelock: int, offer: int, taker: str) -> bytes:
        """Lock `amount` of `asset` for `recipient` until `timelock`, under `hash_`, for `offer` and
        the Tesserax account `taker`, approving the token first if needed. Returns the id of the
        lock."""
        # A payment sent to an address without the contract would be lost.
        if not self.web3.eth.get_code(self.contract.address):
            raise InvalidInput(f"the swap contract is not deployed on {self.network.name}")
        token = token_of(self.network, asset)
        if token is None:
            raise InvalidInput(f"{asset} is not on {self.network.name}")
        if token != ZERO_ADDRESS:
            erc20 = self.web3.eth.contract(address=self.web3.to_checksum_address(token), abi=_ERC20_ABI)
            if erc20.functions.allowance(self.account.address if self.account else ZERO_ADDRESS, self.contract.address).call() < amount:
                self._send(erc20.functions.approve(self.contract.address, amount))
        function = self.contract.functions.lock(
            self.web3.to_checksum_address(recipient), self.web3.to_checksum_address(token), amount, hash_, timelock, offer, parse_address(taker)
        )
        receipt = self._send(function, amount if token == ZERO_ADDRESS else 0)
        locked = self.contract.events.Locked().process_receipt(receipt)
        if not locked:
            raise TransactionFailed("no lock in the receipt")
        return bytes(locked[0]["args"]["id"])

    def claim(self, lock_id: bytes, secret: bytes) -> bytes:
        """Pay lock `lock_id` to its recipient with `secret`. Returns the transaction hash."""
        return bytes(self._send(self.contract.functions.claim(lock_id, secret))["transactionHash"])

    def refund(self, lock_id: bytes) -> bytes:
        """Return lock `lock_id` to its sender once its timelock passed. Returns the transaction hash."""
        return bytes(self._send(self.contract.functions.refund(lock_id))["transactionHash"])
