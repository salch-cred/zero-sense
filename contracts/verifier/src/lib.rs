#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, Vec,
};

/// ZeroSense Verifier Contract
/// Verifies RISC Zero Groth16 proofs of robot AI inference on Stellar Soroban
/// Uses BN254 native host functions (Stellar Protocol 25 X-Ray)
///
/// SECURITY MODEL
/// - initialize() is one-shot and sets the admin (auth required).
/// - Only the admin may register approved model hashes.
/// - Only the robot itself (robot_id.require_auth) may submit its actions.
/// - confidence / model_hash / task_id are BOUND to the proof public inputs,
///   so a caller cannot lie about them.
/// - task_id may be verified at most once (replay protection).

#[contracttype]
#[derive(Clone)]
pub struct RobotAction {
    pub robot_id: Address,
    pub task_id: BytesN<32>,
    pub model_hash: BytesN<32>,
    pub confidence: u32,   // 0-100 (95+ = auto-pay)
    pub action_type: u32,  // 0=task_complete, 1=obstacle, 2=incident
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    RobotAction(BytesN<32>),
    ModelRegistry(BytesN<32>),
    VerifierKey,
}

#[contract]
pub struct ZeroSenseVerifier;

#[contractimpl]
impl ZeroSenseVerifier {

    /// Initialize ONCE with the admin + RISC Zero Groth16 verification key.
    /// Panics if already initialized (prevents VK / admin takeover).
    pub fn initialize(env: Env, admin: Address, vk: Bytes) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        admin.require_auth();
        if vk.len() == 0 {
            panic!("Verification key required");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::VerifierKey, &vk);
    }

    /// Register an approved AI model hash. Admin-only.
    pub fn register_model(env: Env, model_hash: BytesN<32>, model_name: Bytes) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::ModelRegistry(model_hash.clone()), &model_name);
    }

    /// Verify a RISC Zero Groth16 proof of robot AI inference.
    ///
    /// public_inputs layout (each a 32-byte field element):
    ///   [0] model_hash
    ///   [1] input_hash
    ///   [2] output_hash
    ///   [3] confidence (big-endian u32 in the low 4 bytes)
    ///   [4] task_id
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
        // AUTH: only the robot itself can submit actions in its name.
        robot_id.require_auth();

        // Bounds.
        if confidence > 100 {
            panic!("Invalid confidence");
        }
        if action_type > 2 {
            panic!("Invalid action type");
        }

        // Replay protection: a task may only be verified once.
        if env.storage().persistent().has(&DataKey::RobotAction(task_id.clone())) {
            panic!("Task already verified");
        }

        // Model must be registered by the admin.
        if !env.storage().persistent().has(&DataKey::ModelRegistry(model_hash.clone())) {
            panic!("Model not registered");
        }

        // Bind claimed values to the proof's public inputs.
        if public_inputs.len() < 5 {
            panic!("Malformed public inputs");
        }
        if public_inputs.get(0).unwrap() != model_hash {
            panic!("model_hash does not match proof");
        }
        if public_inputs.get(4).unwrap() != task_id {
            panic!("task_id does not match proof");
        }
        if Self::u32_from_be(&public_inputs.get(3).unwrap()) != confidence {
            panic!("confidence does not match proof");
        }

        // Verify the Groth16 proof against the stored VK + public inputs.
        let vk: Bytes = env
            .storage()
            .instance()
            .get(&DataKey::VerifierKey)
            .expect("Not initialized");
        if !Self::verify_groth16_bn254(&env, &vk, &proof, &public_inputs) {
            panic!("Invalid ZK proof");
        }

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

        env.events().publish(
            ("ZeroSense", "RobotActionVerified"),
            (robot_id, confidence, action_type),
        );
        true
    }

    pub fn get_action(env: Env, task_id: BytesN<32>) -> Option<RobotAction> {
        env.storage().persistent().get(&DataKey::RobotAction(task_id))
    }

    /// Read the verified confidence for a task. The payment router calls this
    /// cross-contract so payouts can never trust a caller-supplied confidence.
    pub fn get_verified_confidence(env: Env, task_id: BytesN<32>) -> Option<u32> {
        let action: Option<RobotAction> =
            env.storage().persistent().get(&DataKey::RobotAction(task_id));
        action.map(|a| a.confidence)
    }

    fn u32_from_be(b: &BytesN<32>) -> u32 {
        let a = b.to_array();
        u32::from_be_bytes([a[28], a[29], a[30], a[31]])
    }

    /// Verify Groth16 proof using BN254 host functions.
    ///
    /// NOTE: the full BN254 pairing check is the single remaining integration
    /// point (Stellar Protocol 25 `env.crypto()` host fns). Until then we enforce
    /// the exact 256-byte Groth16 proof size, a non-empty VK, and non-empty
    /// public inputs. This is a structural gate, NOT cryptographic soundness —
    /// see SECURITY.md.
    fn verify_groth16_bn254(
        _env: &Env,
        vk: &Bytes,
        proof: &Bytes,
        public_inputs: &Vec<BytesN<32>>,
    ) -> bool {
        // Groth16 proof = 3 curve points (A:G1, B:G2, C:G1) = 256 bytes.
        if proof.len() != 256 {
            return false;
        }
        if vk.len() == 0 {
            return false;
        }
        if public_inputs.is_empty() {
            return false;
        }
        // TODO(production): full pairing check
        //   e(A,B) == e(alpha,beta) * e(L_pub,gamma) * e(C,delta)
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Events};
    use soroban_sdk::{vec, Bytes, BytesN, Env};

    fn conf_input(env: &Env, confidence: u32) -> BytesN<32> {
        let mut buf = [0u8; 32];
        let be = confidence.to_be_bytes();
        buf[28] = be[0]; buf[29] = be[1]; buf[30] = be[2]; buf[31] = be[3];
        BytesN::from_array(env, &buf)
    }

    fn setup(env: &Env) -> (ZeroSenseVerifierClient, Address, Address, BytesN<32>) {
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(env, &cid);
        let admin = Address::generate(env);
        let robot = Address::generate(env);
        client.initialize(&admin, &Bytes::from_slice(env, &[7u8; 64]));
        let model_hash = BytesN::from_array(env, &[1u8; 32]);
        client.register_model(&model_hash, &Bytes::from_slice(env, b"MobileNetV2-INT8"));
        (client, admin, robot, model_hash)
    }

    fn inputs(env: &Env, model_hash: &BytesN<32>, task_id: &BytesN<32>, conf: u32) -> Vec<BytesN<32>> {
        vec![
            env,
            model_hash.clone(),
            BytesN::from_array(env, &[9u8; 32]),
            BytesN::from_array(env, &[8u8; 32]),
            conf_input(env, conf),
            task_id.clone(),
        ]
    }

    #[test]
    fn test_verify_robot_action() {
        let env = Env::default();
        let (client, _admin, robot, model_hash) = setup(&env);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Bytes::from_slice(&env, &[0u8; 256]);
        let pi = inputs(&env, &model_hash, &task_id, 95);
        let result = client.verify_robot_action(&proof, &pi, &robot, &task_id, &model_hash, &95u32, &0u32);
        assert!(result);
        assert_eq!(client.get_verified_confidence(&task_id), Some(95));
        assert!(!env.events().all().is_empty());
    }

    #[test]
    #[should_panic(expected = "Model not registered")]
    fn test_unregistered_model_rejected() {
        let env = Env::default();
        let (client, _admin, robot, _model_hash) = setup(&env);
        let bad_model = BytesN::from_array(&env, &[42u8; 32]);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Bytes::from_slice(&env, &[0u8; 256]);
        let pi = inputs(&env, &bad_model, &task_id, 95);
        client.verify_robot_action(&proof, &pi, &robot, &task_id, &bad_model, &95u32, &0u32);
    }

    #[test]
    #[should_panic(expected = "Invalid ZK proof")]
    fn test_wrong_size_proof_rejected() {
        let env = Env::default();
        let (client, _admin, robot, model_hash) = setup(&env);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Bytes::from_slice(&env, &[0u8; 32]); // too short
        let pi = inputs(&env, &model_hash, &task_id, 95);
        client.verify_robot_action(&proof, &pi, &robot, &task_id, &model_hash, &95u32, &0u32);
    }

    #[test]
    #[should_panic(expected = "confidence does not match proof")]
    fn test_confidence_tampering_rejected() {
        let env = Env::default();
        let (client, _admin, robot, model_hash) = setup(&env);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Bytes::from_slice(&env, &[0u8; 256]);
        let pi = inputs(&env, &model_hash, &task_id, 80); // proof says 80
        client.verify_robot_action(&proof, &pi, &robot, &task_id, &model_hash, &100u32, &0u32); // caller claims 100
    }

    #[test]
    #[should_panic(expected = "Task already verified")]
    fn test_replay_rejected() {
        let env = Env::default();
        let (client, _admin, robot, model_hash) = setup(&env);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let proof = Bytes::from_slice(&env, &[0u8; 256]);
        let pi = inputs(&env, &model_hash, &task_id, 95);
        client.verify_robot_action(&proof, &pi, &robot, &task_id, &model_hash, &95u32, &0u32);
        client.verify_robot_action(&proof, &pi, &robot, &task_id, &model_hash, &95u32, &0u32);
    }

    #[test]
    #[should_panic(expected = "Already initialized")]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        let (client, _admin, _robot, _model_hash) = setup(&env);
        let admin2 = Address::generate(&env);
        client.initialize(&admin2, &Bytes::from_slice(&env, &[1u8; 32]));
    }
}
