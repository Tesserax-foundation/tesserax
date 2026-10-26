"""The payment side of swaps on Bitcoin (`docs/swaps.md` of the chain), with embit
(`pip install "tesserax[swaps]"`): a P2WSH output locked by a hash time-locked script, which pays
the maker with the secret, or the taker once its timelock (a unix time) passed.

    OP_IF
        OP_SIZE 32 OP_EQUALVERIFY OP_SHA256 <hash> OP_EQUALVERIFY <maker>
    OP_ELSE
        <timelock> OP_CHECKLOCKTIMEVERIFY OP_DROP <taker>
    OP_ENDIF
    OP_CHECKSIG

The taker locks the payment in one transaction: the script, `NOTICE_SATS` to the P2WPKH address of
the maker, which a maker watches, and an OP_RETURN output announcing the offer, the Tesserax
account of the taker, the hash and the timelock: `TR` with the refund key (111 bytes), or `TX`
without it (78 bytes), the refund key being then the key of the first input. Amounts are in
satoshis.
"""

from __future__ import annotations

import hashlib
import json
import math
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Optional

from .errors import InvalidAmount, InvalidInput, TesseraxError
from .keys import format_address, parse_address

try:
    from embit import ec, networks
    from embit.script import Script, Witness, p2wpkh, p2wsh
    from embit.transaction import Transaction, TransactionInput, TransactionOutput
except ImportError as error:  # pragma: no cover
    raise ImportError('the Bitcoin side of swaps needs embit: pip install "tesserax[swaps]"') from error

#: Satoshis sent to the address of the maker to notify it, above the dust limit of P2WPKH.
NOTICE_SATS = 330

#: Confirmations a maker waits for before filling: an unconfirmed transaction can be replaced.
MIN_CONFIRMATIONS = 2

#: Seconds a payment stays locked: 24 hours, compared to the median time of the last blocks.
LOCK_SECONDS = 24 * 3600

MAGIC = b"TX"
MAGIC_WITH_REFUND = b"TR"
ANNOUNCEMENT_LEN = 2 + 8 + 32 + 32 + 4
ANNOUNCEMENT_WITH_REFUND_LEN = ANNOUNCEMENT_LEN + 33

#: Networks: Bitcoin for the mainnet of Tesserax, testnet4 for its testnet, regtest for tests on
#: one's own machine (`swaps/bitcoin/esplora-regtest.mjs` of the chain).
ESPLORA = {
    "bitcoin": "https://mempool.space/api",
    "testnet4": "https://mempool.space/testnet4/api",
    "regtest": "http://127.0.0.1:3002",
}
_PARAMS = {"bitcoin": networks.NETWORKS["main"], "testnet4": networks.NETWORKS["test"], "regtest": networks.NETWORKS["regtest"]}


def network_for(testnet: bool) -> str:
    """The Bitcoin network of a Tesserax network."""
    return "testnet4" if testnet else "bitcoin"


def _push(data: bytes) -> bytes:
    if len(data) < 0x4C:
        return bytes([len(data)]) + data
    if len(data) <= 0xFF:
        return bytes([0x4C, len(data)]) + data
    raise InvalidInput("push too large")


def _script_number(n: int) -> bytes:
    """`n` as a minimal script number."""
    out = bytearray()
    while n:
        out.append(n & 0xFF)
        n >>= 8
    if out and out[-1] & 0x80:
        out.append(0)
    return bytes(out)


@dataclass(frozen=True)
class Htlc:
    """The parameters of a hash time-locked script; keys are compressed, 33 bytes."""

    hash: bytes
    #: Key of the maker, which claims with the secret.
    maker: bytes
    #: Key of the taker, which takes the payment back after `timelock`.
    taker: bytes
    #: Unix time from which the taker may take it back.
    timelock: int

    def script(self) -> bytes:
        return b"".join(
            [
                b"\x63\x82",  # OP_IF OP_SIZE
                _push(_script_number(32)),
                b"\x88\xa8",  # OP_EQUALVERIFY OP_SHA256
                _push(self.hash),
                b"\x88",  # OP_EQUALVERIFY
                _push(self.maker),
                b"\x67",  # OP_ELSE
                _push(_script_number(self.timelock)),
                b"\xb1\x75",  # OP_CHECKLOCKTIMEVERIFY OP_DROP
                _push(self.taker),
                b"\x68\xac",  # OP_ENDIF OP_CHECKSIG
            ]
        )

    def script_pubkey(self) -> bytes:
        return b"\x00\x20" + hashlib.sha256(self.script()).digest()

    def address(self, network: str) -> str:
        return p2wsh(Script(self.script())).address(_PARAMS[network])


def address_of_key(key: bytes, network: str) -> str:
    """The P2WPKH address of a compressed key: where a maker is notified and paid."""
    return p2wpkh(ec.PublicKey.parse(key)).address(_PARAMS[network])


def _p2wpkh_script(key: bytes) -> bytes:
    return p2wpkh(ec.PublicKey.parse(key)).data


@dataclass(frozen=True)
class Announcement:
    """What the OP_RETURN output of a payment announces to its maker."""

    offer: int
    #: Tesserax account receiving the TESS.
    taker: str
    hash: bytes
    timelock: int
    #: The refund key of the taker, if not the key of the first input.
    refund: Optional[bytes] = None

    def encode(self) -> bytes:
        data = (MAGIC_WITH_REFUND if self.refund else MAGIC) + self.offer.to_bytes(8, "little") + parse_address(self.taker)
        data += self.hash + self.timelock.to_bytes(4, "little")
        return data + (self.refund or b"")

    @staticmethod
    def decode(data: bytes) -> Optional["Announcement"]:
        if (len(data), data[:2]) not in ((ANNOUNCEMENT_LEN, MAGIC), (ANNOUNCEMENT_WITH_REFUND_LEN, MAGIC_WITH_REFUND)):
            return None
        return Announcement(
            offer=int.from_bytes(data[2:10], "little"),
            taker=format_address(data[10:42]),
            hash=data[42:74],
            timelock=int.from_bytes(data[74:78], "little"),
            refund=data[78:] or None,
        )

    def script_pubkey(self) -> bytes:
        return b"\x6a" + _push(self.encode())


#: Order of the curve secp256k1.
_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141


def refund_key(seed: bytes, hash_: bytes) -> ec.PrivateKey:
    """The key the Tesserax wallet takes the payment of hash `hash_` back with, derived from the seed
    of the account (`Keypair.seed()`), as the web wallet and the TypeScript SDK derive it."""
    secret = hashlib.sha256(b"tesserax-swap-refund" + seed + hash_).digest()
    # A digest outside the scalars of the curve is hashed again: about one chance in 2^128.
    while not 0 < int.from_bytes(secret, "big") < _ORDER:
        secret = hashlib.sha256(secret).digest()
    return ec.PrivateKey(secret)


@dataclass
class BitcoinLock:
    """A payment locked by a script, as a transaction shows it."""

    txid: str
    vout: int
    amount: int
    htlc: Htlc
    offer: int
    taker: str

    def lock_bytes(self) -> bytes:
        """The payment lock of the trade on Tesserax: the outpoint, 36 bytes."""
        return outpoint_bytes(self.txid, self.vout)


def outpoint_bytes(txid: str, vout: int) -> bytes:
    """The outpoint as Tesserax stores it: the transaction id in its internal order, then the
    output index, little endian."""
    return bytes.fromhex(txid)[::-1] + vout.to_bytes(4, "little")


def find_lock(tx: Transaction, maker: bytes) -> Optional[BitcoinLock]:
    """The payment `tx` locks for the maker of compressed key `maker`, if it is a payment of swaps."""
    announcement = None
    for output in tx.vout:
        data = output.script_pubkey.data
        if data[:1] == b"\x6a":
            body = data[2:] if len(data) > 1 and data[1] < 0x4C else data[3:]
            announcement = Announcement.decode(body)
            if announcement:
                break
    if announcement is None:
        return None
    taker = announcement.refund
    if taker is None:
        witness = tx.vin[0].witness.items if tx.vin and tx.vin[0].witness else []
        if len(witness) != 2 or len(witness[1]) != 33:
            return None
        taker = witness[1]
    htlc = Htlc(announcement.hash, maker, taker, announcement.timelock)
    expected = htlc.script_pubkey()
    for vout, output in enumerate(tx.vout):
        if output.script_pubkey.data == expected:
            return BitcoinLock(tx.txid().hex(), vout, output.value, htlc, announcement.offer, announcement.taker)
    return None


@dataclass
class Utxo:
    """An output a key can spend."""

    txid: str
    vout: int
    amount: int


def _signature(tx: Transaction, index: int, script_code: bytes, amount: int, key: ec.PrivateKey) -> bytes:
    digest = tx.sighash_segwit(index, Script(script_code), amount)
    return key.sign(digest).serialize() + b"\x01"


def _with_fee(fee_rate: float, build):
    """A fee of `fee_rate` satoshis per virtual byte of the transaction `build(fee)` returns."""
    measured = build(0)
    # Weight: the size without witness counts four times, the witness once.
    vsize = math.ceil((len(_without_witness(measured)) * 3 + len(measured.serialize())) / 4)
    return build(math.ceil((vsize + 1) * fee_rate))


def _without_witness(tx: Transaction) -> bytes:
    stripped = Transaction(tx.version, [TransactionInput(i.txid, i.vout, sequence=i.sequence) for i in tx.vin], tx.vout, tx.locktime)
    return stripped.serialize()


def lock_transaction(
    key: ec.PrivateKey, utxos: list[Utxo], htlc: Htlc, announcement: Announcement, amount: int, fee_rate: float, network: str
) -> Transaction:
    """The transaction locking `amount` for the maker of `htlc`, from `utxos` of `key`: the script,
    the notice of the maker, the announcement and the change back to `key`."""
    public = key.get_public_key().sec()
    if (announcement.refund or public) != htlc.taker:
        raise InvalidInput("the refund key of the script is neither announced nor the key paying")
    own = _p2wpkh_script(public)
    total = sum(u.amount for u in utxos)

    def build(fee: int) -> Transaction:
        left = total - amount - NOTICE_SATS - fee
        if left < 0:
            raise InvalidAmount(f"{total} satoshis available, {amount + NOTICE_SATS + fee} needed with the fee")
        outputs = [
            TransactionOutput(amount, Script(htlc.script_pubkey())),
            TransactionOutput(NOTICE_SATS, Script(_p2wpkh_script(htlc.maker))),
            TransactionOutput(0, Script(announcement.script_pubkey())),
        ]
        # Change below the dust limit goes to the fee.
        if left >= 546:
            outputs.append(TransactionOutput(left, Script(own)))
        tx = Transaction(2, [TransactionInput(bytes.fromhex(u.txid), u.vout, sequence=0xFFFFFFFD) for u in utxos], outputs, 0)
        code = b"\x76\xa9\x14" + own[2:] + b"\x88\xac"
        for index, utxo in enumerate(utxos):
            tx.vin[index].witness = Witness([_signature(tx, index, code, utxo.amount, key), public])
        return tx

    return _with_fee(fee_rate, build)


def _spend(lock: BitcoinLock, key: ec.PrivateKey, destination: bytes, fee_rate: float, locktime: int, sequence: int, witness) -> Transaction:
    script = lock.htlc.script()

    def build(fee: int) -> Transaction:
        value = lock.amount - fee
        if value < 294:
            raise InvalidAmount(f"the fee takes the {lock.amount} satoshis of the payment")
        tx = Transaction(2, [TransactionInput(bytes.fromhex(lock.txid), lock.vout, sequence=sequence)], [TransactionOutput(value, Script(destination))], locktime)
        tx.vin[0].witness = Witness(witness(_signature(tx, 0, script, lock.amount, key), script))
        return tx

    return _with_fee(fee_rate, build)


def claim_transaction(lock: BitcoinLock, secret: bytes, key: ec.PrivateKey, fee_rate: float) -> Transaction:
    """The maker claims `lock` with `secret`, to its own P2WPKH address."""
    return _spend(lock, key, _p2wpkh_script(lock.htlc.maker), fee_rate, 0, 0xFFFFFFFD, lambda sig, script: [sig, secret, b"\x01", script])


def refund_transaction(lock: BitcoinLock, key: ec.PrivateKey, destination_address: str, fee_rate: float, network: str) -> Transaction:
    """The taker takes `lock` back to `destination_address` once its timelock passed."""
    from embit.script import address_to_scriptpubkey

    destination = address_to_scriptpubkey(destination_address).data
    return _spend(lock, key, destination, fee_rate, lock.htlc.timelock, 0xFFFFFFFE, lambda sig, script: [sig, b"", script])


class Esplora:
    """A Bitcoin network through an Esplora API, such as the one of mempool.space."""

    def __init__(self, network: str, url: Optional[str] = None):
        self.network = network
        self.base = (url or ESPLORA[network]).rstrip("/")

    def _get(self, path: str) -> bytes:
        for attempt in range(1, 4):
            try:
                with urllib.request.urlopen(self.base + path, timeout=30) as response:
                    return response.read()
            except urllib.error.HTTPError as error:
                if error.code < 500 and error.code != 429 or attempt == 3:
                    raise TesseraxError(f"{path}: {error.code} {error.read()[:200].decode(errors='replace')}") from None
            except urllib.error.URLError as error:
                if attempt == 3:
                    raise TesseraxError(f"{path}: {error.reason}") from None
            time.sleep(attempt)
        raise AssertionError("unreachable")

    def height(self) -> int:
        return int(self._get("/blocks/tip/height"))

    def median_time(self) -> int:
        """Median time of the last 11 blocks: the time a timelock compares to."""
        tip = self._get("/blocks/tip/hash").decode().strip()
        return int(json.loads(self._get(f"/block/{tip}"))["mediantime"])

    def fee_rate(self, blocks: int = 2) -> float:
        estimates = json.loads(self._get("/fee-estimates"))
        rates = [rate for target, rate in estimates.items() if int(target) <= max(blocks, 1)]
        return max(min(rates) if rates else 2.0, 1.0)

    def utxos(self, address: str) -> list[Utxo]:
        return [Utxo(u["txid"], u["vout"], u["value"]) for u in json.loads(self._get(f"/address/{address}/utxo"))]

    def transaction(self, txid: str) -> Transaction:
        return Transaction.from_string(self._get(f"/tx/{txid}/hex").decode().strip())

    def confirmations(self, txid: str) -> int:
        status = json.loads(self._get(f"/tx/{txid}/status"))
        return self.height() - status["block_height"] + 1 if status.get("confirmed") else 0

    def is_spent(self, txid: str, vout: int) -> bool:
        return bool(json.loads(self._get(f"/tx/{txid}/outspend/{vout}"))["spent"])

    def broadcast(self, tx: Transaction) -> str:
        request = urllib.request.Request(self.base + "/tx", data=tx.serialize().hex().encode(), method="POST")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.read().decode().strip()
        except urllib.error.HTTPError as error:
            raise TesseraxError(f"broadcast refused: {error.read()[:300].decode(errors='replace')}") from None
