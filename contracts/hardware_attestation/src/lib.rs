#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env,
    IntoVal, Symbol,
};

/// ZeroSense — zk-Robot Hardware Root-of-Trust (Anti-Sybil Attestation)
///
/// ## The problem this solves
/// Every ZK-robotics protocol (including the rest of ZeroSense) proves "an
/// AI model produced this output" but not "this proof came from one genuine,
/// distinct physical robot, not a script replaying the same identity 10,000
/// times." Without hardware-rooted anti-Sybil protection, a single attacker
/// can spin up unlimited software "robots", each with its own Stellar
/// keypair, and drain fleet-learning rewards, insurance payouts, or
/// consensus votes (see `contracts/consensus`) by simply out-voting honest
/// robots.
///
/// ## What this contract adds
/// A one-time, non-transferable binding between a secure-hardware commitment
/// (e.g. a TPM/secure-enclave-derived device public key commitment) and a
/// Stellar robot identity, gated by a REAL, already pairing-verified ZK proof
/// recorded in `contracts/verifier` — reusing the exact same trust anchor
/// `contracts/consensus` uses (`get_action_robot` / `get_verified_confidence`),
/// rather than trusting a caller-supplied claim.
///
/// ## Prior art we found (see project README for full citations)
/// World ID, Cloudflare's cross-vendor ZK hardware attestation, and
/// Microsoft Vega all do ZK hardware/personhood attestation for *humans*.
/// We found no prior art applying this to *robot fleets* bound to an
/// on-chain ZK-verified action, which is what makes this a genuine
/// hackathon-native extension rather than a copy of existing work.
///
/// ## Honesty note (matches the project's existing disclosure style)
/// The `manufacturer_root` argument is admin-whitelisted but is **not yet**
/// bound inside the proof's own public inputs — that requires a dedicated
/// hardware-attestation circuit (same category of future work as
/// `circuits/fleet_membership.circom`). Today the security guarantee is:
/// "this robot really produced a genuine, pairing-verified ZK proof, and
/// its declared manufacturer root is on an admin-controlled allowlist" — not
/// yet "the proof itself cryptographically proves which manufacturer root
/// produced it."
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    ManufacturerNotTrusted = 3,
    AlreadyAttested = 4,
    TaskAlreadyUsed = 5,
    TaskNotVerified = 6,
    WitnessMismatch = 7,
    ConfidenceTooLow = 8,
}

#[contracttype]
#[derive(Clone)]
pub struct AttestationRecord {
    pub robot_id: Address,
    pub manufacturer_root: BytesN<32>,
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier,
    MinConfidence,
    TrustedManufacturer(BytesN<32>),
    Attestation(BytesN<32>),
    UsedTask(BytesN<32>),
}

#[contract]
pub struct ZeroSenseHardwareAttestation;

#[contractimpl]
impl ZeroSenseHardwareAttestation {
    /// Initialize ONCE with the admin, the ZeroSense verifier contract to
    /// trust for witness proofs, and the minimum confidence a liveness /
    /// attestation proof must carry.
    pub fn initialize(
        env: Env,
        admin: Address,
        verifier: Address,
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
            .set(&DataKey::MinConfidence, &min_confidence);
        Ok(())
    }

    /// Admin-only: whitelist a trusted hardware manufacturer/root-of-trust
    /// commitment (e.g. a manufacturer's device-attestation CA root hash).
    pub fn register_manufacturer(env: Env, manufacturer_root: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::TrustedManufacturer(manufacturer_root), &true);
        Ok(())
    }

    /// Bind a hardware commitment to a robot identity. `task_id` must
    /// reference an already-verified, pairing-checked ZK proof recorded by
    /// `robot_id` in the verifier contract (the liveness/attestation proof).
    pub fn attest_hardware(
        env: Env,
        robot_id: Address,
        commitment: BytesN<32>,
        manufacturer_root: BytesN<32>,
        task_id: BytesN<32>,
    ) -> Result<bool, Error> {
        robot_id.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&DataKey::TrustedManufacturer(manufacturer_root.clone()))
        {
            return Err(Error::ManufacturerNotTrusted);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Attestation(commitment.clone()))
        {
            return Err(Error::AlreadyAttested);
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
        let record = AttestationRecord {
            robot_id: robot_id.clone(),
            manufacturer_root,
            timestamp: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Attestation(commitment.clone()), &record);

        env.events()
            .publish((symbol_short!("attested"),), (commitment, robot_id));
        Ok(true)
    }

    pub fn is_attested(env: Env, commitment: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Attestation(commitment))
    }

    pub fn get_attestation(env: Env, commitment: BytesN<32>) -> Option<AttestationRecord> {
        env.storage().persistent().get(&DataKey::Attestation(commitment))
    }

    // ----------------------------- internal -----------------------------

    /// Shared witness-verification helper: confirms `task_id` is a genuine,
    /// already pairing-verified proof produced by `robot_id` (via the
    /// verifier's `get_action_robot`), and that its verified confidence
    /// meets `min_confidence` (via `get_verified_confidence`). This is the
    /// exact same cross-contract trust pattern `contracts/consensus` uses.
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

    fn setup(env: &Env, min_confidence: u32) -> (ZeroSenseHardwareAttestationClient<'static>, Address, Address) {
        let cid = env.register_contract(None, ZeroSenseHardwareAttestation);
        let client = ZeroSenseHardwareAttestationClient::new(env, &cid);
        let admin = Address::generate(env);
        let vcid = env.register_contract(None, MockVerifier);
        client.initialize(&admin, &vcid, &min_confidence);
        (client, vcid, admin)
    }

    #[test]
    fn test_successful_attestation() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid, _admin) = setup(&env, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let root = BytesN::from_array(&env, &[1u8; 32]);
        client.register_manufacturer(&root);

        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let commitment = BytesN::from_array(&env, &[3u8; 32]);
        vclient.set_action(&task_id, &robot, &90u32);

        let ok = client.attest_hardware(&robot, &commitment, &root, &task_id);
        assert!(ok);
        assert!(client.is_attested(&commitment));
        assert_eq!(client.get_attestation(&commitment).unwrap().robot_id, robot);
    }

    #[test]
    fn test_untrusted_manufacturer_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid, _admin) = setup(&env, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let root = BytesN::from_array(&env, &[9u8; 32]);
        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[2u8; 32]);
        let commitment = BytesN::from_array(&env, &[3u8; 32]);
        vclient.set_action(&task_id, &robot, &90u32);

        let res = client.try_attest_hardware(&robot, &commitment, &root, &task_id);
        assert_eq!(res, Err(Ok(Error::ManufacturerNotTrusted)));
    }

    #[test]
    fn test_duplicate_commitment_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid, _admin) = setup(&env, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let root = BytesN::from_array(&env, &[1u8; 32]);
        client.register_manufacturer(&root);
        let robot = Address::generate(&env);
        let commitment = BytesN::from_array(&env, &[3u8; 32]);

        let task_a = BytesN::from_array(&env, &[4u8; 32]);
        vclient.set_action(&task_a, &robot, &90u32);
        client.attest_hardware(&robot, &commitment, &root, &task_a);

        let task_b = BytesN::from_array(&env, &[5u8; 32]);
        vclient.set_action(&task_b, &robot, &90u32);
        let res = client.try_attest_hardware(&robot, &commitment, &root, &task_b);
        assert_eq!(res, Err(Ok(Error::AlreadyAttested)));
    }

    #[test]
    fn test_task_reuse_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid, _admin) = setup(&env, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let root = BytesN::from_array(&env, &[1u8; 32]);
        client.register_manufacturer(&root);
        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[4u8; 32]);
        vclient.set_action(&task_id, &robot, &90u32);

        client.attest_hardware(&robot, &BytesN::from_array(&env, &[6u8; 32]), &root, &task_id);
        let res = client.try_attest_hardware(
            &robot,
            &BytesN::from_array(&env, &[7u8; 32]),
            &root,
            &task_id,
        );
        assert_eq!(res, Err(Ok(Error::TaskAlreadyUsed)));
    }

    #[test]
    fn test_confidence_too_low_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid, _admin) = setup(&env, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let root = BytesN::from_array(&env, &[1u8; 32]);
        client.register_manufacturer(&root);
        let robot = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[4u8; 32]);
        vclient.set_action(&task_id, &robot, &40u32);

        let res = client.try_attest_hardware(
            &robot,
            &BytesN::from_array(&env, &[6u8; 32]),
            &root,
            &task_id,
        );
        assert_eq!(res, Err(Ok(Error::ConfidenceTooLow)));
    }

    #[test]
    fn test_unverified_task_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _vcid, _admin) = setup(&env, 80);

        let root = BytesN::from_array(&env, &[1u8; 32]);
        client.register_manufacturer(&root);
        let robot = Address::generate(&env);
        let unverified_task = BytesN::from_array(&env, &[8u8; 32]);

        let res = client.try_attest_hardware(
            &robot,
            &BytesN::from_array(&env, &[6u8; 32]),
            &root,
            &unverified_task,
        );
        assert_eq!(res, Err(Ok(Error::TaskNotVerified)));
    }

    #[test]
    fn test_witness_mismatch_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, vcid, _admin) = setup(&env, 80);
        let vclient = MockVerifierClient::new(&env, &vcid);

        let root = BytesN::from_array(&env, &[1u8; 32]);
        client.register_manufacturer(&root);
        let real_robot = Address::generate(&env);
        let impostor = Address::generate(&env);
        let task_id = BytesN::from_array(&env, &[4u8; 32]);
        vclient.set_action(&task_id, &real_robot, &90u32);

        let res = client.try_attest_hardware(
            &impostor,
            &BytesN::from_array(&env, &[6u8; 32]),
            &root,
            &task_id,
        );
        assert_eq!(res, Err(Ok(Error::WitnessMismatch)));
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, ZeroSenseHardwareAttestation);
        let client = ZeroSenseHardwareAttestationClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let vcid = env.register_contract(None, MockVerifier);
        client.initialize(&admin, &vcid, &80u32);
        let res = client.try_initialize(&admin, &vcid, &80u32);
        assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
    }
}
