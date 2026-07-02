#![cfg(test)]

use super::*;
use soroban_poseidon::poseidon_hash;
use soroban_sdk::crypto::bn254::{Bn254Fr, Bn254G1Affine, Bn254G2Affine};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{vec, Address, Env, U256};

/// Independent Poseidon(a, b), used to reconstruct expected roots without
/// trusting the contract's own folding logic.
fn h2(env: &Env, a: &U256, b: &U256) -> U256 {
    poseidon_hash::<3, Bn254Fr>(env, &vec![env, a.clone(), b.clone()])
}

fn setup() -> (
    Env,
    FleetLearningCoordinatorClient<'static>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    // No real bn254_verifier is deployed in these unit tests — see
    // `finalize_round_traps_without_a_real_verifier_approval` below, which
    // documents that finalize_round is load-bearing against a real verifier.
    // The positive proof-verification path is covered by
    // contracts/bn254_verifier's own Gnark-fixture tests.
    let verifier = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract.address();

    let id = env.register(
        FleetLearningCoordinator,
        (
            admin.clone(),
            verifier.clone(),
            token_address.clone(),
            100i128,
        ),
    );
    let client = FleetLearningCoordinatorClient::new(&env, &id);
    (env, client, admin, token_address)
}

#[test]
fn submit_local_update_records_contribution_and_blocks_double_submit() {
    let (env, client, _admin, _token) = setup();
    let robot = Address::generate(&env);
    let commitment = U256::from_u32(&env, 42);

    let idx = client.submit_local_update(&robot, &1u32, &commitment);
    assert_eq!(idx, 0);
    assert_eq!(client.get_contributor_count(&1u32), 1);
    assert!(client.is_contributor(&1u32, &robot));

    // Same robot, same round -> rejected.
    assert!(client
        .try_submit_local_update(&robot, &1u32, &commitment)
        .is_err());
}

#[test]
fn commitments_root_matches_independent_poseidon_fold() {
    let (env, client, _admin, _token) = setup();
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    let c1 = U256::from_u32(&env, 11);
    let c2 = U256::from_u32(&env, 22);
    let c3 = U256::from_u32(&env, 33);

    client.submit_local_update(&r1, &5u32, &c1);
    client.submit_local_update(&r2, &5u32, &c2);
    client.submit_local_update(&r3, &5u32, &c3);

    let onchain_root = client.compute_round_commitments_root(&5u32);
    // Left-fold: H(H(c1, c2), c3) — must match the contract's own order.
    let expected = h2(&env, &h2(&env, &c1, &c2), &c3);
    assert_eq!(onchain_root, expected);
}

#[test]
fn finalize_round_rejects_when_no_contributors() {
    let (env, client, _admin, _token) = setup();
    let dummy_proof = Proof {
        a: Bn254G1Affine::from_array(&env, &[0u8; 64]),
        b: Bn254G2Affine::from_array(&env, &[0u8; 128]),
        c: Bn254G1Affine::from_array(&env, &[0u8; 64]),
    };
    assert!(client
        .try_finalize_round(&9u32, &U256::from_u32(&env, 1), &dummy_proof)
        .is_err());
}

#[test]
fn finalize_round_traps_without_a_real_verifier_approval() {
    let (env, client, _admin, _token) = setup();
    let robot = Address::generate(&env);
    client.submit_local_update(&robot, &2u32, &U256::from_u32(&env, 7));

    let dummy_proof = Proof {
        a: Bn254G1Affine::from_array(&env, &[0u8; 64]),
        b: Bn254G2Affine::from_array(&env, &[0u8; 128]),
        c: Bn254G1Affine::from_array(&env, &[0u8; 64]),
    };
    // The configured verifier address has no deployed contract in this test,
    // so the cross-contract call traps. This documents that finalize_round
    // is load-bearing: it cannot succeed without a real bn254_verifier
    // instance actually approving the proof (see contracts/bn254_verifier's
    // own Gnark-fixture tests for the positive path, and README.md's
    // integration checklist for wiring a live instance here).
    assert!(client
        .try_finalize_round(&2u32, &U256::from_u32(&env, 1), &dummy_proof)
        .is_err());
}

#[test]
fn claim_reward_rejects_before_finalization_and_for_non_contributors() {
    let (env, client, _admin, token) = setup();
    let robot = Address::generate(&env);
    let outsider = Address::generate(&env);
    client.submit_local_update(&robot, &3u32, &U256::from_u32(&env, 1));

    // Round 3 never finalized.
    assert!(client.try_claim_reward(&robot, &3u32).is_err());

    // Fund the contract's reward pool directly (bypassing fund_rewards'
    // auth flow, which is exercised implicitly via mock_all_auths elsewhere)
    // and simulate a finalized round directly in storage. This isolates the
    // claim-gating logic from the proof-verification path, which is tested
    // separately (see `finalize_round_traps_without_a_real_verifier_approval`
    // and contracts/bn254_verifier's own real-proof tests).
    StellarAssetClient::new(&env, &token).mint(&client.address, &1_000i128);
    env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .set(&DataKey::RoundFinalized(3u32), &true);
    });

    // Non-contributor still cannot claim.
    assert!(client.try_claim_reward(&outsider, &3u32).is_err());

    let paid = client.claim_reward(&robot, &3u32);
    assert_eq!(paid, 100i128);
    assert!(client.has_claimed(&3u32, &robot));

    // Double-claim rejected.
    assert!(client.try_claim_reward(&robot, &3u32).is_err());
}
