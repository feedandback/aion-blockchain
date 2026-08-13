use std::path::PathBuf;

use kybernetes::bootstrap::canonical_bootstrap;
use kybernetes::economy::Economy;
use kybernetes::node::Node;
use kybernetes::state::State;
use kybernetes::transaction_submission::prepare_signed_transaction;
use kybernetes::wallet::Wallet;

fn signer() -> Wallet {
    Wallet::new()
}

fn recipient() -> Wallet {
    Wallet::new()
}

fn funded_state(
    signer: &Wallet,
    balance: u64,
    nonce: u64,
) -> State {
    let mut state = State::new();
    state.create_account(
        signer.address().to_string(),
        balance,
    );
    state
        .accounts
        .get_mut(signer.address())
        .expect("test signer account must exist")
        .nonce = nonce;
    state
}

#[test]
fn valid_signer_balance_and_nonce_create_a_signed_transaction() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let state = funded_state(&signer, 50_000, 3);

    let transaction = prepare_signed_transaction(
        &signer,
        recipient.address(),
        10_000,
        &state,
        &economy,
    )
    .expect("funded signer transaction must be prepared");

    assert_eq!(transaction.from, signer.address());
    assert_eq!(transaction.to, recipient.address());
    assert_eq!(transaction.amount, 10_000);
    assert_eq!(transaction.nonce, 3);
    assert!(transaction.is_signed());
    assert_eq!(transaction.id, transaction.calculate_id());

    let bootstrap = canonical_bootstrap()
        .expect("canonical test bootstrap must be valid");
    let mut node = Node::new_with_data_directory(
        bootstrap.blockchain,
        state,
        bootstrap.consensus,
        PathBuf::new(),
    );
    assert!(node.add_transaction(transaction));
    assert_eq!(node.mempool.transactions.len(), 1);
}

#[test]
fn transaction_nonce_is_read_automatically_from_confirmed_state() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let state = funded_state(&signer, 50_000, 7);

    let transaction = prepare_signed_transaction(
        &signer,
        recipient.address(),
        1_000,
        &state,
        &economy,
    )
    .expect("transaction must use confirmed state nonce");

    assert_eq!(transaction.nonce, 7);
    assert_eq!(state.nonce_of(signer.address()), 7);
}

#[test]
fn invalid_recipient_address_is_rejected() {
    let signer = signer();
    let economy = Economy::new();
    let state = funded_state(&signer, 50_000, 0);

    let error = prepare_signed_transaction(
        &signer,
        "not-a-kybernetes-address",
        1_000,
        &state,
        &economy,
    )
    .expect_err("invalid recipient must be rejected");

    assert!(error.contains("Recipient"));
    assert_eq!(state.balance_of(signer.address()), 50_000);
    assert_eq!(state.nonce_of(signer.address()), 0);
}

#[test]
fn zero_amount_is_rejected() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let state = funded_state(&signer, 50_000, 0);

    let error = prepare_signed_transaction(
        &signer,
        recipient.address(),
        0,
        &state,
        &economy,
    )
    .expect_err("zero amount must be rejected");

    assert!(error.contains("sıfır"));
    assert_eq!(state.balance_of(signer.address()), 50_000);
}

#[test]
fn insufficient_balance_is_rejected() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let amount = 1_000;
    let fee = economy.calculate_fee(amount);
    let state = funded_state(&signer, amount + fee - 1, 0);

    let error = prepare_signed_transaction(
        &signer,
        recipient.address(),
        amount,
        &state,
        &economy,
    )
    .expect_err("insufficient balance must be rejected");

    assert!(error.contains("bakiye yetersiz"));
    assert_eq!(state.balance_of(signer.address()), amount + fee - 1);
}

#[test]
fn amount_and_fee_overflow_is_rejected() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let state = funded_state(&signer, u64::MAX, 0);

    let error = prepare_signed_transaction(
        &signer,
        recipient.address(),
        u64::MAX,
        &state,
        &economy,
    )
    .expect_err("amount plus fee overflow must be rejected");

    assert!(error.contains("overflow"));
    assert_eq!(state.balance_of(signer.address()), u64::MAX);
}

#[test]
fn generated_transaction_signature_verifies() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let state = funded_state(&signer, 50_000, 0);
    let transaction = prepare_signed_transaction(
        &signer,
        recipient.address(),
        1_000,
        &state,
        &economy,
    )
    .expect("transaction must be signed");
    let signature = transaction
        .signature
        .as_deref()
        .expect("transaction signature must exist");

    assert!(Wallet::verify(
        &transaction.public_key,
        &transaction.message(),
        signature,
    ));
    assert_eq!(
        Wallet::address_from_public_key(&transaction.public_key).as_deref(),
        Some(transaction.from.as_str())
    );
}

#[test]
fn generated_fee_matches_the_production_economy_rule() {
    let signer = signer();
    let recipient = recipient();
    let economy = Economy::new();
    let amount = 2_500_000;
    let state = funded_state(&signer, amount + 25, 0);
    let transaction = prepare_signed_transaction(
        &signer,
        recipient.address(),
        amount,
        &state,
        &economy,
    )
    .expect("transaction with exact fee balance must be prepared");

    assert_eq!(economy.calculate_fee(amount), 25);
    assert_eq!(transaction.fee, economy.calculate_fee(amount));
}
