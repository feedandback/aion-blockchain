use crate::chain::{Blockchain, Mempool};
use crate::consensus::Consensus;
use crate::core::{Block, Transaction};
use crate::node::Node;
use crate::protocol::{GENESIS_PREVIOUS_HASH, GENESIS_TIMESTAMP, GENESIS_VALIDATOR};
use crate::state::State;
use crate::wallet::Wallet;

const INITIAL_SUPPLY: u64 = 20_000_000;
const TRANSFER_AMOUNT: u64 = 1_000_000;
const TRANSFER_FEE: u64 = 1_000;

fn wallet(byte: &str) -> Wallet {
    Wallet::from_private_key_hex(&byte.repeat(32)).expect("test private key must be valid")
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

fn initial_state(sender: &Wallet, recipient: &Wallet, validator: &Wallet) -> State {
    let mut state = State::new();
    state.create_account(sender.address().to_string(), INITIAL_SUPPLY);
    state.create_account(recipient.address().to_string(), 0);
    state.create_account(validator.address().to_string(), 0);
    state
}

fn node_and_valid_block() -> (Node, Block, String, String, String) {
    let sender = wallet("01");
    let recipient = wallet("02");
    let validator = wallet("03");
    let genesis = genesis_block();

    let mut producer_blockchain = Blockchain::new(genesis.clone());
    producer_blockchain
        .economy
        .mint(INITIAL_SUPPLY)
        .expect("test supply must fit below max supply");
    let mut producer_state = initial_state(&sender, &recipient, &validator);
    let mut producer_mempool = Mempool::new();

    let mut transaction = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        TRANSFER_AMOUNT,
        TRANSFER_FEE,
        0,
    );
    transaction.sign(sender.sign(&transaction.message()));
    assert!(producer_mempool.add_transaction(transaction));

    let block = producer_blockchain
        .create_block_from_mempool(
            GENESIS_TIMESTAMP + 1,
            &validator,
            &mut producer_mempool,
            &mut producer_state,
        )
        .expect("fixture block must be produced");

    let mut receiver_blockchain = Blockchain::new(genesis);
    receiver_blockchain
        .economy
        .mint(INITIAL_SUPPLY)
        .expect("test supply must fit below max supply");
    let receiver_state = initial_state(&sender, &recipient, &validator);
    let mut consensus = Consensus::new();
    assert!(consensus.add_validator(validator.address().to_string(), 1));

    let node = Node::new(receiver_blockchain, receiver_state, consensus);

    (
        node,
        block,
        sender.address().to_string(),
        recipient.address().to_string(),
        validator.address().to_string(),
    )
}

#[test]
fn node_accepts_a_valid_block_without_persisting_it() {
    let (node, block, sender, recipient, validator) = node_and_valid_block();

    let (accepted_blockchain, accepted_state) = node
        .validate_and_apply_block_for_test(block)
        .expect("a valid block must pass the node validation pipeline");

    assert_eq!(accepted_blockchain.height(), 2);
    assert_eq!(accepted_state.balance_of(&sender), 18_999_000);
    assert_eq!(accepted_state.balance_of(&recipient), 1_000_000);
    assert_eq!(accepted_state.balance_of(&validator), 10_000_700);
    assert_eq!(accepted_state.treasury(), 200);
    assert_eq!(accepted_state.burned(), 100);
    assert_eq!(accepted_blockchain.economy.supply(), 29_999_900);
}

#[test]
fn node_rejects_a_manipulated_block_without_changing_live_state() {
    let (node, mut block, sender, recipient, validator) = node_and_valid_block();
    let original_height = node.blockchain.height();
    let original_sender_balance = node.state.balance_of(&sender);
    let original_recipient_balance = node.state.balance_of(&recipient);
    let original_validator_balance = node.state.balance_of(&validator);

    block.transactions[0].amount += 1;

    assert!(node.validate_and_apply_block_for_test(block).is_err());
    assert_eq!(node.blockchain.height(), original_height);
    assert_eq!(node.state.balance_of(&sender), original_sender_balance);
    assert_eq!(
        node.state.balance_of(&recipient),
        original_recipient_balance
    );
    assert_eq!(
        node.state.balance_of(&validator),
        original_validator_balance
    );
}
