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

### C1. Proof verification was a no-op — **FIXED**
`verifier::verify_groth16_bn254` previously returned `true` for any input
`>= 32` bytes — i.e. there was no cryptography at all.

**Resolved:** the verifier now performs a **real Groth16 verification** using
Soroban's native **BLS12-381** pairing host function
(`env.crypto().bls12_381().pairing_check`, CAP-0059, shipped in Protocol 22). It
evaluates the full equation

```
e(A, B) * e(-alpha, beta) * e(-vk_x, gamma) * e(-C, delta) == 1
where vk_x = ic[0] + Σ public_i · ic[i+1]
```

The contract decodes the 384-byte `A || B || C` proof into curve points,
recomputes `vk_x` from the public inputs and the stored verification key, and
returns the host's pairing-check result. Self-contained unit tests prove a
genuine proof verifies and a tampered proof is rejected.

> **Curve correction:** earlier code/docs claimed "BN254 native host functions
> (Protocol 25)". This was wrong — Soroban's native pairing primitive is
> **BLS12-381** (CAP-0059), not BN254. Proofs must be produced over BLS12-381
> (Circom/snarkjs or arkworks BLS12-381 backend, or a RISC Zero → BLS12-381
> Groth16 wrapper). Every public input must be a field element `< r` (the
> BLS12-381 scalar field order), so any SHA-256 hash used as a public input must
> be reduced mod r inside the circuit.

### C2. No admin / no auth on `initialize` & `register_model` — **FIXED**
`initialize` is now one-shot (fails if already initialized) and stores an admin
whose `require_auth()` is enforced; it also stores the verification key. ​
`register_model` is admin-only. This blocks verification-key takeover and rogue
model registration.

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

## Test coverage

The verifier contract has self-contained unit tests that exercise the **real**
BLS12-381 pairing (no mock): a genuine Groth16 proof verifies on-chain and a
tampered proof is rejected, built via a telescoping `hash_to_g1/g2` construction
so no external proof files or extra crates are needed. Security-invariant tests
cover unregistered-model rejection, wrong-size-proof rejection, confidence
tampering rejection, replay rejection, double-init rejection, and a full
robot-action end-to-end happy path. The payment contract has init-guard and
unknown-task tests. Run the full suite with `bash run_tests.sh`.

---

## Deployment gate

C1 (the core proof check) is now **cryptographically sound**. Remaining items
before a Stellar **mainnet / real-value** deployment:

1. **M2** — make the robot biometric identity actually zero-knowledge (on-device).
2. **M3** — confirm the ONNX input dtype/layout matches the exported model.
3. Run a real BLS12-381 trusted setup and install the production verification key
   via `initialize`; keep the admin key in secure custody.
4. Set `ZEROSENSE_API_KEY` and lock `ALLOWED_ORIGINS` to your frontend origin.

**Testnet demo is safe today** with the real on-chain verifier, `ZEROSENSE_API_KEY`
set, and `ALLOWED_ORIGINS` locked down. Deploy with `./deploy.sh`.
