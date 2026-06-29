"""ZeroSense AI Inference Engine

Runs MobileNetV2 INT8 ONNX model on robot sensor frames.
Outputs (action, confidence, input_hash) for ZK proof generation.

The model weights are committed on-chain as a hash —
anyone can verify the EXACT model was used.
"""

import hashlib
import os
from typing import Optional

import numpy as np

try:
    import onnxruntime as ort
    ONNX_AVAILABLE = True
except ImportError:
    ONNX_AVAILABLE = False
    print("[Inference] ⚠️  onnxruntime not installed — using mock inference")


ACTION_LABELS = {
    0: "task_complete",
    1: "obstacle_detected",
    2: "incident",
}


def _softmax(logits: np.ndarray) -> np.ndarray:
    """Numerically stable softmax.

    Subtracting the max before exp prevents float overflow on large logits,
    which would otherwise produce inf/inf = nan and crash int(nan).
    """
    logits = np.asarray(logits, dtype=np.float64)
    shifted = logits - np.max(logits)
    exp = np.exp(shifted)
    total = np.sum(exp)
    if total == 0 or not np.isfinite(total):
        # Degenerate case — fall back to uniform distribution.
        return np.full_like(exp, 1.0 / exp.size)
    return exp / total


class RobotInferenceEngine:
    """Runs AI inference on robot sensor data.
    Supports ONNX Runtime (production) and mock (development).
    """

    def __init__(self, model_path: Optional[str] = None):
        self.model_path = model_path or os.path.join(
            os.path.dirname(__file__), "mobilenet_v2.onnx"
        )
        self.session = None
        self.model_hash = self._compute_model_hash()

        if ONNX_AVAILABLE and os.path.exists(self.model_path):
            self.session = ort.InferenceSession(self.model_path)
            print(f"[Inference] ✅ Loaded ONNX model: {self.model_path}")
            print(f"[Inference] Model hash: {self.model_hash[:16]}...")
        else:
            print(f"[Inference] ⚠️  Running in mock mode (no ONNX model found)")
            print(f"[Inference] Place mobilenet_v2.onnx in model/ directory for production")

    def _compute_model_hash(self) -> str:
        """Compute SHA256 hash of model weights.
        This hash is committed on-chain — proves EXACT model was used.
        """
        if os.path.exists(self.model_path):
            with open(self.model_path, "rb") as f:
                return hashlib.sha256(f.read()).hexdigest()
        return "mock_model_hash_" + "0" * 48

    def run_inference(
        self, sensor_frames: list[list[float]]
    ) -> tuple[int, int, str]:
        """Run AI inference on sensor frames.

        Args:
            sensor_frames: List of sensor data frames (camera pixels, LiDAR, etc.)

        Returns:
            (action, confidence, input_hash)
            - action: 0=task_complete, 1=obstacle, 2=incident
            - confidence: 0-100
            - input_hash: SHA256 of sensor input (public commitment)
        """
        # Compute input hash (public commitment — sensor data stays private)
        input_hash = self._compute_input_hash(sensor_frames)

        if self.session is not None:
            # Real ONNX inference
            action, confidence = self._run_onnx(sensor_frames)
        else:
            # Mock inference for development
            action, confidence = self._mock_inference(sensor_frames)

        print(f"[Inference] → Action: {ACTION_LABELS[action]} ({confidence}% confidence)")
        return action, confidence, input_hash

    def _run_onnx(self, frames: list[list[float]]) -> tuple[int, int]:
        """Run actual MobileNetV2 ONNX inference."""
        # Prepare input: resize to 224x224x3
        if not frames or not frames[0]:
            return 2, 0

        # Flatten and reshape sensor data to model input format
        flat = np.array(frames[0], dtype=np.float32)
        # Pad or trim to 224*224*3 = 150528
        target_size = 224 * 224 * 3
        if len(flat) < target_size:
            flat = np.pad(flat, (0, target_size - len(flat)))
        else:
            flat = flat[:target_size]

        input_tensor = flat.reshape(1, 3, 224, 224)
        input_name = self.session.get_inputs()[0].name
        outputs = self.session.run(None, {input_name: input_tensor})

        logits = outputs[0][0]
        probs = _softmax(logits)
        predicted_class = int(np.argmax(probs))
        confidence = int(np.max(probs) * 100)

        # Map class to robot action
        action = predicted_class % 3  # Map to 0, 1, or 2
        return action, min(confidence, 99)

    def _mock_inference(self, frames: list[list[float]]) -> tuple[int, int]:
        """Mock inference for development (no ONNX model needed)."""
        if not frames or not frames[0]:
            return 2, 50

        avg = sum(frames[0]) / len(frames[0]) if frames[0] else 0.5

        if avg > 0.7:
            return 0, 97   # task_complete, high confidence
        elif avg > 0.4:
            return 1, 88   # obstacle_detected
        else:
            return 0, 72   # task_complete, lower confidence

    def _compute_input_hash(self, frames: list[list[float]]) -> str:
        """Compute SHA256 of sensor input."""
        h = hashlib.sha256()
        for frame in frames:
            h.update(bytes([int(abs(x) * 255) % 256 for x in frame]))
        return h.hexdigest()
