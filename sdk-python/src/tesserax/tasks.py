"""Task market: work ordered between agents, with the reward held in escrow. See
`docs/agents.md`."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import TYPE_CHECKING, Optional

from .amount import Amount
from .errors import InvalidInput, TransactionFailed
from .keys import format_address, parse_address

if TYPE_CHECKING:
    from .client import Client, TxOutcome
    from .keys import Keypair


def blocks_for(minutes: float) -> int:
    """Blocks of 18 seconds covering `minutes`."""
    return math.ceil(minutes * 60 / 18)


@dataclass
class NewTask:
    """A task to create."""

    #: What to do: a URI or a short text, at most 256 bytes.
    spec: str
    #: Reward, held in escrow until the task ends.
    reward: Amount
    #: Blocks the worker has to deliver after accepting.
    delivery_blocks: int
    #: Blocks the client has to approve or dispute after the delivery.
    review_blocks: int
    #: Capability tag of the work, such as "translation".
    capability: Optional[str] = None
    #: The only worker allowed to accept.
    invited: Optional[str] = None
    #: Arbiter of disputes and its fee. Without arbiter, results are approved by the client or
    #: automatically after the review period.
    arbiter: Optional[tuple[str, Amount]] = None
    #: Bond the worker holds, lost if it does not deliver in time.
    bond: Amount = Amount(0)


@dataclass
class TaskInfo:
    id: int
    client: str
    spec: str
    capability: Optional[str]
    reward: Amount
    invited: Optional[str]
    arbiter: Optional[tuple[str, Amount]]
    bond: Amount
    delivery_blocks: int
    review_blocks: int
    #: "open", "assigned", "delivered" or "disputed".
    state: str
    worker: Optional[str] = None
    result: Optional[str] = None
    #: Deadline of the delivery, end of the review or of the arbitration, by state.
    until: Optional[int] = None


def _hex_text(text: str, limit: int, name: str) -> str:
    raw = text.encode()
    if len(raw) > limit:
        raise InvalidInput(f"{name} is longer than {limit} bytes")
    return "0x" + raw.hex()


def _text(value) -> str:
    if isinstance(value, (list, tuple)) and len(value) == 1:
        value = value[0]
    if isinstance(value, str) and value.startswith("0x"):
        try:
            return bytes.fromhex(value[2:]).decode()
        except ValueError:
            return value
    return str(value)


def _address(value) -> str:
    if isinstance(value, str) and value.startswith("0x"):
        return format_address(bytes.fromhex(value[2:]))
    return format_address(parse_address(value))


def task_info(task_id: int, t: dict) -> TaskInfo:
    status = t["status"]
    state, worker, result, until = "open", None, None, None
    if isinstance(status, dict):
        (variant, fields), = status.items()
        state = variant.lower()
        worker = _address(fields["worker"])
        result = _text(fields["result"]) if "result" in fields else None
        until = fields.get("deadline") or fields.get("review_until") or fields.get("until")
    arbiter = t["arbiter"]
    return TaskInfo(
        id=task_id,
        client=_address(t["client"]),
        spec=_text(t["spec"]),
        capability=_text(t["capability"]) if t["capability"] is not None else None,
        reward=Amount(t["reward"]),
        invited=_address(t["invited"]) if t["invited"] else None,
        arbiter=(_address(arbiter["account"]), Amount(arbiter["fee"])) if arbiter else None,
        bond=Amount(t["bond"]),
        delivery_blocks=t["delivery_period"],
        review_blocks=t["review_period"],
        state=state,
        worker=worker,
        result=result,
        until=until,
    )


class Tasks:
    """Task calls and queries. Every call takes an optional owner to act through the signer's
    delegation."""

    def __init__(self, client: "Client"):
        self.client = client

    def _send(self, signer: "Keypair", owner: Optional[str], function: str, params: dict) -> "TxOutcome":
        if owner:
            return self.client.agent_execute(signer, owner, "Tasks", function, params)
        return self.client.submit(signer, "Tasks", function, params)

    def create(self, signer: "Keypair", task: NewTask, owner: Optional[str] = None) -> int:
        """Create a task; returns its id."""
        params = {
            "spec": _hex_text(task.spec, 256, "spec"),
            "capability": _hex_text(task.capability, 32, "capability") if task.capability else None,
            "reward": task.reward.planck,
            "invited": "0x" + parse_address(task.invited).hex() if task.invited else None,
            "arbiter": {"account": "0x" + parse_address(task.arbiter[0]).hex(), "fee": task.arbiter[1].planck}
            if task.arbiter
            else None,
            "bond": task.bond.planck,
            "delivery_period": task.delivery_blocks,
            "review_period": task.review_blocks,
        }
        outcome = self._send(signer, owner, "create", params)
        for pallet, event, attributes in outcome.events:
            if pallet == "Tasks" and event == "Created":
                return attributes["id"] if isinstance(attributes, dict) else attributes[0]
        raise TransactionFailed("the task was not created")

    def cancel(self, signer: "Keypair", task_id: int, owner: Optional[str] = None) -> "TxOutcome":
        return self._send(signer, owner, "cancel", {"id": task_id})

    def accept(self, signer: "Keypair", task_id: int, owner: Optional[str] = None) -> "TxOutcome":
        return self._send(signer, owner, "accept", {"id": task_id})

    def deliver(self, signer: "Keypair", task_id: int, result: str, owner: Optional[str] = None) -> "TxOutcome":
        return self._send(signer, owner, "deliver", {"id": task_id, "result": _hex_text(result, 256, "result")})

    def approve(self, signer: "Keypair", task_id: int, owner: Optional[str] = None) -> "TxOutcome":
        return self._send(signer, owner, "approve", {"id": task_id})

    def dispute(self, signer: "Keypair", task_id: int, owner: Optional[str] = None) -> "TxOutcome":
        return self._send(signer, owner, "dispute", {"id": task_id})

    def resolve(self, arbiter: "Keypair", task_id: int, worker_percent: int) -> "TxOutcome":
        """Decide a dispute as the arbiter: `worker_percent` of the reward goes to the worker."""
        if not 0 <= worker_percent <= 100:
            raise InvalidInput("a share is from 0 to 100 percent")
        return self._send(arbiter, None, "resolve", {"id": task_id, "worker_share": worker_percent})

    def finalize(self, signer: "Keypair", task_id: int) -> "TxOutcome":
        """Finalize a task whose time is up; anyone can call it."""
        return self._send(signer, None, "finalize", {"id": task_id})

    def get(self, task_id: int) -> Optional[TaskInfo]:
        result = self.client.substrate.query("Tasks", "Tasks", [task_id])
        return task_info(task_id, result.value) if result.value is not None else None

    def all(self) -> list[TaskInfo]:
        """Tasks in progress, newest first."""
        tasks = [task_info(key.value, value.value) for key, value in self.client.substrate.query_map("Tasks", "Tasks", page_size=100)]
        return sorted(tasks, key=lambda t: t.id, reverse=True)

    def open(self, capability: Optional[str] = None) -> list[TaskInfo]:
        """Open tasks, optionally only those of `capability`, best paid first."""
        tasks = [t for t in self.all() if t.state == "open" and (capability is None or t.capability == capability)]
        return sorted(tasks, key=lambda t: t.reward.planck, reverse=True)
