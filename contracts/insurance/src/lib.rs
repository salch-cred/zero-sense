#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, BytesN, Env,
};

/// Insurance Claim Contract
/// Stores ZK proof references as insurance evidence
/// Raw sensor data is NEVER revealed — only the ZK proof hash
///
/// SECURITY MODEL (patched)
/// - `initialize` is now one-shot: it panics if the contract already has an
///   admin. Previously it had NO such guard, so anyone could call
///   `initialize(attacker)` at any time — since it only checked
///   `admin.require_auth()` against the *new* admin, not the existing one —
///   and then call `resolve_claim` as that attacker to approve/reject any
///   claim. This is now closed.
/// - `file_claim` requires a positive `claim_amount`.

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    InvalidAmount = 3,
    ClaimNotFound = 4,
}

#[contracttype]
#[derive(Clone)]
pub struct InsuranceClaim {
    pub claim_id: u64,
    pub robot_id: Address,
    pub incident_proof_hash: BytesN<32>,  // Hash of ZK proof — never raw data
    pub claim_amount: i128,
    pub status: u32,  // 0=pending, 1=approved, 2=rejected
    pub filed_at: u64,
    pub resolved_at: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    ClaimCount,
    Claim(u64),
    RobotClaims(Address),
}

#[contract]
pub struct InsuranceClaimContract;

#[contractimpl]
impl InsuranceClaimContract {
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        let st = env.storage().instance();
        if st.has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        st.set(&DataKey::Admin, &admin);
        st.set(&DataKey::ClaimCount, &0u64);
    }

    /// File an insurance claim with ZK proof as evidence.
    /// `incident_proof_hash`: The hash of the RISC Zero proof that captured
    /// the incident. This proves what happened WITHOUT revealing sensitive
    /// robot data.
    pub fn file_claim(
        env: Env,
        robot_id: Address,
        incident_proof_hash: BytesN<32>,
        claim_amount: i128,
    ) -> u64 {
        robot_id.require_auth();
        if claim_amount <= 0 {
            panic_with_error!(&env, Error::InvalidAmount);
        }

        let claim_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ClaimCount)
            .unwrap_or(0)
            + 1;

        let claim = InsuranceClaim {
            claim_id,
            robot_id: robot_id.clone(),
            incident_proof_hash,
            claim_amount,
            status: 0, // pending
            filed_at: env.ledger().timestamp(),
            resolved_at: 0,
        };

        env.storage().instance().set(&DataKey::ClaimCount, &claim_id);
        env.storage().persistent().set(&DataKey::Claim(claim_id), &claim);

        env.events().publish(
            ("ZeroSense", "InsuranceClaimFiled"),
            (robot_id, claim_id, claim_amount),
        );

        claim_id
    }

    /// Resolve a claim (admin/insurer).
    pub fn resolve_claim(env: Env, admin: Address, claim_id: u64, approved: bool) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic_with_error!(&env, Error::NotAdmin);
        }

        let mut claim: InsuranceClaim = match env.storage().persistent().get(&DataKey::Claim(claim_id)) {
            Some(c) => c,
            None => panic_with_error!(&env, Error::ClaimNotFound),
        };

        claim.status = if approved { 1 } else { 2 };
        claim.resolved_at = env.ledger().timestamp();
        env.storage().persistent().set(&DataKey::Claim(claim_id), &claim);

        env.events().publish(
            ("ZeroSense", "InsuranceClaimResolved"),
            (claim_id, approved, claim.claim_amount),
        );
    }

    pub fn get_claim(env: Env, claim_id: u64) -> Option<InsuranceClaim> {
        env.storage().persistent().get(&DataKey::Claim(claim_id))
    }

    pub fn claim_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::ClaimCount).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{BytesN, Env};

    fn setup(env: &Env) -> (InsuranceClaimContractClient<'static>, Address) {
        let cid = env.register_contract(None, InsuranceClaimContract);
        let client = InsuranceClaimContractClient::new(env, &cid);
        let admin = Address::generate(env);
        client.initialize(&admin);
        (client, admin)
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        assert!(client.try_initialize(&admin).is_err());
    }

    #[test]
    fn test_reinitialize_with_attacker_admin_rejected() {
        // Regression test for the admin-takeover vulnerability: an attacker
        // must NOT be able to re-run initialize() to become admin and then
        // approve/reject claims at will.
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin) = setup(&env);
        let attacker = Address::generate(&env);
        assert!(client.try_initialize(&attacker).is_err());
    }

    #[test]
    fn test_file_and_resolve_claim_happy_path() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        let robot = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[7u8; 32]);
        let claim_id = client.file_claim(&robot, &hash, &500);
        assert_eq!(claim_id, 1);

        client.resolve_claim(&admin, &claim_id, &true);
        let claim = client.get_claim(&claim_id).unwrap();
        assert_eq!(claim.status, 1);
        assert_eq!(client.claim_count(), 1);
    }

    #[test]
    fn test_resolve_by_non_admin_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin) = setup(&env);
        let robot = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[1u8; 32]);
        let claim_id = client.file_claim(&robot, &hash, &100);

        let impostor = Address::generate(&env);
        assert!(client
            .try_resolve_claim(&impostor, &claim_id, &true)
            .is_err());
    }

    #[test]
    fn test_file_claim_non_positive_amount_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin) = setup(&env);
        let robot = Address::generate(&env);
        let hash = BytesN::from_array(&env, &[2u8; 32]);
        assert!(client.try_file_claim(&robot, &hash, &0).is_err());
    }

    #[test]
    fn test_resolve_unknown_claim_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = setup(&env);
        assert!(client.try_resolve_claim(&admin, &999u64, &true).is_err());
    }
}
