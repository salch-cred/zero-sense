"""Tests for warehouse robot simulation (simulation/robot_sim.py)."""
import math
import pytest
from simulation.robot_sim import WarehouseRobot


@pytest.fixture
def robot():
    return WarehouseRobot(robot_id="test-robot-001")


class TestWarehouseRobot:
    def test_init(self, robot):
        assert robot.robot_id == "test-robot-001"
        assert robot.position == [0.0, 0.0]
        assert robot.task_count == 0

    def test_sensor_frame_64_values(self, robot):
        assert len(robot.capture_sensor_frame()) == 64

    def test_sensor_values_in_range(self, robot):
        for v in robot.capture_sensor_frame():
            assert 0.0 <= v <= 1.0

    def test_generate_task_data_num_frames(self, robot):
        assert len(robot.generate_task_sensor_data(num_frames=5)) == 5

    def test_generate_task_data_frame_shape(self, robot):
        for frame in robot.generate_task_sensor_data(num_frames=3):
            assert len(frame) == 64

    def test_navigate_already_at_target(self, robot):
        robot.position = [0.0, 0.0]
        assert robot.navigate_to_task((0.05, 0.05)) is True

    def test_navigate_moves_toward_target(self, robot):
        robot.position = [0.0, 0.0]
        robot.navigate_to_task((5.0, 0.0))
        assert robot.position[0] > 0.0

    def test_navigate_eventually_reaches(self, robot):
        robot.position = [0.0, 0.0]
        reached = any(robot.navigate_to_task((1.0, 0.0)) for _ in range(100))
        assert reached

    def test_heading_updates(self, robot):
        robot.position = [0.0, 0.0]
        robot.navigate_to_task((0.0, 5.0))
        assert abs(robot.heading - math.pi / 2) < 0.01

    def test_frames_have_noise(self, robot):
        f1, f2 = robot.capture_sensor_frame(), robot.capture_sensor_frame()
        diffs = sum(1 for a, b in zip(f1, f2) if abs(a-b) > 0.001)
        assert diffs > 0
