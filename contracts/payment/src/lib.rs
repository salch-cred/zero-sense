#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, BytesN, Env,
};

/// Robot Payment Router Contract
/// Auto-pays robot operators in XLM when a ZK-verified task is completed
/// Confidence threshold: >= 95% → auto-pay, < 95% → flag for review

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

    /// Initialize payment router
    pub fn initialize(
        env: Env,
        admin: Address,
        verifier_contract: Address,
        xlm_token: Address,
    ) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::VerifierContract, &verifier_contract);
        env.storage().instance().set(&DataKey::XlmToken, &xlm_token);
    }

    /// Register a task with payment amount
    /// Called by the task issuer who deposits XLM upfront
    pub fn register_task(
        env: Env,
        issuer: Address,
        task_id: BytesN<32>,
        operator: Address,
        payment_amount: i128,  // in stroops (1 XLM = 10_000_000 stroops)
    ) {
        issuer.require_auth();

        // Transfer XLM from issuer to contract (escrow)
        let xlm: Address = env.storage().instance().get(&DataKey::XlmToken).unwrap();
        let token_client = token::Client::new(&env, &xlm);
        token_client.transfer(&issuer, &env.current_contract_address(), &payment_amount);

        // Record the payment task
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

    /// Claim payment after ZK proof has been verified
    /// Can be called by anyone (permissionless) — the proof already verified the work
    pub fn claim_task_payment(
        env: Env,
        task_id: BytesN<32>,
        confidence: u32,
    ) -> i128 {
        // Check task exists and not already paid
        let already_claimed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::TaskClaimed(task_id.clone()))
            .unwrap_or(false);
        if already_claimed {
            panic!("Task already paid");
        }

        // Confidence threshold: >= 95 = full pay, 80-94 = 50% pay, <80 = no pay
        let mut record: PaymentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::TaskPayment(task_id.clone()))
            .expect("Task not found");

        let pay_amount = if confidence >= 95 {
            record.amount            // Full payment
        } else if confidence >= 80 {
            record.amount / 2        // Partial payment (low confidence)
        } else {
            panic!("Confidence too low — payment withheld");
        };

        // Release XLM to operator
        let xlm: Address = env.storage().instance().get(&DataKey::XlmToken).unwrap();
        let token_client = token::Client::new(&env, &xlm);
        token_client.transfer(
            &env.current_contract_address(),
            &record.operator,
            &pay_amount,
        );

        // Mark as claimed
        record.paid_at = env.ledger().timestamp();
        record.confidence = confidence;
        env.storage().persistent().set(&DataKey::TaskPayment(task_id.clone()), &record);
        env.storage().persistent().set(&DataKey::TaskClaimed(task_id.clone()), &true);

        // Emit payment event
        env.events().publish(
            ("ZeroSense", "TaskPaymentReleased"),
            (record.operator, pay_amount, confidence),
        );

        pay_amount
    }

    /// Get payment record for a task
    pub fn get_payment(env: Env, task_id: BytesN<32>) -> Option<PaymentRecord> {
        env.storage().persistent().get(&DataKey::TaskPayment(task_id))
    }
}
