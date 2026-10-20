"""ML-DSA-87 keys and Tesserax addresses.

Keys follow the node and the Rust SDK exactly: the 32-byte seed of a 24-word phrase is the
Substrate mini secret (PBKDF2-HMAC-SHA512 of the phrase entropy), hard derivation junctions
(`//name`) hash the parent seed, and an account is the BLAKE2-256 hash of the public key.
Transactions are signed with the FIPS 204 context "tesserax", with hedged randomness.
"""

from __future__ import annotations

import hashlib
import re

from bip39 import bip39_generate, bip39_to_mini_secret
from dilithium_py.ml_dsa import ML_DSA_87
from scalecodec.utils.ss58 import ss58_decode, ss58_encode

from .errors import InvalidAddress, InvalidKey

#: SS58 prefix of Tesserax addresses: they start with "te".
SS58_PREFIX = 2261

#: FIPS 204 context of transaction signatures.
SIGNING_CONTEXT = b"tesserax"

PUBLIC_KEY_SIZE = 2592
SIGNATURE_SIZE = 4627

#: Phrase of the development keys (`//Alice`, `//Bob`, ...).
DEV_PHRASE = "bottom drive obey lake curtain smoke basket hold race lonely fit walk"

_URI = re.compile(r"^(?P<secret>[^/]*)(?P<path>(//?[^/]+)*)(///(?P<password>.*))?$")
_JUNCTION = re.compile(r"(//?)([^/]+)")


def _blake2_256(data: bytes) -> bytes:
    return hashlib.blake2b(data, digest_size=32).digest()


def _compact(n: int) -> bytes:
    if n < 1 << 6:
        return bytes([n << 2])
    if n < 1 << 14:
        return ((n << 2) | 1).to_bytes(2, "little")
    if n < 1 << 30:
        return ((n << 2) | 2).to_bytes(4, "little")
    raise ValueError("value too large")


def _chain_code(junction: str) -> bytes:
    """Chain code of a junction, as Substrate computes it: SCALE encoded, padded or hashed."""
    if junction.isdigit() and int(junction) < 1 << 64:
        encoded = int(junction).to_bytes(8, "little")
    else:
        raw = junction.encode()
        encoded = _compact(len(raw)) + raw
    return _blake2_256(encoded) if len(encoded) > 32 else encoded.ljust(32, b"\0")


def _derive_hard(seed: bytes, junction: str) -> bytes:
    tag = b"MlDsaHDKD"
    return _blake2_256(_compact(len(tag)) + tag + seed + _chain_code(junction))


def account_id(public_key: bytes) -> bytes:
    """Account of an ML-DSA-87 public key."""
    return _blake2_256(public_key)


def format_address(account: bytes) -> str:
    """Tesserax address of a 32-byte account."""
    return ss58_encode(account, SS58_PREFIX)


def parse_address(text: str) -> bytes:
    """32-byte account of a Tesserax address, or of a `0x` hexadecimal account id."""
    text = text.strip()
    try:
        if text.startswith("0x"):
            account = bytes.fromhex(text[2:])
        else:
            account = bytes.fromhex(ss58_decode(text, valid_ss58_format=SS58_PREFIX))
    except Exception as error:
        raise InvalidAddress(f"{text}: {error}") from None
    if len(account) != 32:
        raise InvalidAddress(f"{text}: not a 32-byte account")
    return account


def verify(public_key: bytes, message: bytes, signature: bytes, context: bytes = SIGNING_CONTEXT) -> bool:
    """Verify an ML-DSA-87 signature."""
    return ML_DSA_87.verify(public_key, message, signature, ctx=context)


class Keypair:
    """An ML-DSA-87 key pair that signs transactions."""

    def __init__(self, seed: bytes):
        if len(seed) != 32:
            raise InvalidKey("a seed is 32 bytes")
        self._seed = bytes(seed)
        self.public_key, self._secret_key = ML_DSA_87.key_derive(self._seed)

    @classmethod
    def from_uri(cls, uri: str) -> "Keypair":
        """A key from a secret URI: a 24-word phrase with optional hard derivation
        (`phrase//agent`) and password (`phrase///password`), a development key (`//Alice`), or a
        `0x` hexadecimal 32-byte seed."""
        match = _URI.match(uri.strip())
        if not match:
            raise InvalidKey("malformed secret URI")
        secret = match.group("secret").strip() or DEV_PHRASE
        password = match.group("password") or ""
        if secret.startswith("0x"):
            try:
                seed = bytes.fromhex(secret[2:])
            except ValueError:
                raise InvalidKey("malformed hexadecimal seed") from None
            if len(seed) != 32:
                raise InvalidKey("a seed is 32 bytes")
        else:
            try:
                seed = bytes(bip39_to_mini_secret(secret, password))
            except Exception:
                raise InvalidKey("invalid phrase") from None
        for separator, junction in _JUNCTION.findall(match.group("path") or ""):
            if separator == "/":
                raise InvalidKey("ML-DSA keys only support hard derivation (//)")
            seed = _derive_hard(seed, junction)
        return cls(seed)

    @classmethod
    def generate(cls) -> tuple["Keypair", str]:
        """A new random key and its 24-word phrase (256 bits of entropy)."""
        phrase = bip39_generate(24)
        return cls.from_uri(phrase), phrase

    @property
    def account_id(self) -> bytes:
        return account_id(self.public_key)

    @property
    def address(self) -> str:
        return format_address(self.account_id)

    def sign(self, message: bytes, context: bytes = SIGNING_CONTEXT) -> bytes:
        """Hedged ML-DSA-87 signature of `message`."""
        return ML_DSA_87.sign(self._secret_key, message, ctx=context)

    def __repr__(self) -> str:
        return f"Keypair({self.address})"
