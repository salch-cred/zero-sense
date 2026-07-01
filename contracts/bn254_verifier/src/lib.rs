//! Generic Groth16 zero-knowledge proof verifier over the native BN254 curve
//! (Stellar Protocol 25 / CAP-0074 host functions: `g1_add`, `g1_mul`,
//! `pairing_check`).
//!
//! This contract is curve/circuit-generic: deploy one instance per circuit,
//! supplying that circuit's verification key at construction time. It backs
//! two ZeroSense features:
//!   1. The zk-Fleet Identity Poseidon-Merkle membership circuit
//!      (`circuits/fleet_membership.circom`), consumed by
//!      `fleet_identity::submit_action_zk`.
//!   2. Native BN254 verification of RISC Zero receipts re-wrapped as
//!      Groth16/BN254 proofs (the STARK-to-SNARK wrapping RISC Zero itself
//!      uses for cheap on-chain verification).
//!
//! Verification equation (standard Groth16):
//!   e(A, B) == e(alpha, beta) * e(vk_x, gamma) * e(C, delta)
//! where `vk_x = ic[0] + sum(public_input[i] * ic[i+1])`.
//!
//! Rearranged into a single multi-pairing check (as required by the host's
//! `pairing_check`, which checks that the product of all pairings is 1):
//!   pairing_check([-A, alpha, vk_x, C], [B, beta, gamma, delta]) == true
//!
//! This contract, its verification equation, and its interface intentionally
//! mirror the official Stellar reference implementation in
//! `stellar/soroban-examples` (`groth16_verifier/contracts/bn254_verifier`,
//! PR #396) so it can be validated against that prior art rather than
//! hand-derived from scratch.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    crypto::bn254::{Bn254Fr, Bn254G1Affine, Bn254G2Affine},
    Address, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Groth16Error {
    /// Number of public signals provided doesn't match the verification key's IC vector.
    MalformedVerifyingKey = 0,
    /// No verification key has been set yet.
    VerificationKeyNotSet = 1,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    VerificationKey,
}

#[derive(Clone)]
#[contracttype]
pub struct VerificationKey {
    pub alpha: Bn254G1Affine,
    pub beta: Bn254G2Affine,
    pub gamma: Bn254G2Affine,
    pub delta: Bn254G2Affine,
    pub ic: Vec<Bn254G1Affine>,
}

#[derive(Clone)]
#[contracttype]
pub struct Proof {
    pub a: Bn254G1Affine,
    pub b: Bn254G2Affine,
    pub c: Bn254G1Affine,
}

#[contract]
pub struct Groth16Verifier;

#[contractimpl]
impl Groth16Verifier {
    /// One-shot init: stores the admin and this deployment's verification key.
    pub fn __constructor(env: Env, admin: Address, verification_key: VerificationKey) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::VerificationKey, &verification_key);
    }

    /// Admin-gated rotation of the verification key (e.g. after a new trusted
    /// setup / circuit upgrade).
    pub fn set_verification_key(env: Env, verification_key: VerificationKey) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::VerificationKey, &verification_key);
    }

    /// Verifies a Groth16 proof against the stored verification key and the
    /// given public signals, using the native BN254 host functions.
    pub fn verify_proof(
        env: Env,
        proof: Proof,
        pub_signals: Vec<Bn254Fr>,
    ) -> Result<bool, Groth16Error> {
        let vk: VerificationKey = env
            .storage()
            .instance()
            .get(&DataKey::VerificationKey)
            .ok_or(Groth16Error::VerificationKeyNotSet)?;

        if pub_signals.len() + 1 != vk.ic.len() {
            return Err(Groth16Error::MalformedVerifyingKey);
        }

        let bn = env.crypto().bn254();
        let mut vk_x = vk.ic.get(0).unwrap();
        for (signal, point) in pub_signals.iter().zip(vk.ic.iter().skip(1)) {
            let term = bn.g1_mul(&point, &signal);
            vk_x = bn.g1_add(&vk_x, &term);
        }

        let neg_a = -proof.a;
        let lhs = soroban_sdk::vec![&env, neg_a, vk.alpha, vk_x, proof.c];
        let rhs = soroban_sdk::vec![&env, proof.b, vk.beta, vk.gamma, vk.delta];

        Ok(bn.pairing_check(lhs, rhs))
    }
}
