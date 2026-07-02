#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env,
};

/// ZRep Token — Robot Reputation Token on Stellar
/// Earned by robots for each verified successful task completion
/// Tradeable on Stellar DEX
/// Mintable ONLY by the ZeroSenseVerifier contract (trustless)
///
/// SECURITY MODEL (patched)
/// - `initialize` is now one-shot: it panics if the contract already has an
///   admin. Previously it had NO such guard, so anyone could call
///   `initialize(attacker, attacker_controlled_verifier)` — since it only
///   checked `admin.require_auth()` against the *new* admin, not the
///   existing one — and then call `mint_reputation` as that attacker-owned
///   "verifier" to mint unlimited ZREP. This is now closed.
/// - `mint_reputation` requires a positive amount and uses `checked_add` for
///   both the balance and total-supply updates so a huge amount can't
///   silently misbehave on overflow.
/// - `slash_reputation` requires a positive amount and remains admin-only.

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotVerifier = 2,
    NotAdmin = 3,
    InvalidAmount = 4,
    Overflow = 5,
}

#[contracttype]
pub enum DataKey {
    Admin,
    VerifierContract,
    Balance(Address),
    TotalSupply,
}

#[contract]
pub struct ZRepToken;

#[contractimpl]
impl ZRepToken {
    /// Initialize ONCE. Panics if already initialized.
    pub fn initialize(env: Env, admin: Address, verifier_contract: Address) {
        admin.require_auth();
        let st = env.storage().instance();
        if st.has(&DataKey::Admin) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        st.set(&DataKey::Admin, &admin);
        st.set(&DataKey::VerifierContract, &verifier_contract);
        st.set(&DataKey::TotalSupply, &0i128);
    }

    /// Mint ZREP tokens to a robot address.
    /// Can ONLY be called by the ZeroSenseVerifier contract configured at
    /// `initialize()` time.
    pub fn mint_reputation(env: Env, caller: Address, robot_id: Address, amount: i128) {
        caller.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, Error::InvalidAmount);
        }
        let verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::VerifierContract)
            .unwrap();
        if caller != verifier {
            panic_with_error!(&env, Error::NotVerifier);
        }

        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(robot_id.clone()))
            .unwrap_or(0);
        let new_balance = match current.checked_add(amount) {
            Some(v) => v,
            None => panic_with_error!(&env, Error::Overflow),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Balance(robot_id.clone()), &new_balance);

        let supply: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        let new_supply = match supply.checked_add(amount) {
            Some(v) => v,
            None => panic_with_error!(&env, Error::Overflow),
        };
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &new_supply);

        env.events().publish(("ZREP", "Minted"), (robot_id, amount));
    }

    /// Slash ZREP tokens (called by AnomalyAgent on behavioral deviation).
    pub fn slash_reputation(env: Env, caller: Address, robot_id: Address, amount: i128) {
        caller.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, Error::InvalidAmount);
        }
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != admin {
            panic_with_error!(&env, Error::NotAdmin);
        }

        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(robot_id.clone()))
            .unwrap_or(0);
        let new_balance = if current > amount { current - amount } else { 0 };
        env.storage()
            .persistent()
            .set(&DataKey::Balance(robot_id.clone()), &new_balance);

        env.events().publish(("ZREP", "Slashed"), (robot_id, amount));
    }

    pub fn balance(env: Env, robot_id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(robot_id))
            .unwrap_or(0)
    }

    pub fn total_supply(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    fn setup(env: &Env) -> (ZRepTokenClient<'static>, Address, Address) {
        let cid = env.register_contract(None, ZRepToken);
        let client = ZRepTokenClient::new(env, &cid);
        let admin = Address::generate(env);
        let verifier = Address::generate(env);
        client.initialize(&admin, &verifier);
        (client, admin, verifier)
    }

    #[test]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin, verifier) = setup(&env);
        assert!(client.try_initialize(&admin, &verifier).is_err());
    }

    #[test]
    fn test_reinitialize_with_attacker_verifier_rejected() {
        // Regression test for the admin-takeover vulnerability: an attacker
        // must NOT be able to re-run initialize() with a verifier address
        // they control, which would otherwise let them mint unlimited ZREP.
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin, _verifier) = setup(&env);
        let attacker = Address::generate(&env);
        let attacker_verifier = Address::generate(&env);
        assert!(client
            .try_initialize(&attacker, &attacker_verifier)
            .is_err());
    }

    #[test]
    fn test_mint_by_verifier_updates_balance_and_supply() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin, verifier) = setup(&env);
        let robot = Address::generate(&env);
        client.mint_reputation(&verifier, &robot, &100);
        assert_eq!(client.balance(&robot), 100);
        assert_eq!(client.total_supply(), 100);
    }

    #[test]
    fn test_mint_by_non_verifier_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin, _verifier) = setup(&env);
        let robot = Address::generate(&env);
        let impostor = Address::generate(&env);
        assert!(client.try_mint_reputation(&impostor, &robot, &100).is_err());
    }

    #[test]
    fn test_mint_non_positive_amount_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin, verifier) = setup(&env);
        let robot = Address::generate(&env);
        assert!(client.try_mint_reputation(&verifier, &robot, &0).is_err());
    }

    #[test]
    fn test_slash_by_admin_reduces_balance_and_floors_at_zero() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin, verifier) = setup(&env);
        let robot = Address::generate(&env);
        client.mint_reputation(&verifier, &robot, &50);
        client.slash_reputation(&admin, &robot, &80);
        assert_eq!(client.balance(&robot), 0);
    }

    #[test]
    fn test_slash_by_non_admin_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _admin, verifier) = setup(&env);
        let robot = Address::generate(&env);
        client.mint_reputation(&verifier, &robot, &50);
        let impostor = Address::generate(&env);
        assert!(client.try_slash_reputation(&impostor, &robot, &10).is_err());
    }
}
