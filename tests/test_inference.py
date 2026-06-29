"""Tests for AI inference engine (model/inference.py).

All 10 tests run in mock mode — no ONNX file required.
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))

import pytest
from model.inference import RobotInferenceEngine, ACTION_LABELS


@pytest.fixture
def engine():
    return RobotInferenceEngine()


class TestRobotInferenceEngine:

    def test_engine_initializes(self, engine):
        assert engine is not None

    def test_model_hash_set(self, engine):
        assert engine.model_hash is not None
        assert len(engine.model_hash) > 0

    def test_run_inference_returns_triple(self, engine):
        frames = [[0.5] * 64]
        result = engine.run_inference(frames)
        assert len(result) == 3

    def test_action_in_valid_range(self, engine):
        action, _, _ = engine.run_inference([[0.5] * 64])
        assert action in (0, 1, 2)

    def test_confidence_in_valid_range(self, engine):
        _, confidence, _ = engine.run_inference([[0.5] * 64])
        assert 0 <= confidence <= 100

    def test_input_hash_is_sha256(self, engine):
        _, _, h = engine.run_inference([[0.5] * 64])
        assert len(h) == 64

    def test_high_avg_gives_task_complete(self, engine):
        action, confidence, _ = engine.run_inference([[0.9] * 64])
        assert action == 0   # task_complete
        assert confidence >= 90

    def test_mid_avg_gives_obstacle(self, engine):
        action, _, _ = engine.run_inference([[0.55] * 64])
        assert action == 1   # obstacle_detected

    def test_low_avg_gives_low_confidence(self, engine):
        _, confidence, _ = engine.run_inference([[0.2] * 64])
        assert confidence < 80

    def test_empty_frames_returns_incident(self, engine):
        action, _, _ = engine.run_inference([])
        assert action == 2   # incident

    def test_hash_is_deterministic(self, engine):
        _, _, h1 = engine.run_inference([[0.5] * 64])
        _, _, h2 = engine.run_inference([[0.5] * 64])
        assert h1 == h2

    def test_different_inputs_different_hashes(self, engine):
        _, _, h1 = engine.run_inference([[0.1] * 64])
        _, _, h2 = engine.run_inference([[0.9] * 64])
        assert h1 != h2

    def test_action_labels_complete(self):
        assert ACTION_LABELS[0] == "task_complete"
        assert ACTION_LABELS[1] == "obstacle_detected"
        assert ACTION_LABELS[2] == "incident"
