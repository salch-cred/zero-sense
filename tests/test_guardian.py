"""Tests for ZeroSense Guardian v2 — all 7 autonomous agents."""
import asyncio
import pytest
from unittest.mock import AsyncMock, MagicMock
from api.agents.guardian import (
    ZeroSenseGuardianV2, PaymentAgent, AnomalyAgent,
    InsuranceAgent, ReputationAgent,
)


@pytest.fixture
def stellar():
    m = MagicMock()
    m.get_recent_contract_events = AsyncMock(return_value=[])
    m.claim_payment = AsyncMock(return_value="tx_mock_paid")
    m.file_insurance_claim = AsyncMock(return_value=42)
    return m

@pytest.fixture
def guardian(stellar):
    return ZeroSenseGuardianV2(stellar_client=stellar)

def run(coro):
    return asyncio.get_event_loop().run_until_complete(coro)


class TestGuardianInit:
    def test_not_running(self, guardian): assert guardian.running is False
    def test_has_7_agents(self, guardian): assert len(guardian.agents) == 7
    def test_has_payment(self, guardian): assert "payment" in guardian.agents
    def test_has_anomaly(self, guardian): assert "anomaly" in guardian.agents
    def test_has_insurance(self, guardian): assert "insurance" in guardian.agents
    def test_has_reputation(self, guardian): assert "reputation" in guardian.agents
    def test_has_learning(self, guardian): assert "learning" in guardian.agents
    def test_has_oracle(self, guardian): assert "oracle" in guardian.agents
    def test_has_assistant(self, guardian): assert "assistant" in guardian.agents
    def test_status_returns_all(self, guardian):
        assert set(guardian.status().keys()) == {
            "payment","anomaly","insurance","reputation","learning","oracle","assistant"
        }
    def test_initial_counts_zero(self, guardian):
        for info in guardian.status().values():
            assert info["processed"] == 0


class TestPaymentAgent:
    def test_init(self, stellar):
        a = PaymentAgent(stellar)
        assert a.threshold == 0.95 and len(a.paid_tasks) == 0

    def test_no_events_no_pay(self, stellar):
        a = PaymentAgent(stellar)
        run(a.tick())
        assert a.processed_count == 0

    def test_high_conf_pays(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"task_id":"t001","confidence":97}]
        )
        a = PaymentAgent(stellar)
        run(a.tick())
        assert a.processed_count == 1
        stellar.claim_payment.assert_called_once()

    def test_low_conf_skips(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"task_id":"t002","confidence":80}]
        )
        a = PaymentAgent(stellar)
        run(a.tick())
        assert a.processed_count == 0
        stellar.claim_payment.assert_not_called()

    def test_no_duplicate_pay(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"task_id":"t003","confidence":98}]
        )
        a = PaymentAgent(stellar)
        run(a.tick())
        run(a.tick())   # second tick — same task
        assert stellar.claim_payment.call_count == 1


class TestAnomalyAgent:
    def test_high_conf_no_count(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"robot_id":"r1","confidence":97}]
        )
        a = AnomalyAgent(stellar)
        run(a.tick())
        assert a.low_confidence_counts.get("r1", 0) == 0

    def test_low_conf_increments(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"robot_id":"r1","confidence":70}]
        )
        a = AnomalyAgent(stellar)
        run(a.tick())
        assert a.low_confidence_counts["r1"] == 1

    def test_killswitch_at_threshold(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"robot_id":"r-bad","confidence":50}]
        )
        a = AnomalyAgent(stellar, anomaly_threshold=3)
        for _ in range(3):
            run(a.tick())
        assert a.processed_count >= 1

    def test_good_reading_resets(self, stellar):
        a = AnomalyAgent(stellar)
        a.low_confidence_counts["r1"] = 2
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"robot_id":"r1","confidence":98}]
        )
        run(a.tick())
        assert a.low_confidence_counts["r1"] == 0


class TestInsuranceAgent:
    def test_files_claim(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"proof_hash":"deadbeef"*8,"robot_id":"r1","estimated_claim":1_000_000}]
        )
        a = InsuranceAgent(stellar)
        run(a.tick())
        assert a.processed_count == 1
        stellar.file_insurance_claim.assert_called_once()

    def test_no_duplicate_claim(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"proof_hash":"cafebabe"*8,"robot_id":"r1","estimated_claim":500_000}]
        )
        a = InsuranceAgent(stellar)
        run(a.tick())
        run(a.tick())
        assert stellar.file_insurance_claim.call_count == 1


class TestReputationAgent:
    def test_processes_payment_event(self, stellar):
        stellar.get_recent_contract_events = AsyncMock(
            return_value=[{"task_id":"trep1","robot_id":"r1","confidence":97}]
        )
        a = ReputationAgent(stellar)
        run(a.tick())
        assert a.processed_count == 1
        assert "trep1" in a.processed_tasks
