"""Tests for Stellar SDK client (api/stellar/client.py).

Runs in mock/dev mode — no real Stellar keys needed.
"""
import asyncio
import pytest
from api.stellar.client import StellarClient


@pytest.fixture
def client():
    return StellarClient(secret_key="", network="testnet")


class TestStellarClientInit:

    def test_client_initializes(self, client):
        assert client is not None
        assert client.network == "testnet"

    def test_testnet_urls(self, client):
        assert "testnet" in client.horizon_url
        assert "testnet" in client.soroban_url

    def test_public_key_length(self, client):
        assert len(client.keypair.public_key) == 56

    def test_public_key_starts_with_G(self, client):
        assert client.keypair.public_key.startswith("G")


class TestStellarClientAsync:

    def _run(self, coro):
        return asyncio.get_event_loop().run_until_complete(coro)

    def test_verify_proof_returns_tx(self, client):
        tx = self._run(client.verify_proof_on_chain(
            proof_hex="00" * 256, robot_id="robot-001",
            task_id="task_test_verify", model_hash="mobilenet_v2_int8",
            confidence=97, action_type=0,
        ))
        assert "verified" in tx

    def test_claim_payment_returns_tx(self, client):
        tx = self._run(client.claim_payment(task_id="task_pay_001", confidence=95))
        assert "paid" in tx

    def test_insurance_claim_returns_id(self, client):
        cid = self._run(client.file_insurance_claim(
            robot_id="robot-001", proof_hash="abc" * 20, claim_amount=1_000_000
        ))
        assert isinstance(cid, int) and cid >= 1

    def test_zrep_balance_returns_int(self, client):
        b = self._run(client.get_zrep_balance("robot-001"))
        assert isinstance(b, int)

    def test_mint_soulbound_returns_tx(self, client):
        tx = self._run(client.mint_soulbound_identity_token("robot-001", "deadbeef" * 8))
        assert "identity" in tx

    def test_events_returns_list(self, client):
        events = self._run(client.get_recent_contract_events("RobotActionVerified"))
        assert isinstance(events, list)
