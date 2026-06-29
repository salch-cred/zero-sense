#![no_std]

//! # ZeroSense — zk-Fleet Identity
//!
//! A world-first **anonymous-but-accountable identity layer for autonomous robot
//! fleets**, built on Stellar **X-Ray (Protocol 25)** native Poseidon host
//! functions (CAP-75) over the **BN254** scalar field.
//!
//! ## What it does
//!
//! 1. An admin (fleet operator) registers each robot by inserting its **identity
//!    commitment** `C = Poseidon(secret, robot_id)` as a leaf in an on-chain
//!    **Poseidon Merkle tree**. Only the commitment is published — never the
//!    secret.
//! 2. When a robot performs an economic action, it proves it is a member of the
//!    fleet (Merkle inclusion against a known root) and burns a **nullifier**
//!    `N = Poseidon(leaf, task_id)`. The nullifier guarantees **one action per
//!    (identity, task)** — a robot cannot double-claim, and the same physical
//!    robot cannot be cloned to replay an action.
//!
//! ## Two operating modes
//!
//! * **Transparent mode (implemented & tested here):** the caller supplies the
//!   leaf and its Merkle path; the contract recomputes the root with the native
//!   Poseidon host function and burns the derived nullifier. This exercises the
//!   full Poseidon + Merkle + nullifier machinery on-chain and is fully
//!   self-testable, but the path is revealed (not zero-knowledge on its own).
//! * **Private mode (upgrade path):** replace the transparent inclusion check
//!   with a Groth16 circuit (same Poseidon hash, BN254 — circomlib-compatible)
//!   that proves "I know a `secret` whose commitment is in the tree with root R,
//!   and `N = Poseidon(secret, task_id)`" with public inputs `[root, nullifier,
//!   action_hash]`. The on-chain root maintained here is exactly the public
//!   input that circuit checks against, and the proof is verified by the
//!   ZeroSense Groth16 verifier contract. That makes the action fully
//!   zero-knowledge (the robot's identity is never revealed).
//!
//! All field inputs (commitments, task ids) must be valid BN254 field elements
//! (`< r`); Poseidon panics otherwise.

use soroban_poseidon::poseidon_hash;
use soroban_sdk::crypto::bn254::Bn254Fr;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, vec,
    Address, Env, Vec, U256,
};

#[cfg(test)]
mod test;

/// Number of recent Merkle roots accepted by `submit_action`. A rolling window
/// lets actions reference a slightly stale root while new robots are still being
/// registered, without ever accepting an unknown root.
const ROOT_HISTORY_SIZE: u32 = 32;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    TreeFull = 2,
    UnknownRoot = 3,
    InvalidMembership = 4,
    NullifierUsed = 5,
    InvalidDepth = 6,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Depth,
    NextIndex,
    Root,
    Zeros,
    Filled,
    RootHistory,
    Nullifiers,
}

#[contract]
pub struct FleetIdentityContract;

#[contractimpl]
impl FleetIdentityContract {
    /// Initialize the fleet identity tree.
    ///
    /// * `admin` — the fleet operator allowed to register robots.
    /// * `depth` — Merkle tree depth (capacity = 2^depth robots), 1..=32.
    pub fn __constructor(env: Env, admin: Address, depth: u32) {
        if depth == 0 || depth > 32 {
            panic_with_error!(&env, Error::InvalidDepth);
        }
        let st = env.storage().instance();
        if st.has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }

        // Precompute the "zero" subtree hashes for each level using Poseidon.
        // zeros[0] is the empty-leaf value; zeros[i] = Poseidon(zeros[i-1], zeros[i-1]).
        let mut zeros: Vec<U256> = Vec::new(&env);
        let mut cur = U256::from_u32(&env, 0);
        zeros.push_back(cur.clone());
        let mut i = 0u32;
        while i < depth {
            cur = Self::hash2(&env, &cur, &cur);
            zeros.push_back(cur.clone());
            i += 1;
        }

        // filled_subtrees[i] starts as the zero hash for that level.
        let mut filled: Vec<U256> = Vec::new(&env);
        let mut j = 0u32;
        while j < depth {
            filled.push_back(zeros.get(j).unwrap());
            j += 1;
        }

        let root = zeros.get(depth).unwrap();
        let mut history: Vec<U256> = Vec::new(&env);
        history.push_back(root.clone());

        st.set(&DataKey::Admin, &admin);
        st.set(&DataKey::Depth, &depth);
        st.set(&DataKey::NextIndex, &0u32);
        st.set(&DataKey::Zeros, &zeros);
        st.set(&DataKey::Filled, &filled);
        st.set(&DataKey::Root, &root);
        st.set(&DataKey::RootHistory, &history);
        st.set(&DataKey::Nullifiers, &Vec::<U256>::new(&env));
    }

    /// Register a robot by inserting its identity commitment as a Merkle leaf.
    /// Admin-only. Returns the leaf index assigned to the robot.
    ///
    /// Uses the native Poseidon host function for every parent hash along the
    /// insertion path (O(depth) hashes).
    pub fn register_robot(env: Env, commitment: U256) -> u32 {
        let st = env.storage().instance();
        let admin: Address = st.get(&DataKey::Admin).unwrap();
        admin.require_auth();

        let depth: u32 = st.get(&DataKey::Depth).unwrap();
        let mut next: u32 = st.get(&DataKey::NextIndex).unwrap();
        let capacity: u64 = 1u64 << depth;
        if (next as u64) >= capacity {
            panic_with_error!(&env, Error::TreeFull);
        }

        let zeros: Vec<U256> = st.get(&DataKey::Zeros).unwrap();
        let mut filled: Vec<U256> = st.get(&DataKey::Filled).unwrap();

        let leaf_index = next;
        let mut idx = next;
        let mut cur = commitment.clone();
        let mut i = 0u32;
        while i < depth {
            if idx % 2 == 0 {
                // current node is a left child: remember it, pair with the level's zero.
                filled.set(i, cur.clone());
                let right = zeros.get(i).unwrap();
                cur = Self::hash2(&env, &cur, &right);
            } else {
                // current node is a right child: pair with the stored left sibling.
                let left = filled.get(i).unwrap();
                cur = Self::hash2(&env, &left, &cur);
            }
            idx /= 2;
            i += 1;
        }

        next += 1;
        st.set(&DataKey::NextIndex, &next);
        st.set(&DataKey::Filled, &filled);
        st.set(&DataKey::Root, &cur);

        let mut history: Vec<U256> = st.get(&DataKey::RootHistory).unwrap();
        history.push_back(cur.clone());
        while history.len() > ROOT_HISTORY_SIZE {
            history.pop_front();
        }
        st.set(&DataKey::RootHistory, &history);

        env.events()
            .publish((symbol_short!("register"),), (leaf_index, cur));
        leaf_index
    }

    /// Submit an economic action on behalf of a fleet robot.
    ///
    /// Verifies (a) the robot authorized the call, (b) `root` is a known tree
    /// root, (c) `leaf` is included under `root` via the supplied Poseidon
    /// Merkle path, and (d) the derived nullifier `Poseidon(leaf, task_id)` has
    /// not been used. On success it burns the nullifier and returns it.
    ///
    /// `path_indices[i] == 0` means the running node is the left child at level
    /// `i` (sibling on the right); `1` means it is the right child.
    pub fn submit_action(
        env: Env,
        robot: Address,
        leaf: U256,
        path_elements: Vec<U256>,
        path_indices: Vec<u32>,
        root: U256,
        task_id: U256,
        action_hash: U256,
    ) -> U256 {
        robot.require_auth();
        let st = env.storage().instance();

        let history: Vec<U256> = st.get(&DataKey::RootHistory).unwrap();
        if !history.contains(&root) {
            panic_with_error!(&env, Error::UnknownRoot);
        }

        let depth: u32 = st.get(&DataKey::Depth).unwrap();
        if path_elements.len() != depth || path_indices.len() != depth {
            panic_with_error!(&env, Error::InvalidMembership);
        }
        if Self::recompute_root(&env, &leaf, &path_elements, &path_indices) != root {
            panic_with_error!(&env, Error::InvalidMembership);
        }

        // One action per (identity leaf, task). Derived on-chain so it cannot be forged.
        let nullifier = Self::hash2(&env, &leaf, &task_id);
        let mut nullifiers: Vec<U256> = st.get(&DataKey::Nullifiers).unwrap();
        if nullifiers.contains(&nullifier) {
            panic_with_error!(&env, Error::NullifierUsed);
        }
        nullifiers.push_back(nullifier.clone());
        st.set(&DataKey::Nullifiers, &nullifiers);

        env.events().publish(
            (symbol_short!("action"),),
            (robot, nullifier.clone(), action_hash),
        );
        nullifier
    }

    /// Read-only Poseidon Merkle inclusion check against a known root.
    pub fn verify_membership(
        env: Env,
        leaf: U256,
        path_elements: Vec<U256>,
        path_indices: Vec<u32>,
        root: U256,
    ) -> bool {
        let depth: u32 = env.storage().instance().get(&DataKey::Depth).unwrap();
        if path_elements.len() != depth || path_indices.len() != depth {
            return false;
        }
        if Self::recompute_root(&env, &leaf, &path_elements, &path_indices) != root {
            return false;
        }
        Self::is_known_root(env, root)
    }

    pub fn is_known_root(env: Env, root: U256) -> bool {
        let history: Vec<U256> = env
            .storage()
            .instance()
            .get(&DataKey::RootHistory)
            .unwrap_or(Vec::new(&env));
        history.contains(&root)
    }

    pub fn is_nullifier_used(env: Env, nullifier: U256) -> bool {
        let nullifiers: Vec<U256> = env
            .storage()
            .instance()
            .get(&DataKey::Nullifiers)
            .unwrap_or(Vec::new(&env));
        nullifiers.contains(&nullifier)
    }

    pub fn get_root(env: Env) -> U256 {
        env.storage().instance().get(&DataKey::Root).unwrap()
    }

    pub fn get_depth(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Depth).unwrap()
    }

    /// Number of robots registered so far (next free leaf index).
    pub fn get_robot_count(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::NextIndex).unwrap()
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    // ---- internal helpers (not exported) ----

    /// Poseidon hash of two field elements (t=3, BN254) via the native host fn.
    fn hash2(env: &Env, a: &U256, b: &U256) -> U256 {
        poseidon_hash::<3, Bn254Fr>(env, &vec![env, a.clone(), b.clone()])
    }

    /// Recompute a Merkle root from a leaf and its authentication path.
    fn recompute_root(
        env: &Env,
        leaf: &U256,
        path_elements: &Vec<U256>,
        path_indices: &Vec<u32>,
    ) -> U256 {
        let mut cur = leaf.clone();
        let mut i = 0u32;
        let depth = path_elements.len();
        while i < depth {
            let sibling = path_elements.get(i).unwrap();
            let bit = path_indices.get(i).unwrap();
            if bit == 0 {
                cur = Self::hash2(env, &cur, &sibling);
            } else {
                cur = Self::hash2(env, &sibling, &cur);
            }
            i += 1;
        }
        cur
    }
}
