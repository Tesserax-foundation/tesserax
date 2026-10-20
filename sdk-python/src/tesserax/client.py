"""Client of a Tesserax node."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from substrateinterface import SubstrateInterface
from substrateinterface.exceptions import SubstrateRequestException

from .amount import DECIMALS, Amount
from .errors import InvalidInput, TransactionFailed
from .keys import SS58_PREFIX, Keypair, format_address, parse_address

#: Blocks a transaction stays valid, counted from the best block: about 19 minutes.
MORTALITY = 64


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
        self.substrate = SubstrateInterface(url=url, ss58_format=SS58_PREFIX)

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
        s = self.substrate
        call = s.compose_call(call_module=module, call_function=function, call_params=params)
        nonce = s.rpc_request("system_accountNextIndex", [signer.address])["result"]
        era = {"period": MORTALITY, "current": s.get_block_number(None)}
        payload = s.generate_signature_payload(call=call, era=era, nonce=nonce, tip=0)
        signature = signer.sign(payload.data)

        extrinsic = s.runtime_config.create_scale_object(type_string="Extrinsic", metadata=s.metadata)
        extrinsic.encode(
            {
                "account_id": _hex(signer.account_id),
                "signature": {"signer": _hex(signer.public_key), "signature": _hex(signature)},
                "call_function": call.value["call_function"],
                "call_module": call.value["call_module"],
                "call_args": call.value["call_args"],
                "nonce": nonce,
                "era": era,
                "tip": 0,
                "mode": "Disabled",
            }
        )
        try:
            receipt = s.submit_extrinsic(extrinsic, wait_for_inclusion=True)
        except SubstrateRequestException as error:
            raise TransactionFailed(f"transaction refused: {error}") from None
        if not receipt.is_success:
            message = receipt.error_message or {}
            name = message.get("name")
            module_name = message.get("module") or message.get("type")
            error = f"{module_name}::{name}" if module_name and name else name
            raise TransactionFailed(f"transaction failed: {error or message}", error)
        return TxOutcome(block_hash=receipt.block_hash, extrinsic_hash=receipt.extrinsic_hash)

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
