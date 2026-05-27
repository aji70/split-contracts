#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
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

// ---------------------------------------------------------------------------
// #36 — verify_invoice tests
// ---------------------------------------------------------------------------

#[test]
fn test_verify_invoice_pending() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);

    assert!(c.verify_invoice(&id, &InvoiceStatus::Pending));
    assert!(!c.verify_invoice(&id, &InvoiceStatus::Released));
    assert!(!c.verify_invoice(&id, &InvoiceStatus::Refunded));
}

#[test]
fn test_verify_invoice_released() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128);

    assert!(c.verify_invoice(&id, &InvoiceStatus::Released));
    assert!(!c.verify_invoice(&id, &InvoiceStatus::Pending));
}

#[test]
fn test_verify_invoice_refunded() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &50);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 2_000);
    c.pay(&payer, &id, &50_i128);
    env.ledger().set_timestamp(3_000);
    c.refund(&id);

    assert!(c.verify_invoice(&id, &InvoiceStatus::Refunded));
    assert!(!c.verify_invoice(&id, &InvoiceStatus::Pending));
}

#[test]
fn test_verify_invoice_nonexistent_returns_false() {
    let (env, contract_id, _token_id) = setup();
    let c = client(&env, &contract_id);

    assert!(!c.verify_invoice(&999_u64, &InvoiceStatus::Pending));
}

// ---------------------------------------------------------------------------
// #37 — Early withdrawal tests
// ---------------------------------------------------------------------------

#[test]
fn test_early_withdrawal_success() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);
    let tk = token_client(&env, &token_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &200);
    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(500_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &Vec::new(&env),
        &true, // allow_early_withdrawal
    );

    c.pay(&payer, &id, &200_i128);
    assert_eq!(c.get_invoice(&id).funded, 200);
    assert_eq!(tk.balance(&payer), 0);

    c.withdraw(&id, &payer);

    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.funded, 0);
    assert_eq!(tk.balance(&payer), 200);
}

#[test]
#[should_panic(expected = "early withdrawal not allowed")]
fn test_early_withdrawal_not_allowed_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 500, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128);
    c.withdraw(&id, &payer);
}

// ---------------------------------------------------------------------------
// #38 — Co-creator tests
// ---------------------------------------------------------------------------

#[test]
fn test_co_creator_can_call_creator_gated_action() {
    // We verify co-creator auth by checking is_authorized_creator logic indirectly:
    // create an invoice with a co_creator, then confirm the co_creator field is stored.
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let co_creator = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(100_i128);
    let mut co_creators = Vec::new(&env);
    co_creators.push_back(co_creator.clone());

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &co_creators,
        &false,
    );

    let invoice = c.get_invoice(&id);
    assert!(invoice.co_creators.contains(&co_creator));
    assert_eq!(invoice.co_creators.len(), 1);
}

#[test]
fn test_empty_co_creators_behaves_normally() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 100, &token_id, 9_999);
    let invoice = c.get_invoice(&id);
    assert_eq!(invoice.co_creators.len(), 0);
}

// ---------------------------------------------------------------------------
// #39 — Vote to extend deadline tests
// ---------------------------------------------------------------------------

#[test]
fn test_vote_extend_deadline_majority_triggers_extension() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let payer3 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &100);
    sa.mint(&payer2, &100);
    sa.mint(&payer3, &100);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(400_i128); // needs 400 total, each pays 100 (partial)

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &Vec::new(&env),
        &false,
    );

    c.pay(&payer1, &id, &100_i128);
    c.pay(&payer2, &id, &100_i128);
    c.pay(&payer3, &id, &100_i128);

    let original_deadline = c.get_invoice(&id).deadline;

    // 1st vote — no majority yet (1 of 3)
    c.vote_extend_deadline(&id, &payer1);
    assert_eq!(c.get_invoice(&id).deadline, original_deadline);

    // 2nd vote — majority reached (2 of 3 > 50%)
    c.vote_extend_deadline(&id, &payer2);
    let new_deadline = c.get_invoice(&id).deadline;
    assert_eq!(new_deadline, original_deadline + 7 * 24 * 60 * 60);
}

#[test]
fn test_vote_extend_deadline_duplicate_vote_ignored() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);
    let payer3 = Address::generate(&env);
    let recipient = Address::generate(&env);

    let sa = StellarAssetClient::new(&env, &token_id);
    sa.mint(&payer1, &100);
    sa.mint(&payer2, &100);
    sa.mint(&payer3, &100);

    env.ledger().set_timestamp(1_000);

    let mut recipients = Vec::new(&env);
    recipients.push_back(recipient.clone());
    let mut amounts = Vec::new(&env);
    amounts.push_back(400_i128);

    let id = c.create_invoice(
        &creator,
        &recipients,
        &amounts,
        &token_id,
        &9_999_u64,
        &Vec::new(&env),
        &false,
    );

    c.pay(&payer1, &id, &100_i128);
    c.pay(&payer2, &id, &100_i128);
    c.pay(&payer3, &id, &100_i128);

    let original_deadline = c.get_invoice(&id).deadline;

    // payer1 votes twice — second should be ignored
    c.vote_extend_deadline(&id, &payer1);
    c.vote_extend_deadline(&id, &payer1);

    // Still no majority (only 1 unique vote)
    assert_eq!(c.get_invoice(&id).deadline, original_deadline);
}

#[test]
#[should_panic(expected = "only payers can vote")]
fn test_vote_extend_deadline_non_payer_panics() {
    let (env, contract_id, token_id) = setup();
    let c = client(&env, &contract_id);

    let creator = Address::generate(&env);
    let payer = Address::generate(&env);
    let non_payer = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&payer, &100);
    env.ledger().set_timestamp(1_000);

    let id = make_invoice(&env, &c, &creator, &recipient, 400, &token_id, 9_999);
    c.pay(&payer, &id, &100_i128);
    c.vote_extend_deadline(&id, &non_payer);
}
