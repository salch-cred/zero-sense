# ZeroSense — Security Audit & Debug Report

_Self-audit of the ZeroSense ZK robot-economy protocol. Findings are ordered by
severity. Items marked **FIXED** were patched in the commits referenced inline;
items marked **OPEN** are remaining integration work that must be completed
before any mainnet / real-value deployment._

---

## Threat model

The protocol moves real value (XLM payouts, insurance claims, reputation tokens)
in response to ZK proofs of robot AI inference. The two assets an attacker most
wants are: (1) trigger a payout for work that was never verified, and (2) inflate
the confidence of a real task to get a larger payout. The audit focuses there.

---

## 🔴 Critical

### C1. Proof verification was a no-op — **PARTIALLY FIXED**
`verifier::verify_groth16_bn254` previously returned `true` for any input
`>= 32` bytes. The contract now enforces the exact **256-byte** Groth16 proof
size, a non-empty verification key, and non-empty public inputs, and binds the
public inputs to the claimed values (see C3).

**OPEN:** the full BN254 pairing check
`e(A,B) == e(alpha,beta)·e(L_pub,gamma)·e(C,delta)` using the Stellar Protocol 25
`env.crypto()` host functions is the single remaining integration point. Until it
lands, the contract is structurally sound but not cryptographically sound — do
not deploy to mainnet.

### C2. No admin / no auth on `initialize` & `register_model` — **FIXED**
`initialize` is now one-shot (panics if already initialized) and stores an admin
whose `require_auth()` is enforced. `register_model` is admin-only. This blocks
verification-key takeover and rogue model registration.

### C3. `verify_robot_action` trusted caller inputs — **FIXED**
Now enforces:
- `robot_id.require_auth()` — only the robot can submit its own actions.
- `confidence`, `model_hash`, `task_id` are bound to the proof's public inputs
  (`public_inputs[0]=model_hash`, `[3]=confidence`, `[4]=task_id`), so a caller
  cannot lie about them.
- Replay protection: a `task_id` can be verified at most once.
- Bounds checks on `confidence (<=100)` and `action_type (<=2)`.

### C4. Payment fraud via caller-supplied confidence — **FIXED**
`payment::claim_task_payment` no longer accepts a `confidence` argument. It now
reads the verified confidence from the verifier via a cross-contract call
(`get_verified_confidence`). An unverified task cannot be paid, and the payout
tier (full / 50% / withheld) is derived only from on-chain verified data.
`register_task` also rejects non-positive amounts and duplicate task IDs.

---

## 🟠 High

### H1. Open CORS + unauthenticated money endpoints — **FIXED (dev-safe)**
CORS origins now come from `ALLOWED_ORIGINS` (defaults to localhost). All
state-changing endpoints require `X-API-Key` matching `ZEROSENSE_API_KEY` when
that var is set. **Note:** when `ZEROSENSE_API_KEY` is unset the check is skipped
for local dev — this is fail-open, so the key MUST be set in any real deployment.

### H2. Bonsai proof fetch had no error handling — **FIXED**
`_generate_risc_zero_proof` now calls `response.raise_for_status()` and returns
HTTP 502 if Bonsai returns no proof, instead of silently shipping a mock proof.

---

## 🟡 Medium

### M1. Softmax overflow crash — **FIXED**
`model/inference.py` computed `exp(logits)/sum(exp(logits))` without subtracting
the max, producing `inf/inf = nan` → `int(nan)` crash on large logits. Replaced
with a numerically stable `_softmax()` helper (max-subtraction + degenerate-case
fallback).

### M2. "ZK biometric identity" is not actually zero-knowledge — **OPEN**
`/robot/register-identity` sends the raw `sensor_noise_sample` to the server,
which hashes it server-side. There is no ZK proof — the raw sample leaves the
device. Either generate the PRNU commitment + proof on-device and submit only the
proof, or relabel the feature honestly.

### M3. ONNX input dtype mismatch — **OPEN**
The model is described as INT8-quantized but `_run_onnx` feeds `float32`. Confirm
the exact input tensor dtype/layout expected by the exported `mobilenet_v2.onnx`
and cast accordingly, or ONNX Runtime will raise at inference time.

---

## 🟢 Low / hardening notes

- **L1.** `i128` arithmetic in reputation/insurance contracts should use checked
  math (`checked_add`/`checked_sub`) to be explicit about overflow behavior.
- **L2.** Mock transaction IDs truncate `task_id` to 8 chars (`tx_{id[:8]}`) —
  fine for demo logs, but don't use truncated IDs as keys anywhere real.
- **L3.** `StellarClient` falls back to a random keypair when no secret key is
  set; good for dev, but log a louder warning so it can't be mistaken for prod.
- **L4.** Consider rate-limiting `/generate-proof` (CPU-heavy inference) to avoid
  trivial DoS.

---

## Test coverage added

The verifier contract now has unit tests covering the security invariants:
happy-path verification, unregistered-model rejection, wrong-size-proof
rejection, confidence tampering rejection, replay rejection, and double-init
rejection. The payment contract has init-guard and unknown-task tests. Run the
full suite with `bash run_tests.sh`.

---

## Deployment gate

**Do not deploy to Stellar mainnet until C1 (BN254 pairing check), M2 (on-device
ZK identity), and M3 (ONNX dtype) are resolved.** Testnet demo is safe with
`ZEROSENSE_API_KEY` set and `ALLOWED_ORIGINS` locked to your frontend origin.
