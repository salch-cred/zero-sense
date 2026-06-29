#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, BytesN, Env, String,
};

/// ZRep Token — Robot Reputation Token on Stellar
/// Earned by robots for each verified successful task completion
/// Tradeable on Stellar DEX
/// Mintable ONLY by the ZeroSenseVerifier contract (trustless)

#[contracttype]
pub enum DataKey {
    Admin,
    VerifierContract,
    Balance(Address),
    TotalSupply,
    Authorized(Address),
}

#[contract]
pub struct ZRepToken;

#[contractimpl]
impl ZRepToken {

    pub fn initialize(env: Env, admin: Address, verifier_contract: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::VerifierContract, &verifier_contract);
        env.storage().instance().set(&DataKey::TotalSupply, &0i128);
    }

    /// Mint ZREP tokens to a robot address
    /// Can ONLY be called by the ZeroSenseVerifier contract
    pub fn mint_reputation(
        env: Env,
        caller: Address,
        robot_id: Address,
        amount: i128,
    ) {
        // Only verifier contract can mint
        caller.require_auth();
        let verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::VerifierContract)
            .unwrap();
        if caller != verifier {
            panic!("Only verifier contract can mint ZREP");
        }

        // Update balance
        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(robot_id.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(robot_id.clone()), &(current + amount));

        // Update total supply
        let supply: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &(supply + amount));

        env.events().publish(
            ("ZREP", "Minted"),
            (robot_id, amount),
        );
    }

    /// Slash ZREP tokens (called by AnomalyAgent on behavioral deviation)
    pub fn slash_reputation(
        env: Env,
        caller: Address,
        robot_id: Address,
        amount: i128,
    ) {
        caller.require_auth();
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if caller != admin {
            panic!("Only admin can slash ZREP");
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

        env.events().publish(
            ("ZREP", "Slashed"),
            (robot_id, amount),
        );
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
