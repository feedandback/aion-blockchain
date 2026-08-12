use aion::chain::{Blockchain, Mempool};
use aion::consensus::Consensus;
use aion::core::{Block, Transaction};
use aion::economy::Economy;
use aion::node::Node;
use aion::protocol::{GENESIS_PREVIOUS_HASH, GENESIS_TIMESTAMP, GENESIS_VALIDATOR};
use aion::state::State;
use aion::wallet::Wallet;

const PRIVATE_KEY_ONE: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const PRIVATE_KEY_TWO: &str = "0202020202020202020202020202020202020202020202020202020202020202";
const PRIVATE_KEY_THREE: &str = "0303030303030303030303030303030303030303030303030303030303030303";
const PUBLIC_KEY_ONE: &str = "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
const ADDRESS_ONE: &str = "34750f98bd59fcfc946da45aaabe933be154a4b5094e1c4abf42866505f3c97e";
const ADDRESS_TWO: &str = "6a3803d5f059902a1c6dafbc9ba4729212f7caac08634cc3ae76b27529f03827";
const TRANSACTION_ID: &str = "e6032174544abb55e3223a6e5a692cd0026023cb64e35903885af13f77fedc1f";
const WALLET_SIGNATURE: &str = "8f6b383585f7dfff7b7e0370b4edfa306400b82aff451c67f0659d1bbee735dd77851f39e20dd2dd59a6e8e9745bd467e2fbe2782d16436224d19268e539fa01";

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
        1_000,
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
    let message = b"AION wallet signature fixture";
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
        b"AION wallet signature fixture!",
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
    let transaction = signed_transaction(&sender, &recipient, 3_000, 1_000, 0);

    assert!(node.add_transaction(transaction));
    assert_eq!(node.mempool.len(), 1);
}

#[test]
fn node_rejects_an_invalid_nonce() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let mut node = node_with_balance(&sender, 10_000);
    let transaction = signed_transaction(&sender, &recipient, 3_000, 1_000, 1);

    assert!(!node.add_transaction(transaction));
    assert!(node.mempool.is_empty());
    assert_eq!(node.state.balance_of(sender.address()), 10_000);
    assert_eq!(node.state.nonce_of(sender.address()), 0);
}

#[test]
fn node_rejects_a_transaction_with_insufficient_balance() {
    let sender = wallet(PRIVATE_KEY_ONE);
    let recipient = wallet(PRIVATE_KEY_TWO);
    let mut node = node_with_balance(&sender, 3_999);
    let transaction = signed_transaction(&sender, &recipient, 3_000, 1_000, 0);

    assert!(!node.add_transaction(transaction));
    assert!(node.mempool.is_empty());
    assert_eq!(node.state.balance_of(sender.address()), 3_999);
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
    let transaction = signed_transaction(&sender, &recipient, 1_000_000, 1_000, 0);
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
        1_000,
        0,
    );
    let mut tampered = signed_transaction(&sender, &recipient, 1_000_000, 1_000, 0);
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
    let transaction = signed_transaction(&sender, &recipient, 3_000, 1_000, 0);
    let mut state = State::new();
    state.create_account(sender.address().to_string(), 10_000);
    state.create_account(recipient.address().to_string(), 500);

    state
        .apply_transaction(&transaction)
        .expect("valid state transition must succeed");

    assert_eq!(state.balance_of(sender.address()), 6_000);
    assert_eq!(state.balance_of(recipient.address()), 3_500);
    assert_eq!(state.nonce_of(sender.address()), 1);
}

#[test]
fn fee_treasury_and_burn_calculations_match_protocol_percentages() {
    let economy = Economy::new();
    assert_eq!(economy.calculate_fee(1), 1_000);
    assert_eq!(economy.calculate_fee(999_999), 1_000);
    assert_eq!(economy.calculate_fee(2_500_000), 2_500);
    assert!(economy.validate_fee(2_500_000, 2_500));
    assert!(!economy.validate_fee(2_500_000, 2_499));

    let (validator_fee, treasury_fee, burn_fee) = economy.distribute_fee(1_000);
    assert_eq!((validator_fee, treasury_fee, burn_fee), (700, 200, 100));

    let mut state = State::new();
    state.create_account("validator".to_string(), 0);
    state
        .add_balance("validator", validator_fee)
        .expect("validator fee must fit");
    state
        .add_treasury(treasury_fee)
        .expect("treasury fee must fit");
    state.burn(burn_fee).expect("burn fee must fit");

    assert_eq!(state.balance_of("validator"), 700);
    assert_eq!(state.treasury(), 200);
    assert_eq!(state.burned(), 100);
}

#[test]
fn fee_distribution_preserves_every_charged_unit() {
    let economy = Economy::new();
    let fee = economy.calculate_fee(1_001_000);
    let (validator_fee, treasury_fee, burn_fee) = economy.distribute_fee(fee);

    assert_eq!(fee, 1_001);
    assert_eq!(
        validator_fee + treasury_fee + burn_fee,
        fee,
        "fee distribution must not create or lose micro-AION"
    );
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
