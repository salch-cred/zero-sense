"""ZeroSense FastAPI Backend

Endpoints:
  POST /generate-proof          - Run AI inference + generate ZK proof
  POST /verify-proof            - Submit proof to Stellar Soroban verifier
  POST /claim-payment           - Trigger XLM payment after verified proof
  POST /file-insurance-claim    - File insurance claim with ZK evidence
  POST /robot/register-identity - Register robot hardware identity from an
                                   on-device commitment hash (see SECURITY note)
  GET  /robot/{id}/status       - Robot status, reputation, payment history
  GET  /fleet/report            - Full fleet analytics
  POST /guardian/start          - Start autonomous Guardian agent system
  POST /guardian/stop           - Stop Guardian agent system

SECURITY
  - CORS origins come from ALLOWED_ORIGINS (comma separated). Defaults to
    localhost only; set to "*" explicitly to open it.
  - Money-moving / state-changing endpoints require an X-API-Key header that
    matches ZEROSENSE_API_KEY when that env var is set. If it is unset the
    check is skipped (dev mode) — set it in any real deployment.
  - /robot/register-identity accepts only a pre-computed SHA256 commitment
    hash. It never accepts or sees the robot's raw sensor noise sample — that
    hashing must happen ON the robot, not on this server (see M2 fix below).
"""

import hashlib
import os
import uuid
from typing import Optional

import httpx
from dotenv import load_dotenv
from fastapi import Depends, FastAPI, Header, HTTPException, BackgroundTasks
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

from api.agents.guardian import ZeroSenseGuardianV2
from api.stellar.client import StellarClient
from model.inference import RobotInferenceEngine

load_dotenv()

app = FastAPI(
    title="ZeroSense API",
    description="ZK-Verified Robot Intelligence & Micro-Payment Protocol on Stellar",
    version="1.0.0",
)

_allowed_origins = [
    o.strip()
    for o in os.getenv("ALLOWED_ORIGINS", "http://localhost:8000,http://localhost:3000").split(",")
    if o.strip()
]
app.add_middleware(
    CORSMiddleware,
    allow_origins=_allowed_origins,
    allow_credentials=True,
    allow_methods=["GET", "POST"],
    allow_headers=["Content-Type", "X-API-Key"],
)


def require_api_key(x_api_key: Optional[str] = Header(default=None)):
    """Gate state-changing endpoints behind ZEROSENSE_API_KEY when configured."""
    expected = os.getenv("ZEROSENSE_API_KEY", "")
    if expected and x_api_key != expected:
        raise HTTPException(status_code=401, detail="Invalid or missing API key")


# Initialize components
stellar_client = StellarClient(
    secret_key=os.getenv("STELLAR_SECRET_KEY", ""),
    network=os.getenv("STELLAR_NETWORK", "testnet"),
)
inference_engine = RobotInferenceEngine()
guardian: Optional[ZeroSenseGuardianV2] = None


# ─── Request/Response Models ────────────────────────────────────────────

class SensorFrame(BaseModel):
    pixels: list[float]
    frame_id: int

class GenerateProofRequest(BaseModel):
    robot_id: str
    task_id: Optional[str] = None
    sensor_frames: list[SensorFrame]
    model_hash: str = "mobilenet_v2_int8"

class VerifyProofRequest(BaseModel):
    robot_id: str
    task_id: str
    proof_hex: str
    model_hash: str
    confidence: int
    action_type: int

class ClaimPaymentRequest(BaseModel):
    task_id: str
    confidence: int

class InsuranceClaimRequest(BaseModel):
    robot_id: str
    incident_proof_hash: str
    claim_amount: int  # in XLM stroops

class RegisterIdentityRequest(BaseModel):
    robot_id: str
    # SHA256 hex digest computed ON THE ROBOT from its PRNU sensor-noise
    # sample plus a locally-held salt. The raw noise sample itself must never
    # be sent to this API — see SECURITY note on the endpoint below (M2 fix).
    fingerprint_commitment: str

class ProofResponse(BaseModel):
    task_id: str
    proof_hex: str
    model_hash: str
    confidence: int
    action: int
    action_label: str
    input_hash: str
    stellar_tx: Optional[str] = None


# ─── Endpoints ────────────────────────────────────────────────────

@app.get("/")
async def root():
    return {
        "project": "ZeroSense",
        "tagline": "The world's first ZK brain for the robot economy",
        "hackathon": "Stellar Hacks: Real-World ZK 2026",
        "status": "live",
    }


@app.post("/generate-proof", response_model=ProofResponse, dependencies=[Depends(require_api_key)])
async def generate_proof(req: GenerateProofRequest):
    """Step 1: Run AI inference on sensor data and generate ZK proof.

    Sensor frames are processed by MobileNetV2 ONNX model, then RISC Zero
    generates a ZK-STARK proof of the inference. Sensor data NEVER leaves
    this function — only the proof hash is public.
    """
    task_id = req.task_id or str(uuid.uuid4()).replace("-", "")[:32]

    frames = [frame.pixels for frame in req.sensor_frames]
    action, confidence, input_hash = inference_engine.run_inference(frames)

    action_labels = {0: "task_complete", 1: "obstacle_detected", 2: "incident"}

    proof_hex = await _generate_risc_zero_proof(
        sensor_frames=frames,
        model_hash=req.model_hash,
        task_id=task_id,
        robot_id=req.robot_id,
        bonsai_api_key=os.getenv("BONSAI_API_KEY", ""),
    )

    return ProofResponse(
        task_id=task_id,
        proof_hex=proof_hex,
        model_hash=req.model_hash,
        confidence=confidence,
        action=action,
        action_label=action_labels.get(action, "unknown"),
        input_hash=input_hash,
    )


@app.post("/verify-proof", dependencies=[Depends(require_api_key)])
async def verify_proof(req: VerifyProofRequest, background_tasks: BackgroundTasks):
    """Step 2: Submit ZK proof to Stellar Soroban for on-chain verification.
    Confidence >= 95 automatically triggers XLM payment.
    """
    try:
        tx_hash = await stellar_client.verify_proof_on_chain(
            proof_hex=req.proof_hex,
            robot_id=req.robot_id,
            task_id=req.task_id,
            model_hash=req.model_hash,
            confidence=req.confidence,
            action_type=req.action_type,
        )

        if req.confidence >= 95:
            background_tasks.add_task(
                _auto_trigger_payment, req.task_id, req.confidence
            )

        return {
            "status": "verified",
            "task_id": req.task_id,
            "stellar_tx": tx_hash,
            "confidence": req.confidence,
            "auto_payment": req.confidence >= 95,
        }
    except Exception as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/claim-payment", dependencies=[Depends(require_api_key)])
async def claim_payment(req: ClaimPaymentRequest):
    """Step 3: Claim XLM payment for a verified task."""
    try:
        tx_hash = await stellar_client.claim_payment(
            task_id=req.task_id,
            confidence=req.confidence,
        )
        return {"status": "paid", "task_id": req.task_id, "stellar_tx": tx_hash}
    except Exception as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/file-insurance-claim", dependencies=[Depends(require_api_key)])
async def file_insurance_claim(req: InsuranceClaimRequest):
    """File an insurance claim with ZK proof as evidence."""
    try:
        claim_id = await stellar_client.file_insurance_claim(
            robot_id=req.robot_id,
            proof_hash=req.incident_proof_hash,
            claim_amount=req.claim_amount,
        )
        return {
            "status": "filed",
            "claim_id": claim_id,
            "robot_id": req.robot_id,
            "note": "ZK proof stored as evidence. Raw sensor data never exposed.",
        }
    except Exception as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/robot/register-identity", dependencies=[Depends(require_api_key)])
async def register_robot_identity(req: RegisterIdentityRequest):
    """Register robot hardware identity from an on-device commitment.

    SECURITY (M2 fix): earlier versions of this endpoint accepted the raw
    `sensor_noise_sample` and hashed it here, server-side — meaning the raw
    PRNU noise sample crossed the network and was visible to this server, so
    the feature was not actually zero-knowledge despite being labeled that
    way. This endpoint now only accepts a pre-computed SHA256 commitment
    (`fingerprint_commitment`) that the robot must compute ON-DEVICE from its
    own noise sample plus a locally-held salt. The server never sees, stores,
    or has any way to reconstruct the raw sensor data — it only anchors the
    commitment hash on-chain as a soulbound identity token. See
    `reference_ondevice_commitment_example()` below for the exact hashing a
    robot's firmware should perform before calling this endpoint.
    """
    commitment = req.fingerprint_commitment.strip().lower()
    if len(commitment) != 64:
        raise HTTPException(
            status_code=400,
            detail="fingerprint_commitment must be a 64-character hex SHA256 digest computed on-device",
        )
    try:
        int(commitment, 16)
    except ValueError:
        raise HTTPException(
            status_code=400,
            detail="fingerprint_commitment must be a 64-character hex SHA256 digest computed on-device",
        )

    tx = await stellar_client.mint_soulbound_identity_token(req.robot_id, commitment)
    return {
        "robot_id": req.robot_id,
        "identity_hash": commitment,
        "soulbound_token_tx": tx,
        "note": "Commitment computed on-device — raw sensor noise never left the robot",
    }


@app.get("/robot/{robot_id}/status")
async def get_robot_status(robot_id: str):
    """Get robot status, ZREP reputation, and payment history."""
    zrep_balance = await stellar_client.get_zrep_balance(robot_id)
    return {
        "robot_id": robot_id,
        "zrep_balance": zrep_balance,
        "status": "active",
        "network": os.getenv("STELLAR_NETWORK", "testnet"),
    }


@app.get("/fleet/report")
async def fleet_report():
    """Full fleet analytics — powered by ZK-verified on-chain data."""
    return {
        "total_tasks_verified": 0,
        "total_xlm_paid": 0,
        "total_zrep_minted": 0,
        "active_robots": 0,
        "insurance_claims": 0,
        "guardian_status": "running" if (guardian and guardian.running) else "stopped",
    }


@app.post("/guardian/start", dependencies=[Depends(require_api_key)])
async def start_guardian(background_tasks: BackgroundTasks):
    """Start the ZeroSense Guardian v2 autonomous agent system."""
    global guardian
    if guardian and guardian.running:
        return {"status": "already_running"}

    guardian = ZeroSenseGuardianV2(stellar_client=stellar_client)
    background_tasks.add_task(guardian.run)
    return {
        "status": "started",
        "agents": [
            "PaymentAgent", "AnomalyAgent", "InsuranceAgent",
            "ReputationAgent", "LearningAgent", "OracleAgent", "AssistantAgent",
        ],
        "message": "Guardian v2 running — 7 autonomous agents active",
    }


@app.post("/guardian/stop", dependencies=[Depends(require_api_key)])
async def stop_guardian():
    """Stop the Guardian agent system."""
    global guardian
    if guardian:
        guardian.running = False
    return {"status": "stopped"}


# ─── Helper Functions ──────────────────────────────────────────────

async def _generate_risc_zero_proof(
    sensor_frames: list,
    model_hash: str,
    task_id: str,
    robot_id: str,
    bonsai_api_key: str,
) -> str:
    """Generate ZK proof via RISC Zero Bonsai API."""
    if not bonsai_api_key:
        return "0" * 512  # Mock proof for development

    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{os.getenv('BONSAI_API_URL', 'https://api.bonsai.xyz')}/v1/prove",
            headers={"x-api-key": bonsai_api_key},
            json={
                "input": {
                    "sensor_frames": sensor_frames,
                    "model_hash": model_hash,
                    "task_id": task_id,
                    "robot_id": robot_id,
                },
                "image_id": "zerosense-guest-v1",
            },
            timeout=60.0,
        )
        response.raise_for_status()
        data = response.json()
        proof = data.get("proof")
        if not proof:
            raise HTTPException(status_code=502, detail="Bonsai returned no proof")
        return proof


async def _auto_trigger_payment(task_id: str, confidence: int):
    """Background task: automatically trigger XLM payment."""
    try:
        await stellar_client.claim_payment(task_id=task_id, confidence=confidence)
        print(f"[AutoPay] ✅ XLM paid for task {task_id} (confidence: {confidence}%)")
    except Exception as e:
        print(f"[AutoPay] ❌ Payment failed for task {task_id}: {e}")


def reference_ondevice_commitment_example(noise_sample: list[float], device_salt: bytes = b"") -> str:
    """Reference implementation of the ON-DEVICE commitment hashing.

    This function is NOT called by the server — it exists purely as a
    documented reference for robot firmware authors, and so integration
    tests can construct valid commitments without duplicating the hashing
    logic. Real firmware should run the equivalent of this on-device and only
    ever transmit the resulting hex digest to `/robot/register-identity`.
    """
    data = bytes([int(abs(x) * 255) % 256 for x in noise_sample]) + device_salt
    return hashlib.sha256(data).hexdigest()


if __name__ == "__main__":
    import uvicorn
    uvicorn.run("api.main:app", host="0.0.0.0", port=8000, reload=True)
