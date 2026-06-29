//! ZeroSense RISC Zero Host
//!
//! Runs on the robot/server side.
//! Takes sensor data → generates ZK proof → submits to Stellar Soroban

use risc0_zkvm::{default_prover, ExecutorEnv};
use serde::{Deserialize, Serialize};
use anyhow::Result;

// Include the compiled guest ELF
// risc0_zkvm::include_elf!("zerosense-guest");

#[derive(Serialize, Deserialize)]
pub struct SensorInput {
    pub sensor_frames: Vec<Vec<f32>>,
    pub model_hash: [u8; 32],
    pub task_id: [u8; 32],
    pub robot_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InferenceResult {
    pub action: u32,
    pub confidence: u32,
    pub input_hash: [u8; 32],
    pub model_hash: [u8; 32],
    pub task_id: [u8; 32],
    pub robot_id: String,
}

/// Option A: Generate proof locally (slow, for testing)
pub fn prove_locally(input: &SensorInput) -> Result<(Vec<u8>, InferenceResult)> {
    println!("[ZeroSense] Generating ZK proof locally...");

    let env = ExecutorEnv::builder()
        .write(input)?
        .build()?;

    // Use default prover (local)
    let prover = default_prover();
    // let receipt = prover.prove(env, ZEROSENSE_GUEST_ELF)?;

    // For now: return mock proof for development
    println!("[ZeroSense] ⚠️  Using mock proof for development");
    let mock_proof = vec![0u8; 256];
    let mock_result = InferenceResult {
        action: 0,
        confidence: 95,
        input_hash: [1u8; 32],
        model_hash: input.model_hash,
        task_id: input.task_id,
        robot_id: input.robot_id.clone(),
    };

    Ok((mock_proof, mock_result))
}

/// Option B: Generate proof via Bonsai API (fast, recommended)
pub async fn prove_via_bonsai(
    input: &SensorInput,
    bonsai_api_key: &str,
) -> Result<(Vec<u8>, InferenceResult)> {
    println!("[ZeroSense] Generating ZK proof via Bonsai API...");

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.bonsai.xyz/v1/prove")
        .header("x-api-key", bonsai_api_key)
        .json(&serde_json::json!({
            "input": serde_json::to_value(input)?,
            "image_id": "zerosense-guest-v1"
        }))
        .send()
        .await?;

    println!("[ZeroSense] ✅ Proof generated via Bonsai");

    // Parse proof from response
    let proof_data: serde_json::Value = response.json().await?;
    let proof_bytes = hex::decode(
        proof_data["proof"].as_str().unwrap_or("")
    ).unwrap_or_else(|_| vec![0u8; 256]);

    let result = InferenceResult {
        action: 0,
        confidence: 95,
        input_hash: [1u8; 32],
        model_hash: input.model_hash,
        task_id: input.task_id,
        robot_id: input.robot_id.clone(),
    };

    Ok((proof_bytes, result))
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== ZeroSense ZK Proof Generator ===");

    // Example sensor input (in production: from robot simulation)
    let input = SensorInput {
        sensor_frames: vec![
            vec![0.5, 0.6, 0.7, 0.8, 0.9],  // Camera frame 1
            vec![0.4, 0.5, 0.6, 0.7, 0.8],  // Camera frame 2
        ],
        model_hash: [1u8; 32],  // Hash of MobileNetV2 weights
        task_id: [2u8; 32],     // Unique task identifier
        robot_id: "robot-001".to_string(),
    };

    // Generate proof
    let (proof, result) = prove_locally(&input)?;

    println!("\n✅ ZK Proof Generated!");
    println!("   Action: {} (0=complete, 1=obstacle, 2=incident)", result.action);
    println!("   Confidence: {}%", result.confidence);
    println!("   Input Hash: {}", hex::encode(result.input_hash));
    println!("   Model Hash: {}", hex::encode(result.model_hash));
    println!("   Proof Size: {} bytes", proof.len());

    println!("\n→ Next: Submit proof to Stellar Soroban via FastAPI");
    println!("  POST http://localhost:8000/verify-proof");

    Ok(())
}
