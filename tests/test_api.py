"""Tests for FastAPI endpoints (api/main.py).

Uses FastAPI TestClient — no server startup needed.
"""
import pytest
from fastapi.testclient import TestClient
from api.main import app


@pytest.fixture(scope="module")
def c():
    return TestClient(app)


# ── Root ──
class TestRoot:
    def test_200(self, c): assert c.get("/").status_code == 200
    def test_project(self, c): assert c.get("/").json()["project"] == "ZeroSense"
    def test_status(self, c): assert c.get("/").json()["status"] == "live"
    def test_hackathon(self, c): assert "Stellar Hacks" in c.get("/").json()["hackathon"]


# ── Generate Proof ──
class TestGenerateProof:
    def _req(self, c, pixels=None, task_id=None):
        body = {
            "robot_id": "robot-001",
            "sensor_frames": [{"pixels": pixels or [0.8]*64, "frame_id": 0}],
        }
        if task_id:
            body["task_id"] = task_id
        return c.post("/generate-proof", json=body)

    def test_200(self, c): assert self._req(c).status_code == 200
    def test_has_task_id(self, c): assert "task_id" in self._req(c).json()
    def test_has_proof_hex(self, c): assert "proof_hex" in self._req(c).json()
    def test_has_confidence(self, c): assert "confidence" in self._req(c).json()
    def test_has_action_label(self, c): assert "action_label" in self._req(c).json()
    def test_has_input_hash(self, c): assert "input_hash" in self._req(c).json()
    def test_confidence_range(self, c):
        assert 0 <= self._req(c).json()["confidence"] <= 100
    def test_action_label_valid(self, c):
        assert self._req(c).json()["action_label"] in ("task_complete","obstacle_detected","incident")
    def test_explicit_task_id(self, c):
        r = self._req(c, task_id="mytask00000000000000000000000001")
        assert r.json()["task_id"] == "mytask00000000000000000000000001"
    def test_mock_proof_512_chars(self, c):
        assert len(self._req(c).json()["proof_hex"]) == 512
    def test_multi_frame(self, c):
        r = c.post("/generate-proof", json={
            "robot_id": "robot-001",
            "sensor_frames": [
                {"pixels": [0.8]*64, "frame_id": 0},
                {"pixels": [0.9]*64, "frame_id": 1},
            ]
        })
        assert r.status_code == 200


# ── Verify Proof ──
class TestVerifyProof:
    def _verify(self, c, confidence=97, task_id="task_v_001"):
        return c.post("/verify-proof", json={
            "robot_id": "robot-001", "task_id": task_id,
            "proof_hex": "0"*512, "model_hash": "mobilenet_v2_int8",
            "confidence": confidence, "action_type": 0,
        })

    def test_200(self, c): assert self._verify(c).status_code == 200
    def test_status_verified(self, c): assert self._verify(c).json()["status"] == "verified"
    def test_auto_pay_high_conf(self, c): assert self._verify(c, 98, "task_v_002").json()["auto_payment"] is True
    def test_no_auto_pay_low_conf(self, c): assert self._verify(c, 80, "task_v_003").json()["auto_payment"] is False
    def test_has_stellar_tx(self, c): assert self._verify(c, task_id="task_v_004").json()["stellar_tx"] is not None


# ── Claim Payment ──
class TestClaimPayment:
    def test_200(self, c):
        assert c.post("/claim-payment", json={"task_id":"task_c_001","confidence":96}).status_code == 200
    def test_paid_status(self, c):
        assert c.post("/claim-payment", json={"task_id":"task_c_002","confidence":97}).json()["status"] == "paid"
    def test_has_stellar_tx(self, c):
        assert "stellar_tx" in c.post("/claim-payment", json={"task_id":"task_c_003","confidence":95}).json()


# ── Insurance Claim ──
class TestInsurance:
    def test_200(self, c):
        r = c.post("/file-insurance-claim", json={
            "robot_id":"robot-001","incident_proof_hash":"abc"*21+"d","claim_amount":1_000_000
        })
        assert r.status_code == 200
    def test_filed_status(self, c):
        r = c.post("/file-insurance-claim", json={
            "robot_id":"robot-001","incident_proof_hash":"def"*21+"g","claim_amount":500_000
        })
        assert r.json()["status"] == "filed"


# ── Robot Identity ──
class TestIdentity:
    def test_200(self, c):
        r = c.post("/robot/register-identity", json={
            "robot_id":"robot-001","sensor_noise_sample":[0.01]*64
        })
        assert r.status_code == 200
    def test_has_identity_hash(self, c):
        r = c.post("/robot/register-identity", json={
            "robot_id":"robot-002","sensor_noise_sample":[0.02]*64
        })
        assert len(r.json()["identity_hash"]) == 64
    def test_different_robots_different_hashes(self, c):
        r1 = c.post("/robot/register-identity", json={"robot_id":"rA","sensor_noise_sample":[0.01]*64})
        r2 = c.post("/robot/register-identity", json={"robot_id":"rB","sensor_noise_sample":[0.99]*64})
        assert r1.json()["identity_hash"] != r2.json()["identity_hash"]


# ── Robot Status + Fleet ──
class TestStatus:
    def test_robot_status_200(self, c): assert c.get("/robot/robot-001/status").status_code == 200
    def test_robot_has_fields(self, c):
        d = c.get("/robot/robot-001/status").json()
        assert "robot_id" in d and "zrep_balance" in d
    def test_fleet_200(self, c): assert c.get("/fleet/report").status_code == 200
    def test_fleet_has_fields(self, c):
        d = c.get("/fleet/report").json()
        assert "total_tasks_verified" in d and "guardian_status" in d


# ── Guardian ──
class TestGuardian:
    def test_start_200(self, c): assert c.post("/guardian/start").status_code == 200
    def test_start_status(self, c):
        assert c.post("/guardian/start").json()["status"] in ("started","already_running")
    def test_stop_200(self, c): assert c.post("/guardian/stop").status_code == 200
    def test_stop_status(self, c): assert c.post("/guardian/stop").json()["status"] == "stopped"
    def test_agents_count(self, c):
        c.post("/guardian/stop")
        r = c.post("/guardian/start")
        if "agents" in r.json():
            assert len(r.json()["agents"]) == 7
        c.post("/guardian/stop")
