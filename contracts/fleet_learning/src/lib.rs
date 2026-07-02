#![no_std]

//! # ZeroSense — zk-FedAvg Fleet Learning
//!
//! A world-first (as far as our research could determine) **ZK-verified
//! federated-learning aggregation coordinator for autonomous robot fleets**,
//! settled on Stellar.
//!
//! ## The statement this contract makes load-bearing
//!
//! Most zero-knowledge federated learning (ZK-FL) work proves that a
//! *client's local training* was executed correctly (see the survey "SoK:
//! Verifiable Federated Learning", Bruschi et al. 2025, IACR ePrint 2025/2296).
//! The harder, less-explored half is proving the **aggregation** step itself
//! is correct — that the published global model really is the claimed
//! weighted average of the local updates, not something the aggregator
//! swapped in. This contract makes exactly that the on-chain-checked fact,
//! following the ZK-proved-FedAvg approach in Xu et al. 2023
//! ("An efficient and privacy-preserving decentralized federated learning
//! algorithm", arXiv:2312.04579) and Jin et al. 2025 ("Zero-Knowledge
//! Federated Learning", arXiv:2503.15550).
//!
//! ## How it's bound to real on-chain state (not a disconnected proof check)
//!
//! 1. Each robot calls [`FleetLearningCoordinator::submit_local_update`] with
//!    a Poseidon commitment to its local model delta for a training round.
//!    Commitments accumulate on-chain, in submission order.
//! 2. To finalize a round, the aggregator supplies a Groth16/BN254 proof plus
//!    the claimed `aggregated_hash`. [`FleetLearningCoordinator::finalize_round`]
//!    **recomputes `commitments_root` itself** (a Poseidon left-fold over the
//!    on-chain-recorded commitments — see `circuits/fedavg_aggregation.circom`
//!    in the repo root for the exact statement) and feeds `[round,
//!    commitments_root, aggregated_hash]` to the verifier as public signals.
//!    A prover cannot submit a valid-looking proof about a different set of
//!    contributions, because the contract — not the caller — derives the
//!    root the proof is checked against.
//! 3. Verification is delegated to the same generic, real-proof-tested
//!    `contracts/bn254_verifier` that backs zk-Fleet Identity — one audited
//!    verifier, two circuits.
//! 4. Once finalized, every robot that is a recorded contributor for that
//!    round (and only those robots, and only once) can
//!    [`FleetLearningCoordinator::claim_reward`] in XLM (or any Stellar asset
//!    configured as the reward token) — fully autonomous settlement, no
//!    manual reconciliation.
//!
//! ## Why this is a different trust model than prior blockchain+FL+robotics work
//!
//! Pacheco et al. (DARS 2024, "Securing Federated Learning in Robot Swarms
//! using Blockchain") secure swarm FL aggregation with a **reputation-token**
//! economy — good robots earn tokens, bad ones get slashed, but there is no
//! cryptographic proof that the aggregation arithmetic itself was correct.
//! This contract instead makes a **ZK proof of aggregation correctness**
//! the gate for finalizing a round at all. Existing ZK-FL *code* we found
//! (`Veriblock-FL`, `fl-chain-data-sharing`) targets Ethereum and generic
//! clients, not a physical robot fleet with on-chain identity and autonomous
//! per-round Stellar payout.
//!
//! ## Status
//!
//! This contract and its on-chain verification path are real and tested.
//! `circuits/fedavg_aggregation.circom` is a fully-specified, cited design;
//! compiling it and running a trusted setup (so `finalize_round` can be
//! called with a genuine proof) is the remaining integration step — see
//! README.md's "What's Real vs. What's Mocked" table for the honest status.

use soroban_poseidon::poseidon_hash;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    vec,
    crypto::bn254::{Bn254Fr, Bn254G1Affine, Bn254G2Affine},
    Address, Env, IntoVal, Symbol, Vec, U256,
};

#[cfg(test)]
mod test;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    RoundFinalized = 2,
    RoundNotFinalized = 3,
    AlreadyClaimed = 4,
    NotContributor = 5,
    ProofRejected = 6,
    NoContributors = 7,
    AlreadySubmitted = 8,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Verifier,
    RewardToken,
    RewardPerContributor,
    RoundContributors(u32),
    RoundCommitments(u32),
    RoundFinalized(u32),
    RoundAggregatedHash(u32),
    HasSubmitted(u32, Address),
    Claimed(u32, Address),
}

/// Mirrors `bn254_verifier::Proof` field-for-field so it encodes identically
/// across the cross-contract call — Soroban contract structs are transmitted
/// by field name/order, not by Rust type identity, so this local
/// re-declaration is the standard way to share a type shape across contracts.
#[contracttype]
#[derive(Clone)]
pub struct Proof {
    pub a: Bn254G1Affine,
    pub b: Bn254G2Affine,
    pub c: Bn254G1Affine,
}

#[contract]
pub struct FleetLearningCoordinator;

#[contractimpl]
impl FleetLearningCoordinator {
    /// One-shot init.
    ///
    /// * `verifier` — the deployed `bn254_verifier` (Groth16Verifier) instance
    ///   configured with `circuits/fedavg_aggregation.circom`'s verification key.
    /// * `reward_token` — a Stellar asset contract (e.g. the native XLM SAC).
    /// * `reward_per_contributor` — flat payout per verified contributor per round.
    pub fn __constructor(
        env: Env,
        admin: Address,
        verifier: Address,
        reward_token: Address,
        reward_per_contributor: i128,
    ) {
        admin.require_auth();
        let st = env.storage().instance();
        if st.has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        st.set(&DataKey::Admin, &admin);
        st.set(&DataKey::Verifier, &verifier);
        st.set(&DataKey::RewardToken, &reward_token);
        st.set(&DataKey::RewardPerContributor, &reward_per_contributor);
    }

    /// A robot submits a Poseidon commitment to its local model update for
    /// `round` (e.g. `commitment = Poseidon(delta_digest, sample_count, salt)`).
    /// One submission per (round, robot). Returns the contribution's index.
    pub fn submit_local_update(env: Env, robot: Address, round: u32, commitment: U256) -> u32 {
        robot.require_auth();
        let st = env.storage().instance();

        if st.get(&DataKey::RoundFinalized(round)).unwrap_or(false) {
            panic_with_error!(&env, Error::RoundFinalized);
        }
        let submitted_key = DataKey::HasSubmitted(round, robot.clone());
        if st.get(&submitted_key).unwrap_or(false) {
            panic_with_error!(&env, Error::AlreadySubmitted);
        }

        let mut contributors: Vec<Address> = st
            .get(&DataKey::RoundContributors(round))
            .unwrap_or(Vec::new(&env));
        let mut commitments: Vec<U256> = st
            .get(&DataKey::RoundCommitments(round))
            .unwrap_or(Vec::new(&env));

        contributors.push_back(robot.clone());
        commitments.push_back(commitment.clone());
        let index = contributors.len() - 1;

        st.set(&DataKey::RoundContributors(round), &contributors);
        st.set(&DataKey::RoundCommitments(round), &commitments);
        st.set(&submitted_key, &true);

        env.events()
            .publish((symbol_short!("submit"), round), (robot, commitment));
        index
    }

    /// Finalize a training round.
    ///
    /// Recomputes `commitments_root` from the on-chain-recorded commitments,
    /// verifies the Groth16/BN254 proof against public signals `[round,
    /// commitments_root, aggregated_hash]` via the configured verifier, and —
    /// only if the verifier approves — marks the round finalized so
    /// contributors can claim rewards. Admin-gated (the admin is the fleet's
    /// aggregation operator submitting the proof); the proof itself is what
    /// makes the aggregation trustworthy, not the admin's authority.
    pub fn finalize_round(env: Env, round: u32, aggregated_hash: U256, proof: Proof) -> bool {
        let st = env.storage().instance();
        let admin: Address = st.get(&DataKey::Admin).unwrap();
        admin.require_auth();

        if st.get(&DataKey::RoundFinalized(round)).unwrap_or(false) {
            panic_with_error!(&env, Error::RoundFinalized);
        }

        let commitments: Vec<U256> = st
            .get(&DataKey::RoundCommitments(round))
            .unwrap_or(Vec::new(&env));
        if commitments.len() == 0 {
            panic_with_error!(&env, Error::NoContributors);
        }

        let commitments_root = Self::compute_commitments_root(&env, &commitments);

        let round_fr = Bn254Fr::from_u256(U256::from_u32(&env, round));
        let root_fr = Bn254Fr::from_u256(commitments_root);
        let hash_fr = Bn254Fr::from_u256(aggregated_hash.clone());
        let pub_signals: Vec<Bn254Fr> = vec![&env, round_fr, root_fr, hash_fr];

        // Cross-contract call into the generic BN254 Groth16 verifier. If the
        // configured address has no deployed verifier (or the verifier's own
        // checks fail, e.g. VerificationKeyNotSet), this call traps and so
        // does finalize_round — by design, a round cannot be finalized
        // without a real, approving proof.
        let verifier: Address = st.get(&DataKey::Verifier).unwrap();
        let verified: bool = env.invoke_contract(
            &verifier,
            &Symbol::new(&env, "verify_proof"),
            vec![&env, proof.into_val(&env), pub_signals.into_val(&env)],
        );
        if !verified {
            panic_with_error!(&env, Error::ProofRejected);
        }

        st.set(&DataKey::RoundFinalized(round), &true);
        st.set(&DataKey::RoundAggregatedHash(round), &aggregated_hash);

        env.events().publish(
            (symbol_short!("finalize"), round),
            (aggregated_hash, commitments.len()),
        );
        true
    }

    /// A contributor of a finalized round claims its flat per-round reward.
    /// One claim per (round, robot); only recorded contributors can claim.
    pub fn claim_reward(env: Env, robot: Address, round: u32) -> i128 {
        robot.require_auth();
        let st = env.storage().instance();

        if !st.get(&DataKey::RoundFinalized(round)).unwrap_or(false) {
            panic_with_error!(&env, Error::RoundNotFinalized);
        }
        let claimed_key = DataKey::Claimed(round, robot.clone());
        if st.get(&claimed_key).unwrap_or(false) {
            panic_with_error!(&env, Error::AlreadyClaimed);
        }
        let contributors: Vec<Address> = st
            .get(&DataKey::RoundContributors(round))
            .unwrap_or(Vec::new(&env));
        if !contributors.contains(&robot) {
            panic_with_error!(&env, Error::NotContributor);
        }

        let amount: i128 = st.get(&DataKey::RewardPerContributor).unwrap();
        let reward_token: Address = st.get(&DataKey::RewardToken).unwrap();
        let token_client = token::Client::new(&env, &reward_token);
        token_client.transfer(&env.current_contract_address(), &robot, &amount);

        st.set(&claimed_key, &true);
        env.events()
            .publish((symbol_short!("claim"), round), (robot, amount));
        amount
    }

    /// Top up the contract's reward pool (e.g. the fleet operator, per round).
    pub fn fund_rewards(env: Env, funder: Address, amount: i128) {
        funder.require_auth();
        let reward_token: Address = env.storage().instance().get(&DataKey::RewardToken).unwrap();
        let token_client = token::Client::new(&env, &reward_token);
        token_client.transfer(&funder, &env.current_contract_address(), &amount);
    }

    /// Admin-gated adjustment of the flat per-contributor reward.
    pub fn set_reward_per_contributor(env: Env, amount: i128) {
        let st = env.storage().instance();
        let admin: Address = st.get(&DataKey::Admin).unwrap();
        admin.require_auth();
        st.set(&DataKey::RewardPerContributor, &amount);
    }

    /// Read-only: recompute the Poseidon commitments-root for a round exactly
    /// as `finalize_round` does, so an off-chain prover can confirm its
    /// circuit input matches on-chain state before generating a proof.
    pub fn compute_round_commitments_root(env: Env, round: u32) -> U256 {
        let commitments: Vec<U256> = env
            .storage()
            .instance()
            .get(&DataKey::RoundCommitments(round))
            .unwrap_or(Vec::new(&env));
        Self::compute_commitments_root(&env, &commitments)
    }

    pub fn is_round_finalized(env: Env, round: u32) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::RoundFinalized(round))
            .unwrap_or(false)
    }

    pub fn get_aggregated_hash(env: Env, round: u32) -> Option<U256> {
        env.storage()
            .instance()
            .get(&DataKey::RoundAggregatedHash(round))
    }

    pub fn get_contributor_count(env: Env, round: u32) -> u32 {
        let contributors: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::RoundContributors(round))
            .unwrap_or(Vec::new(&env));
        contributors.len()
    }

    pub fn is_contributor(env: Env, round: u32, robot: Address) -> bool {
        let contributors: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::RoundContributors(round))
            .unwrap_or(Vec::new(&env));
        contributors.contains(&robot)
    }

    pub fn has_claimed(env: Env, round: u32, robot: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Claimed(round, robot))
            .unwrap_or(false)
    }

    pub fn get_reward_per_contributor(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::RewardPerContributor)
            .unwrap()
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    // ---- internal helpers (not exported) ----

    /// Poseidon left-fold over the round's commitments, in submission order.
    /// Must match `circuits/fedavg_aggregation.circom`'s root computation
    /// exactly, since this is what the ZK proof's public signal is checked
    /// against.
    fn compute_commitments_root(env: &Env, commitments: &Vec<U256>) -> U256 {
        if commitments.len() == 0 {
            return U256::from_u32(env, 0);
        }
        let mut acc = commitments.get(0).unwrap();
        let mut i = 1u32;
        while i < commitments.len() {
            let c = commitments.get(i).unwrap();
            acc = poseidon_hash::<3, Bn254Fr>(env, &vec![env, acc.clone(), c.clone()]);
            i += 1;
        }
        acc
    }
}
