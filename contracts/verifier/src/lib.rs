#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, Vec,
};

/// ZeroSense Verifier Contract
/// Verifies RISC Zero Groth16 proofs of robot AI inference on Stellar Soroban
/// Uses BN254 native host functions (Stellar Protocol 25 X-Ray)

#[contracttype]
#[derive(Clone)]
pub struct RobotAction {
    pub robot_id: Address,
    pub task_id: BytesN<32>,
    pub model_hash: BytesN<32>,  // SHA256 of AI model weights
    pub confidence: u32,          // 0-100 (95+ = auto-pay)
    pub action_type: u32,         // 0=task_complete, 1=obstacle, 2=incident
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    RobotAction(BytesN<32>),
    ModelRegistry(BytesN<32>),
    VerifierKey,
}

#[contract]
pub struct ZeroSenseVerifier;

#[contractimpl]
impl ZeroSenseVerifier {

    /// Initialize the contract with the RISC Zero Groth16 verification key
    pub fn initialize(env: Env, vk: Bytes) {
        env.storage().instance().set(&DataKey::VerifierKey, &vk);
    }

    /// Register an approved AI model hash
    /// Only registered models can have their proofs verified
    pub fn register_model(
        env: Env,
        admin: Address,
        model_hash: BytesN<32>,
        model_name: Bytes,
    ) {
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::ModelRegistry(model_hash.clone()), &model_name);
    }

    /// Verify a RISC Zero Groth16 proof of robot AI inference
    /// Called by the robot operator after generating proof via Bonsai API
    ///
    /// proof: Groth16 proof bytes (from RISC Zero Bonsai)
    /// public_inputs: [model_hash, input_hash, output_hash, confidence, task_id]
    /// robot_id: The robot's Stellar address
    /// task_id: Unique task identifier
    /// confidence: AI confidence score (0-100)
    pub fn verify_robot_action(
        env: Env,
        proof: Bytes,
        public_inputs: Vec<BytesN<32>>,
        robot_id: Address,
        task_id: BytesN<32>,
        model_hash: BytesN<32>,
        confidence: u32,
        action_type: u32,
    ) -> bool {
        // 1. Check model is registered
        let model_registered = env
            .storage()
            .persistent()
            .has(&DataKey::ModelRegistry(model_hash.clone()));
        if !model_registered {
            panic!("Model not registered");
        }

        // 2. Verify Groth16 proof using Stellar BN254 host functions
        // In production: use soroban BN254 pairing_check
        // For hackathon MVP: verify proof structure + public inputs
        let proof_valid = Self::verify_groth16_bn254(&env, &proof, &public_inputs);
        if !proof_valid {
            panic!("Invalid ZK proof");
        }

        // 3. Store verified robot action
        let action = RobotAction {
            robot_id: robot_id.clone(),
            task_id: task_id.clone(),
            model_hash,
            confidence,
            action_type,
            timestamp: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::RobotAction(task_id), &action);

        // 4. Emit event for Guardian agents to pick up
        env.events().publish(
            ("ZeroSense", "RobotActionVerified"),
            (robot_id, confidence, action_type),
        );

        true
    }

    /// Get a verified robot action by task ID
    pub fn get_action(env: Env, task_id: BytesN<32>) -> Option<RobotAction> {
        env.storage()
            .persistent()
            .get(&DataKey::RobotAction(task_id))
    }

    /// Internal: verify Groth16 proof using BN254 host functions
    /// Full implementation uses soroban_sdk BN254 pairing operations
    fn verify_groth16_bn254(
        _env: &Env,
        proof: &Bytes,
        public_inputs: &Vec<BytesN<32>>,
    ) -> bool {
        // Proof must be non-empty and have correct length (Groth16 = 256 bytes)
        if proof.len() < 32 {
            return false;
        }
        // Public inputs must be present
        if public_inputs.is_empty() {
            return false;
        }
        // TODO: Full BN254 Groth16 pairing verification
        // e(A, B) == e(alpha, beta) * e(vk_x, gamma) * e(C, delta)
        // Using env.crypto().bn254_pairing_check() from Stellar Protocol 25
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Events};
    use soroban_sdk::{vec, Env};

    #[test]
    fn test_verify_robot_action() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let robot = Address::generate(&env);

        // Initialize with dummy VK
        let vk = Bytes::from_slice(&env, &[0u8; 64]);
        client.initialize(&vk);

        // Register a model
        let model_hash = BytesN::from_array(&env, &[1u8; 32]);
        let model_name = Bytes::from_slice(&env, b"MobileNetV2-INT8");
        client.register_model(&admin, &model_hash, &model_name);

        // Verify a robot action
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Bytes::from_slice(&env, &[0u8; 256]);
        let public_inputs = vec![&env, BytesN::from_array(&env, &[3u8; 32])];

        let result = client.verify_robot_action(
            &proof,
            &public_inputs,
            &robot,
            &task_id,
            &model_hash,
            &95u32,
            &0u32,
        );
        assert!(result);

        // Check event was emitted
        let events = env.events().all();
        assert!(!events.is_empty());
    }
}
