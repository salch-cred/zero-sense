#![cfg(test)]

use super::*;
use soroban_poseidon::poseidon_hash;
use soroban_sdk::crypto::bn254::Bn254Fr;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Address, Env, U256};

/// Independent Poseidon(a, b) used by the tests to reconstruct expected roots
/// without trusting the contract's own hashing.
fn h2(env: &Env, a: &U256, b: &U256) -> U256 {
    poseidon_hash::<3, Bn254Fr>(env, &vec![env, a.clone(), b.clone()])
}

/// Register 4 robots into a depth-2 tree (full capacity) and return the
/// environment, client, the four leaves, and the final root.
fn fill_tree() -> (
    Env,
    FleetIdentityContractClient<'static>,
    U256,
    U256,
    U256,
    U256,
    U256,
) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(FleetIdentityContract, (admin.clone(), 2u32));
    let client = FleetIdentityContractClient::new(&env, &id);

    let l0 = U256::from_u32(&env, 111);
    let l1 = U256::from_u32(&env, 222);
    let l2 = U256::from_u32(&env, 333);
    let l3 = U256::from_u32(&env, 444);

    client.register_robot(&l0);
    client.register_robot(&l1);
    client.register_robot(&l2);
    let idx3 = client.register_robot(&l3);
    assert_eq!(idx3, 3);
    assert_eq!(client.get_robot_count(), 4);

    let root = client.get_root();
    (env, client, l0, l1, l2, l3, root)
}

#[test]
fn root_matches_independent_poseidon_tree() {
    let (env, client, l0, l1, l2, l3, root) = fill_tree();
    // Full depth-2 tree: root = H( H(l0,l1), H(l2,l3) ).
    let left = h2(&env, &l0, &l1);
    let right = h2(&env, &l2, &l3);
    let expected = h2(&env, &left, &right);
    assert_eq!(root, expected);
    assert!(client.is_known_root(&root));
}

#[test]
fn anonymous_membership_verifies() {
    let (env, client, l0, _l1, l2, l3, root) = fill_tree();
    // Path for l0: sibling l1 (right) at level 0, sibling H(l2,l3) (right) at level 1.
    let l1 = U256::from_u32(&env, 222);
    let right = h2(&env, &l2, &l3);
    let path = vec![&env, l1.clone(), right.clone()];
    let indices = vec![&env, 0u32, 0u32];
    assert!(client.verify_membership(&l0, &path, &indices, &root));

    // A forged path (wrong sibling) must fail.
    let bad = vec![&env, l2.clone(), right.clone()];
    assert!(!client.verify_membership(&l0, &bad, &indices, &root));
}

#[test]
fn nullifier_blocks_double_spend_but_allows_new_task() {
    let (env, client, l0, _l1, l2, l3, root) = fill_tree();
    let l1 = U256::from_u32(&env, 222);
    let right = h2(&env, &l2, &l3);
    let path = vec![&env, l1.clone(), right.clone()];
    let indices = vec![&env, 0u32, 0u32];

    let robot = Address::generate(&env);
    let task1 = U256::from_u32(&env, 9001);
    let action = U256::from_u32(&env, 7);

    let n1 = client.submit_action(&robot, &l0, &path, &indices, &root, &task1, &action);
    assert!(client.is_nullifier_used(&n1));
    assert_eq!(n1, h2(&env, &l0, &task1));

    // Same (identity, task) again -> rejected.
    assert!(client
        .try_submit_action(&robot, &l0, &path, &indices, &root, &task1, &action)
        .is_err());

    // A different task -> fresh nullifier, allowed.
    let task2 = U256::from_u32(&env, 9002);
    let n2 = client.submit_action(&robot, &l0, &path, &indices, &root, &task2, &action);
    assert_ne!(n1, n2);
    assert!(client.is_nullifier_used(&n2));
}

#[test]
fn unknown_root_is_rejected() {
    let (env, client, l0, _l1, l2, l3, _root) = fill_tree();
    let l1 = U256::from_u32(&env, 222);
    let right = h2(&env, &l2, &l3);
    let path = vec![&env, l1.clone(), right.clone()];
    let indices = vec![&env, 0u32, 0u32];

    let robot = Address::generate(&env);
    let fake_root = U256::from_u32(&env, 1234567);
    let task = U256::from_u32(&env, 1);
    let action = U256::from_u32(&env, 2);
    assert!(client
        .try_submit_action(&robot, &l0, &path, &indices, &fake_root, &task, &action)
        .is_err());
}

#[test]
fn forged_membership_is_rejected_on_action() {
    let (env, client, l0, _l1, l2, l3, root) = fill_tree();
    // Wrong sibling at level 0 -> recomputed root won't match.
    let right = h2(&env, &l2, &l3);
    let bad_path = vec![&env, l2.clone(), right.clone()];
    let indices = vec![&env, 0u32, 0u32];

    let robot = Address::generate(&env);
    let task = U256::from_u32(&env, 1);
    let action = U256::from_u32(&env, 2);
    assert!(client
        .try_submit_action(&robot, &l0, &bad_path, &indices, &root, &task, &action)
        .is_err());
}
