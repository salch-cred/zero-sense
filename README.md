# 🤖 ZeroSense
## ZK-Verified Autonomous Robot Intelligence & Micro-Payment Protocol on Stellar

> **Stellar Hacks: Real-World ZK** | $5,000 XLM First Prize | Deadline: July 3, 2026

**The world's first trustless robot brain on Stellar.**

ZeroSense proves — cryptographically — what a robot saw, what its AI decided, and pays it instantly on Stellar. No trust. Just math.

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
RISC Zero zkVM → ZK-STARK Proof
      ↓  (via Boundless relay)
Stellar Soroban Verifier Contract
      ↓
Auto XLM Payment + ZREP Token + Insurance Claim
      ↓
ZeroSense Guardian (7 Autonomous Agents)
```

---

## 🏗️ Architecture

```
zerosense/
├── contracts/           # Soroban smart contracts (Rust)
│   ├── verifier/        # ZeroSenseVerifier — verifies RISC Zero Groth16 proof
│   ├── payment/         # RobotPaymentRouter — auto XLM payment on proof
│   ├── reputation/      # ZRepToken — robot reputation Stellar asset
│   └── insurance/       # InsuranceClaim — ZK-evidence insurance
├── zkvm/                # RISC Zero ZK proof system
│   ├── guest/           # Rust guest program (runs inside zkVM)
│   └── host/            # Host: generates proof via Bonsai API
├── api/                 # FastAPI Python backend
│   ├── main.py          # API endpoints
│   ├── agents/          # ZeroSense Guardian v2 autonomous agents
│   └── stellar/         # Stellar SDK integration
├── model/               # AI inference
│   ├── inference.py     # ONNX Runtime MobileNetV2
│   └── mobilenet_v2.onnx  # Quantized model (download separately)
├── simulation/          # PyBullet robot simulation
│   └── robot_sim.py     # Warehouse robot simulation
└── frontend/            # Web dashboard
    └── index.html       # Live demo dashboard
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

### 1. Setup Stellar Testnet
```bash
stellar keys generate --global zerosense-dev
stellar keys fund zerosense-dev --network testnet
```

### 2. Deploy Contracts
```bash
cd contracts/verifier && cargo build --target wasm32v1-none --release
stellar contract deploy --wasm target/wasm32v1-none/release/verifier.wasm --network testnet

cd ../payment && cargo build --target wasm32v1-none --release
stellar contract deploy --wasm target/wasm32v1-none/release/payment.wasm --network testnet

cd ../reputation && cargo build --target wasm32v1-none --release
stellar contract deploy --wasm target/wasm32v1-none/release/reputation.wasm --network testnet

cd ../insurance && cargo build --target wasm32v1-none --release
stellar contract deploy --wasm target/wasm32v1-none/release/insurance.wasm --network testnet
```

### 3. Generate ZK Proof (Test)
```bash
cd zkvm
cargo run --release --bin host
# → Generates proof, submits to Stellar testnet
```

### 4. Start FastAPI Backend
```bash
cd api
cp .env.example .env  # Fill in your keys
uvicorn main:app --reload --port 8000
```

### 5. Run Robot Simulation
```bash
cd simulation
python robot_sim.py
# → Warehouse robot navigates, generates sensor frames → triggers ZK proof pipeline
```

### 6. Launch Dashboard
```bash
open frontend/index.html
# → Live robot sim + proof visualizer + Stellar tx explorer
```

---

## 🔬 ZK Stack

| Component | Technology | What it proves |
|---|---|---|
| Proof System | RISC Zero zkVM | Program executed correctly |
| ML Proof | Ezkl (ONNX → ZK circuit) | Neural net inference correct |
| On-chain Verify | Boundless + Soroban | Groth16 proof valid on Stellar |
| Curve | BN254 | Stellar Protocol 25 native |

---

## 🤖 ZeroSense Guardian v2 — 7 Autonomous Agents

| Agent | Does |
|---|---|
| PaymentAgent | Auto XLM payment on proof verification |
| AnomalyAgent | Kill-switch on behavioral deviation |
| InsuranceAgent | Auto-files claims with ZK evidence |
| ReputationAgent | Mints/slashes ZREP on Stellar DEX |
| LearningAgent | Aggregates federated model updates |
| OracleAgent | ZK-verified real-world data feeds |
| AssistantAgent | LLM failure prediction + reporting |

Each agent has its own Stellar wallet and earns XLM autonomously.

---

## 🌍 Why This Is World-First

| Innovation | Status Anywhere |
|---|---|
| ZK proof of robot AI inference on Stellar | ❌ Never built |
| ZK hardware biometric robot identity | ❌ Never built |
| ZK federated robot learning on blockchain | ❌ Never built |
| ZK anomaly kill-switch for robots | ❌ Never built |
| Multi-robot ZK consensus (M-of-N) | ❌ Never built |

---

## 📄 License
MIT

---

*Built for Stellar Hacks: Real-World ZK — June 2026*
*"Not just ZK on Stellar. The first ZK brain for the robot economy."*
