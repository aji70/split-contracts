//! StellarSplit — on-chain invoice & payment splitting contract.
//!
//! Allows a creator to define an invoice with multiple recipients and amounts.
//! Payers contribute funds; once fully funded the contract auto-routes USDC to
//! each recipient. If the deadline passes unfunded, payers are refunded.

#![no_std]

mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};
use types::{Invoice, InvoiceStatus, Payment};

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn counter_key() -> Symbol {
    symbol_short!("counter")
}

fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}

fn ext_vote_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ext_vote"), id)
}

fn load_invoice(env: &Env, id: u64) -> Invoice {
    env.storage()
        .persistent()
        .get(&invoice_key(id))
        .expect("invoice not found")
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    env.storage().persistent().set(&invoice_key(id), invoice);
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitContract;

#[contractimpl]
impl SplitContract {
    /// Create a new invoice.
    ///
    /// # Arguments
    /// * `creator`              – address that owns the invoice (must authorise)
    /// * `recipients`           – ordered list of recipient addresses
    /// * `amounts`              – amount owed to each recipient (parallel to `recipients`)
    /// * `token`                – USDC token contract address
    /// * `deadline`             – Unix timestamp; after this refunds become available
    /// * `co_creators`          – optional additional addresses with creator permissions
    /// * `allow_early_withdrawal` – whether payers may withdraw before deadline
    ///
    /// # Returns
    /// The new invoice ID (monotonically increasing u64).
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        co_creators: Vec<Address>,
        allow_early_withdrawal: bool,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(
            deadline > env.ledger().timestamp(),
            "deadline must be in the future"
        );

        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);

        let total: i128 = amounts.iter().sum();

        let invoice = Invoice {
            creator: creator.clone(),
            co_creators,
            recipients: recipients.clone(),
            amounts,
            token,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
            allow_early_withdrawal,
        };

        save_invoice(&env, id, &invoice);
        events::invoice_created(&env, id, &creator, total);

        id
    }

    /// Pay toward an invoice.
    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(amount > 0, "payment amount must be positive");

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        invoice.payments.push_back(Payment {
            payer: payer.clone(),
            amount,
        });
        invoice.funded += amount;

        events::payment_received(&env, invoice_id, &payer, amount);

        if invoice.funded >= total {
            Self::_release(&env, invoice_id, &mut invoice);
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    /// Release funds to all recipients once the invoice is fully funded.
    pub fn release(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        Self::_release(&env, invoice_id, &mut invoice);
    }

    /// Refund all payers if the deadline has passed and the invoice is not fully funded.
    pub fn refund(env: Env, invoice_id: u64) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() > invoice.deadline,
            "deadline has not passed"
        );

        let token_client = token::Client::new(&env, &invoice.token);

        for payment in invoice.payments.iter() {
            token_client.transfer(
                &env.current_contract_address(),
                &payment.payer,
                &payment.amount,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        save_invoice(&env, invoice_id, &invoice);
        events::invoice_refunded(&env, invoice_id);
    }

    /// Retrieve an invoice by ID.
    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    // -----------------------------------------------------------------------
    // #36 — Third-party invoice verification
    // -----------------------------------------------------------------------

    /// Returns true if the invoice exists and its status matches `expected_status`.
    /// Returns false for non-existent invoices or status mismatch. No auth required.
    pub fn verify_invoice(env: Env, invoice_id: u64, expected_status: InvoiceStatus) -> bool {
        match env
            .storage()
            .persistent()
            .get::<(Symbol, u64), Invoice>(&invoice_key(invoice_id))
        {
            Some(invoice) => invoice.status == expected_status,
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // #37 — Early withdrawal
    // -----------------------------------------------------------------------

    /// Allows a payer to reclaim their contribution before the deadline,
    /// only when `allow_early_withdrawal` is enabled on the invoice.
    pub fn withdraw(env: Env, invoice_id: u64, payer: Address) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.allow_early_withdrawal, "early withdrawal not allowed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        // Sum all payments from this payer.
        let mut total_paid: i128 = 0;
        for payment in invoice.payments.iter() {
            if payment.payer == payer {
                total_paid += payment.amount;
            }
        }
        assert!(total_paid > 0, "no contributions to withdraw");

        // Remove payer's entries and rebuild payments vec.
        let mut new_payments: Vec<Payment> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if payment.payer != payer {
                new_payments.push_back(payment);
            }
        }
        invoice.payments = new_payments;
        invoice.funded -= total_paid;

        let token_client = token::Client::new(&env, &invoice.token);
        token_client.transfer(&env.current_contract_address(), &payer, &total_paid);

        save_invoice(&env, invoice_id, &invoice);
    }

    // -----------------------------------------------------------------------
    // #39 — Deadline extension by payer vote
    // -----------------------------------------------------------------------

    /// Vote to extend the invoice deadline by 7 days.
    /// Once a strict majority (> 50%) of unique payers have voted, the deadline
    /// is extended and votes are cleared.
    pub fn vote_extend_deadline(env: Env, invoice_id: u64, voter: Address) {
        voter.require_auth();

        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        // Verify voter has paid.
        let has_paid = invoice.payments.iter().any(|p| p.payer == voter);
        assert!(has_paid, "only payers can vote");

        // Count unique payers.
        let mut unique_payers: Vec<Address> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if !unique_payers.contains(&payment.payer) {
                unique_payers.push_back(payment.payer);
            }
        }

        // Load or init votes.
        let vote_key = ext_vote_key(invoice_id);
        let mut votes: Vec<Address> = env
            .storage()
            .persistent()
            .get(&vote_key)
            .unwrap_or_else(|| Vec::new(&env));

        // Ignore duplicate votes.
        if votes.contains(&voter) {
            return;
        }
        votes.push_back(voter);

        let unique_payer_count = unique_payers.len();
        if votes.len() > unique_payer_count / 2 {
            // Majority reached — extend deadline by 7 days and clear votes.
            let mut invoice = load_invoice(&env, invoice_id);
            invoice.deadline += 7 * 24 * 60 * 60;
            save_invoice(&env, invoice_id, &invoice);
            env.storage().persistent().remove(&vote_key);
        } else {
            env.storage().persistent().set(&vote_key, &votes);
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice) {
        let token_client = token::Client::new(env, &invoice.token);

        for (recipient, amount) in invoice.recipients.iter().zip(invoice.amounts.iter()) {
            token_client.transfer(&env.current_contract_address(), &recipient, &amount);
        }

        invoice.status = InvoiceStatus::Released;
        save_invoice(env, invoice_id, invoice);
        events::invoice_released(env, invoice_id, &invoice.recipients);
    }

    // -----------------------------------------------------------------------
    // #38 — Co-creator auth helper (used by creator-gated functions)
    // -----------------------------------------------------------------------

    /// Returns true if `caller` is the invoice creator or a listed co-creator.
    fn is_authorized_creator(invoice: &Invoice, caller: &Address) -> bool {
        if &invoice.creator == caller {
            return true;
        }
        invoice.co_creators.contains(caller)
    }
}
