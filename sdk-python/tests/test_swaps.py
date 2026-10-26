"""Swaps. The swap itself runs against a development chain and a local EVM chain with the swap
contract deployed (`swaps/evm/deploy.sh` of the chain):

    tesserax --dev --tmp --mine --author <address>
    anvil
    TESSERAX_URL=ws://127.0.0.1:9944 EVM_URL=http://127.0.0.1:8545 pytest tests/test_swaps.py
"""

import os

import pytest

from tesserax import (
    Amount,
    Claimed,
    Client,
    EvmNetwork,
    EvmSwaps,
    Keypair,
    Payment,
    hash_of,
    new_secret,
    payment_due,
    tess_for,
)
from tesserax.errors import InvalidInput
from tesserax.swaps import PLANCK_PER_TESS, SWAP_CONTRACT, ZERO_ADDRESS, asset_of, evm_networks, token_of

URL = os.environ.get("TESSERAX_URL")
EVM_URL = os.environ.get("EVM_URL")



def anvil_key(index: int) -> str:
    """Key of a development account of anvil, from its public test mnemonic."""
    from eth_account import Account

    Account.enable_unaudited_hdwallet_features()
    mnemonic = "test test test test test test test test test test test junk"
    return Account.from_mnemonic(mnemonic, account_path=f"m/44'/60'/0'/0/{index}").key.hex()


EVM_MAKER, EVM_TAKER = (anvil_key(5), anvil_key(6)) if EVM_URL else ("", "")


def local_network() -> EvmNetwork:
    return EvmNetwork("Base", "Local EVM", 31_337, EVM_URL or "", ZERO_ADDRESS, None, "")


def test_payments_round_up_and_buy_back_the_amount():
    # 0.05 USDC (6 decimals) per TESS.
    price = 50_000
    assert payment_due(100 * PLANCK_PER_TESS, price) == 5_000_000
    assert payment_due(1, price) == 1
    for amount in (1, 999, PLANCK_PER_TESS, 123_456_789_012_345):
        due = payment_due(amount, price)
        assert tess_for(due, price) >= amount
        assert tess_for(due - 1, price) < amount
    assert tess_for(1_000, 0) == 0


def test_secrets_hash_with_sha256():
    assert hash_of(bytes(32)).hex() == "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
    secret, hash_ = new_secret()
    assert hash_of(secret) == hash_
    assert new_secret()[0] != secret


def test_payments_and_networks():
    assert (Payment("Bitcoin", "Native").symbol, Payment("Bitcoin", "Native").decimals) == ("BTC", 8)
    assert (Payment("Base", "Native").symbol, Payment("Base", "Native").decimals) == ("ETH", 18)
    assert (Payment("Arbitrum", "Usdt").symbol, Payment("Arbitrum", "Usdt").decimals) == ("USDT", 6)
    assert [n.chain_id for n in evm_networks(testnet=True)] == [11_155_111, 84_532, 421_614]
    base = next(n for n in evm_networks(testnet=False) if n.chain == "Base")
    assert token_of(base, "Usdt") is None
    assert asset_of(base, base.usdc.lower()) == "Usdc"
    assert asset_of(base, ZERO_ADDRESS) == "Native"


@pytest.mark.chain
@pytest.mark.skipif(not URL or not EVM_URL, reason="set TESSERAX_URL and EVM_URL")
def test_a_swap_of_tess_against_eth():
    with Client(URL) as client:
        taker_evm = EvmSwaps(local_network(), private_key=EVM_TAKER)
        maker_evm = EvmSwaps(local_network(), private_key=EVM_MAKER)
        payout = maker_evm.account.address

        # The maker offers 100 TESS at 0.001 ETH.
        maker = Keypair.from_uri("//Alice")
        price = 10**15
        offer = client.swaps.create_offer(
            maker, Payment("Base", "Native"), price, Amount.from_tess("100"), Amount.from_tess("1"), bytes.fromhex(payout[2:])
        )
        assert client.swaps.offer(offer).available == Amount.from_tess("100")

        # A taker without TESS locks the payment of 2 TESS.
        taker, _ = Keypair.generate()
        amount = Amount.from_tess("2")
        due = payment_due(amount.planck, price)
        secret, hash_ = new_secret()
        start, now = taker_evm.head()
        lock = taker_evm.lock_payment("Native", payout, due, hash_, now + 86_400, offer, taker.address)

        # The maker finds it, checks it and fills.
        found = [l for l in maker_evm.locks(start, maker_evm.head()[0], payout) if l.id == lock]
        assert len(found) == 1
        assert (found[0].offer, found[0].taker, found[0].state) == (offer, taker.address, "Locked")
        assert maker_evm.amount_of(found[0], Payment("Base", "Native")) == due
        assert tess_for(found[0].amount, price) == amount.planck
        trade = client.swaps.fill(maker, offer, found[0].taker, amount, found[0].hash, lock)
        assert client.swaps.trade_of_lock("Base", lock) == trade
        assert client.swaps.trade(trade).taker == taker.address

        # The taker claims without fee; the secret pays the maker.
        claimed = client.swaps.claim(trade, secret)
        assert client.balance(taker.address).free == amount
        assert client.swaps.trade(trade) is None
        revealed = [e for e in client.swaps.events(claimed.block_number) if isinstance(e, Claimed) and e.trade == trade]
        assert revealed and revealed[0].secret == secret
        before = maker_evm.balance(payout, "Native")
        maker_evm.claim(lock, revealed[0].secret)
        assert maker_evm.lock_status(lock).state == "Claimed"
        assert maker_evm.balance(payout, "Native") > before - 10**15

        client.swaps.cancel_offer(maker, offer)
        assert client.swaps.offer(offer) is None


@pytest.mark.skipif(not EVM_URL, reason="set EVM_URL")
def test_no_payment_is_locked_where_the_contract_is_missing():
    evm = EvmSwaps(local_network(), private_key=EVM_TAKER)
    provider = evm.web3.provider
    code = evm.web3.eth.get_code(SWAP_CONTRACT)
    snapshot = provider.make_request("evm_snapshot", [])["result"]
    try:
        provider.make_request("anvil_setCode", [SWAP_CONTRACT, "0x"])
        with pytest.raises(InvalidInput, match="not deployed on Local EVM"):
            evm.lock_payment("Native", evm.account.address, 1, bytes(32), 4_000_000_000, 0, Keypair.from_uri("//Alice").address)
    finally:
        provider.make_request("evm_revert", [snapshot])
    assert evm.web3.eth.get_code(SWAP_CONTRACT) == code
