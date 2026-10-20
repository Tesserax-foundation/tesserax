"""Against a running development chain:

    tesserax --dev --tmp --mine --author <address>
    TESSERAX_URL=ws://127.0.0.1:9944 pytest -m chain
"""

import os

import pytest

from tesserax import Amount, Client, Keypair, NewTask, Policy, Tasks, TransactionFailed, blocks_for

URL = os.environ.get("TESSERAX_URL")
pytestmark = [pytest.mark.chain, pytest.mark.skipif(not URL, reason="set TESSERAX_URL to a development node")]


def tess(text):
    return Amount.from_tess(text)


def test_agents_trade_work_through_the_task_market():
    owner, worker = Keypair.from_uri("//Alice"), Keypair.from_uri("//Bob")
    agent, _ = Keypair.generate()
    with Client(URL) as client:
        tasks = Tasks(client)
        client.transfer(owner, agent.address, tess("1"))
        client.authorize_agent(owner, agent.address, Policy(budget=tess("10"), max_per_call=tess("5")))

        task_id = tasks.create(
            agent,
            NewTask(
                spec="Translate https://example.com/contract-fr.txt to English",
                capability="translation",
                reward=tess("2"),
                bond=tess("0.1"),
                delivery_blocks=blocks_for(30),
                review_blocks=blocks_for(30),
            ),
            owner=owner.address,
        )
        assert any(t.id == task_id and t.client == owner.address for t in tasks.open("translation"))

        tasks.accept(worker, task_id)
        tasks.deliver(worker, task_id, "ipfs://contract-en")
        delivered = tasks.get(task_id)
        assert delivered.state == "delivered" and delivered.result == "ipfs://contract-en"

        with pytest.raises(TransactionFailed):
            tasks.dispute(agent, task_id, owner=owner.address)

        before = client.balance(worker.address).free
        tasks.approve(agent, task_id, owner=owner.address)
        assert client.balance(worker.address).free == before + tess("2.1")
        assert tasks.get(task_id) is None

        client.revoke_agent(owner, agent.address)
