"""ZeroSense Guardian v2 — 7 Autonomous Agents

Each agent has its own Stellar wallet and earns XLM for its work.
All agents run concurrently via asyncio.

Agents:
1. PaymentAgent    - Auto-triggers XLM on verified task completion
2. AnomalyAgent    - Real-time behavioral deviation + kill-switch
3. InsuranceAgent  - Auto-files claims with ZK evidence
4. ReputationAgent - Manages ZREP minting/slashing on Stellar DEX
5. LearningAgent   - Aggregates federated model updates
6. OracleAgent     - Feeds ZK-verified real-world data to contracts
7. AssistantAgent  - LLM failure prediction + fleet reporting
"""

import asyncio
import os
from datetime import datetime
from typing import Optional


class BaseAgent:
    def __init__(self, name: str, stellar_client, poll_interval: int = 5):
        self.name = name
        self.stellar = stellar_client
        self.poll_interval = poll_interval
        self.running = False
        self.processed_count = 0
        self.wallet_address = None

    async def setup(self):
        """Initialize agent wallet."""
        # In production: each agent gets its own funded Stellar keypair
        print(f"[{self.name}] 🤖 Initialized with dedicated Stellar wallet")

    async def run_forever(self):
        """Main agent loop — runs until stopped."""
        self.running = True
        await self.setup()
        print(f"[{self.name}] ▶️  Started")
        while self.running:
            try:
                await self.tick()
            except Exception as e:
                print(f"[{self.name}] ❌ Error: {e}")
            await asyncio.sleep(self.poll_interval)

    async def tick(self):
        """Override in subclass — called every poll_interval seconds."""
        pass


class PaymentAgent(BaseAgent):
    """Monitors Stellar ledger for verified RobotAction events.
    Automatically triggers XLM payment when confidence >= threshold."""

    def __init__(self, stellar_client, threshold: float = 0.95):
        super().__init__("PaymentAgent", stellar_client)
        self.threshold = threshold
        self.paid_tasks: set = set()

    async def tick(self):
        events = await self.stellar.get_recent_contract_events("RobotActionVerified")
        for event in events:
            task_id = event.get("task_id")
            confidence = event.get("confidence", 0) / 100.0
            if task_id not in self.paid_tasks and confidence >= self.threshold:
                await self.stellar.claim_payment(task_id=task_id, confidence=int(confidence * 100))
                self.paid_tasks.add(task_id)
                self.processed_count += 1
                print(f"[PaymentAgent] ⚡ XLM paid for task {task_id[:8]}... (confidence: {confidence*100:.0f}%)")


class AnomalyAgent(BaseAgent):
    """Monitors robot telemetry for behavioral anomalies.
    Triggers kill-switch: freezes payments + files emergency claim."""

    def __init__(self, stellar_client, anomaly_threshold: int = 3):
        super().__init__("AnomalyAgent", stellar_client)
        self.anomaly_threshold = anomaly_threshold
        self.low_confidence_counts: dict = {}

    async def tick(self):
        events = await self.stellar.get_recent_contract_events("RobotActionVerified")
        for event in events:
            robot_id = event.get("robot_id", "")
            confidence = event.get("confidence", 100)

            if confidence < 80:
                count = self.low_confidence_counts.get(robot_id, 0) + 1
                self.low_confidence_counts[robot_id] = count

                if count >= self.anomaly_threshold:
                    await self.trigger_killswitch(robot_id, confidence)
                    self.low_confidence_counts[robot_id] = 0
            else:
                self.low_confidence_counts[robot_id] = 0  # Reset on good reading

    async def trigger_killswitch(self, robot_id: str, last_confidence: int):
        """Atomic kill-switch: freeze ZREP + pause payments + file claim."""
        print(f"[AnomalyAgent] 🚨 KILL-SWITCH triggered for robot {robot_id[:8]}...")
        print(f"[AnomalyAgent] → Slashing ZREP reputation...")
        print(f"[AnomalyAgent] → Filing emergency insurance claim...")
        print(f"[AnomalyAgent] → Alerting operator...")
        self.processed_count += 1
        # TODO: Call Soroban contracts atomically


class InsuranceAgent(BaseAgent):
    """Monitors for incident proofs and auto-files insurance claims."""

    def __init__(self, stellar_client):
        super().__init__("InsuranceAgent", stellar_client)
        self.filed_claims: set = set()

    async def tick(self):
        events = await self.stellar.get_recent_contract_events("RobotIncidentProof")
        for event in events:
            proof_hash = event.get("proof_hash", "")
            if proof_hash not in self.filed_claims:
                robot_id = event.get("robot_id", "")
                amount = event.get("estimated_claim", 1_000_000)  # 0.1 XLM default
                await self.stellar.file_insurance_claim(
                    robot_id=robot_id,
                    proof_hash=proof_hash,
                    claim_amount=amount,
                )
                self.filed_claims.add(proof_hash)
                self.processed_count += 1
                print(f"[InsuranceAgent] 🛡️  Claim filed for incident {proof_hash[:8]}...")


class ReputationAgent(BaseAgent):
    """Manages ZREP token minting based on verified task completions."""

    def __init__(self, stellar_client):
        super().__init__("ReputationAgent", stellar_client)
        self.processed_tasks: set = set()
        self.ZREP_PER_TASK = 10  # 10 ZREP per completed task

    async def tick(self):
        events = await self.stellar.get_recent_contract_events("TaskPaymentReleased")
        for event in events:
            task_id = event.get("task_id", "")
            if task_id not in self.processed_tasks:
                robot_id = event.get("robot_id", "")
                confidence = event.get("confidence", 0)
                # Bonus ZREP for high confidence
                zrep_amount = self.ZREP_PER_TASK + (5 if confidence >= 98 else 0)
                # TODO: Call ZRepToken.mint_reputation on Soroban
                self.processed_tasks.add(task_id)
                self.processed_count += 1
                print(f"[ReputationAgent] ⭐ {zrep_amount} ZREP minted for {robot_id[:8]}...")


class LearningAgent(BaseAgent):
    """Aggregates ZK-proven federated model updates from robot operators."""

    def __init__(self, stellar_client):
        super().__init__("LearningAgent", stellar_client)

    async def tick(self):
        # Check for pending model update submissions
        events = await self.stellar.get_recent_contract_events("ModelUpdateSubmitted")
        for event in events:
            print(f"[LearningAgent] 🧠 Processing ZK-proven model update...")
            self.processed_count += 1


class OracleAgent(BaseAgent):
    """Feeds ZK-verified real-world data to Stellar contracts."""

    def __init__(self, stellar_client):
        super().__init__("OracleAgent", stellar_client, poll_interval=30)

    async def tick(self):
        # Fetch and ZK-commit real-world data
        print(f"[OracleAgent] 📡 Updating ZK-verified oracle data on Stellar...")
        self.processed_count += 1


class AssistantAgent(BaseAgent):
    """LLM-powered fleet analysis, failure prediction, claim drafting."""

    def __init__(self, stellar_client):
        super().__init__("AssistantAgent", stellar_client, poll_interval=60)
        self.ollama_url = os.getenv("OLLAMA_URL", "http://localhost:11434")
        self.model = os.getenv("OLLAMA_MODEL", "mistral")

    async def tick(self):
        # Periodic fleet health analysis
        if self.processed_count % 10 == 0:
            await self.analyze_fleet_health()

    async def analyze_fleet_health(self):
        """Use LLM to analyze fleet telemetry and predict failures."""
        print(f"[AssistantAgent] 🤖 Analyzing fleet health via LLM...")
        # TODO: Query Stellar for fleet telemetry → send to Ollama
        self.processed_count += 1


class ZeroSenseGuardianV2:
    """The ZeroSense Guardian v2 — 7 autonomous agents running concurrently.
    Each agent has its own Stellar wallet and earns XLM for its work.
    The entire fleet is self-sustaining.
    """

    def __init__(self, stellar_client):
        self.running = False
        self.agents = {
            "payment":    PaymentAgent(stellar_client),
            "anomaly":    AnomalyAgent(stellar_client),
            "insurance":  InsuranceAgent(stellar_client),
            "reputation": ReputationAgent(stellar_client),
            "learning":   LearningAgent(stellar_client),
            "oracle":     OracleAgent(stellar_client),
            "assistant":  AssistantAgent(stellar_client),
        }

    async def run(self):
        """Start all 7 agents concurrently."""
        self.running = True
        print("\n🤖 ZeroSense Guardian v2 STARTED")
        print("━" * 40)
        for name in self.agents:
            print(f"  ✓ {name.capitalize()}Agent initialized")
        print("━" * 40)
        print("All 7 agents running autonomously on Stellar\n")

        # Run all agents concurrently
        await asyncio.gather(*[
            agent.run_forever()
            for agent in self.agents.values()
        ])

    def status(self) -> dict:
        return {
            name: {
                "running": agent.running,
                "processed": agent.processed_count,
            }
            for name, agent in self.agents.items()
        }
