//! ZeroSense RISC Zero Guest Program
//!
//! This program runs inside the zkVM and generates a ZK proof that:
//! 1. An AI model with a specific hash (model_hash) ran inference
//! 2. On sensor data with a specific hash (input_hash)
//! 3. And produced output (action, confidence) correctly
//!
//! The proof is verified on Stellar Soroban via BN254 Groth16

use risc0_zkvm::guest::env;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
pub struct SensorInput {
    /// Sensor data frames (camera, LiDAR, etc.) - kept private
    pub sensor_frames: Vec<Vec<f32>>,
    /// Hash of the AI model weights (public commitment)
    pub model_hash: [u8; 32],
    /// Task identifier
    pub task_id: [u8; 32],
    /// Robot ID
    pub robot_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct InferenceResult {
    /// Action decided by AI: 0=task_complete, 1=obstacle_detected, 2=incident
    pub action: u32,
    /// Confidence score 0-100
    pub confidence: u32,
    /// Hash of the sensor input (public, for verification)
    pub input_hash: [u8; 32],
    /// Hash of the model (public, matches registered model)
    pub model_hash: [u8; 32],
    /// Task ID (public)
    pub task_id: [u8; 32],
    /// Robot ID (public)
    pub robot_id: String,
}

fn main() {
    // Read private sensor input from host
    let input: SensorInput = env::read();

    // Compute hash of sensor data (this becomes a public commitment)
    // The actual sensor frames remain PRIVATE inside the zkVM
    let input_hash = compute_sensor_hash(&input.sensor_frames);

    // Run AI inference simulation
    // In production: run actual ONNX model inside zkVM via Ezkl
    let (action, confidence) = run_inference(&input.sensor_frames, &input.model_hash);

    // Construct public output (what goes on-chain)
    let result = InferenceResult {
        action,
        confidence,
        input_hash,
        model_hash: input.model_hash,
        task_id: input.task_id,
        robot_id: input.robot_id,
    };

    // Commit the result publicly — this becomes the proof's public journal
    env::commit(&result);
}

/// Compute SHA256 hash of sensor frames
/// The frames stay private — only this hash is revealed publicly
fn compute_sensor_hash(frames: &[Vec<f32>]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for frame in frames {
        for pixel in frame {
            hasher.update(&pixel.to_le_bytes());
        }
    }
    hasher.finalize().into()
}

/// Simulate AI inference
/// Returns (action_type, confidence_score)
/// In production: use Ezkl to prove actual neural network inference
fn run_inference(frames: &[Vec<f32>], _model_hash: &[u8; 32]) -> (u32, u32) {
    if frames.is_empty() {
        return (2, 0); // incident - no sensor data
    }

    // Simplified inference: detect obstacles by checking average pixel values
    let avg: f32 = frames[0].iter().sum::<f32>() / frames[0].len() as f32;

    if avg > 0.8 {
        (1, 97)  // obstacle detected, high confidence
    } else if avg > 0.3 {
        (0, 95)  // task complete, high confidence
    } else {
        (0, 72)  // task complete, low confidence
    }
}
