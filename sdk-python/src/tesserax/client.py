"""Client of a Tesserax node."""

from __future__ import annotations

import hashlib
import time
from dataclasses import dataclass, field
from typing import Any, Optional, Sequence

from substrateinterface import SubstrateInterface
from substrateinterface.base import ExtrinsicReceipt
from substrateinterface.exceptions import SubstrateRequestException
from websocket import WebSocket, WebSocketConnectionClosedException

from .amount import DECIMALS, Amount
from .contracts import Contract, Contracts, _compact
from .errors import InvalidInput, TransactionFailed
from .keys import SS58_PREFIX, Keypair, format_address, parse_address

#: Blocks a transaction stays valid, counted from the best block: about 19 minutes.
MORTALITY = 64


class _WebSocket(WebSocket):
    """A websocket that reconnects instead of sending on a connection the node may have closed.

    A node closes a connection whose client does not answer its pings, which a client only does
    while it reads: a connection left idle for a minute or two is closed. Before sending on a
    connection idle for longer than `IDLE_SECONDS`, and when a send fails, this websocket reports
    itself closed: `substrate-interface` then opens a new connection and sends the request on it.
    The request had not left, so it is never sent twice."""

    IDLE_SECONDS = 50

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._active = time.monotonic()

    def send(self, *args, **kwargs):
        if time.monotonic() - self._active > self.IDLE_SECONDS:
            try:
                self.close(timeout=1)
            except Exception:
                pass
            raise WebSocketConnectionClosedException("the connection was idle")
        try:
            sent = super().send(*args, **kwargs)
        except ConnectionError as error:
            raise WebSocketConnectionClosedException(str(error)) from error
        self._active = time.monotonic()
        return sent

    def recv(self, *args, **kwargs):
        data = super().recv(*args, **kwargs)
        self._active = time.monotonic()
        return data


@dataclass
class ChainInfo:
    chain: str
    best_block: int
    spec_version: int
    token: str = "TESS"
    decimals: int = DECIMALS
    ss58_prefix: int = SS58_PREFIX


@dataclass
class Balance:
    free: Amount
    reserved: Amount
    frozen: Amount
    transferable: Amount


@dataclass
class TxOutcome:
    block_hash: str
    extrinsic_hash: str
    #: Events of the transaction, as `(pallet, event, attributes)`.
    events: list = field(default_factory=list, repr=False)
    block_number: int = 0


@dataclass
class Policy:
    """Limits of a delegation from an owner to an agent."""

    budget: Amount
    max_per_call: Amount
    period_blocks: int = 4_800
    expires_at: Optional[int] = None
    transfers: bool = True
    contracts: bool = False
    targets: list[str] = field(default_factory=list)


@dataclass
class Delegation:
    owner: str
    agent: str
    policy: Policy
    spent: Amount
    period_start: int
    remaining: Amount
    expired: bool


@dataclass
class Profile:
    name: str
    description: str = ""
    endpoint: str = ""
    capabilities: list[str] = field(default_factory=list)
    metadata: Optional[str] = None


@dataclass
class Reputation:
    payments: int = 0
    volume: Amount = field(default_factory=lambda: Amount(0))
    payers: int = 0
    positive: int = 0
    negative: int = 0
    tasks_completed: int = 0
    tasks_failed: int = 0


@dataclass
class AgentRecord:
    address: str
    owner: Optional[str]
    owner_confirmed: bool
    active: bool
    registered_at: int
    profile: Profile
    reputation: Reputation


def _hex(data: bytes) -> str:
    return "0x" + data.hex()


def _bounded(text: str, limit: int, name: str) -> str:
    raw = text.encode()
    if len(raw) > limit:
        raise InvalidInput(f"{name} is longer than {limit} bytes")
    # Hexadecimal, so that a text starting with "0x" is not read as hexadecimal.
    return _hex(raw)


def _text(value: Any) -> str:
    if isinstance(value, str) and value.startswith("0x"):
        try:
            return bytes.fromhex(value[2:]).decode()
        except ValueError:
            return value
    if isinstance(value, (bytes, bytearray)):
        return bytes(value).decode(errors="replace")
    return str(value)


def _list(value: Any) -> list:
    """The elements of a decoded bounded vector, which may come wrapped in a one-field tuple."""
    if isinstance(value, (list, tuple)) and len(value) == 1 and isinstance(value[0], (list, tuple)):
        return list(value[0])
    return list(value)


def _account(value: Any) -> str:
    """An address from a decoded account, which may be an address or hexadecimal."""
    if isinstance(value, str) and value.startswith("0x"):
        return format_address(bytes.fromhex(value[2:]))
    return value


class Client:
    """A connection to a Tesserax node.

    The client reads the state of the best block and builds transactions on top of it:
    proof-of-work blocks are final only 60 blocks deep. Calls return once the transaction is
    included in a block, or raise `TransactionFailed` with the error of the chain.
    """

    def __init__(self, url: str = "ws://127.0.0.1:9944"):
        self.substrate = SubstrateInterface(url=url, ss58_format=SS58_PREFIX, ws_options={"class_": _WebSocket})
        #: Smart contracts: dry runs, deployments, calls and events.
        self.contracts = Contracts(self)

    def close(self) -> None:
        self.substrate.close()

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *_exc) -> None:
        self.close()

    # Chain.

    def chain_info(self) -> ChainInfo:
        s = self.substrate
        return ChainInfo(
            chain=s.rpc_request("system_chain", [])["result"],
            best_block=s.get_block_number(None),
            spec_version=s.runtime_version,
        )

    def balance(self, address: str) -> Balance:
        account = parse_address(address)
        data = self.substrate.query("System", "Account", [_hex(account)]).value["data"]
        free, frozen = data["free"], data["frozen"]
        return Balance(
            free=Amount(free),
            reserved=Amount(data["reserved"]),
            frozen=Amount(frozen),
            transferable=Amount(max(free - frozen, 0)),
        )

    # Transactions.

    def submit(self, signer: Keypair, module: str, function: str, params: dict) -> TxOutcome:
        """Sign a call with `signer`, submit it and wait until a block includes it successfully."""
        call = self.substrate.compose_call(call_module=module, call_function=function, call_params=params)
        return self._submit_call(signer, call)

    def _submit_call(self, signer: Keypair, call, call_data: Optional[bytes] = None) -> TxOutcome:
        """Sign `call`, a call of `substrate-interface`, submit it and wait for its block.

        With `call_data`, the encoded call, the extrinsic carries these bytes as they are: some
        calls decode but do not encode again with `substrate-interface`, such as the shielding of
        TESS, whose digests are tuples of one array."""
        s = self.substrate
        nonce = s.rpc_request("system_accountNextIndex", [signer.address])["result"]
        era = {"period": MORTALITY, "current": s.get_block_number(None)}
        payload = s.generate_signature_payload(call=call, era=era, nonce=nonce, tip=0)
        signature = signer.sign(payload.data)

        # The call is the last field of an extrinsic: a placeholder takes its place, then its bytes.
        placeholder = s.compose_call("System", "remark", {"remark": "0x"}) if call_data is not None else call
        extrinsic = s.runtime_config.create_scale_object(type_string="Extrinsic", metadata=s.metadata)
        extrinsic.encode(
            {
                "account_id": _hex(signer.account_id),
                "signature": {"signer": _hex(signer.public_key), "signature": _hex(signature)},
                "call_function": placeholder.value["call_function"],
                "call_module": placeholder.value["call_module"],
                "call_args": placeholder.value["call_args"],
                "nonce": nonce,
                "era": era,
                "tip": 0,
                "mode": "Disabled",
            }
        )
        if call_data is not None:
            encoded = bytes(extrinsic.data.data)
            mode = encoded[0] & 3
            body = encoded[{0: 1, 1: 2, 2: 4}.get(mode, 1 + (encoded[0] >> 2) + 4) :]
            removed = bytes(placeholder.data.data)
            if not body.endswith(removed):
                raise InvalidInput("unexpected encoding of the extrinsic")
            body = body[: len(body) - len(removed)] + call_data
            return self._submit_unsigned(_compact(len(body)) + body)
        try:
            receipt = s.submit_extrinsic(extrinsic, wait_for_inclusion=True)
        except SubstrateRequestException as error:
            raise TransactionFailed(f"transaction refused: {error}") from None
        return self._outcome(receipt.block_hash, receipt.extrinsic_hash)

    def _submit_unsigned(self, extrinsic: bytes) -> TxOutcome:
        """Submit an encoded extrinsic, such as a private transaction its proof authorizes without
        signer, and wait until a block includes it."""

        def included(message, _update, subscription):
            result = message.get("params", {}).get("result")
            if isinstance(result, dict):
                status = {k.lower(): v for k, v in result.items()}
                if "inblock" in status or "finalized" in status:
                    self.substrate.rpc_request("author_unwatchExtrinsic", [subscription])
                    return status.get("inblock") or status["finalized"]
            elif result in ("dropped", "invalid", "usurped"):
                raise TransactionFailed(f"transaction {result}")

        try:
            block_hash = self.substrate.rpc_request("author_submitAndWatchExtrinsic", [_hex(extrinsic)], result_handler=included)
        except SubstrateRequestException as error:
            raise TransactionFailed(f"transaction refused: {error}") from None
        return self._outcome(block_hash, _hex(hashlib.blake2b(extrinsic, digest_size=32).digest()))

    def _outcome(self, block_hash: str, extrinsic_hash: str) -> TxOutcome:
        """The outcome of the extrinsic `extrinsic_hash` in the block `block_hash`, read from the
        events of the block. The extrinsics of the block are not decoded: `substrate-interface`
        cannot decode all of them, such as private transactions."""
        s = self.substrate
        extrinsics = s.rpc_request("chain_getBlock", [block_hash])["result"]["block"]["extrinsics"]
        hashes = [_hex(hashlib.blake2b(bytes.fromhex(x[2:]), digest_size=32).digest()) for x in extrinsics]
        if extrinsic_hash not in hashes:
            raise TransactionFailed(f"transaction {extrinsic_hash} not found in block {block_hash}")
        receipt = ExtrinsicReceipt(substrate=s, extrinsic_hash=extrinsic_hash, block_hash=block_hash, extrinsic_idx=hashes.index(extrinsic_hash))
        if not receipt.is_success:
            message = receipt.error_message or {}
            name = message.get("name")
            module_name = message.get("module") or message.get("type")
            error = f"{module_name}::{name}" if module_name and name else name
            raise TransactionFailed(f"transaction failed: {error or message}", error)
        events = []
        for event in receipt.triggered_events:
            value = event.value
            inner = value.get("event", value)
            events.append((inner.get("module_id"), inner.get("event_id"), inner.get("attributes")))
        return TxOutcome(
            block_hash=receipt.block_hash,
            extrinsic_hash=receipt.extrinsic_hash,
            events=events,
            block_number=s.get_block_number(receipt.block_hash),
        )

    # Smart contracts.

    def deploy_contract(
        self,
        signer: Keypair,
        code: bytes,
        abi: list,
        args: Sequence = (),
        value: Amount = Amount(0),
        salt: Optional[bytes] = None,
    ) -> Contract:
        """Deploy `code`, a contract compiled to PolkaVM (`.polkavm` from `resolc --bin`), with the
        constructor arguments `args` of `abi`, signed by `signer`. Returns the contract once a block
        includes the deployment; its `deployment` is the outcome, with the constructor's events."""
        return self.contracts.deploy(signer, code, abi, args, value, salt)

    def contract(self, address: str, abi: list) -> Contract:
        """The contract at `address` (`0x` and 40 digits), called through `abi`."""
        return Contract(self, address, abi)

    def transfer(self, signer: Keypair, to: str, amount: Amount) -> TxOutcome:
        """Transfer `amount` to `to`, keeping the signer's account alive."""
        params = {"dest": {"Id": _hex(parse_address(to))}, "value": amount.planck}
        return self.submit(signer, "Balances", "transfer_keep_alive", params)

    # Delegations.

    def authorize_agent(self, owner: Keypair, agent: str, policy: Policy) -> TxOutcome:
        """Delegate to `agent` within `policy`, signed by the owner."""
        params = {"agent": _hex(parse_address(agent)), "policy": self._policy(policy)}
        return self.submit(owner, "Agents", "authorize", params)

    def revoke_agent(self, owner: Keypair, agent: str) -> TxOutcome:
        return self.submit(owner, "Agents", "revoke", {"agent": _hex(parse_address(agent))})

    def renounce_delegation(self, agent: Keypair, owner: str) -> TxOutcome:
        return self.submit(agent, "Agents", "renounce", {"owner": _hex(parse_address(owner))})

    def delegation(self, owner: str, agent: str) -> Optional[Delegation]:
        """The delegation of `owner` to `agent`, if any, with what is left of its budget."""
        result = self.substrate.query(
            "Agents", "Delegations", [_hex(parse_address(owner)), _hex(parse_address(agent))]
        )
        if result.value is None:
            return None
        d = result.value
        p = d["policy"]
        policy = Policy(
            budget=Amount(p["budget"]),
            max_per_call=Amount(p["max_per_call"]),
            period_blocks=p["period"],
            expires_at=p["expires_at"],
            transfers=p["permissions"]["transfers"],
            contracts=p["permissions"]["contracts"],
            targets=[_account(t) for t in _list(p["targets"])],
        )
        now = self.substrate.get_block_number(None)
        expired = policy.expires_at is not None and now >= policy.expires_at
        if expired:
            remaining = 0
        elif now >= d["period_start"] + policy.period_blocks:
            remaining = policy.budget.planck
        else:
            remaining = max(policy.budget.planck - d["spent"], 0)
        return Delegation(
            owner=format_address(parse_address(owner)),
            agent=format_address(parse_address(agent)),
            policy=policy,
            spent=Amount(d["spent"]),
            period_start=d["period_start"],
            remaining=Amount(remaining),
            expired=expired,
        )

    def agent_execute(self, agent: Keypair, owner: str, module: str, function: str, params: dict) -> TxOutcome:
        """Make a call for `owner`, signed by the agent, within its delegation."""
        call = {"call_module": module, "call_function": function, "call_args": params}
        return self.submit(agent, "Agents", "execute", {"owner": _hex(parse_address(owner)), "call": call})

    def agent_transfer(self, agent: Keypair, owner: str, to: str, amount: Amount) -> TxOutcome:
        """Transfer `amount` from `owner` to `to`, signed by the agent, within its delegation."""
        params = {"dest": {"Id": _hex(parse_address(to))}, "value": amount.planck}
        return self.agent_execute(agent, owner, "Balances", "transfer_keep_alive", params)

    def agent_pay_agent(self, agent: Keypair, owner: str, payee: str, amount: Amount) -> TxOutcome:
        """Pay the registered agent `payee` from `owner`'s funds, within the delegation."""
        params = {"agent": _hex(parse_address(payee)), "amount": amount.planck}
        return self.agent_execute(agent, owner, "AgentRegistry", "pay", params)

    # Registry.

    def register_agent(self, signer: Keypair, profile: Profile, owner: Optional[str] = None) -> TxOutcome:
        """Register `signer` as an agent, optionally naming its owner, who must confirm."""
        params = {
            "profile": self._profile(profile),
            "owner": _hex(parse_address(owner)) if owner else None,
        }
        return self.submit(signer, "AgentRegistry", "register", params)

    def update_profile(self, signer: Keypair, profile: Profile) -> TxOutcome:
        return self.submit(signer, "AgentRegistry", "update_profile", {"profile": self._profile(profile)})

    def confirm_owner(self, owner: Keypair, agent: str) -> TxOutcome:
        return self.submit(owner, "AgentRegistry", "confirm_owner", {"agent": _hex(parse_address(agent))})

    def set_agent_active(self, signer: Keypair, agent: str, active: bool) -> TxOutcome:
        params = {"agent": _hex(parse_address(agent)), "active": active}
        return self.submit(signer, "AgentRegistry", "set_active", params)

    def pay_agent(self, payer: Keypair, agent: str, amount: Amount) -> TxOutcome:
        """Pay the registered agent `agent`; the payment counts in its reputation."""
        params = {"agent": _hex(parse_address(agent)), "amount": amount.planck}
        return self.submit(payer, "AgentRegistry", "pay", params)

    def rate_agent(self, payer: Keypair, agent: str, positive: bool) -> TxOutcome:
        params = {"agent": _hex(parse_address(agent)), "positive": positive}
        return self.submit(payer, "AgentRegistry", "rate", params)

    def agent(self, address: str) -> Optional[AgentRecord]:
        """The registered agent at `address`, if any, with its reputation."""
        account = _hex(parse_address(address))
        result = self.substrate.query("AgentRegistry", "Agents", [account])
        if result.value is None:
            return None
        return self._record(format_address(parse_address(address)), result.value)

    def find_agents(self, capability: Optional[str] = None, active_only: bool = True) -> list[AgentRecord]:
        """Registered agents, optionally only the active ones offering `capability`, best rated
        first."""
        found = []
        for key, value in self.substrate.query_map("AgentRegistry", "Agents", page_size=100):
            info = value.value
            if active_only and not info["active"]:
                continue
            record = self._record(_account(key.value), info)
            if capability and capability not in record.profile.capabilities:
                continue
            found.append(record)
        found.sort(
            key=lambda a: (
                a.reputation.positive - a.reputation.negative,
                a.reputation.payers,
                a.reputation.volume.planck,
            ),
            reverse=True,
        )
        return found

    # Payment channels.

    def genesis_hash(self) -> bytes:
        """Genesis hash of the chain, which vouchers sign."""
        return bytes.fromhex(self.substrate.get_block_hash(0)[2:])

    def channel(self, channel_id: int):
        """The channel `channel_id`, if open."""
        from .channels import channel_info

        result = self.substrate.query("Channels", "Channels", [channel_id])
        return channel_info(channel_id, result.value) if result.value is not None else None

    def channels_of(self, payer: str) -> list:
        """Open channels paid by `payer`."""
        from .channels import channel_info

        account = format_address(parse_address(payer))
        return [
            channel_info(key.value, value.value)
            for key, value in self.substrate.query_map("Channels", "Channels", page_size=100)
            if _account(value.value["payer"]) == account
        ]

    @staticmethod
    def _opened(outcome: TxOutcome) -> int:
        for pallet, event, attributes in outcome.events:
            if pallet == "Channels" and event == "Opened":
                return attributes["id"] if isinstance(attributes, dict) else attributes[0]
        raise TransactionFailed("the channel was not opened")

    @staticmethod
    def _open_params(payee: str, signer: Keypair, deposit: Amount) -> dict:
        return {"payee": _hex(parse_address(payee)), "signer": _hex(signer.public_key), "deposit": deposit.planck}

    def open_channel(self, payer: Keypair, payee: str, deposit: Amount) -> int:
        """Open a channel from `payer` to `payee`, whose vouchers the payer's key signs."""
        return self._opened(self.submit(payer, "Channels", "open", self._open_params(payee, payer, deposit)))

    def agent_open_channel(self, agent: Keypair, owner: str, payee: str, deposit: Amount) -> int:
        """Open a channel from `owner` to `payee` through the delegation of `agent`, whose key
        signs the vouchers."""
        params = self._open_params(payee, agent, deposit)
        return self._opened(self.agent_execute(agent, owner, "Channels", "open", params))

    def _payer_call(self, signer: Keypair, owner: Optional[str], function: str, params: dict) -> TxOutcome:
        if owner:
            return self.agent_execute(signer, owner, "Channels", function, params)
        return self.submit(signer, "Channels", function, params)

    def top_up_channel(self, signer: Keypair, channel_id: int, amount: Amount, owner: Optional[str] = None) -> TxOutcome:
        """Add `amount` to the deposit, as the payer, or as its agent when `owner` is given."""
        return self._payer_call(signer, owner, "top_up", {"id": channel_id, "amount": amount.planck})

    def request_close_channel(self, signer: Keypair, channel_id: int, owner: Optional[str] = None) -> TxOutcome:
        return self._payer_call(signer, owner, "request_close", {"id": channel_id})

    def finalize_close_channel(self, signer: Keypair, channel_id: int) -> TxOutcome:
        return self.submit(signer, "Channels", "finalize_close", {"id": channel_id})

    def claim_channel(self, payee: Keypair, voucher) -> TxOutcome:
        """Claim `voucher` as the payee of its channel."""
        params = {"id": voucher.channel, "amount": voucher.amount.planck, "signature": _hex(voucher.signature)}
        return self.submit(payee, "Channels", "claim", params)

    def close_channel(self, payee: Keypair, channel_id: int) -> TxOutcome:
        return self.submit(payee, "Channels", "close", {"id": channel_id})

    # Encoding and decoding.

    @staticmethod
    def _policy(policy: Policy) -> dict:
        return {
            "budget": policy.budget.planck,
            "period": policy.period_blocks,
            "max_per_call": policy.max_per_call.planck,
            "expires_at": policy.expires_at,
            "permissions": {"transfers": policy.transfers, "contracts": policy.contracts},
            # Bounded vectors are single-field tuples for the codec.
            "targets": [[_hex(parse_address(t)) for t in policy.targets]],
        }

    @staticmethod
    def _profile(profile: Profile) -> dict:
        if len(profile.capabilities) > 16:
            raise InvalidInput("at most 16 capabilities")
        metadata = None
        if profile.metadata is not None:
            raw = bytes.fromhex(profile.metadata.removeprefix("0x"))
            if len(raw) != 32:
                raise InvalidInput("metadata must be a 32-byte hash")
            metadata = _hex(raw)
        return {
            "name": _bounded(profile.name, 64, "name"),
            "description": _bounded(profile.description, 512, "description"),
            "endpoint": _bounded(profile.endpoint, 256, "endpoint"),
            "capabilities": [[_bounded(c, 32, "capability") for c in profile.capabilities]],
            "metadata": metadata,
        }

    def _reputation(self, account_hex: str) -> Reputation:
        r = self.substrate.query("AgentRegistry", "Reputations", [account_hex]).value
        return Reputation(
            payments=r["payments"],
            volume=Amount(r["volume"]),
            payers=r["payers"],
            positive=r["positive"],
            negative=r["negative"],
            tasks_completed=r["tasks_completed"],
            tasks_failed=r["tasks_failed"],
        )

    def _record(self, address: str, info: dict) -> AgentRecord:
        p = info["profile"]
        profile = Profile(
            name=_text(p["name"]),
            description=_text(p["description"]),
            endpoint=_text(p["endpoint"]),
            capabilities=[_text(c) for c in _list(p["capabilities"])],
            metadata=p["metadata"],
        )
        return AgentRecord(
            address=address,
            owner=_account(info["owner"]) if info["owner"] else None,
            owner_confirmed=info["owner_confirmed"],
            active=info["active"],
            registered_at=info["registered_at"],
            profile=profile,
            reputation=self._reputation(_hex(parse_address(address))),
        )
