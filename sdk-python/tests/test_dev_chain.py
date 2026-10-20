"""Against a running development chain:

    tesserax --dev --tmp --mine --author <address>
    TESSERAX_URL=ws://127.0.0.1:9944 pytest -m chain
"""

import os

import pytest

from tesserax import Amount, Client, Keypair, Policy, Profile, TransactionFailed

URL = os.environ.get("TESSERAX_URL")
pytestmark = [pytest.mark.chain, pytest.mark.skipif(not URL, reason="set TESSERAX_URL to a development node")]


def tess(text):
    return Amount.from_tess(text)


def test_agents_on_a_development_chain():
    with Client(URL) as client:
        info = client.chain_info()
        assert info.ss58_prefix == 2261

        owner, merchant = Keypair.from_uri("//Alice"), Keypair.from_uri("//Bob")
        agent, _ = Keypair.generate()

        client.transfer(owner, agent.address, tess("1"))
        client.authorize_agent(owner, agent.address, Policy(budget=tess("10"), max_per_call=tess("6")))
        delegation = client.delegation(owner.address, agent.address)
        assert delegation.remaining == tess("10")

        before = client.balance(merchant.address).free
        client.agent_transfer(agent, owner.address, merchant.address, tess("5"))
        assert client.balance(merchant.address).free == before + tess("5")
        assert client.delegation(owner.address, agent.address).remaining == tess("5")

        with pytest.raises(TransactionFailed) as refused:
            client.agent_transfer(agent, owner.address, merchant.address, tess("6"))
        assert refused.value.error and refused.value.error.endswith("BudgetExceeded"), refused.value

        if client.agent(merchant.address) is None:
            profile = Profile(
                name="translator",
                description="French to English",
                endpoint="https://translator.example/mcp",
                capabilities=["translation"],
            )
            client.register_agent(merchant, profile)
        assert any(a.address == merchant.address for a in client.find_agents("translation"))

        payments = client.agent(merchant.address).reputation.payments
        client.agent_pay_agent(agent, owner.address, merchant.address, tess("1"))
        assert client.agent(merchant.address).reputation.payments == payments + 1

        client.revoke_agent(owner, agent.address)
        assert client.delegation(owner.address, agent.address) is None
