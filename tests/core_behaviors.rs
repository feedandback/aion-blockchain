use kybernetes::chain::{Blockchain, Mempool};
use kybernetes::consensus::Consensus;
use kybernetes::core::{Block, Transaction};
use kybernetes::economy::Economy;
use kybernetes::node::Node;
use kybernetes::protocol::{GENESIS_PREVIOUS_HASH, GENESIS_TIMESTAMP, GENESIS_VALIDATOR};
use kybernetes::state::State;
use kybernetes::wallet::Wallet;

const PRIVATE_KEY_ONE: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const PRIVATE_KEY_TWO: &str = "0202020202020202020202020202020202020202020202020202020202020202";
const PRIVATE_KEY_THREE: &str = "0303030303030303030303030303030303030303030303030303030303030303";
const PUBLIC_KEY_ONE: &str = "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
const ADDRESS_ONE: &str = "34750f98bd59fcfc946da45aaabe933be154a4b5094e1c4abf42866505f3c97e";
const ADDRESS_TWO: &str = "6a3803d5f059902a1c6dafbc9ba4729212f7caac08634cc3ae76b27529f03827";
const TRANSACTION_ID: &str = "ec830f7529f3a6a9b7d21d5503e2fb55df286498dccce3ddf2f91757c30c0c58";
const WALLET_SIGNATURE: &str = "644dd46aeec3e80c0db0d40bc1bcff5d140e19228354b3da66937b29130484ae9c8d38e2e393c5308743cf5d73d89d94a83cf3c7f57e84056f93f0c11bd72b0f";

fn wallet(private_key: &str) -> Wallet {
    Wallet::from_private_key_hex(private_key).expect("test private key must be valid")
}

fn signed_transaction(
    sender: &Wallet,
    recipient: &Wallet,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> Transaction {
    let mut transaction = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        amount,
        fee,
        nonce,
    );
    transaction.sign(sender.sign(&transaction.message()));
    transaction
}

fn genesis_block() -> Block {
    Block::new(
        0,
        GENESIS_TIMESTAMP,
        GENESIS_PREVIOUS_HASH.to_string(),
        GENESIS_VALIDATOR.to_string(),
        String::new(),
        Vec::new(),
    )
}

fn signed_empty_block(genesis: &Block, validator: &Wallet) -> Block {
    let mut block = Block::new(
        1,
        GENESIS_TIMESTAMP + 1,
        genesis.hash.clone(),
        validator.address().to_string(),
        validator.public_key_hex(),
        Vec::new(),
    );
    block.sign(validator.sign(block.hash.as_bytes()));
    block
}

fn node_with_balance(sender: &Wallet, balance: u64) -> Node {
    let mut blockchain = Blockchain::new(genesis_block());
    blockchain
        .economy
        .mint(balance)
        .expect("test balance must fit below max supply");
    let mut state = State::new();
    state.create_account(sender.address().to_string(), balance);
    Node::new(blockchain, state, Consensus::new())
}

#[test]
fn transaction_creation_has_a_stable_id_and_detects_payload_changes() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let mut transaction = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        1_000_000,
        10,
        0,
    );

    assert_eq!(transaction.from, ADDRESS_ONE);
    assert_eq!(transaction.public_key, PUBLIC_KEY_ONE);
    assert_eq!(transaction.to, ADDRESS_TWO);
    assert_eq!(transaction.id, TRANSACTION_ID);
    assert_eq!(transaction.id, transaction.calculate_id());
    assert!(!transaction.coinbase);
    assert_eq!(transaction.reward_marker, 0);
    assert!(!transaction.is_signed());

    let stable_id = transaction.id.clone();
    transaction.amount += 1;
    assert_ne!(stable_id, transaction.calculate_id());
}

#[test]
fn wallet_signatures_verify_and_reject_tampered_inputs() {
    let signer = wallet(PRIVATE_KEY_ONE);
    let other_wallet = wallet(PRIVATE_KEY_TWO);
    let message = b"Kybernetes wallet signature fixture";
    let signature = signer.sign(message);

    assert_eq!(signer.public_key_hex(), PUBLIC_KEY_ONE);
    assert_eq!(signer.address(), ADDRESS_ONE);
    assert_eq!(signature, WALLET_SIGNATURE);
    assert!(Wallet::verify(
        &signer.public_key_hex(),
        message,
        &signature
    ));
    assert!(!Wallet::verify(
        &signer.public_key_hex(),
        b"Kybernetes wallet signature fixture!",
        &signature
    ));
    assert!(!Wallet::verify(
        &other_wallet.public_key_hex(),
        message,
        &signature
    ));
}

#[test]
fn node_accepts_the_next_valid_nonce() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let mut node = node_with_balance(&sender, 10_000);
    let transaction = signed_transaction(&sender, &recipient, 3_000, 10, 0);

    assert!(node.add_transaction(transaction));
    assert_eq!(node.mempool.len(), 1);
}

#[test]
fn node_rejects_an_invalid_nonce() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let mut node = node_with_balance(&sender, 10_000);
    let transaction = signed_transaction(&sender, &recipient, 3_000, 10, 1);

    assert!(!node.add_transaction(transaction));
    assert!(node.mempool.is_empty());
    assert_eq!(node.state.balance_of(sender.address()), 10_000);
    assert_eq!(node.state.nonce_of(sender.address()), 0);
}

#[test]
fn node_rejects_a_transaction_with_insufficient_balance() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let mut node = node_with_balance(&sender, 3_009);
    let transaction = signed_transaction(&sender, &recipient, 3_000, 10, 0);

    assert!(!node.add_transaction(transaction));
    assert!(node.mempool.is_empty());
    assert_eq!(node.state.balance_of(sender.address()), 3_009);
    assert_eq!(node.state.nonce_of(sender.address()), 0);
}

#[test]
fn block_hash_and_previous_hash_are_validated() {
    let validator = wallet(PRIVATE_KEY_THREE);
    let genesis = genesis_block();
    let valid_block = signed_empty_block(&genesis, &validator);

    assert!(valid_block.is_hash_valid());

    let mut chain = Blockchain::new(genesis.clone());
    assert!(chain.add_received_block(valid_block).is_ok());
    assert!(chain.is_valid());

    let mut wrong_previous_hash = Block::new(
        1,
        GENESIS_TIMESTAMP + 1,
        "11".repeat(32),
        validator.address().to_string(),
        validator.public_key_hex(),
        Vec::new(),
    );
    wrong_previous_hash.sign(validator.sign(wrong_previous_hash.hash.as_bytes()));

    let mut fresh_chain = Blockchain::new(genesis);
    assert!(fresh_chain.add_received_block(wrong_previous_hash).is_err());
}

#[test]
fn mempool_accepts_a_valid_signed_transaction_once() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let transaction = signed_transaction(&sender, &recipient, 1_000_000, 10, 0);
    let mut mempool = Mempool::new();

    assert!(mempool.add_transaction(transaction.clone()));
    assert_eq!(mempool.len(), 1);
    assert!(!mempool.add_transaction(transaction));
    assert_eq!(mempool.len(), 1);
}

#[test]
fn mempool_rejects_unsigned_and_tampered_transactions() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let unsigned = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        1_000_000,
        10,
        0,
    );
    let mut tampered = signed_transaction(&sender, &recipient, 1_000_000, 10, 0);
    tampered.amount += 1;

    let mut mempool = Mempool::new();
    assert!(!mempool.add_transaction(unsigned));
    assert!(!mempool.add_transaction(tampered));
    assert!(mempool.is_empty());
}

#[test]
fn state_applies_balance_and_nonce_changes() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let transaction = signed_transaction(&sender, &recipient, 3_000, 10, 0);
    let mut state = State::new();
    state.create_account(sender.address().to_string(), 10_000);
    state.create_account(recipient.address().to_string(), 500);

    state
        .apply_transaction(&transaction)
        .expect("valid state transition must succeed");

    assert_eq!(state.balance_of(sender.address()), 6_990);
    assert_eq!(state.balance_of(recipient.address()), 3_500);
    assert_eq!(state.nonce_of(sender.address()), 1);
}

#[test]
fn fee_liquidity_treasury_and_burn_match_protocol_percentages() {
    let mut economy = Economy::new();
    assert_eq!(economy.calculate_fee(1), 10);
    assert_eq!(economy.calculate_fee(999_999), 10);
    assert_eq!(economy.calculate_fee(2_500_000), 25);
    assert!(economy.validate_fee(2_500_000, 25));
    assert!(!economy.validate_fee(2_500_000, 24));

    let (validator_fee, liquidity_fee, treasury_fee, burn_fee) =
        economy.distribute_fee(1_000);
    assert_eq!(
        (validator_fee, liquidity_fee, treasury_fee, burn_fee),
        (150, 800, 50, 0)
    );

    let mut state = State::new();
    state.create_account("validator".to_string(), 0);
    state
        .add_balance("validator", validator_fee)
        .expect("validator fee must fit");
    state
        .add_treasury(treasury_fee)
        .expect("treasury fee must fit");
    state.burn(burn_fee).expect("burn fee must fit");
    economy
        .add_liquidity_reserve(liquidity_fee)
        .expect("liquidity reserve fee must fit");

    assert_eq!(state.balance_of("validator"), 150);
    assert_eq!(economy.liquidity_reserve(), 800);
    assert_eq!(state.treasury(), 50);
    assert_eq!(state.burned(), 0);
}

#[test]
fn fee_distribution_preserves_every_charged_unit() {
    let economy = Economy::new();
    let fee = economy.calculate_fee(100_100_000);
    let (validator_fee, liquidity_fee, treasury_fee, burn_fee) =
        economy.distribute_fee(fee);

    assert_eq!(fee, 1_001);
    assert_eq!(
        validator_fee + liquidity_fee + treasury_fee + burn_fee,
        fee,
        "fee distribution must not create or lose microKBN"
    );
}

#[test]
fn fee_never_falls_below_ten_micro_kbn() {
    let economy = Economy::new();

    for amount in [0, 1, 999_999, 1_000_000] {
        assert_eq!(economy.calculate_fee(amount), 10);
    }
}

#[test]
fn fee_uses_one_part_per_hundred_thousand() {
    let economy = Economy::new();

    assert_eq!(economy.calculate_fee(2_500_000), 25);
    assert_eq!(economy.calculate_fee(100_000_000), 1_000);
}

#[test]
fn liquidity_reserve_receives_eighty_percent_of_fees() {
    let economy = Economy::new();
    let (_, liquidity_fee, _, _) = economy.distribute_fee(1_000);

    assert_eq!(economy.liquidity_fee_percent, 80);
    assert_eq!(liquidity_fee, 800);
}

#[test]
fn treasury_receives_five_percent_of_fees() {
    let economy = Economy::new();
    let (_, _, treasury_fee, _) = economy.distribute_fee(1_000);

    assert_eq!(economy.treasury_fee_percent, 5);
    assert_eq!(treasury_fee, 50);
}

#[test]
fn validator_receives_the_fee_distribution_remainder() {
    let economy = Economy::new();
    let (validator_fee, liquidity_fee, treasury_fee, burn_fee) =
        economy.distribute_fee(1_001);

    assert_eq!(economy.validator_fee_percent, 15);
    assert_eq!(liquidity_fee, 800);
    assert_eq!(treasury_fee, 50);
    assert_eq!(validator_fee, 151);
    assert_eq!(burn_fee, 0);
}

#[test]
fn fee_distribution_never_burns_kbn() {
    let economy = Economy::new();

    for fee in [10, 1_000, 1_001, u64::MAX] {
        let (_, _, _, burn_fee) = economy.distribute_fee(fee);
        assert_eq!(burn_fee, 0);
    }
}

#[test]
fn fee_distribution_never_loses_a_micro_kbn() {
    let economy = Economy::new();

    for fee in [10, 11, 19, 20, 21, 99, 100, 101, 1_001, u64::MAX] {
        let (validator_fee, liquidity_fee, treasury_fee, burn_fee) =
            economy.distribute_fee(fee);
        let distributed = u128::from(validator_fee)
            + u128::from(liquidity_fee)
            + u128::from(treasury_fee)
            + u128::from(burn_fee);

        assert_eq!(distributed, u128::from(fee));
    }
}

#[test]
fn tiny_transfer_pays_the_minimum_fee_without_underflow() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let economy = Economy::new();
    let fee = economy.calculate_fee(1);
    let transaction = signed_transaction(&sender, &recipient, 1, fee, 0);
    let mut state = State::new();
    state.create_account(sender.address().to_string(), 11);
    state.create_account(recipient.address().to_string(), 0);

    state
        .apply_transaction(&transaction)
        .expect("the smallest funded transfer must apply");

    assert_eq!(fee, 10);
    assert_eq!(state.balance_of(sender.address()), 0);
    assert_eq!(state.balance_of(recipient.address()), 1);
    assert_eq!(state.nonce_of(sender.address()), 1);
}

#[test]
fn large_transfer_fee_and_distribution_do_not_overflow() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let economy = Economy::new();
    let large_amount = u64::MAX / 2;
    let large_fee = economy.calculate_fee(large_amount);
    let transaction = signed_transaction(&sender, &recipient, large_amount, large_fee, 0);
    let mut state = State::new();
    state.create_account(sender.address().to_string(), u64::MAX);
    state.create_account(recipient.address().to_string(), 0);

    state
        .apply_transaction(&transaction)
        .expect("a funded large transfer must not overflow");

    assert_eq!(state.balance_of(recipient.address()), large_amount);
    assert_eq!(
        state.balance_of(sender.address()),
        u64::MAX - large_amount - large_fee
    );

    let fee = economy.calculate_fee(u64::MAX);
    let (validator_fee, liquidity_fee, treasury_fee, burn_fee) =
        economy.distribute_fee(fee);

    assert_eq!(fee, 184_467_440_737_095);
    assert_eq!(
        u128::from(validator_fee)
            + u128::from(liquidity_fee)
            + u128::from(treasury_fee)
            + u128::from(burn_fee),
        u128::from(fee)
    );

    let (max_validator, max_liquidity, max_treasury, max_burn) =
        economy.distribute_fee(u64::MAX);
    assert_eq!(max_validator, 2_767_011_611_056_432_743);
    assert_eq!(max_liquidity, 14_757_395_258_967_641_292);
    assert_eq!(max_treasury, 922_337_203_685_477_580);
    assert_eq!(max_burn, 0);
}

#[test]
fn validator_selection_is_deterministic_for_the_same_hash() {
    let mut consensus = Consensus::new();
    assert!(consensus.add_validator("validator-a".to_string(), 700));
    assert!(consensus.add_validator("validator-b".to_string(), 300));
    let cloned = consensus.clone();

    let first = consensus
        .select_validator_from_hash("seed-0")
        .expect("a validator must be selected")
        .address
        .clone();

    for _ in 0..10 {
        assert_eq!(
            consensus
                .select_validator_from_hash("seed-0")
                .expect("a validator must be selected")
                .address,
            first
        );
    }

    assert_eq!(first, "validator-b");
    assert_eq!(
        cloned
            .select_validator_from_hash("seed-0")
            .expect("a validator must be selected")
            .address,
        first
    );
}

#[test]
fn blockchain_accepts_a_valid_signed_block() {
    let validator = wallet(PRIVATE_KEY_THREE);
    let genesis = genesis_block();
    let block = signed_empty_block(&genesis, &validator);
    let mut blockchain = Blockchain::new(genesis);

    assert!(blockchain.add_received_block(block.clone()).is_ok());
    assert_eq!(blockchain.height(), 2);
    assert_eq!(blockchain.chain[1].hash, block.hash);
    assert!(blockchain.is_valid());
}

#[test]
fn blockchain_rejects_a_manipulated_block_hash() {
    let validator = wallet(PRIVATE_KEY_THREE);
    let genesis = genesis_block();
    let mut block = signed_empty_block(&genesis, &validator);
    block.hash.replace_range(..1, "f");
    let mut blockchain = Blockchain::new(genesis);

    assert!(!block.is_hash_valid());
    assert!(blockchain.add_received_block(block).is_err());
    assert_eq!(blockchain.height(), 1);
}
