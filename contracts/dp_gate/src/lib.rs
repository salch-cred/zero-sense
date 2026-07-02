#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env,
    IntoVal, Symbol,
};

/// ZeroSense — zk-Verified Differential-Privacy Gate for Fleet Learning
///
/// ## The problem this solves
/// `contracts/fleet_learning` proves a robot submitted a real, committed
/// local-model update for a federated-learning round, but it does not prove
/// that update was actually **differentially private** (gradient-clipped and
/// noised within an epsilon budget) before being aggregated. A robot could
/// submit a raw, unclipped, unnoised gradient and still pass fleet_learning's
/// existing checks, silently leaking training-data information through the
/// aggregated model (a real membership-inference attack surface).
///
/// ## What this contract adds
/// A gate that only marks a robot's round contribution DP-approved once it
/// backs the claim with a REAL, already pairing-verified ZK proof recorded in
/// `contracts/verifier` (reusing the same trust anchor as
/// `contracts/consensus` and `contracts/hardware_attestation`), and the
/// caller's declared noise budget (`epsilon_milli`, epsilon * 1000) is within
/// an admin-set ceiling. `fleet_learning`'s reward claim can then additionally
/// require `is_dp_approved(round, robot) == true` before releasing a reward.
///
/// ## Prior art we found
/// A 2025 Springer paper describes verifiable differential privacy via
/// generic ZK proofs, but not in a blockchain/robot-fleet-learning context.
/// We found no prior art gating a federated-learning reward payout on an
/// on-chain ZK-verified DP-compliance proof for physical robot fleets.
///
/// ## Honesty note
/// The exact noise/clipping bound (`epsilon_milli`) is currently a
/// caller-declared value, not yet bound inside the proof's own public inputs
/// (that requires a dedicated DP-compliance circuit — future work, same
/// category as `circuits/fedavg_aggregation.circom`). Today the guarantee is:
/// "this robot produced a genuine ZK proof for this round, and its declared
/// epsilon is within the admin ceiling" — not yet "the proof itself
/// cryptographically proves the exact noise applied."
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    EpsilonExceeded = 3,
    RoundAlreadyApproved = 4,
    TaskAlreadyUsed = 5,
    TaskNotVerified = 6,
    WitnessMismatch = 7,
    ConfidenceTooLow = 8,
}

#[contracttype]
#[derive(Clone)]
pub struct DpApproval {
    pub robot_id: Address,
    pub round: u64,
    pub epsilon_milli: u32,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct RoundRobotKey {
    pub round: u64,
    pub robot_id: Address,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier,
    MinConfidence,
    MaxEpsilonMilli,
    Approval(RoundRobotKey),
    UsedTask(BytesN<32>),
}

#[contract]
pub struct ZeroSenseDpGate;

#[contractimpl]
impl ZeroSenseDpGate {
    /// Initialize ONCE with admin, the verifier to trust, the maximum allowed
    /// epsilon (scaled by 1000, e.g. epsilon=1.0 -> 1000), and the minimum
    /// proof confidence required.
    pub fn initialize(
        env: Env,
        admin: Address,
        verifier: Address,
        max_epsilon_milli: u32,
        min_confidence: u32,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Verifier, &verifier);
        env.storage()
            .instance()
            .set(&DataKey::MaxEpsilonMilli, &max_epsilon_milli);
        env.storage()
            .instance()
            .set(&DataKey::MinConfidence, &min_confidence);
        Ok(())
    }

    /// Approve a robot's DP-compliant local update for `round`, backed by its
    /// own already-verified ZK proof `task_id`.
    pub fn approve_dp_update(
        env: Env,
        robot_id: Address,
        round: u64,
        epsilon_milli: u32,
        task_id: BytesN<32>,
    ) -> Result<bool, Error> {
        robot_id.require_auth();

        let max_epsilon: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MaxEpsilonMilli)
            .ok_or(Error::NotInitialized)?;
        if epsilon_milli > max_epsilon {
            return Err(Error::EpsilonExceeded);
        }
        let key = RoundRobotKey {
            round,
            robot_id: robot_id.clone(),
        };
        if env.storage().persistent().has(&DataKey::Approval(key.clone())) {
            return Err(Error::RoundAlreadyApproved);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::UsedTask(task_id.clone()))
        {
            return Err(Error::TaskAlreadyUsed);
        }

        let verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::Verifier)
            .ok_or(Error::NotInitialized)?;
        let min_confidence: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MinConfidence)
            .ok_or(Error::NotInitialized)?;
        Self::verify_witness(&env, &verifier, &robot_id, &task_id, min_confidence)?;

        env.storage()
            .persistent()
            .set(&DataKey::UsedTask(task_id.clone()), &true);
        let approval = DpApproval {
            robot_id: robot_id.clone(),
            round,
            epsilon_milli,
            timestamp: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::Approval(key), &approval);

        env.events()
            .publish((symbol_short!("dpapprov"),), (round, robot_id));
        Ok(true)
    }

    pub fn is_dp_approved(env: Env, round: u64, robot_id: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Approval(RoundRobotKey { round, robot_id }))
    }

    pub fn get_approval(env: Env, round: u64, robot_id: Address) -> Option<DpApproval> {
        env.storage()
            .persistent()
            .get(&DataKey::Approval(RoundRobotKey { round, robot_id }))
    }

    // ----------------------------- internal -----------------------------

    fn verify_witness(
        env: &Env,
        verifier: &Address,
        robot_id: &Address,
        task_id: &BytesN<32>,
        min_confidence: u32,
    ) -> Result<u32, Error> {
        let proof_robot: Option<Address> = env.invoke_contract(
            verifier,
            &Symbol::new(env, "get_action_robot"),
            vec![env, task_id.clone().into_val(env)],
        );
        let proof_robot = proof_robot.ok_or(Error::TaskNotVerified)?;
        if &proof_robot != robot_id {
            return Err(Error::WitnessMismatch);
        }
        let confidence: Option<u32> = env.invoke_contract(
            verifier,
            &Symbol::new(env, "get_verified_confidence"),
            vec![env, task_id.clone().into_val(env)],
        );
        let confidence = confidence.ok_or(Error::TaskNotVerified)?;
        if confidence < min_confidence {
            return Err(Error::ConfidenceTooLow);
        }
        Ok(confidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env};

    #[contract]
    pub struct MockVerifier;

    #[contracttype]
    pub enum MockKey {
        Robot(BytesN<32>),
        Confidence(BytesN<32>),
    }

    #[contractimpl]
    impl MockVerifier {
        pub fn set_action(env: Env, task_id: BytesN<32>, robot: Address, confidence: u32) {
            env.storage()
                .persistent()
                .set(&MockKey::Robot(task_id.clone()), &robot);
            env.storage()
                .persistent()
                .set(&MockKey::Confidence(task_id), &confidence);
        }
        pub fn get_action_robot(env: Env, task_id: BytesN<32>) -> Option<Address> {
            env.storage().persistent().get(&MockKey::Robot(task_id))
        }
        pub fn get_verified_confidence(env: Env, task_id: BytesN<32>) -> Option<u32> {
            env.storage()
                .persistent()
                .get(&MockKey::Confidence(task_id))
        }
    }

    fn setup(env: &Env, max_epsilon_milli: u32, min_confidence: u32) -> (ZeroSenseDpGateClient<'static>, Address) {
        let cid = env.register_contract(None, ZeroSenseDpGate);
        let client = ZeroSenseDpGateClient::new(env, &cid);
        let admin = Address::generate(env);
        let vcid = env.register_contract(None, MockVerifier);
        client.initialize(&admin, &vcid, &max_epsilon_milli, &min_confidence);
        (client, vcid)
    }

    #[test]
    fn test_successful_approval() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env, 1000, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[1u8; 32]);
        vclient.set_action(&task_id, &robot, &95u32);

        let ok = client.approve_dp_update(&robot, &1u64, &500u32, &task_id);
        assert!(ok);
        assert!(client.is_dp_approved(&1u64, &robot));
        assert_eq!(client.get_approval(&1u64, &robot).unwrap().epsilon_milli, 500);
    }

    #[test]
    fn test_epsilon_exceeded_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env, 500, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[1u8; 32]);
        vclient.set_action(&task_id, &robot, &95u32);

        let res = client.try_approve_dp_update(&robot, &1u64, &1000u32, &task_id);
        assert_eq!(res, Err(Ok(Error::EpsilonExceeded)));
    }

    #[test]
    fn test_duplicate_round_robot_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env, 1000, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let robot = Address::generate(&env);
        let task_a = BytesN::from_array(&env, &[1u8; 32]);
        vclient.set_action(&task_a, &robot, &95u32);
        client.approve_dp_update(&robot, &1u64, &500u32, &task_a);

        let task_b = BytesN::from_array(&env, &[2u8; 32]);
        vclient.set_action(&task_b, &robot, &95u32);
        let res = client.try_approve_dp_update(&robot, &1u64, &500u32, &task_b);
        assert_eq!(res, Err(Ok(Error::RoundAlreadyApproved)));
    }

    #[test]
    fn test_task_reuse_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env, 1000, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[1u8; 32]);
        vclient.set_action(&task_id, &robot, &95u32);
        client.approve_dp_update(&robot, &1u64, &500u32, &task_id);

        let res = client.try_approve_dp_update(&robot, &2u64, &500u32, &task_id);
        assert_eq!(res, Err(Ok(Error::TaskAlreadyUsed)));
    }

    #[test]
    fn test_confidence_too_low_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env, 1000, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[1u8; 32]);
        vclient.set_action(&task_id, &robot, &40u32);

        let res = client.try_approve_dp_update(&robot, &1u64, &500u32, &task_id);
        assert_eq!(res, Err(Ok(Error::ConfidenceTooLow)));
    }

    #[test]
    fn test_unverified_task_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _vcid) = setup(&env, 1000, 80);

        let robot = Address::generate(&env);
        let unverified_task = BytesN::from_array(&env, &[9u8; 32]);
        let res = client.try_approve_dp_update(&robot, &1u64, &500u32, &unverified_task);
        assert_eq!(res, Err(Ok(Error::TaskNotVerified)));
    }

    #[test]
    fn test_witness_mismatch_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env, 1000, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let real_robot = Address::generate(&env);
        let impostor = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[1u8; 32]);
        vclient.set_action(&task_id, &real_robot, &95u32);

        let res = client.try_approve_dp_update(&impostor, &1u64, &500u32, &task_id);
        assert_eq!(res, Err(Ok(Error::WitnessMismatch)));
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseDpGate);
        let client = ZeroSenseDpGateClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let vcid = env.register_contract(None, MockVerifier);
        client.initialize(&admin, &vcid, &1000u32, &80u32);
        let res = client.try_initialize(&admin, &vcid, &1000u32, &80u32);
        assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
    }
}
