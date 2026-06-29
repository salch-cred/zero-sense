#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, vec, Address, BytesN, Env, IntoVal, Symbol,
};

/// Robot Payment Router Contract
/// Auto-pays robot operators in XLM when a ZK-verified task is completed.
///
/// SECURITY MODEL
/// - initialize() is one-shot (auth required).
/// - register_task() escrows the issuer's XLM.
/// - claim_task_payment() reads the confidence from the on-chain VERIFIED
///   RobotAction via a cross-contract call to the verifier. The caller can NOT
///   supply or tamper with the confidence — this prevents payout fraud.
/// - A task can be paid at most once.

#[contracttype]
pub enum DataKey {
    Admin,
    VerifierContract,
    TaskPayment(BytesN<32>),
    TaskClaimed(BytesN<32>),
    XlmToken,
}

#[contracttype]
#[derive(Clone)]
pub struct PaymentRecord {
    pub task_id: BytesN<32>,
    pub operator: Address,
    pub amount: i128,
    pub paid_at: u64,
    pub confidence: u32,
}

#[contract]
pub struct RobotPaymentRouter;

#[contractimpl]
impl RobotPaymentRouter {

    /// Initialize ONCE. Panics if already initialized.
    pub fn initialize(
        env: Env,
        admin: Address,
        verifier_contract: Address,
        xlm_token: Address,
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::VerifierContract, &verifier_contract);
        env.storage().instance().set(&DataKey::XlmToken, &xlm_token);
    }

    /// Register a task with payment amount. The issuer escrows XLM upfront.
    pub fn register_task(
        env: Env,
        issuer: Address,
        task_id: BytesN<32>,
        operator: Address,
        payment_amount: i128,  // stroops (1 XLM = 10_000_000 stroops)
    ) {
        issuer.require_auth();
        if payment_amount <= 0 {
            panic!("Payment amount must be positive");
        }
        if env.storage().persistent().has(&DataKey::TaskPayment(task_id.clone())) {
            panic!("Task already registered");
        }

        let xlm: Address = env.storage().instance().get(&DataKey::XlmToken).unwrap();
        let token_client = token::Client::new(&env, &xlm);
        token_client.transfer(&issuer, &env.current_contract_address(), &payment_amount);

        env.storage().persistent().set(
            &DataKey::TaskPayment(task_id.clone()),
            &PaymentRecord {
                task_id,
                operator,
                amount: payment_amount,
                paid_at: 0,
                confidence: 0,
            },
        );
    }

    /// Claim payment for a task. Confidence is read from the verifier contract
    /// (the on-chain verified RobotAction) — NOT supplied by the caller.
    pub fn claim_task_payment(env: Env, task_id: BytesN<32>) -> i128 {
        let already_claimed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::TaskClaimed(task_id.clone()))
            .unwrap_or(false);
        if already_claimed {
            panic!("Task already paid");
        }

        let mut record: PaymentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::TaskPayment(task_id.clone()))
            .expect("Task not found");

        // Cross-contract: fetch the VERIFIED confidence from the verifier.
        let verifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::VerifierContract)
            .expect("Not initialized");
        let verified: Option<u32> = env.invoke_contract(
            &verifier,
            &Symbol::new(&env, "get_verified_confidence"),
            vec![&env, task_id.clone().into_val(&env)],
        );
        let confidence = verified.expect("Task not verified on-chain");

        let pay_amount = if confidence >= 95 {
            record.amount
        } else if confidence >= 80 {
            record.amount / 2
        } else {
            panic!("Confidence too low — payment withheld");
        };

        let xlm: Address = env.storage().instance().get(&DataKey::XlmToken).unwrap();
        let token_client = token::Client::new(&env, &xlm);
        token_client.transfer(
            &env.current_contract_address(),
            &record.operator,
            &pay_amount,
        );

        record.paid_at = env.ledger().timestamp();
        record.confidence = confidence;
        env.storage().persistent().set(&DataKey::TaskPayment(task_id.clone()), &record);
        env.storage().persistent().set(&DataKey::TaskClaimed(task_id.clone()), &true);

        env.events().publish(
            ("ZeroSense", "TaskPaymentReleased"),
            (record.operator, pay_amount, confidence),
        );

        pay_amount
    }

    pub fn get_payment(env: Env, task_id: BytesN<32>) -> Option<PaymentRecord> {
        env.storage().persistent().get(&DataKey::TaskPayment(task_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, BytesN, Env};

    #[test]
    #[should_panic(expected = "Already initialized")]
    fn test_double_initialize_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, RobotPaymentRouter);
        let client = RobotPaymentRouterClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let token = Address::generate(&env);
        client.initialize(&admin, &verifier, &token);
        client.initialize(&admin, &verifier, &token);
    }

    #[test]
    fn test_get_payment_none_for_unknown_task() {
        let env = Env::default();
        env.mock_all_auths();
        let cid = env.register_contract(None, RobotPaymentRouter);
        let client = RobotPaymentRouterClient::new(&env, &cid);
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let token = Address::generate(&env);
        client.initialize(&admin, &verifier, &token);
        let task_id = BytesN::from_array(&env, &[5u8; 32]);
        assert!(client.get_payment(&task_id).is_none());
    }
}
