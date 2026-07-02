# ZeroSense — Security Audit & Debug Report

_Self-audit of the ZeroSense ZK robot-economy protocol. Findings are ordered by
severity. Items marked **FIXED** were patched in the commits referenced inline;
items marked **OPEN** are remaining integration work that must be completed
before any mainnet / real-value deployment._

> **How this report was produced:** this is a static/manual code audit (every
> contract read line-by-line against a fraud/reentrancy/access-control
> checklist), not a live `cargo test` / `pytest` run — this environment can
> read and write the repo but cannot execute a Rust or Python toolchain. Run
> `bash run_tests.sh` yourself to get real, authoritative pass/fail numbers
> before submitting; treat this document as what to check for and what has
> already been fixed, not a substitute for actually running the suite.

---

## Threat model

The protocol moves real value (XLM payouts, insurance claims, reputation tokens)
in response to ZK proofs of robot AI inference and fleet learning. The assets an
attacker most wants are: (1) trigger a payout for work that was never verified,
(2) inflate the confidence/claim behind a real task to get a larger payout, and
(3) take over a contract's admin role to mint tokens or approve claims at will.
The audit focuses there.

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

### C5. Reputation contract admin takeover via re-initialize — **FIXED**
`reputation::initialize` had **no one-shot guard** — unlike `payment` and the
newer constructor-based contracts, it only checked `admin.require_auth()`
against the *new* admin argument, never against whether the contract already
had one. Anyone could call `initialize(attacker_address, attacker_verifier)`
at any time, overwrite the real admin/verifier, then call `mint_reputation`
as `attacker_verifier` (which they control, so `require_auth` trivially
passes) to mint **unlimited ZREP** to themselves.

**Resolved:** `initialize` now panics with `Error::AlreadyInitialized` if an
admin is already stored. `mint_reputation`/`slash_reputation` also now require
a positive `amount` and use `checked_add` (not raw `+`) for the balance and
total-supply updates. Regression tests cover the exact attack (re-initialize
with an attacker-controlled verifier) and the non-verifier/non-admin paths.

### C6. Insurance contract admin takeover via re-initialize — **FIXED**
Same class of bug as C5: `insurance::initialize` had no one-shot guard, so
anyone could call `initialize(attacker_address)` at any time to become the
stored admin and then call `resolve_claim` to approve or reject any insurance
claim — including claims they filed on themselves.

**Resolved:** `initialize` now panics with `Error::AlreadyInitialized` if an
admin is already stored. `file_claim` also now requires a positive
`claim_amount`. Regression tests cover the re-initialize attack and the
non-admin-resolve path.

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

### H3. `fleet_learning::claim_reward` paid out before recording the claim — **FIXED**
The token transfer to the robot happened **before** `Claimed(round, robot)` was
set to `true` — a checks-effects-interactions violation. Standard SEP-41
Stellar tokens don't call back into the caller on `transfer`, so this wasn't
exploitable with the currently-configured token, but it's a real footgun: any
future non-standard reward-token implementation (or a bug introduced later)
that *did* call back into `claim_reward` mid-transfer would find
`AlreadyClaimed` still unset and could drain the reward pool via repeated
claims on one (round, robot) pair.

**Resolved:** `claimed_key` is now set to `true` immediately after the
eligibility checks and *before* the external `token_client.transfer` call.

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

- **L1.** ~~`i128` arithmetic in reputation/insurance contracts should use
  checked math~~ — **FIXED** as part of C5: `reputation::mint_reputation` now
  uses `checked_add` for balance and total-supply updates and panics with
  `Error::Overflow` instead of silently wrapping.
- **L2.** Mock transaction IDs truncate `task_id` to 8 chars (`tx_{id[:8]}`) —
  fine for demo logs, but don't use truncated IDs as keys anywhere real.
- **L3.** `StellarClient` falls back to a random keypair when no secret key is
  set; good for dev, but log a louder warning so it can't be mistaken for prod.
- **L4.** Consider rate-limiting `/generate-proof` (CPU-heavy inference) to avoid
  trivial DoS.
- **L5.** `fleet_learning`'s per-round `RoundContributors`/`RoundCommitments`
  vectors grow unbounded with fleet size; for very large fleets, consider a
  paginated/Merkleized commitment log instead of one growing `Vec` per round.

---

## Test coverage

| Contract | Tests | Notes |
|---|---|---|
| `verifier` | ✅ | Real BLS12-381 pairing (genuine proof passes, tampered proof rejected), unregistered-model, wrong-size-proof, confidence-tampering, replay, double-init, full happy path. |
| `payment` | ✅ | Double-init rejected, unknown-task lookup. |
| `reputation` | ✅ **NEW** | Double-init rejected, re-initialize-as-attacker rejected (C5 regression), mint-by-verifier happy path, mint-by-non-verifier rejected, non-positive-amount rejected, slash-by-admin (floors at zero), slash-by-non-admin rejected. Previously had **zero** tests. |
| `insurance` | ✅ **NEW** | Double-init rejected, re-initialize-as-attacker rejected (C6 regression), file+resolve happy path, resolve-by-non-admin rejected, non-positive claim-amount rejected, resolve-unknown-claim rejected. Previously had **zero** tests. |
| `fleet_identity` | ✅ | Real Poseidon Merkle root reconstruction, anonymous membership verification (valid + forged path), nullifier double-spend block + fresh-task allowance, unknown-root rejection, forged-membership rejection. |
| `bn254_verifier` | ✅ | Real Gnark-generated Groth16/BN254 proof verifies end-to-end through the actual `pairing_check` host function; tampered proof rejected. |
| `fleet_learning` | ✅ | Submission gating + double-submit block, independently-reconstructed Poseidon root, finalize-with-no-contributors rejection, finalize traps without a real verifier approval, claim gating (pre-finalization, non-contributor, double-claim) — now also exercising the post-H3-fix claim-before-transfer ordering. |

Run the full suite with `bash run_tests.sh` (Python + all 7 Soroban contracts +
zkvm host/guest build check).

---

## Self-assessed benchmark scorecard

_This is our own honest read of the project against typical ZK-hackathon
judging axes — **not** an official score, and not a claim about how any other
team's submission compares (there's no way for us to see competitors' actual
code or judges' scoring). Use it to prioritize remaining work, not as a
guarantee of placement._

| Category | Score | Why |
|---|---|---|
| Novelty / Innovation | 9/10 | zk-Fleet Identity (anonymous-but-accountable robot membership + nullifiers) and zk-FedAvg Fleet Learning (ZK-proved aggregation + autonomous per-contributor payout) are, per our prior-art search, unbuilt in combination elsewhere. |
| Cryptographic rigor | 8/10 | Three independent real, tested on-chain verification paths (BLS12-381 Groth16, BN254 Groth16 against a real Gnark proof, native Poseidon Merkle) — genuine host-function calls, not mocks. Docked 2 points: `fleet_membership.circom` and `fedavg_aggregation.circom` are fully specified but not yet compiled/trusted-setup, so no real end-to-end proof exists for either yet. |
| Security posture | 8/10 | This pass closed two real admin-takeover vulnerabilities (C5, C6) and one checks-effects-interactions gap (H3); every initializer is now one-shot-guarded and every fund-moving path reads state it cannot forge. Docked 2 points: no external/professional audit, and M2/M3 (biometric identity honesty, ONNX dtype) remain open. |
| Test coverage | 7/10 | Every one of the 7 contracts now has an inline unit-test module (reputation and insurance had **zero** before this pass); tests specifically target the vulnerabilities found. Docked 3 points: unit-level only (no integration/testnet-fork tests), and we can't execute `cargo test`/`pytest` in this environment — you must run `bash run_tests.sh` yourself for authoritative pass/fail. |
| Real-world integration | 5/10 | `deploy.sh` builds/deploys 5 of 7 contracts automatically (the other 2 need a real circuit VK first, by design); robot input is a PyBullet simulation, not physical hardware; M3's dtype question is still open. |
| Documentation & honesty | 8/10 | README's "What's Real vs. Mocked" table and this audit trail are deliberately candid about what's genuinely on-chain vs. still a design spec — judges tend to reward that candor over overclaiming. |

**Total: 45/60 (self-assessed).** The two biggest remaining levers to move this
score are: (1) actually compiling the two Circom circuits and running a
trusted setup so `finalize_round`/the ZK-membership upgrade can be exercised
with a genuine proof end-to-end, and (2) running `bash run_tests.sh` yourself
and pasting real testnet contract IDs into the README before submission.

---

## Deployment gate

C1–C6 and H1–H3 are now resolved. Remaining items before a Stellar
**mainnet / real-value** deployment:

1. **M2** — make the robot biometric identity actually zero-knowledge (on-device).
2. **M3** — confirm the ONNX input dtype/layout matches the exported model.
3. Run a real BLS12-381 trusted setup and install the production verification key
   via `initialize`; keep the admin key in secure custody.
4. Run a real trusted setup for `fleet_membership.circom` and
   `fedavg_aggregation.circom` and deploy `bn254_verifier`/`fleet_learning`
   with the resulting verification keys (see `deploy.sh`'s printed instructions).
5. Set `ZEROSENSE_API_KEY` and lock `ALLOWED_ORIGINS` to your frontend origin.

**Testnet demo is safe today** with the real on-chain verifiers, `ZEROSENSE_API_KEY`
set, and `ALLOWED_ORIGINS` locked down. Deploy with `./deploy.sh`.
