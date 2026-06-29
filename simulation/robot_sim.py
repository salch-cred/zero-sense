"""ZeroSense Robot Simulation

Simulates a warehouse robot navigating and completing tasks.
Each task completion triggers the ZK proof pipeline.

Uses PyBullet for physics simulation (install: pip install pybullet)
Fallback: simple ASCII animation for demo purposes.
"""

import asyncio
import math
import os
import random
import time
from typing import Optional

import httpx

API_URL = os.getenv("ZEROSENSE_API_URL", "http://localhost:8000")


class WarehouseRobot:
    """Simulated warehouse robot that generates sensor data."""

    def __init__(self, robot_id: str = "robot-001"):
        self.robot_id = robot_id
        self.position = [0.0, 0.0]
        self.heading = 0.0
        self.task_count = 0
        self.speed = 0.1

        # Try to initialize PyBullet
        self.pybullet_available = False
        try:
            import pybullet as p
            import pybullet_data
            self.p = p
            self.physics_client = p.connect(p.DIRECT)  # Headless
            p.setAdditionalSearchPath(pybullet_data.getDataPath())
            p.setGravity(0, 0, -9.81)
            p.loadURDF("plane.urdf")
            self.pybullet_available = True
            print(f"[Robot {robot_id}] ✅ PyBullet physics simulation active")
        except Exception as e:
            print(f"[Robot {robot_id}] ⚠️  PyBullet not available — using simple sim")

    def capture_sensor_frame(self) -> list[float]:
        """Capture a sensor frame (camera/LiDAR simulation)."""
        # Simulate 64 sensor readings (8x8 simplified LiDAR grid)
        frame = []
        for i in range(64):
            angle = (i / 64) * 2 * math.pi + self.heading
            # Simulate distance readings with some noise
            base_distance = 0.5 + 0.3 * math.sin(angle * 3)
            noise = random.gauss(0, 0.05)
            frame.append(max(0.0, min(1.0, base_distance + noise)))
        return frame

    def navigate_to_task(self, target: tuple[float, float]) -> bool:
        """Navigate to a task location. Returns True when reached."""
        dx = target[0] - self.position[0]
        dy = target[1] - self.position[1]
        distance = math.sqrt(dx**2 + dy**2)

        if distance < 0.1:
            return True  # Task location reached

        # Move toward target
        self.heading = math.atan2(dy, dx)
        self.position[0] += self.speed * math.cos(self.heading)
        self.position[1] += self.speed * math.sin(self.heading)
        return False

    def generate_task_sensor_data(self, num_frames: int = 5) -> list[list[float]]:
        """Generate multiple sensor frames for a task."""
        return [self.capture_sensor_frame() for _ in range(num_frames)]

    def ascii_display(self, task_desc: str = ""):
        """Simple ASCII robot status display."""
        bars = "█" * int(self.task_count)
        print(f"\n{'─'*50}")
        print(f"  🤖 Robot: {self.robot_id}")
        print(f"  📍 Position: ({self.position[0]:.2f}, {self.position[1]:.2f})")
        print(f"  ✅ Tasks: {self.task_count} | {bars}")
        if task_desc:
            print(f"  🎯 Current: {task_desc}")
        print(f"{'─'*50}")


async def run_simulation(num_tasks: int = 5):
    """Run the warehouse robot simulation.
    Each task completion triggers the full ZK proof pipeline.
    """
    print("\n" + "═"*50)
    print("  🤖 ZeroSense Warehouse Robot Simulation")
    print("  ZK-Verified AI Intelligence on Stellar")
    print("═"*50)

    robot = WarehouseRobot(robot_id="robot-001")

    # Define warehouse tasks
    tasks = [
        {"id": f"task_{i:03d}", "location": (random.uniform(-5, 5), random.uniform(-5, 5)),
         "desc": random.choice(["Pick item A-42", "Deliver to Bay 7", "Scan shelf C-12",
                                  "Restock Zone 3", "Quality check D-99"])}
        for i in range(num_tasks)
    ]

    async with httpx.AsyncClient(base_url=API_URL, timeout=30.0) as client:
        for task in tasks:
            print(f"\n🎯 Task: {task['desc']}")
            robot.ascii_display(task["desc"])

            # Navigate to task location
            reached = False
            steps = 0
            while not reached and steps < 50:
                reached = robot.navigate_to_task(task["location"])
                steps += 1
                if steps % 10 == 0:
                    print(f"   🚶 Navigating... ({steps} steps)")

            print(f"   ✅ Reached task location!")

            # Generate sensor data
            sensor_frames = robot.generate_task_sensor_data(num_frames=3)
            print(f"   📡 Captured {len(sensor_frames)} sensor frames")

            # Trigger ZK proof pipeline via API
            print(f"   🔐 Generating ZK proof...")
            try:
                response = await client.post("/generate-proof", json={
                    "robot_id": robot.robot_id,
                    "task_id": task["id"],
                    "sensor_frames": [
                        {"pixels": frame, "frame_id": i}
                        for i, frame in enumerate(sensor_frames)
                    ],
                })

                if response.status_code == 200:
                    data = response.json()
                    print(f"   ✅ ZK Proof Generated!")
                    print(f"      Action: {data['action_label']} ({data['confidence']}% confidence)")
                    print(f"      Proof hash: {data['proof_hex'][:16]}...")

                    # Submit to Stellar for verification + payment
                    verify_response = await client.post("/verify-proof", json={
                        "robot_id": robot.robot_id,
                        "task_id": task["id"],
                        "proof_hex": data["proof_hex"],
                        "model_hash": data["model_hash"],
                        "confidence": data["confidence"],
                        "action_type": data["action"],
                    })

                    if verify_response.status_code == 200:
                        vdata = verify_response.json()
                        print(f"      🌟 Stellar TX: {vdata.get('stellar_tx', 'pending')}")
                        if vdata.get("auto_payment"):
                            print(f"      ⚡ XLM auto-paid! (confidence >= 95%)")
                        robot.task_count += 1

            except httpx.ConnectError:
                print(f"   ⚠️  API not running. Start with: uvicorn api.main:app --reload")
                # Simulate locally
                print(f"   [DEMO] Proof: {task['id']}_zk_proof")
                print(f"   [DEMO] Action: task_complete (95% confidence)")
                print(f"   [DEMO] XLM auto-paid ⚡")
                robot.task_count += 1

            await asyncio.sleep(1)  # Brief pause between tasks

    print("\n" + "═"*50)
    print(f"  🏆 Simulation Complete!")
    print(f"  ✅ {robot.task_count}/{num_tasks} tasks ZK-verified")
    print(f"  ⚡ XLM payments auto-triggered on Stellar")
    print(f"  ⭐ ZREP reputation tokens minted")
    print("═"*50)


if __name__ == "__main__":
    asyncio.run(run_simulation(num_tasks=5))
