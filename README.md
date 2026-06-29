# 🤖 ZeroSense
## ZK-Verified Autonomous Robot Intelligence & Micro-Payment Protocol on Stellar

> **Stellar Hacks: Real-World ZK** | $5,000 XLM First Prize | Deadline: July 3, 2026

**The world's first trustless robot brain on Stellar.**

ZeroSense proves — cryptographically, verified on-chain — *which* robot acted, what it saw, what its AI decided, and pays it instantly on Stellar. No trust. Just math.

---

## 🧠 The Problem

Billions of dollars of autonomous robots operate daily. When a robot completes a task, causes an accident, or makes a critical AI decision — **there is zero trustless proof of which robot it was, what it actually saw, and what it decided.**

- Insurance companies **cannot verify** robot behavior without exposing proprietary AI models
- Employers **cannot pay robots** based on verifiable task completion
- Fleets **cannot prove a licensed robot acted** without doxxing the whole fleet roster
- Regulatory bodies **cannot audit** robot decisions without full data exposure

## 💡 The Solution

```
Robot Sensor Data
      ↓
AI Inference (MobileNetV2 ONNX)
      ↓
RISC Zero zkVM → proof of correct inference
      ↓  (wrapped to a BLS12-381 Groth16 proof)
Stellar Soroban Verifier Contract  ← REAL on-chain pairing_check
      ↓
zk-Fleet Identity  ← anonymous-but-accountable: proves a licensed robot acted,
      ↓               burns a one-time nullifier (no double-spend, no doxxing)
Auto XLM Payment + ZREP Token + Insurance Claim
      ↓
ZeroSense Guardian (7 Autonomous Agents)
```

---

## 🆕 World-First: zk-Fleet Identity

`contracts/fleet_identity` answers a question no other Stellar ZK project does: **"prove a *licensed* robot performed this action, without revealing *which* robot — and stop it from claiming the same task twice."**

- **Poseidon Merkle tree of robot commitments.** Each robot enrolls a Poseidon commitment as a leaf. Membership = "this is a licensed fleet robot." The roster stays private.
- **On-chain native Poseidon.** Built on Stellar's **Protocol 25 “X-Ray”** native Poseidon host functions (CAP-0075) over the BN254 scalar field — circomlib-compatible, so the same hash works in an off-chain Circom circuit and in-contract.
- **One-time nullifiers.** Each action derives `nullifier = Poseidon(identity_leaf, task_id)` and burns it on-chain. The same robot cannot double-claim the same task — Tornado/Privacy-Pools-style double-spend protection, applied to a robot fleet.
- **Rolling root history (32 roots).** Proofs against any recent root stay valid as the fleet grows, so enrollment never invalidates in-flight actions.
- **Auth-gated.** `register_robot` is admin-only; `submit_action` requires `robot.require_auth()`. Checks-before-effects ordering throughout.

This is the missing accountability layer for the robot economy: **anonymous, unlinkable, but non-repudiable and replay-proof.**

---

## ✅ What's Real vs. What's Mocked

Radical honesty — because "load-bearing ZK, actually verified on-chain" is the bar.

| Component | Status | Notes |
|---|---|---|
| **On-chain Groth16 verification** | ✅ **REAL** | `contracts/verifier` runs the full Groth16 equation via Soroban's native BLS12-381 `pairing_check` host function (CAP-0059). Not a stub. |
| **zk-Fleet Identity (Poseidon Merkle + nullifiers)** | ✅ **REAL** | `contracts/fleet_identity` builds an incremental Poseidon Merkle tree, rolling 32-root history, and a one-time nullifier set using native X-Ray Poseidon (CAP-0075). Self-contained tests run real on-chain Poseidon — no mocks. |
| **Verifier security model** | ✅ **REAL** | One-shot admin init, admin-only model registry, `robot.require_auth`, replay protection, public-input binding. Unit-tested. |
| **Payment uses verified confidence** | ✅ **REAL** | `payment` reads confidence cross-contract from the verifier; never trusts a caller-supplied value. |
| **Contract test suite** | ✅ **REAL** | Self-contained BLS12-381 pairing tests + real-Poseidon fleet-identity tests (membership, double-spend, unknown-root, tree-full) + Python API tests. |
| **Testnet deployment** | ⚠️ **Run `./deploy.sh`** | Script deploys all five contracts and prints stellar.expert links. Paste the resulting contract IDs below. |
| **Off-chain proving (RISC Zero / Bonsai)** | ⚠️ **Optional/mockable** | The pipeline can run with Bonsai, or with a mock prover for local demos. The on-chain check is identical either way. |
| **Fleet-identity zero-knowledge upgrade** | ⚠️ **Documented** | Membership is currently a transparent on-chain Merkle check. The Circom/Groth16 circuit that makes the membership proof fully zero-knowledge is specified in `contracts/fleet_identity/src/lib.rs` and pluggable into the verifier. |
| **Robot input (PyBullet sim)** | ⚠️ **Simulated** | Warehouse robot is a PyBullet simulation, not physical hardware. |
| **Guardian agents** | ⚠️ **Partial** | Orchestration scaffolding; PaymentAgent path is wired end-to-end, others are in progress. |

> **Deployed testnet contract IDs:** _run `./deploy.sh` and paste here_
> - Verifier: `C...`  — https://stellar.expert/explorer/testnet/contract/C...
> - Payment: `C...`
> - Fleet Identity: `C...`

---

## 🏗️ Architecture

```
zerosense/
├── contracts/           # Soroban smart contracts (Rust)
│   ├── verifier/        # ZeroSenseVerifier — REAL BLS12-381 Groth16 pairing_check
│   ├── fleet_identity/  # zk-Fleet Identity — Poseidon Merkle tree + one-time nullifiers
│   ├── payment/         # RobotPaymentRouter — auto XLM payment on verified proof
│   ├── reputation/      # ZRepToken — robot reputation Stellar asset
│   └── insurance/       # InsuranceClaim — ZK-evidence insurance
├── zkvm/                # RISC Zero proof system (guest + Bonsai host)
├── api/                 # FastAPI Python backend (endpoints, agents, stellar sdk)
├── model/               # ONNX Runtime MobileNetV2 inference
├── simulation/          # PyBullet warehouse robot simulation
├── frontend/            # Web dashboard
└── deploy.sh            # One-command Stellar testnet deployment
```

---

## 🚀 Quickstart

### Prerequisites
```bash
# Rust + RISC Zero
curl -L https://risczero.com/install | bash
rzup install

# Stellar CLI
cargo install stellar-cli

# Python
pip install fastapi uvicorn stellar-sdk onnxruntime pybullet httpx python-dotenv
```

### 1. Test the contracts (real BLS12-381 pairing + real Poseidon Merkle)
```bash
cd contracts/verifier && cargo test        # genuine Groth16 proof passes, tampered fails
cd ../fleet_identity && cargo test          # real Poseidon Merkle membership + nullifier burn
# Or run everything: bash run_tests.sh
```

### 2. Deploy to Stellar testnet
```bash
./deploy.sh
# Builds + deploys verifier/payment/reputation/insurance/fleet_identity,
# writes IDs to deploy/contract_ids.env, prints stellar.expert links.
```

### 3. Initialize the verifier
```bash
source deploy/contract_ids.env
stellar contract invoke --id "$VERIFIER_CONTRACT_ID" --network testnet -- \
  initialize --admin <ADMIN_G...> --vk <VK_JSON>
```

### 4. Enroll a robot into the fleet (admin-only)
```bash
stellar contract invoke --id "$FLEET_IDENTITY_CONTRACT_ID" --network testnet -- \
  register_robot --commitment <POSEIDON_COMMITMENT_U256>
# fleet_identity is deployed with its constructor: --admin <ADMIN_G...> --depth 20
```

### 5. Start FastAPI Backend
```bash
cd api
cp .env.example .env  # Fill in your keys
uvicorn main:app --reload --port 8000
```

### 6. Run Robot Simulation + Dashboard
```bash
cd simulation && python robot_sim.py   # warehouse robot → sensor frames → ZK pipeline
open frontend/index.html               # live sim + proof visualizer + tx explorer
```

---

## 🔬 ZK Stack

| Component | Technology | What it proves |
|---|---|---|
| Proof System | RISC Zero zkVM | Program (AI inference) executed correctly |
| ML Inference | ONNX Runtime (MobileNetV2) | The model run that produced the decision |
| Proof wrapping | Groth16 over **BLS12-381** | Succinct, on-chain-verifiable proof |
| On-chain verify | **Soroban native `pairing_check`** | Groth16 proof valid on Stellar |
| Fleet identity | **Poseidon Merkle tree (BN254 field)** | A licensed robot acted, without revealing which |
| Replay safety | **One-time Poseidon nullifiers** | The same robot can't double-claim a task |

> **On curves — BLS12-381 *and* BN254 are both native now.** As of **Protocol 25
> “X-Ray”** (mainnet Jan 2026), Soroban exposes native host functions for *both*
> BLS12-381 (`pairing_check`, CAP-0059) **and** BN254 G1 ops + pairing (CAP-0074),
> plus native **Poseidon/Poseidon2** hashing (CAP-0075). ZeroSense uses each where
> it fits: the Groth16 **inference verifier runs on BLS12-381** (mature, audited
> host path), while **zk-Fleet Identity uses BN254-field Poseidon** so its Merkle
> commitments and nullifiers are circomlib-compatible with the standard off-chain
> proving toolchain. Public-input field elements are reduced mod the respective
> scalar field in-circuit.

---

## 🤖 ZeroSense Guardian — Autonomous Agents

| Agent | Does |
|---|---|
| PaymentAgent | Auto XLM payment on proof verification (wired end-to-end) |
| AnomalyAgent | Kill-switch on behavioral deviation |
| InsuranceAgent | Auto-files claims with ZK evidence |
| ReputationAgent | Mints/slashes ZREP on Stellar |
| LearningAgent | Aggregates federated model updates |
| OracleAgent | ZK-verified real-world data feeds |
| AssistantAgent | LLM failure prediction + reporting |

---

## 🌍 Why This Is Novel

| Innovation | Status Anywhere |
|---|---|
| ZK proof of robot AI inference verified on-chain on Stellar | First we know of |
| Anonymous-but-accountable robot fleet identity (Poseidon Merkle + nullifiers) on Stellar | First we know of |
| Autonomous XLM payment gated on an on-chain-verified proof | First we know of |
| ZK anomaly kill-switch for robots | First we know of |

---

## 🔐 Security
See [`SECURITY.md`](./SECURITY.md) for the full audit (threat model, findings, and the deployment gate checklist).

## 📄 License
MIT

---

*Built for Stellar Hacks: Real-World ZK — June 2026*
*"Not just ZK on Stellar. The first ZK brain — and ZK identity — for the robot economy."*
