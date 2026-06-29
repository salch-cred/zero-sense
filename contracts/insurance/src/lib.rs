#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, BytesN, Env,
};

/// Insurance Claim Contract
/// Stores ZK proof references as insurance evidence
/// Raw sensor data is NEVER revealed — only the ZK proof hash

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
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ClaimCount, &0u64);
    }

    /// File an insurance claim with ZK proof as evidence
    /// incident_proof_hash: The hash of the RISC Zero proof that captured the incident
    /// This proves what happened WITHOUT revealing sensitive robot data
    pub fn file_claim(
        env: Env,
        robot_id: Address,
        incident_proof_hash: BytesN<32>,
        claim_amount: i128,
    ) -> u64 {
        robot_id.require_auth();

        let claim_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ClaimCount)
            .unwrap_or(0) + 1;

        let claim = InsuranceClaim {
            claim_id,
            robot_id: robot_id.clone(),
            incident_proof_hash,
            claim_amount,
            status: 0,  // pending
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

    /// Resolve a claim (admin/insurer)
    pub fn resolve_claim(
        env: Env,
        admin: Address,
        claim_id: u64,
        approved: bool,
    ) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("Only admin can resolve claims");
        }

        let mut claim: InsuranceClaim = env
            .storage()
            .persistent()
            .get(&DataKey::Claim(claim_id))
            .expect("Claim not found");

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
