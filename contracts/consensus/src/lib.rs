#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env,
    IntoVal, Symbol, Vec,
};

/// ZeroSense — zk-Swarm Consensus
///
/// A **Byzantine-fault-tolerant, zero-knowledge-gated physical-world consensus
/// primitive for robot fleets** on Stellar.
///
/// ## The problem this solves
/// Every ZK-robotics verifier answers "did THIS ONE robot really produce this
/// AI decision?" — a single-witness proof. But high-value real-world actions
/// (a large insurance payout, an autonomous right-of-way call, a safety stop
/// that halts a warehouse line) should never rest on ONE robot's camera and
/// ONE robot's proof — a single compromised, miscalibrated, or spoofed robot
/// could otherwise trigger a costly, unilateral on-chain action.
///
/// `consensus` requires **M independent robots, each backed by their own
/// already-verified on-chain ZK proof** (via `contracts/verifier`), to agree
/// on the same physical-world `event_id` before the event is declared
/// "consensus reached" — a fact other contracts (`payment`, `insurance`) can
/// then gate high-value actions on via `is_consensus_reached`. This is
/// classic M-of-N Byzantine fault tolerance, applied for the first time to
/// ZK-verified robot perception rather than off-chain oracle attestations or
/// unproven majority votes.
///
/// ## Why a witness vote can't be faked
/// - Each vote is backed by a REAL, already pairing-verified, replay-proof
///   proof recorded in the verifier contract — `get_action_robot(task_id)`
///   must equal the voting `robot_id`, so a caller cannot borrow another
///   robot's proof to fake being a witness.
/// - `robot_id.require_auth()` — no one can vote on behalf of another robot.
/// - Every `task_id` may back at most one witness vote, ever, across ALL
///   events (global dedup) — a single proof cannot be "split" into multiple
///   fake witnesses.
/// - A robot may vote at most once per event (per-event dedup).
/// - A minimum per-witness confidence is enforced — a robot that is only 40%
///   sure cannot count as a witness for a high-value action.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidThreshold = 3,
    EventAlreadyExists = 4,
    EventNotFound = 5,
    EventFinalized = 6,
    AlreadyWitnessed = 7,
    TaskAlreadyUsed = 8,
    TaskNotVerified = 9,
    WitnessMismatch = 10,
    ConfidenceTooLow = 11,
}

#[contracttype]
#[derive(Clone)]
pub struct ConsensusEvent {
    pub event_id: BytesN<32>,
    pub threshold: u32,
    pub min_confidence: u32,
    pub witnesses: Vec<Address>,
    pub finalized: bool,
    pub finalized_at: u64,
    pub avg_confidence: u32,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier,
    Event(BytesN<32>),
    UsedTask(BytesN<32>),
}

#[contract]
pub struct ZeroSenseConsensus;

#[contractimpl]
impl ZeroSenseConsensus {
    /// Initialize ONCE with the admin + the ZeroSense verifier contract this
    /// consensus round gates witness eligibility against.
    pub fn initialize(env: Env, admin: Address, verifier: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Verifier, &verifier);
        Ok(())
    }

    /// Open a new consensus round for a physical-world event. Anyone may call
    /// this (usually the first witnessing robot or a fleet coordinator) —
    /// opening a round makes no on-chain claims and costs only storage.
    pub fn create_event(
        env: Env,
        caller: Address,
        event_id: BytesN<32>,
        threshold: u32,
        min_confidence: u32,
    ) -> Result<(), Error> {
        caller.require_auth();
        if threshold < 2 {
            return Err(Error::InvalidThreshold);
        }
        if min_confidence > 100 {
            return Err(Error::InvalidThreshold);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Event(event_id.clone()))
        {
            return Err(Error::EventAlreadyExists);
        }
        let ev = ConsensusEvent {
            event_id: event_id.clone(),
            threshold,
            min_confidence,
            witnesses: Vec::new(&env),
            finalized: false,
            finalized_at: 0,
            avg_confidence: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id.clone()), &ev);
        env.events()
            .publish((symbol_short!("evtopen"),), (event_id, threshold));
        Ok(())
    }

    /// Submit this robot's independent witness vote for `event_id`, backed by
    /// its own already-verified ZK proof (`task_id`) recorded in the verifier
    /// contract. Returns `true` and finalizes the event the instant the
    /// threshold is reached; further votes after finalization are rejected so
    /// the witness set stays audit-clean.
    pub fn submit_witness(
        env: Env,
        robot_id: Address,
        event_id: BytesN<32>,
        task_id: BytesN<32>,
    ) -> Result<bool, Error> {
        robot_id.require_auth();

        let mut ev: ConsensusEvent = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id.clone()))
            .ok_or(Error::EventNotFound)?;
        if ev.finalized {
            return Err(Error::EventFinalized);
        }
        if ev.witnesses.contains(&robot_id) {
            return Err(Error::AlreadyWitnessed);
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

        // Cross-contract: confirm THIS robot actually produced the proof
        // behind task_id. A caller cannot fabricate this — the verifier only
        // returns the robot_id that was require_auth'd when the proof was
        // originally, pairing-verified and recorded.
        let proof_robot: Option<Address> = env.invoke_contract(
            &verifier,
            &Symbol::new(&env, "get_action_robot"),
            vec![&env, task_id.clone().into_val(&env)],
        );
        let proof_robot = proof_robot.ok_or(Error::TaskNotVerified)?;
        if proof_robot != robot_id {
            return Err(Error::WitnessMismatch);
        }

        let confidence: Option<u32> = env.invoke_contract(
            &verifier,
            &Symbol::new(&env, "get_verified_confidence"),
            vec![&env, task_id.clone().into_val(&env)],
        );
        let confidence = confidence.ok_or(Error::TaskNotVerified)?;
        if confidence < ev.min_confidence {
            return Err(Error::ConfidenceTooLow);
        }

        env.storage()
            .persistent()
            .set(&DataKey::UsedTask(task_id), &true);
        ev.witnesses.push_back(robot_id.clone());
        let n = ev.witnesses.len();
        ev.avg_confidence = ((ev.avg_confidence * (n - 1)) + confidence) / n;

        let reached = n >= ev.threshold;
        if reached {
            ev.finalized = true;
            ev.finalized_at = env.ledger().timestamp();
        }
        env.storage()
            .persistent()
            .set(&DataKey::Event(event_id.clone()), &ev);

        env.events().publish(
            (symbol_short!("witness"),),
            (event_id.clone(), robot_id, n),
        );
        if reached {
            env.events().publish(
                (symbol_short!("reached"),),
                (event_id, n, ev.avg_confidence),
            );
        }
        Ok(reached)
    }

    /// Read-only gate other contracts (payment, insurance) call before
    /// releasing a high-value action tied to a physical-world event.
    pub fn is_consensus_reached(env: Env, event_id: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .get::<_, ConsensusEvent>(&DataKey::Event(event_id))
            .map(|e| e.finalized)
            .unwrap_or(false)
    }

    pub fn get_event(env: Env, event_id: BytesN<32>) -> Option<ConsensusEvent> {
        env.storage().persistent().get(&DataKey::Event(event_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env};

    // Minimal mock verifier exposing the two read-only functions consensus
    // depends on, so these tests exercise the REAL cross-contract call path
    // (env.invoke_contract) without needing a full BLS12-381 proof pipeline.
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

    fn setup(env: &Env) -> (ZeroSenseConsensusClient<'static>, Address) {
        let cid = env.register_contract(None, ZeroSenseConsensus);
        let client = ZeroSenseConsensusClient::new(env, &cid);
        let admin = Address::generate(env);
        let vcid = env.register_contract(None, MockVerifier);
        client.initialize(&admin, &vcid);
        (client, vcid)
    }

    #[test]
    fn test_three_of_three_reaches_consensus() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let event_id = BytesN::from_array(&env, &[7u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_id, &3u32, &80u32);

        let robots = [
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ];
        let mut reached = false;
        for (i, r) in robots.iter().enumerate() {
            let task_id = BytesN::from_array(&env, &[(i as u8) + 1; 32]);
            vclient.set_action(&task_id, r, &90u32);
            reached = client.submit_witness(r, &event_id, &task_id);
        }
        assert!(reached);
        assert!(client.is_consensus_reached(&event_id));
        let ev = client.get_event(&event_id).unwrap();
        assert_eq!(ev.witnesses.len(), 3);
        assert_eq!(ev.avg_confidence, 90);
    }

    #[test]
    fn test_below_threshold_not_finalized() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let event_id = BytesN::from_array(&env, &[1u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_id, &3u32, &80u32);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[9u8; 32]);
        vclient.set_action(&task_id, &robot, &95u32);
        let reached = client.submit_witness(&robot, &event_id, &task_id);
        assert!(!reached);
        assert!(!client.is_consensus_reached(&event_id));
    }

    #[test]
    fn test_witness_mismatch_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let event_id = BytesN::from_array(&env, &[2u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_id, &2u32, &80u32);

        let real_robot = Address::generate(&env);
        let impostor = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[3u8; 32]);
        // Proof was genuinely produced by real_robot ...
        vclient.set_action(&task_id, &real_robot, &95u32);
        // ... but impostor tries to vote using it.
        let res = client.try_submit_witness(&impostor, &event_id, &task_id);
        assert_eq!(res, Err(Ok(Error::WitnessMismatch)));
    }

    #[test]
    fn test_duplicate_witness_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let event_id = BytesN::from_array(&env, &[4u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_id, &3u32, &80u32);

        let robot = Address::generate(&env);
        let task_id_a = BytesN::from_array(&env, &[5u8; 32]);
        let task_id_b = BytesN::from_array(&env, &[6u8; 32]);
        vclient.set_action(&task_id_a, &robot, &90u32);
        vclient.set_action(&task_id_b, &robot, &90u32);
        client.submit_witness(&robot, &event_id, &task_id_a);
        let res = client.try_submit_witness(&robot, &event_id, &task_id_b);
        assert_eq!(res, Err(Ok(Error::AlreadyWitnessed)));
    }

    #[test]
    fn test_task_reuse_across_events_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let event_a = BytesN::from_array(&env, &[10u8; 32]);
        let event_b = BytesN::from_array(&env, &[11u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_a, &2u32, &80u32);
        client.create_event(&caller, &event_b, &2u32, &80u32);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[12u8; 32]);
        vclient.set_action(&task_id, &robot, &90u32);
        client.submit_witness(&robot, &event_a, &task_id);
        // Same real proof cannot be replayed as a witness for a second event.
        let res = client.try_submit_witness(&robot, &event_b, &task_id);
        assert_eq!(res, Err(Ok(Error::TaskAlreadyUsed)));
    }

    #[test]
    fn test_low_confidence_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid) = setup(&env);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let event_id = BytesN::from_array(&env, &[13u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_id, &2u32, &80u32);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[14u8; 32]);
        vclient.set_action(&task_id, &robot, &40u32);
        let res = client.try_submit_witness(&robot, &event_id, &task_id);
        assert_eq!(res, Err(Ok(Error::ConfidenceTooLow)));
    }

    #[test]
    fn test_unverified_task_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _vcid) = setup(&env);

        let event_id = BytesN::from_array(&env, &[15u8; 32]);
        let caller = Address::generate(&env);
        client.create_event(&caller, &event_id, &2u32, &80u32);

        let robot = Address::generate(&env);
        let unverified_task = BytesN::from_array(&env, &[16u8; 32]);
        let res = client.try_submit_witness(&robot, &event_id, &unverified_task);
        assert_eq!(res, Err(Ok(Error::TaskNotVerified)));
    }

    #[test]
    fn test_invalid_threshold_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _vcid) = setup(&env);
        let caller = Address::generate(&env);
        let event_id = BytesN::from_array(&env, &[17u8; 32]);
        let res = client.try_create_event(&caller, &event_id, &1u32, &80u32);
        assert_eq!(res, Err(Ok(Error::InvalidThreshold)));
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseConsensus);
        let client = ZeroSenseConsensusClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let vcid = env.register_contract(None, MockVerifier);
        client.initialize(&admin, &vcid);
        let res = client.try_initialize(&admin, &vcid);
        assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
    }
}
