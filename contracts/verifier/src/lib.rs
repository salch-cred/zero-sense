#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    crypto::bls12_381::{Fr, G1Affine, G2Affine},
    vec, Address, Bytes, BytesN, Env, Vec,
};

/// ZeroSense Verifier Contract
/// REAL on-chain Groth16 verification of robot AI-inference proofs on Soroban.
///
/// CRYPTOGRAPHY IS NOT A STUB. `verify_robot_action` and `verify_groth16_proof`
/// run the full Groth16 verification equation through the Soroban host's native
/// BLS12-381 pairing primitive (`env.crypto().bls12_381().pairing_check`,
/// CAP-0059, shipped in Protocol 22):
///
///   e(A, B) == e(alpha, beta) * e(vk_x, gamma) * e(C, delta)
///
/// rearranged into the product-equals-identity form that `pairing_check` accepts:
///
///   e(A, B) * e(-alpha, beta) * e(-vk_x, gamma) * e(-C, delta) == 1
///
/// where  vk_x = ic[0] + Σ public_i · ic[i+1].
///
/// CURVE NOTE (read SECURITY.md): Soroban exposes a *native* pairing host
/// function for BLS12-381 — NOT BN254. Proofs must therefore be produced over
/// BLS12-381 (e.g. Circom/snarkjs or arkworks with the BLS12-381 backend, or a
/// RISC Zero -> BLS12-381 Groth16 wrapper). Every public-input field element
/// must be < r (the BLS12-381 scalar field order), so any SHA-256 hash used as a
/// public input must be reduced mod r inside the circuit.
///
/// SECURITY MODEL
/// - initialize() is one-shot and sets the admin + verification key (auth required).
/// - Only the admin may register approved model hashes.
/// - Only the robot itself (robot_id.require_auth) may submit its actions.
/// - confidence / model_hash / task_id are BOUND to the proof public inputs,
///   so a caller cannot lie about them.
/// - task_id may be verified at most once (replay protection).

/// Uncompressed G1 point length (BLS12-381 uses 48-byte base-field elements).
const G1_LEN: u32 = 96;
/// Uncompressed G2 point length.
const G2_LEN: u32 = 192;
/// Groth16 proof = A (G1) || B (G2) || C (G1).
const PROOF_LEN: u32 = G1_LEN + G2_LEN + G1_LEN; // 384

/// `-1 mod r` for the BLS12-381 scalar field. Multiplying a G1 point by this
/// scalar negates it (cheaper and simpler than re-deriving from coordinates).
const NEG_ONE: [u8; 32] = [
    0x73, 0xed, 0xa7, 0x53, 0x29, 0x9d, 0x7d, 0x48, 0x33, 0x39, 0xd8, 0x08, 0x09, 0xa1, 0xd8, 0x05,
    0x53, 0xbd, 0xa4, 0x02, 0xff, 0xfe, 0x5b, 0xfe, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
];

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    MalformedProof = 3,
    MalformedVerifyingKey = 4,
}

/// Groth16 verification key. Curve points are stored uncompressed (byte form)
/// and rehydrated into curve points on demand.
#[contracttype]
#[derive(Clone)]
pub struct VerificationKey {
    pub alpha: BytesN<96>,
    pub beta: BytesN<192>,
    pub gamma: BytesN<192>,
    pub delta: BytesN<192>,
    /// IC points: `ic.len() == num_public_inputs + 1`.
    pub ic: Vec<BytesN<96>>,
}

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
    Vk,
    RobotAction(BytesN<32>),
    ModelRegistry(BytesN<32>),
}

#[contract]
pub struct ZeroSenseVerifier;

#[contractimpl]
impl ZeroSenseVerifier {
    /// Initialize ONCE with the admin + BLS12-381 Groth16 verification key.
    /// Fails if already initialized (prevents VK / admin takeover).
    pub fn initialize(env: Env, admin: Address, vk: VerificationKey) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        if vk.ic.is_empty() {
            return Err(Error::MalformedVerifyingKey);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Vk, &vk);
        Ok(())
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
            .set(&DataKey::ModelRegistry(model_hash), &model_name);
    }

    /// Pure cryptographic entry point: verify a Groth16 proof against the stored
    /// verification key. No side effects — usable for demos and by other
    /// contracts that only need a yes/no proof check.
    pub fn verify_groth16_proof(
        env: Env,
        proof: Bytes,
        public_inputs: Vec<BytesN<32>>,
    ) -> Result<bool, Error> {
        let vk: VerificationKey = env
            .storage()
            .instance()
            .get(&DataKey::Vk)
            .ok_or(Error::NotInitialized)?;
        Self::groth16_pairing_holds(&env, &vk, &proof, &public_inputs)
    }

    /// Verify a Groth16 proof of robot AI inference and record the action.
    ///
    /// public_inputs layout (each a 32-byte field element, big-endian, < r):
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
        if env
            .storage()
            .persistent()
            .has(&DataKey::RobotAction(task_id.clone()))
        {
            panic!("Task already verified");
        }

        // Model must be registered by the admin.
        if !env
            .storage()
            .persistent()
            .has(&DataKey::ModelRegistry(model_hash.clone()))
        {
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

        // REAL Groth16 pairing verification against the stored VK.
        let vk: VerificationKey = env
            .storage()
            .instance()
            .get(&DataKey::Vk)
            .expect("Not initialized");
        match Self::groth16_pairing_holds(&env, &vk, &proof, &public_inputs) {
            Ok(true) => {}
            _ => panic!("Invalid ZK proof"),
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

    /// Read the robot that produced a verified task's proof. Used by
    /// `contracts/consensus` (zk-Swarm Consensus) to bind a witness vote to
    /// the actual prover: a caller cannot borrow someone else's already-
    /// verified `task_id` to fake being a witness, because this always
    /// returns the `robot_id` that was `require_auth`'d when the proof was
    /// originally submitted — never the caller's own address.
    pub fn get_action_robot(env: Env, task_id: BytesN<32>) -> Option<Address> {
        let action: Option<RobotAction> =
            env.storage().persistent().get(&DataKey::RobotAction(task_id));
        action.map(|a| a.robot_id)
    }

    // ----------------------------- internal -----------------------------

    fn u32_from_be(b: &BytesN<32>) -> u32 {
        let a = b.to_array();
        u32::from_be_bytes([a[28], a[29], a[30], a[31]])
    }

    /// The REAL Groth16 check via the BLS12-381 pairing host function.
    fn groth16_pairing_holds(
        env: &Env,
        vk: &VerificationKey,
        proof: &Bytes,
        public_inputs: &Vec<BytesN<32>>,
    ) -> Result<bool, Error> {
        if proof.len() != PROOF_LEN {
            return Err(Error::MalformedProof);
        }
        if public_inputs.len() + 1 != vk.ic.len() {
            return Err(Error::MalformedVerifyingKey);
        }

        let bls = env.crypto().bls12_381();

        // Decode the proof into curve points: A || B || C.
        let a = G1Affine::from_bytes(slice_g1(proof, 0));
        let b = G2Affine::from_bytes(slice_g2(proof, G1_LEN));
        let c = G1Affine::from_bytes(slice_g1(proof, G1_LEN + G2_LEN));

        // vk_x = ic[0] + Σ public_i · ic[i+1]
        let mut vk_x = G1Affine::from_bytes(vk.ic.get(0).unwrap());
        for i in 0..public_inputs.len() {
            let scalar = Fr::from_bytes(public_inputs.get(i).unwrap());
            let ic_point = G1Affine::from_bytes(vk.ic.get(i + 1).unwrap());
            let term = bls.g1_mul(&ic_point, &scalar);
            vk_x = bls.g1_add(&vk_x, &term);
        }

        let neg_one = Fr::from_bytes(BytesN::from_array(env, &NEG_ONE));
        let alpha = G1Affine::from_bytes(vk.alpha.clone());

        // e(A,B) * e(-alpha,beta) * e(-vk_x,gamma) * e(-C,delta) == 1
        let vp1 = vec![
            env,
            a,
            bls.g1_mul(&alpha, &neg_one),
            bls.g1_mul(&vk_x, &neg_one),
            bls.g1_mul(&c, &neg_one),
        ];
        let vp2 = vec![
            env,
            b,
            G2Affine::from_bytes(vk.beta.clone()),
            G2Affine::from_bytes(vk.gamma.clone()),
            G2Affine::from_bytes(vk.delta.clone()),
        ];

        Ok(bls.pairing_check(vp1, vp2))
    }
}

/// Read an uncompressed G1 point starting at `offset`.
fn slice_g1(data: &Bytes, offset: u32) -> BytesN<96> {
    data.slice(offset..offset + G1_LEN).try_into().unwrap()
}

/// Read an uncompressed G2 point starting at `offset`.
fn slice_g2(data: &Bytes, offset: u32) -> BytesN<192> {
    data.slice(offset..offset + G2_LEN).try_into().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{vec, Address, Bytes, BytesN, Env, Vec};

    const DST: &[u8] = b"ZeroSense-test";

    fn g1(env: &Env, label: &[u8]) -> G1Affine {
        env.crypto()
            .bls12_381()
            .hash_to_g1(&Bytes::from_slice(env, label), &Bytes::from_slice(env, DST))
    }

    fn g2(env: &Env, label: &[u8]) -> G2Affine {
        env.crypto()
            .bls12_381()
            .hash_to_g2(&Bytes::from_slice(env, label), &Bytes::from_slice(env, DST))
    }

    fn conf_input(env: &Env, confidence: u32) -> BytesN<32> {
        let mut buf = [0u8; 32];
        let be = confidence.to_be_bytes();
        buf[28] = be[0];
        buf[29] = be[1];
        buf[30] = be[2];
        buf[31] = be[3];
        BytesN::from_array(env, &buf)
    }

    /// Build (vk, proof) that satisfies the Groth16 equation BY CONSTRUCTION for
    /// the given public inputs, using one shared G2 point H for beta=gamma=delta=B.
    /// Then e(A,B)*e(-alpha,H)*e(-vk_x,H)*e(-C,H) = e(A-alpha-vk_x-C, H) = 1
    /// iff A = alpha + vk_x + C, with vk_x = ic[0] + Σ pub_i·ic[i+1].
    /// This exercises the host's REAL BLS12-381 pairing — no mock.
    fn valid_instance(env: &Env, pub_inputs: &Vec<BytesN<32>>) -> (VerificationKey, Bytes) {
        let bls = env.crypto().bls12_381();
        let h = g2(env, b"H");
        let alpha = g1(env, b"alpha");
        let c = g1(env, b"C");

        let ic0 = g1(env, b"ic0");
        let mut vk_x = ic0.clone();
        let mut ic_bytes: Vec<BytesN<96>> = vec![env, ic0.to_bytes()];
        for i in 0..pub_inputs.len() {
            let ic_i = g1(env, &[b'i', b'c', (i as u8) + 1]);
            let scalar = Fr::from_bytes(pub_inputs.get(i).unwrap());
            vk_x = bls.g1_add(&vk_x, &bls.g1_mul(&ic_i, &scalar));
            ic_bytes.push_back(ic_i.to_bytes());
        }

        // A = alpha + vk_x + C
        let a = bls.g1_add(&bls.g1_add(&alpha, &vk_x), &c);

        let vk = VerificationKey {
            alpha: alpha.to_bytes(),
            beta: h.to_bytes(),
            gamma: h.to_bytes(),
            delta: h.to_bytes(),
            ic: ic_bytes,
        };

        let mut proof = Bytes::new(env);
        proof.append(&a.to_bytes().into());
        proof.append(&h.to_bytes().into());
        proof.append(&c.to_bytes().into());

        (vk, proof)
    }

    #[test]
    fn test_real_pairing_verifies_and_rejects_tampered() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);

        let empty: Vec<BytesN<32>> = vec![&env];
        let (vk, proof) = valid_instance(&env, &empty);
        client.initialize(&admin, &vk);

        // Genuine proof verifies via the REAL BLS12-381 pairing.
        assert!(client.verify_groth16_proof(&proof, &empty));

        // Tamper: swap C for an unrelated point -> equation no longer holds.
        let bogus_c = g1(&env, b"not-C");
        let mut tampered = proof.slice(0..(G1_LEN + G2_LEN));
        tampered.append(&bogus_c.to_bytes().into());
        assert!(!client.verify_groth16_proof(&tampered, &empty));
    }

    #[test]
    fn test_robot_action_end_to_end_real_proof() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let robot = Address::generate(&env);

        let model_hash = BytesN::from_array(&env, &[1u8; 32]);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let pub_inputs = vec![
            &env,
            model_hash.clone(),
            BytesN::from_array(&env, &[9u8; 32]),
            BytesN::from_array(&env, &[8u8; 32]),
            conf_input(&env, 95),
            task_id.clone(),
        ];
        let (vk, proof) = valid_instance(&env, &pub_inputs);
        client.initialize(&admin, &vk);
        client.register_model(&model_hash, &Bytes::from_slice(&env, b"MobileNetV2-INT8"));

        let ok = client.verify_robot_action(
            &proof, &pub_inputs, &robot, &task_id, &model_hash, &95u32, &0u32,
        );
        assert!(ok);
        assert_eq!(client.get_verified_confidence(&task_id), Some(95));
        assert_eq!(client.get_action_robot(&task_id), Some(robot));
    }

    #[test]
    #[should_panic(expected = "Model not registered")]
    fn test_unregistered_model_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let robot = Address::generate(&env);

        let model_hash = BytesN::from_array(&env, &[1u8; 32]);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let pub_inputs = vec![
            &env,
            model_hash.clone(),
            BytesN::from_array(&env, &[9u8; 32]),
            BytesN::from_array(&env, &[8u8; 32]),
            conf_input(&env, 95),
            task_id.clone(),
        ];
        let (vk, proof) = valid_instance(&env, &pub_inputs);
        client.initialize(&admin, &vk);
        // NOTE: model intentionally NOT registered.
        client.verify_robot_action(
            &proof, &pub_inputs, &robot, &task_id, &model_hash, &95u32, &0u32,
        );
    }

    #[test]
    #[should_panic(expected = "confidence does not match proof")]
    fn test_confidence_tampering_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let robot = Address::generate(&env);

        let model_hash = BytesN::from_array(&env, &[1u8; 32]);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        // Proof commits to confidence = 80 ...
        let pub_inputs = vec![
            &env,
            model_hash.clone(),
            BytesN::from_array(&env, &[9u8; 32]),
            BytesN::from_array(&env, &[8u8; 32]),
            conf_input(&env, 80),
            task_id.clone(),
        ];
        let (vk, proof) = valid_instance(&env, &pub_inputs);
        client.initialize(&admin, &vk);
        client.register_model(&model_hash, &Bytes::from_slice(&env, b"MobileNetV2-INT8"));
        // ... but the caller claims 100.
        client.verify_robot_action(
            &proof, &pub_inputs, &robot, &task_id, &model_hash, &100u32, &0u32,
        );
    }

    #[test]
    #[should_panic(expected = "Task already verified")]
    fn test_replay_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let robot = Address::generate(&env);

        let model_hash = BytesN::from_array(&env, &[1u8; 32]);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let pub_inputs = vec![
            &env,
            model_hash.clone(),
            BytesN::from_array(&env, &[9u8; 32]),
            BytesN::from_array(&env, &[8u8; 32]),
            conf_input(&env, 95),
            task_id.clone(),
        ];
        let (vk, proof) = valid_instance(&env, &pub_inputs);
        client.initialize(&admin, &vk);
        client.register_model(&model_hash, &Bytes::from_slice(&env, b"MobileNetV2-INT8"));
        client.verify_robot_action(
            &proof, &pub_inputs, &robot, &task_id, &model_hash, &95u32, &0u32,
        );
        // Second submission of the same task_id must panic.
        client.verify_robot_action(
            &proof, &pub_inputs, &robot, &task_id, &model_hash, &95u32, &0u32,
        );
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);

        let empty: Vec<BytesN<32>> = vec![&env];
        let (vk, _proof) = valid_instance(&env, &empty);
        client.initialize(&admin, &vk);
        let res = client.try_initialize(&admin, &vk);
        assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
    }

    #[test]
    #[should_panic(expected = "Invalid ZK proof")]
    fn test_wrong_size_proof_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let robot = Address::generate(&env);

        let model_hash = BytesN::from_array(&env, &[1u8; 32]);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let pub_inputs = vec![
            &env,
            model_hash.clone(),
            BytesN::from_array(&env, &[9u8; 32]),
            BytesN::from_array(&env, &[8u8; 32]),
            conf_input(&env, 95),
            task_id.clone(),
        ];
        let (vk, _proof) = valid_instance(&env, &pub_inputs);
        client.initialize(&admin, &vk);
        client.register_model(&model_hash, &Bytes::from_slice(&env, b"MobileNetV2-INT8"));
        // Bindings pass, but the proof is the wrong length -> rejected.
        let short = Bytes::from_slice(&env, &[0u8; 32]);
        client.verify_robot_action(
            &short, &pub_inputs, &robot, &task_id, &model_hash, &95u32, &0u32,
        );
    }

    #[test]
    fn test_get_action_robot_none_for_unknown_task() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseVerifier);
        let client = ZeroSenseVerifierClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let empty: Vec<BytesN<32>> = vec![&env];
        let (vk, _proof) = valid_instance(&env, &empty);
        client.initialize(&admin, &vk);
        let unknown_task = BytesN::from_array(&env, &[99u8; 32]);
        assert_eq!(client.get_action_robot(&unknown_task), None);
    }
}
