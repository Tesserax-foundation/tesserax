"""Private transactions with the shielded pool (`docs/privacy.md`).

A `ShieldedAccount` is the shielded side of an account: its key derives from the seed of the
account's ML-DSA key, so the recovery phrase restores it. It reads the chain block after block from
its last synchronization, and its state can be stored and restored. Proofs are generated locally by
the shielded wallet of the chain compiled to WebAssembly (`shielded.wasm`, from the crate
`shielded-wasi`), run by wasmtime: the same module on every platform. Install the extra:
`pip install "tesserax[shielded]"`.

    alice = Keypair.from_uri(phrase)
    private = ShieldedAccount.create(alice)
    private.sync(client)
    private.shield(client, alice, Amount.from_tess("10"))
    private.send(client, "zte...", Amount.from_tess("4"))
"""

from __future__ import annotations

import struct
import threading
from dataclasses import dataclass
from importlib import resources
from typing import Callable, Optional, Union

from .amount import Amount
from .errors import InvalidAddress, InvalidInput, StorageError
from .keys import Keypair, parse_address

try:
    import wasmtime
except ImportError:  # pragma: no cover - depends on the installation
    wasmtime = None


class _Module:
    """The WebAssembly shielded wallet, instantiated once per process. Calls are serialized: the
    module has one result buffer."""

    def __init__(self):
        if wasmtime is None:
            raise ImportError('private transactions need wasmtime: pip install "tesserax[shielded]"')
        engine = wasmtime.Engine()
        module = wasmtime.Module(engine, resources.files(__package__).joinpath("shielded.wasm").read_bytes())
        linker = wasmtime.Linker(engine)
        linker.define_wasi()
        self.store = wasmtime.Store(engine)
        self.store.set_wasi(wasmtime.WasiConfig())
        instance = linker.instantiate(self.store, module)
        exports = instance.exports(self.store)
        self.memory = exports["memory"]
        self.f = {name: exports[name] for name in (
            "alloc", "dealloc", "result_ptr", "result_len", "wallet_new", "wallet_restore", "wallet_free",
            "wallet_address", "wallet_balance", "wallet_pending", "wallet_position", "wallet_ingest",
            "wallet_shield", "wallet_send", "wallet_unshield", "wallet_mark_pending",
            "wallet_release_pending", "wallet_state", "is_address", "transact_extrinsic", "call_data",
        )}
        self.lock = threading.Lock()

    def call(self, name: str, *args):
        return self.f[name](self.store, *args)

    def write(self, data: bytes) -> tuple[int, int]:
        ptr = self.call("alloc", len(data))
        self.memory.write(self.store, data, ptr)
        return ptr, len(data)

    def free(self, *buffers: tuple[int, int]) -> None:
        for ptr, length in buffers:
            self.call("dealloc", ptr, length)

    def result(self) -> bytes:
        ptr, length = self.call("result_ptr"), self.call("result_len")
        return bytes(self.memory.read(self.store, ptr, ptr + length))

    def checked(self, status: int) -> bytes:
        data = self.result()
        if status != 0:
            raise InvalidInput(data.decode("utf-8", "replace"))
        return data


_module: Optional[_Module] = None
_module_lock = threading.Lock()


def _wasm() -> _Module:
    global _module
    with _module_lock:
        if _module is None:
            _module = _Module()
        return _module


def is_shielded_address(text: str) -> bool:
    """Whether `text` is a shielded address, `zte...`."""
    m = _wasm()
    with m.lock:
        buffer = m.write(text.strip().encode())
        try:
            return m.call("is_address", *buffer) == 1
        finally:
            m.free(buffer)


def shielded_minimum_fee(client) -> Amount:
    """The smallest fee of a private transfer or unshield, paid from the shielded balance."""
    return Amount(client.substrate.get_constant("Shielded", "MinimumFee").value)


def _digests(values) -> bytes:
    """Two digests as `substrate-interface` decodes them, four 64-bit limbs each, as the 64 bytes
    of their SCALE encoding."""
    return b"".join(bytes.fromhex(d[2:]) if isinstance(d, str) else struct.pack("<4Q", *d) for d in values)


def _bytes(value) -> bytes:
    return bytes.fromhex(value[2:]) if isinstance(value, str) else bytes(value)


def _indices(client) -> tuple[int, int, int]:
    """Indices of the shielded pallet and of its calls `shield` and `transact`."""
    client.substrate.init_runtime()
    pallet = client.substrate.metadata.get_metadata_pallet("Shielded")
    if pallet is None:
        raise InvalidInput("the chain has no shielded pool")
    calls = {call.name: call.value["index"] for call in pallet.calls}
    return pallet.value["index"], calls["shield"], calls["transact"]


@dataclass
class Synced:
    """What a synchronization found."""

    #: Number of the last block read.
    block: int
    #: Value of the notes received.
    received: Amount


_STATE_VERSION = 1


class ShieldedAccount:
    """The shielded side of an account."""

    def __init__(self, keypair: Keypair, state: Optional[bytes] = None):
        """The shielded account of `keypair`, restored from a state of `state()` if given."""
        self._seed = keypair.seed()
        self._synced: Optional[tuple[int, bytes]] = None
        wallet_state = None
        if state is not None:
            if len(state) < 38 or state[0] != _STATE_VERSION:
                raise StorageError("not a state of a shielded account")
            if state[1] == 1:
                self._synced = (struct.unpack_from("<I", state, 2)[0], bytes(state[6:38]))
            wallet_state = bytes(state[38:])
        self._handle = self._open(wallet_state)

    @classmethod
    def create(cls, keypair: Keypair) -> "ShieldedAccount":
        """The shielded account of `keypair`, not synchronized."""
        return cls(keypair)

    @classmethod
    def restore(cls, keypair: Keypair, state: bytes) -> "ShieldedAccount":
        """The shielded account of `keypair` from a state of `state()`."""
        return cls(keypair, state)

    def _open(self, wallet_state: Optional[bytes]) -> int:
        m = _wasm()
        with m.lock:
            seed = m.write(self._seed)
            try:
                if wallet_state is None:
                    return m.call("wallet_new", seed[0])
                state = m.write(wallet_state)
                try:
                    handle = m.call("wallet_restore", seed[0], *state)
                    if handle == 0:
                        raise StorageError(m.result().decode("utf-8", "replace"))
                    return handle
                finally:
                    m.free(state)
            finally:
                m.free(seed)

    def __del__(self):
        handle = getattr(self, "_handle", 0)
        if handle and _module is not None:
            try:
                with _module.lock:
                    _module.call("wallet_free", handle)
            except Exception:
                # The module is gone already, at the exit of the interpreter.
                pass

    def _bytes_of(self, name: str, *args) -> bytes:
        m = _wasm()
        with m.lock:
            return m.checked(m.call(name, self._handle, *args))

    def state(self) -> bytes:
        """The state to store: the note tree, the notes and the last block read. It holds no key,
        but tells which notes are the account's: keep it private."""
        header = bytearray(38)
        header[0] = _STATE_VERSION
        if self._synced:
            header[1] = 1
            struct.pack_into("<I", header, 2, self._synced[0])
            header[6:38] = self._synced[1]
        return bytes(header) + self._bytes_of("wallet_state")

    @property
    def address(self) -> str:
        """Its shielded address, `zte...`."""
        return self._bytes_of("wallet_address").decode()

    @property
    def balance(self) -> Amount:
        """Spendable balance."""
        m = _wasm()
        with m.lock:
            return Amount(m.call("wallet_balance", self._handle))

    @property
    def pending(self) -> Amount:
        """Balance spent by transactions not yet seen in a block."""
        m = _wasm()
        with m.lock:
            return Amount(m.call("wallet_pending", self._handle))

    @property
    def block(self) -> Optional[int]:
        """Number of the last block read, if any."""
        return self._synced[0] if self._synced else None

    def _ingest(self, attributes: dict) -> int:
        m = _wasm()
        with m.lock:
            buffers = [
                m.write(_digests(attributes["nullifiers"])),
                m.write(_digests(attributes["commitments"])),
                m.write(_bytes(attributes["notes"][0])),
                m.write(_bytes(attributes["notes"][1])),
            ]
            try:
                status = m.call(
                    "wallet_ingest", self._handle, int(attributes["position"]), buffers[0][0], buffers[1][0], *buffers[2], *buffers[3]
                )
                return struct.unpack("<Q", m.checked(status))[0]
            finally:
                m.free(*buffers)

    def sync(self, client, on_progress: Optional[Callable[[int], None]] = None) -> Synced:
        """Read the blocks of the best chain since the last synchronization. After a
        reorganization below the last block read, the account is read again from the genesis."""
        s = client.substrate
        if self._synced:
            hash_ = s.get_block_hash(self._synced[0])
            if hash_ is None or bytes.fromhex(hash_[2:]) != self._synced[1]:
                self._release()
                self._synced = None
                self._handle = self._open(None)
        best = s.get_block_number(None)
        received = 0
        start = self._synced[0] + 1 if self._synced else 0
        for number in range(start, best + 1):
            hash_ = s.get_block_hash(number)
            if hash_ is None:
                break
            for record in s.get_events(hash_):
                event = record.value
                if event["module_id"] == "Shielded" and event["event_id"] == "Transacted":
                    received += self._ingest(event["attributes"])
            self._synced = (number, bytes.fromhex(hash_[2:]))
            if on_progress and number % 100 == 0:
                on_progress(number)
        return Synced(block=self._synced[0] if self._synced else 0, received=Amount(received))

    def _release(self) -> None:
        m = _wasm()
        with m.lock:
            m.call("wallet_free", self._handle)

    def shield(self, client, signer: Keypair, amount: Amount):
        """Move `amount` of the transparent balance of `signer` into this shielded account."""
        from scalecodec.base import ScaleBytes

        pallet, shield, _ = _indices(client)
        m = _wasm()
        with m.lock:
            source = m.write(signer.account_id)
            try:
                transaction = m.checked(m.call("wallet_shield", self._handle, source[0], amount.planck))
            finally:
                m.free(source)
            buffer = m.write(transaction)
            try:
                data = m.checked(m.call("call_data", pallet, shield, *buffer))
            finally:
                m.free(buffer)
        s = client.substrate
        call = s.runtime_config.create_scale_object("Call", data=ScaleBytes(data), metadata=s.metadata)
        call.decode()
        return client._submit_call(signer, call, data)

    def send(self, client, to: str, amount: Amount):
        """Send `amount` privately to the shielded address `to`. The fee, the minimum fee, is paid
        from the shielded balance."""
        if not is_shielded_address(to):
            raise InvalidAddress("not a shielded address: they start with zte")
        fee = shielded_minimum_fee(client)
        m = _wasm()
        with m.lock:
            text = m.write(to.strip().encode())
            try:
                transaction = m.checked(m.call("wallet_send", self._handle, *text, amount.planck, fee.planck))
            finally:
                m.free(text)
        return self._submit(client, transaction)

    def unshield(self, client, to: Union[str, bytes], amount: Amount):
        """Move `amount` out of the pool to the transparent account `to`. The fee, the minimum fee,
        is paid from the shielded balance."""
        recipient = parse_address(to) if isinstance(to, str) else bytes(to)
        fee = shielded_minimum_fee(client)
        m = _wasm()
        with m.lock:
            buffer = m.write(recipient)
            try:
                transaction = m.checked(m.call("wallet_unshield", self._handle, buffer[0], amount.planck, fee.planck))
            finally:
                m.free(buffer)
        return self._submit(client, transaction)

    def release_pending(self) -> None:
        """Make spendable again the notes of transactions that no block included."""
        m = _wasm()
        with m.lock:
            m.call("wallet_release_pending", self._handle)

    def _submit(self, client, transaction: bytes):
        pallet, _, transact = _indices(client)
        m = _wasm()
        with m.lock:
            buffer = m.write(transaction)
            try:
                extrinsic = m.checked(m.call("transact_extrinsic", pallet, transact, *buffer))
            finally:
                m.free(buffer)
        outcome = client._submit_unsigned(extrinsic)
        with m.lock:
            buffer = m.write(transaction)
            try:
                m.checked(m.call("wallet_mark_pending", self._handle, *buffer))
            finally:
                m.free(buffer)
        return outcome
