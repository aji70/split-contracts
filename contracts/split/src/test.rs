#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Bytes, Env, Vec,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SplitContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let stellar_asset = StellarAssetClient::new(&env, &token_id);
    stellar_asset.mint(&token_admin, &1_000_000_000);

    (env, contract_id, token_id)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> SplitContractClient<'a> {
    SplitContractClient::new(env, contract_id)
}

fn token_client<'a>(env: &'a Env, token_id: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token_id)
}

/// Helper: create a basic invoice with no co-creators and no early withdrawal.
fn make_invoice(
    env: &Env,
    c: &SplitContractClient,
    creator: &Address,
    recipient: &Address,
    amount: i128,
    token_id: &Address,
    deadline: u64,
) -> u64 {
    let mut recipients = Vec::new(env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(env);
    amounts.push_back(amount);
    c.create_invoice(
        creator,
        &recipients,
        &amounts,
        token_id,
        &deadline,
        &Vec::new(env),
        &false,
    )
}

// ---------------------------------------------------------------------------
// Existing tests (updated for new create_invoice signature)
// ---------------------------------------------------------------------------

#[test]
fn test_create_invoice() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    assert_eq!(id, 1);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.funded, 0);
}

#[test]
fn test_pay_and_auto_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}

#[test]
fn test_partial_pay_then_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &150);
    sa.mint(&payer2, &150);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 300, &token_id, 9_999);

    c.pay(&payer1, &id, &150_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Pending);

    c.pay(&payer2, &id, &150_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_refund_after_deadline() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 2_000);
    c.pay(&payer, &id, &100_i128);

    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(tk.balance(&payer), 100);
}

#[test]
#[should_panic(expected = "invoice deadline has passed")]
fn test_pay_after_deadline_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    env.ledger().set_timestamp(3_000);
    c.pay(&payer, &id, &100_i128);
}

#[test]
#[should_panic(expected = "payment exceeds remaining balance")]
fn test_overpayment_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &1_000);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128);
}

#[test]
fn test_multi_recipient_release() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &600);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(r1.clone());
    recipients.push_back(r2.clone());
    recipients.push_back(r3.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    amounts.push_back(200_i128);
    amounts.push_back(300_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &Vec::new(&env),
        &false,
    );
    c.pay(&payer, &id, &600_i128);

    assert_eq!(tk.balance(&r1), 100);
    assert_eq!(tk.balance(&r2), 200);
    assert_eq!(tk.balance(&r3), 300);
}

#[test]
fn test_audit_log() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&c, &env, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 2);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("pay"));
    assert_eq!(log.get_unchecked(1).action, symbol_short!("release"));
}

#[test]
fn test_audit_log_with_cancel() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&c, &env, &creator, &recipient, 100, &token_id, 9_999);
    c.cancel_invoice(&creator, &id);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("cancel"));
    assert_eq!(log.get_unchecked(0).actor, creator);
}

#[test]
fn test_audit_log_with_extend() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&c, &env, &creator, &recipient, 100, &token_id, 2_000);
    c.extend_deadline(&creator, &id, &9_999_u64);

    let log = c.get_audit_log(&id);
    assert_eq!(log.len(), 1);
    assert_eq!(log.get_unchecked(0).action, symbol_short!("extend"));
    assert_eq!(log.get_unchecked(0).actor, creator);
}

#[test]
fn test_create_subscription() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(200_i128);

    let id = c.create_subscription(&creator, &recipients, &amounts, &token_id, &3_u32);
    assert_eq!(id, 1);

    c.pay(&payer, &id, &200_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);

    let second_invoice = c.get_invoice(&2);
    assert_eq!(second_invoice.status, InvoiceStatus::Pending);
    assert_eq!(tk.balance(&recipient), 200);
}

// ---------------------------------------------------------------------------
// New tests: pause / unpause
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "contract is paused")]
fn test_pause_blocks_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin);
    let id = make_invoice(&c, &env, &creator, &recipient, 200, &token_id, 9_999);
    c.pause(&admin);

    // Must panic with "contract is paused".
    c.pay(&payer, &id, &100_i128);
}

#[test]
fn test_unpause_restores_pay() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &500);
    env.ledger().set_timestamp(1_000);

    c.initialize(&admin);
    let id = make_invoice(&c, &env, &creator, &recipient, 200, &token_id, 9_999);

    c.pause(&admin);
    c.unpause(&admin);

    // After unpause, pay() succeeds.
    c.pay(&payer, &id, &200_i128);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
fn test_get_invoice_works_while_paused() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    c.initialize(&admin);
    let id = make_invoice(&c, &env, &creator, &recipient, 200, &token_id, 9_999);
    c.pause(&admin);

    // Read-only function still works.
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

// ---------------------------------------------------------------------------
// New tests: transfer_invoice
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_invoice_new_creator_can_cancel() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let new_creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&c, &env, &creator, &recipient, 100, &token_id, 9_999);

    c.transfer_invoice(&id, &new_creator);

    // New creator can cancel.
    c.cancel_invoice(&new_creator, &id);
    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Cancelled);
}

#[test]
#[should_panic(expected = "only creator can cancel")]
fn test_transfer_invoice_old_creator_cannot_cancel() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let new_creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&c, &env, &creator, &recipient, 100, &token_id, 9_999);
    c.transfer_invoice(&id, &new_creator);

    // Old creator must fail.
    c.cancel_invoice(&creator, &id);
}

// ---------------------------------------------------------------------------
// New tests: bonus pool
// ---------------------------------------------------------------------------

#[test]
fn test_bonus_pool_distributed_to_first_payer() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let early_payer = Address::generate(&env);
    let late_payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&creator, &50);   // bonus pool
    sa.mint(&early_payer, &150);
    sa.mint(&late_payer, &150);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(300_i128);

    // bonus_pool=50, bonus_max_payers=1 → only first unique payer gets 50.
    let id = c.create_invoice(
        &creator, &recipients, &amounts, &token_id, &9_999_u64,
        &50_i128, &1_u32, &None,
    );

    c.pay(&early_payer, &id, &150_i128);
    c.pay(&late_payer, &id, &150_i128);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    // early_payer gets bonus 50; late_payer does not.
    assert_eq!(tk.balance(&early_payer), 50);
    assert_eq!(tk.balance(&late_payer), 0);
    assert_eq!(tk.balance(&recipient), 300);
}

#[test]
fn test_bonus_pool_zero_behaves_identically() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&c, &env, &creator, &recipient, 200, &token_id, 9_999);
    c.pay(&payer, &id, &200_i128);

    assert_eq!(c.get_invoice(&id).status, InvoiceStatus::Released);
    assert_eq!(tk.balance(&recipient), 200);
}
