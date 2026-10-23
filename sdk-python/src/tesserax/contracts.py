"""Smart contracts: Solidity compiled to PolkaVM with `resolc`, deployed and called with ML-DSA
signed transactions, their arguments, results and events encoded with the Solidity ABI.

    token = client.deploy_contract(alice, code, abi, args=["Tesserax Coin", 1_000_000])
    token.write(alice, "transfer", [to_h160(bob.address), 10])
    balance = token.read("balanceOf", [to_h160(bob.address)])

Reads are dry runs: the node executes the call on the state of the best block and returns its
result, without a transaction or a fee. Writes are dry run first, which sizes their weight and
storage deposit and stops a call that would revert before it costs a fee.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, Iterator, Optional, Sequence

from eth_abi import decode as abi_decode
from eth_abi import encode as abi_encode
from eth_utils import keccak, to_checksum_address

from .amount import Amount
from .errors import InvalidInput, TransactionFailed
from .keys import Keypair, parse_address

if TYPE_CHECKING:
    from .client import Client, TxOutcome

#: Weight and storage deposit of a transaction are those of its dry run, plus this margin in
#: percent: the state may change between the dry run and the block that includes it.
MARGIN_PERCENT = 10

#: Blocks `Contract.events` reads at most at once.
MAX_EVENT_RANGE = 10_000

_ERROR_SELECTOR = bytes.fromhex("08c379a0")
_PANIC_SELECTOR = bytes.fromhex("4e487b71")
_PANICS = {
    0x01: "assertion failed",
    0x11: "arithmetic overflow or underflow",
    0x12: "division by zero",
    0x21: "invalid enum value",
    0x22: "invalid storage byte array",
    0x31: "pop on an empty array",
    0x32: "index out of bounds",
    0x41: "out of memory",
    0x51: "call to an uninitialized function",
}


class ContractReverted(TransactionFailed):
    """A contract reverted. `reason` is decoded from its revert data, such as
    `InsufficientBalance(available: 5, required: 10)`."""

    def __init__(self, reason: str, data: bytes):
        super().__init__(f"contract reverted: {reason}", "Revive::ContractReverted")
        self.reason = reason
        self.data = data


def to_h160(address: str | bytes) -> str:
    """Address of an account in contracts: its `msg.sender`, and what `address` arguments name.
    The last 20 bytes of the Keccak-256 hash of the account id, as the runtime maps it."""
    account = parse_address(address) if isinstance(address, str) else bytes(address)
    # Accounts derived from an Ethereum address end with twelve 0xEE bytes: they map back to it.
    raw = account[:20] if account[20:] == b"\xee" * 12 else keccak(account)[12:]
    return to_checksum_address(raw)


@dataclass
class DryRun:
    #: Data returned, or the revert data when `reverted`.
    output: bytes
    reverted: bool
    #: Weight the call needs: time (picoseconds) and proof size (bytes).
    weight: tuple[int, int]
    #: Most storage deposit the call charges at once.
    storage_deposit: Amount
    #: Address of the contract, for a deployment.
    contract: Optional[str] = None


@dataclass
class ContractEvent:
    """An event a contract emitted, decoded with its ABI when it declares it."""

    contract: str
    #: Name of the event, when the ABI declares it.
    name: Optional[str]
    #: Arguments by name, indexed ones included.
    args: dict
    topics: list[str]
    data: str
    block_number: int
    block_hash: str


@dataclass
class ContractTxOutcome:
    """Result of a transaction to a contract."""

    block_hash: str
    block_number: int
    extrinsic_hash: str
    #: Events emitted by the contracts the transaction called.
    contract_events: list[ContractEvent] = field(default_factory=list)
    #: All the events of the transaction, as `(pallet, event, attributes)`.
    events: list = field(default_factory=list, repr=False)


# ABI.


def _type(param: dict) -> str:
    """Canonical type of an ABI parameter, tuples spelled out."""
    kind = param["type"]
    if kind.startswith("tuple"):
        return "(" + ",".join(_type(c) for c in param.get("components", [])) + ")" + kind[len("tuple") :]
    return kind


def _signature(item: dict) -> str:
    return f"{item['name']}({','.join(_type(p) for p in item.get('inputs', []))})"


def _checksummed(kind: str, value: Any) -> Any:
    """`value` of the ABI type `kind`, its addresses checksummed as `to_h160` gives them."""
    if kind == "address":
        return to_checksum_address(value)
    if kind.endswith("]"):
        inner = kind[: kind.rindex("[")]
        return tuple(_checksummed(inner, v) for v in value)
    if kind.startswith("("):
        depth, parts, start = 0, [], 1
        for i, ch in enumerate(kind[1 : kind.rindex(")")], 1):
            depth += ch == "("
            depth -= ch == ")"
            if ch == "," and depth == 0:
                parts.append(kind[start:i])
                start = i + 1
        parts.append(kind[start : kind.rindex(")")])
        return tuple(_checksummed(k, v) for k, v in zip(parts, value))
    return value


def _decode(types: list[str], data: bytes) -> list:
    return [_checksummed(kind, value) for kind, value in zip(types, abi_decode(types, data))]


def _show(value: Any) -> str:
    if isinstance(value, bytes):
        return "0x" + value.hex()
    if isinstance(value, str) and not value.startswith("0x"):
        return f'"{value}"'
    if isinstance(value, (list, tuple)):
        return "[" + ", ".join(_show(v) for v in value) + "]"
    return str(value)


def revert_reason(abi: Optional[list], data: bytes) -> str:
    """The reason a call reverted, from its revert data: `Error(string)`, `Panic(uint256)`, or an
    error the ABI declares."""
    if not data:
        return "reverted without a reason"
    selector, args = data[:4], data[4:]
    try:
        if selector == _ERROR_SELECTOR:
            return abi_decode(["string"], args)[0]
        if selector == _PANIC_SELECTOR:
            code = abi_decode(["uint256"], args)[0]
            meaning = f" ({_PANICS[code]})" if code in _PANICS else ""
            return f"panic {hex(code)}{meaning}"
        for item in abi or []:
            if item.get("type") == "error" and keccak(text=_signature(item))[:4] == selector:
                values = _decode([_type(p) for p in item["inputs"]], args)
                fields = ", ".join(f"{p['name']}: {_show(v)}" for p, v in zip(item["inputs"], values))
                return f"{item['name']}({fields})"
    except Exception:
        pass
    return "reverted: 0x" + data.hex()


# SCALE, for the runtime API of the contracts.


def _compact(n: int) -> bytes:
    if n < 1 << 6:
        return bytes([n << 2])
    if n < 1 << 14:
        return ((n << 2) | 1).to_bytes(2, "little")
    if n < 1 << 30:
        return ((n << 2) | 2).to_bytes(4, "little")
    raw = n.to_bytes((n.bit_length() + 7) // 8, "little")
    return bytes([((len(raw) - 4) << 2) | 3]) + raw


def _vec(data: bytes) -> bytes:
    return _compact(len(data)) + data


class _Reader:
    def __init__(self, data: bytes):
        self.data, self.at = data, 0

    def take(self, n: int) -> bytes:
        if self.at + n > len(self.data):
            raise InvalidInput("truncated result of the node")
        out = self.data[self.at : self.at + n]
        self.at += n
        return out

    def u8(self) -> int:
        return self.take(1)[0]

    def int(self, n: int) -> int:
        return int.from_bytes(self.take(n), "little")

    def compact(self) -> int:
        first = self.data[self.at]
        mode = first & 3
        if mode == 0:
            return self.u8() >> 2
        if mode == 1:
            return self.int(2) >> 2
        if mode == 2:
            return self.int(4) >> 2
        self.u8()
        return self.int((first >> 2) + 4)

    def vec(self) -> bytes:
        return self.take(self.compact())

    def weight(self) -> tuple[int, int]:
        return self.compact(), self.compact()

    def deposit(self) -> int:
        # StorageDeposit: Refund(0) or Charge(1), then the amount.
        kind = self.u8()
        amount = self.int(16)
        return amount if kind == 1 else 0


def _margin(value: int) -> int:
    return value + -(-value * MARGIN_PERCENT // 100)


class Contracts:
    """Contract calls of a client: dry runs, deployments, calls and events."""

    def __init__(self, client: "Client"):
        self.client = client

    def _state_call(self, method: str, params: bytes) -> bytes:
        s = self.client.substrate
        best = s.get_chain_head()
        result = s.rpc_request("state_call", [method, "0x" + params.hex(), best])["result"]
        return bytes.fromhex(result[2:])

    def _dispatch_error(self, reader: _Reader) -> str:
        kind = reader.u8()
        if kind == 3:
            index, error = reader.u8(), reader.take(4)
            try:
                pallet = next(p for p in self.client.substrate.metadata.pallets if p.value["index"] == index)
                name = pallet.errors[error[0]].value["name"]
                return f"{pallet.name}::{name}"
            except Exception:
                return f"Module({index}, {error[0]})"
        names = ["Other", "CannotLookup", "BadOrigin", "Module", "ConsumerRemaining", "NoProviders", "TooManyConsumers",
                 "Token", "Arithmetic", "Transactional", "Exhausted", "Corruption", "Unavailable", "RootNotAllowed", "Trie"]
        return names[kind] if kind < len(names) else f"DispatchError({kind})"

    def _result(self, raw: bytes, deploy: bool) -> DryRun:
        r = _Reader(raw)
        r.weight()  # consumed
        weight = r.weight()  # required
        r.deposit()  # storage deposit of the whole call
        deposit = r.deposit()  # most charged at once
        r.int(16)  # gas consumed
        if r.u8() == 1:
            error = self._dispatch_error(r)
            raise TransactionFailed(f"dry run failed: {error}", error)
        flags = r.int(4)
        output = r.vec()
        contract = to_checksum_address(r.take(20)) if deploy else None
        return DryRun(output=output, reverted=bool(flags & 1), weight=weight, storage_deposit=Amount(deposit), contract=contract)

    def dry_run_call(self, origin: str, contract: str, data: bytes, value: Amount = Amount(0)) -> DryRun:
        """Dry run of a call of `contract` with `data` by `origin`, on the state of the best block."""
        params = (
            parse_address(origin)
            + bytes.fromhex(contract.removeprefix("0x"))
            + value.planck.to_bytes(16, "little")
            + b"\x00\x00"  # no weight limit, no storage deposit limit
            + _vec(data)
        )
        return self._result(self._state_call("ReviveApi_call", params), deploy=False)

    def dry_run_deploy(
        self, origin: str, code: bytes, data: bytes, value: Amount = Amount(0), salt: Optional[bytes] = None
    ) -> DryRun:
        """Dry run of the deployment of `code` with the constructor input `data` by `origin`."""
        params = (
            parse_address(origin)
            + value.planck.to_bytes(16, "little")
            + b"\x00\x00"
            + b"\x00" + _vec(code)  # Code::Upload
            + _vec(data)
            + (b"\x01" + salt if salt else b"\x00")
        )
        return self._result(self._state_call("ReviveApi_instantiate", params), deploy=True)

    def deploy(
        self,
        signer: Keypair,
        code: bytes,
        abi: list,
        args: Sequence = (),
        value: Amount = Amount(0),
        salt: Optional[bytes] = None,
    ) -> "Contract":
        """Deploy `code`, a contract compiled to PolkaVM, with the constructor arguments `args` of
        `abi`, signed by `signer`. Returns the contract once a block includes the deployment; its
        `deployment` is the outcome of the transaction."""
        if salt is not None and len(salt) != 32:
            raise InvalidInput("the salt is 32 bytes")
        constructor = next((item for item in abi if item.get("type") == "constructor"), None)
        if constructor is None and args:
            raise InvalidInput("the contract has no constructor to take arguments")
        data = abi_encode([_type(p) for p in constructor["inputs"]], list(args)) if constructor else b""
        dry = self.dry_run_deploy(signer.address, code, data, value, salt)
        if dry.reverted:
            raise ContractReverted(revert_reason(abi, dry.output), dry.output)
        params = {
            "value": value.planck,
            "weight_limit": {"ref_time": _margin(dry.weight[0]), "proof_size": _margin(dry.weight[1])},
            "storage_deposit_limit": _margin(dry.storage_deposit.planck),
            "code": "0x" + code.hex(),
            "data": "0x" + data.hex(),
            "salt": "0x" + salt.hex() if salt else None,
        }
        outcome = self.client.submit(signer, "Revive", "instantiate_with_code", params)
        address = dry.contract
        for pallet, event, attributes in outcome.events:
            if pallet == "Revive" and event == "Instantiated":
                address = to_checksum_address(attributes["contract"])
        contract = Contract(self.client, address, abi)
        contract.deployment = contract._outcome(outcome)
        return contract

    def call(
        self, signer: Keypair, contract: str, data: bytes, value: Amount = Amount(0), abi: Optional[list] = None
    ) -> "TxOutcome":
        """Call `contract` with the encoded input `data`, signed by `signer`, after a dry run that
        stops a call that would revert."""
        dry = self.dry_run_call(signer.address, contract, data, value)
        if dry.reverted:
            raise ContractReverted(revert_reason(abi, dry.output), dry.output)
        params = {
            "dest": contract.lower(),
            "value": value.planck,
            "weight_limit": {"ref_time": _margin(dry.weight[0]), "proof_size": _margin(dry.weight[1])},
            "storage_deposit_limit": _margin(dry.storage_deposit.planck),
            "data": "0x" + data.hex(),
        }
        return self.client.submit(signer, "Revive", "call", params)

    def emitted_at(self, block_hash: str, block_number: int) -> list[dict]:
        """Contract events of the block `block_hash`, raw."""
        found = []
        for record in self.client.substrate.get_events(block_hash):
            event = record.value.get("event", record.value)
            if event.get("module_id") == "Revive" and event.get("event_id") == "ContractEmitted":
                found.append(_raw(event["attributes"], block_number, block_hash))
        return found

    def raw_events(self, contract: Optional[str], from_block: int, to_block: int) -> list[dict]:
        """Events emitted by `contract`, if given, in the blocks `from_block` to `to_block` of the
        best chain. The node must keep the state of these blocks, as an archive node does."""
        if to_block < from_block or to_block - from_block >= MAX_EVENT_RANGE:
            raise InvalidInput(f"read at most {MAX_EVENT_RANGE} blocks at once, from <= to")
        address = to_checksum_address(contract) if contract else None
        found = []
        for number in range(from_block, to_block + 1):
            block_hash = self.client.substrate.get_block_hash(number)
            if block_hash is None:
                break
            found += [e for e in self.emitted_at(block_hash, number) if address is None or e["contract"] == address]
        return found


def _hex(value: Any) -> str:
    """Hexadecimal of decoded bytes. The codec gives bytes as text when they are valid UTF-8, and
    as `0x` hexadecimal otherwise."""
    if isinstance(value, (bytes, bytearray)):
        return "0x" + bytes(value).hex()
    text = str(value)
    if text.startswith("0x") and len(text) % 2 == 0:
        try:
            bytes.fromhex(text[2:])
            return text.lower()
        except ValueError:
            pass
    return "0x" + text.encode().hex()


def _raw(attributes: Any, block_number: int, block_hash: str) -> dict:
    if not isinstance(attributes, dict):
        attributes = dict(zip(("contract", "data", "topics"), attributes))
    return {
        "contract": to_checksum_address(_hex(attributes["contract"])),
        "data": _hex(attributes["data"]),
        "topics": [_hex(t) for t in attributes["topics"]],
        "block_number": block_number,
        "block_hash": block_hash,
    }


class Contract:
    """A deployed contract and its ABI."""

    def __init__(self, client: "Client", address: str, abi: list):
        self.client = client
        self.address = to_checksum_address(address)
        self.abi = abi
        #: Outcome of the deployment, for a contract deployed by `Client.deploy_contract`.
        self.deployment: Optional[ContractTxOutcome] = None

    def _function(self, name: str, arguments: int) -> dict:
        functions = [item for item in self.abi if item.get("type") == "function"]
        if "(" in name:
            matching = [f for f in functions if _signature(f) == name]
        else:
            matching = [f for f in functions if f["name"] == name and len(f.get("inputs", [])) == arguments]
            if not matching and any(f["name"] == name for f in functions):
                raise InvalidInput(f"{name} does not take {arguments} arguments")
        if not matching:
            raise InvalidInput(f"no function {name} in the ABI")
        if len(matching) > 1:
            raise InvalidInput(f"{name} is overloaded: name it by its signature")
        return matching[0]

    def encode(self, function: str, args: Sequence = ()) -> bytes:
        """Input of a call of `function` with `args`: its selector and its encoded arguments."""
        item = self._function(function, len(args))
        return keccak(text=_signature(item))[:4] + abi_encode([_type(p) for p in item["inputs"]], list(args))

    def decode_output(self, function: str, arguments: int, output: bytes) -> Any:
        """What `function` returned in `output`: its value, or a tuple for several outputs."""
        item = self._function(function, arguments)
        values = _decode([_type(p) for p in item.get("outputs", [])], output)
        return values[0] if len(values) == 1 else tuple(values)

    def read(self, function: str, args: Sequence = (), sender: Optional[str] = None) -> Any:
        """Call `function` with `args` without a transaction and return what it returns. The caller
        (`msg.sender`) is the address `sender`, or the zero account."""
        from .keys import format_address

        origin = sender or format_address(bytes(32))
        dry = self.client.contracts.dry_run_call(origin, self.address, self.encode(function, args))
        if dry.reverted:
            raise ContractReverted(revert_reason(self.abi, dry.output), dry.output)
        return self.decode_output(function, len(args), dry.output)

    def write(self, signer: Keypair, function: str, args: Sequence = (), value: Amount = Amount(0)) -> ContractTxOutcome:
        """Call `function` with `args` in a transaction signed by `signer`, sending `value` to a
        `payable` function, and return once a block includes it, with the events it emitted."""
        data = self.encode(function, args)
        return self._outcome(self.client.contracts.call(signer, self.address, data, value, self.abi))

    def decode_event(self, topics: list[str], data: str) -> Optional[tuple[str, dict]]:
        """The event of `topics` and `data`, decoded with the ABI: its name and its arguments."""
        if not topics:
            return None
        first = bytes.fromhex(topics[0].removeprefix("0x"))
        for item in self.abi:
            if item.get("type") != "event" or item.get("anonymous") or keccak(text=_signature(item)) != first:
                continue
            inputs = item["inputs"]
            indexed = [p for p in inputs if p.get("indexed")]
            body = [p for p in inputs if not p.get("indexed")]
            try:
                values = _decode([_type(p) for p in body], bytes.fromhex(data.removeprefix("0x")))
                args = {}
                for i, param in enumerate(indexed):
                    topic = bytes.fromhex(topics[1 + i].removeprefix("0x"))
                    kind = _type(param)
                    # Dynamic types are indexed by their hash.
                    dynamic = kind in ("string", "bytes") or kind.endswith("]") or kind.startswith("(")
                    args[param["name"]] = "0x" + topic.hex() if dynamic else _decode([kind], topic)[0]
                for param in body:
                    args[param["name"]] = values.pop(0)
                return item["name"], {p["name"]: args[p["name"]] for p in inputs}
            except Exception:
                return None
        return None

    def _event(self, raw: dict) -> ContractEvent:
        decoded = self.decode_event(raw["topics"], raw["data"])
        name, args = decoded if decoded else (None, {})
        return ContractEvent(name=name, args=args, **raw)

    def _outcome(self, outcome: "TxOutcome") -> ContractTxOutcome:
        events = [
            self._event(_raw(attributes, outcome.block_number, outcome.block_hash))
            for pallet, event, attributes in outcome.events
            if pallet == "Revive" and event == "ContractEmitted"
        ]
        return ContractTxOutcome(
            block_hash=outcome.block_hash,
            block_number=outcome.block_number,
            extrinsic_hash=outcome.extrinsic_hash,
            contract_events=events,
            events=outcome.events,
        )

    def events(self, from_block: int, to_block: int) -> list[ContractEvent]:
        """Events this contract emitted in the blocks `from_block` to `to_block` of the best chain,
        at most `MAX_EVENT_RANGE` blocks at once."""
        return [self._event(raw) for raw in self.client.contracts.raw_events(self.address, from_block, to_block)]

    def watch_events(self, poll_seconds: float = 3.0) -> Iterator[ContractEvent]:
        """The events this contract emits from now on, block after block of the best chain, as
        they come. A block of the best chain may still leave it, with its events, until it is 60
        blocks deep."""
        s = self.client.substrate
        last = s.get_block_number(None)
        while True:
            best = s.get_block_number(None)
            for number in range(last + 1, best + 1):
                block_hash = s.get_block_hash(number)
                for raw in self.client.contracts.emitted_at(block_hash, number):
                    if raw["contract"] == self.address:
                        yield self._event(raw)
            last = max(last, best)
            time.sleep(poll_seconds)
