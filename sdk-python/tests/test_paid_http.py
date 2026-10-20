"""Against a running development chain:

    tesserax --dev --tmp --mine --author <address>
    TESSERAX_URL=ws://127.0.0.1:9944 pytest -m chain
"""

import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from tesserax import (
    PAYMENT_HEADER,
    VOUCHER_HEADER,
    Amount,
    ChannelPayer,
    ChannelReceiver,
    Client,
    InvalidInput,
    Keypair,
    PaymentRejected,
    Policy,
)

URL = os.environ.get("TESSERAX_URL")
pytestmark = [pytest.mark.chain, pytest.mark.skipif(not URL, reason="set TESSERAX_URL to a development node")]


def tess(text):
    return Amount.from_tess(text)


def paid_service(receiver, price):
    served = []

    class Handler(BaseHTTPRequestHandler):
        def ask_payment(self, rejection=None):
            self.send_response(402)
            self.send_header(PAYMENT_HEADER, receiver.payment_required(price, rejection))
            self.end_headers()

        def do_GET(self):
            voucher = self.headers.get(VOUCHER_HEADER)
            if voucher is None:
                return self.ask_payment()
            try:
                receiver.verify(voucher, price)
            except PaymentRejected as rejected:
                return self.ask_payment(rejected.rejection)
            served.append(voucher)
            body = f"translation number {len(served)}".encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, f"http://127.0.0.1:{server.server_address[1]}"


def test_an_agent_pays_a_service_per_request():
    owner, service_key = Keypair.from_uri("//Alice"), Keypair.from_uri("//Bob")
    agent, _ = Keypair.generate()
    with Client(URL) as client, Client(URL) as service_client:
        client.transfer(owner, agent.address, tess("1"))
        client.authorize_agent(owner, agent.address, Policy(budget=tess("5"), max_per_call=tess("3")))

        receiver = ChannelReceiver(service_client, service_key)
        server, service = paid_service(receiver, tess("0.001"))
        try:
            payer = ChannelPayer(client, agent, deposit=tess("2"), max_price=tess("0.01"), owner=owner.address)
            for request in range(1, 6):
                status, _, body = payer.request(f"{service}/translate")
                assert status == 200
                assert body.decode() == f"translation number {request}"
            (channel,) = payer.channels.values()
            assert str(channel.spent) == "0.005"

            with pytest.raises(InvalidInput, match="more than the maximum"):
                payer.request(f"{service}/translate", max_price=tess("0.0001"))

            assert str(receiver.unclaimed()) == "0.005"
            before = service_client.balance(service_key.address).free.planck
            receiver.claim_all()
            fee = before + tess("0.005").planck - service_client.balance(service_key.address).free.planck
            assert 0 < fee < tess("0.005").planck
        finally:
            server.shutdown()
        client.revoke_agent(owner, agent.address)
