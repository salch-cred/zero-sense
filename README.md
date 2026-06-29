# 🤖 ZeroSense
## ZK-Verified Autonomous Robot Intelligence & Micro-Payment Protocol on Stellar

> **Stellar Hacks: Real-World ZK** | $5,000 XLM First Prize | Deadline: July 3, 2026

**The world's first trustless robot brain on Stellar.**

ZeroSense proves — cryptographically, verified on-chain — what a robot saw, what its AI decided, and pays it instantly on Stellar. No trust. Just math.

---

## 🧠 The Problem

Billions of dollars of autonomous robots operate daily. When a robot completes a task, causes an accident, or makes a critical AI decision — **there is zero trustless proof of what it actually saw and decided.**

- Insurance companies **cannot verify** robot behavior without exposing proprietary AI models
- Employers **cannot pay robots** based on verifiable task completion
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
Auto XLM Payment + ZREP Token + Insurance Claim
      ↓
ZeroSense Guardian (7 Autonomous Agents)
```

---

## ✅ What's Real vs. What's Mocked

Radical honesty — because "load-bearing ZK, actually verified on-chain" is the bar.

| Component | Status | Notes |
|---|---|---|
| **On-chain Groth16 verification** | ✅ **REAL** | `contracts/verifier` runs the full Groth16 equation via Soroban's native BLS12-381 `pairing_check` host function (CAP-0059). Not a stub. |
| **Verifier security model** | ✅ **REAL** | One-shot admin init, admin-only model registry, `robot.require_auth`, replay protection, public-input binding. Unit-tested. |
| **Payment uses verified confidence** | ✅ **REAL** | `payment` reads confidence cross-contract from the verifier; never trusts a caller-supplied value. |
| **Contract test suite** | ✅ **REAL** | Self-contained BLS12-381 pairing tests (genuine proof passes, tampered proof fails) + Python API tests. |
| **Testnet deployment** | ⚠️ **Run `./deploy.sh`** | Script deploys all four contracts and prints stellar.expert links. Paste the resulting contract IDs below. |
| **Off-chain proving (RISC Zero / Bonsai)** | ⚠️ **Optional/mockable** | The pipeline can run with Bonsai, or with a mock prover for local demos. The on-chain check is identical either way. |
| **Robot input (PyBullet sim)** | ⚠️ **Simulated** | Warehouse robot is a PyBullet simulation, not physical hardware. |
| **Guardian agents** | ⚠️ **Partial** | Orchestration scaffolding; PaymentAgent path is wired end-to-end, others are in progress. |

> **Deployed testnet contract IDs:** _run `./deploy.sh` and paste here_
> - Verifier: `C...`  — https://stellar.expert/explorer/testnet/contract/C...
> - Payment: `C...`

---

## 🏗️ Architecture

```
zerosense/
├── contracts/           # Soroban smart contracts (Rust)
│   ├── verifier/        # ZeroSenseVerifier — REAL BLS12-381 Groth16 pairing_check
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

### 1. Test the contracts (real BLS12-381 pairing)
```bash
cd contracts/verifier && cargo test
# Verifies a genuine BLS12-381 Groth16 proof on-chain and rejects a tampered one.
```

### 2. Deploy to Stellar testnet
```bash
./deploy.sh
# Builds + deploys verifier/payment/reputation/insurance,
# writes IDs to deploy/contract_ids.env, prints stellar.expert links.
```

### 3. Initialize the verifier
```bash
source deploy/contract_ids.env
stellar contract invoke --id "$VERIFIER_CONTRACT_ID" --network testnet -- \
  initialize --admin <ADMIN_G...> --vk <VK_JSON>
```

### 4. Start FastAPI Backend
```bash
cd api
cp .env.example .env  # Fill in your keys
uvicorn main:app --reload --port 8000
```

### 5. Run Robot Simulation + Dashboard
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
| Curve | **BLS12-381** | Soroban native pairing host fns (CAP-0059, Protocol 22+) |

> **Why BLS12-381, not BN254?** Soroban's native pairing host function is
> BLS12-381 (CAP-0059). Implementing BN254 pairings in-contract would be
> prohibitively expensive, so ZeroSense verifies BLS12-381 Groth16 proofs — the
> same curve the Stellar host accelerates. Public-input field elements are
> reduced mod r in-circuit.

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
| Autonomous XLM payment gated on an on-chain-verified proof | First we know of |
| ZK anomaly kill-switch for robots | First we know of |

---

## 🔐 Security
See [`SECURITY.md`](./SECURITY.md) for the full audit (threat model, findings, and the deployment gate checklist).

## 📄 License
MIT

---

*Built for Stellar Hacks: Real-World ZK — June 2026*
*"Not just ZK on Stellar. The first ZK brain for the robot economy."*
