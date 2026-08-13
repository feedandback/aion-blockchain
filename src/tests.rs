use std::sync::OnceLock;

use crate::bootstrap::canonical_bootstrap;
use crate::chain::{Blockchain, Mempool};
use crate::consensus::Consensus;
use crate::core::{Block, Transaction};
use crate::economy::Economy;
use crate::network::NetworkMessage;
use crate::node::Node;
use crate::protocol::{GENESIS_TIMESTAMP, MAX_SYNC_BLOCKS_PER_MESSAGE};
use crate::state::State;
use crate::wallet::Wallet;

const INITIAL_SUPPLY: u64 = 20_000_000;
const TRANSFER_AMOUNT: u64 = 1_000_000;
const TRANSFER_FEE: u64 = 10;

fn wallet(byte: &str) -> Wallet {
    Wallet::from_private_key_hex(&byte.repeat(32)).expect("test private key must be valid")
}

#[test]
fn committed_configuration_changes_alter_fingerprint_and_genesis_hash() {
    let ((baseline_fingerprint, baseline_hash), variants) =
        crate::bootstrap::genesis_identity_test_vectors()
            .expect("genesis identity test vectors must be serializable");
    let fields = variants
        .iter()
        .map(|(field, _, _)| *field)
        .collect::<Vec<_>>();

    assert_eq!(
        fields,
        vec![
            "network_id",
            "network_protocol_version",
            "genesis_timestamp",
            "validator_address",
            "validator_stake",
            "genesis_allocation",
            "genesis_supply",
            "max_supply",
            "block_reward",
            "minimum_fee",
            "fee_divisor",
            "validator_percent",
            "liquidity_reserve_percent",
            "treasury_percent",
            "burn_percent",
            "initial_liquidity_reserve",
            "initial_treasury_balance",
            "initial_burned_amount",
        ]
    );

    for (field, fingerprint, hash) in variants {
        assert_ne!(
            fingerprint, baseline_fingerprint,
            "{field} fingerprint'e bağlı değil"
        );
        assert_ne!(hash, baseline_hash, "{field} genesis hash'e bağlı değil");
    }
}

fn genesis_block() -> Block {
    canonical_bootstrap()
        .expect("canonical test bootstrap must be valid")
        .blockchain
        .chain
        .into_iter()
        .next()
        .expect("canonical test chain must contain genesis")
}

fn initial_state(sender: &Wallet, recipient: &Wallet, validator: &Wallet) -> State {
    let mut state = State::new();
    state.create_account(sender.address().to_string(), INITIAL_SUPPLY);
    state.create_account(recipient.address().to_string(), 0);
    state.create_account(validator.address().to_string(), 0);
    state
}

fn node_and_valid_block() -> (Node, Block, String, String, String, u64) {
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

    let producer_liquidity_reserve =
        producer_blockchain.economy.liquidity_reserve();

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
        producer_liquidity_reserve,
    )
}

#[test]
fn node_accepts_a_valid_block_without_persisting_it() {
    let (node, block, sender, recipient, validator, producer_liquidity_reserve) =
        node_and_valid_block();

    let (accepted_blockchain, accepted_state) = node
        .validate_and_apply_block_for_test(block)
        .expect("a valid block must pass the node validation pipeline");

    assert_eq!(accepted_blockchain.height(), 2);
    assert_eq!(accepted_state.balance_of(&sender), 18_999_990);
    assert_eq!(accepted_state.balance_of(&recipient), 1_000_000);
    assert_eq!(accepted_state.balance_of(&validator), 10_000_002);
    assert_eq!(accepted_state.treasury(), 0);
    assert_eq!(accepted_state.burned(), 0);
    assert_eq!(accepted_blockchain.economy.supply(), 30_000_000);
    assert_eq!(producer_liquidity_reserve, 8);
    assert_eq!(accepted_blockchain.economy.liquidity_reserve(), 8);
}

#[test]
fn node_rejects_a_manipulated_block_without_changing_live_state() {
    let (node, mut block, sender, recipient, validator, _) = node_and_valid_block();
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

#[test]
fn chain_replay_rebuilds_the_same_liquidity_reserve() {
    let (mut node, block, _, _, _, producer_liquidity_reserve) = node_and_valid_block();
    let remote_chain = vec![genesis_block(), block];

    let completed = node
        .apply_chain_chunk_without_persisting_for_test(0, 2, remote_chain)
        .expect("a valid remote chain must replay successfully");

    assert_eq!(completed, None);
    assert_eq!(node.blockchain.height(), 2);
    assert_eq!(producer_liquidity_reserve, 8);
    assert_eq!(
        node.blockchain.economy.liquidity_reserve(),
        producer_liquidity_reserve
    );
}

fn build_remote_chain() -> Vec<Block> {
    let validator = wallet("03");
    let reward = Economy::new().block_reward;
    let total_blocks = MAX_SYNC_BLOCKS_PER_MESSAGE * 2 + 1;
    let mut chain = Vec::with_capacity(total_blocks);
    chain.push(genesis_block());

    for index in 1..total_blocks {
        let block_index = u64::try_from(index).expect("test block index must fit u64");
        let coinbase =
            Transaction::new_coinbase(validator.address().to_string(), reward, block_index);
        let mut block = Block::new(
            block_index,
            GENESIS_TIMESTAMP + block_index,
            chain
                .last()
                .expect("remote chain must contain genesis")
                .hash
                .clone(),
            validator.address().to_string(),
            validator.public_key_hex(),
            vec![coinbase],
        );
        block.sign(validator.sign(block.hash.as_bytes()));
        chain.push(block);
    }

    chain
}

fn remote_chain(total_blocks: usize) -> Vec<Block> {
    static REMOTE_CHAIN: OnceLock<Vec<Block>> = OnceLock::new();
    let chain = REMOTE_CHAIN.get_or_init(build_remote_chain);
    assert!(total_blocks <= chain.len());
    chain[..total_blocks].to_vec()
}

fn chain_sync_node() -> Node {
    let validator = wallet("03");
    let mut state = State::new();
    state.create_account(validator.address().to_string(), 0);
    let mut consensus = Consensus::new();
    assert!(consensus.add_validator(validator.address().to_string(), 1));

    Node::new(Blockchain::new(genesis_block()), state, consensus)
}

#[test]
fn canonical_node_rejects_a_different_valid_genesis_commitment() {
    let local = canonical_bootstrap().expect("canonical local bootstrap must be valid");
    let local_genesis_hash = local.blockchain.chain[0].hash.clone();
    let mut node = Node::new(local.blockchain, local.state, local.consensus);
    let mut remote_genesis = genesis_block();

    remote_genesis.validator_public_key = "0".repeat(64);
    remote_genesis.hash = remote_genesis.calculate_hash();

    assert!(remote_genesis.is_hash_valid());
    assert!(Blockchain::new(remote_genesis.clone()).is_valid());
    assert_ne!(remote_genesis.hash, local_genesis_hash);

    let result = node.apply_chain_chunk_without_persisting_for_test(0, 1, vec![remote_genesis]);

    assert!(result.is_err());
    assert_eq!(node.blockchain.height(), 1);
    assert_eq!(node.blockchain.chain[0].hash, local_genesis_hash);
}

#[test]
fn node_accepts_a_valid_chain_chunk_response() {
    let remote = remote_chain(2);
    let remote_tip_hash = remote
        .last()
        .expect("remote chain must contain a tip")
        .hash
        .clone();
    let mut node = chain_sync_node();

    node.receive_message_without_persisting_for_test(NetworkMessage::ChainChunkResponse {
        start_index: 0,
        total_blocks: 2,
        blocks: remote,
    });

    assert_eq!(node.blockchain.height(), 2);
    assert_eq!(
        node.blockchain
            .chain
            .last()
            .expect("local chain must contain a tip")
            .hash,
        remote_tip_hash
    );
}

#[test]
fn node_rejects_a_chain_chunk_above_the_256_block_limit() {
    let total_blocks = MAX_SYNC_BLOCKS_PER_MESSAGE + 1;
    let remote = remote_chain(total_blocks);
    let mut node = chain_sync_node();

    let result = node.apply_chain_chunk_without_persisting_for_test(
        0,
        u64::try_from(total_blocks).expect("test block count must fit u64"),
        remote,
    );

    assert!(result.is_err());
    assert_eq!(node.blockchain.height(), 1);
}

#[test]
fn node_rejects_a_chain_chunk_with_the_wrong_start_index() {
    let total_blocks = MAX_SYNC_BLOCKS_PER_MESSAGE + 1;
    let remote = remote_chain(total_blocks);
    let mut node = chain_sync_node();

    let first_result = node
        .apply_chain_chunk_without_persisting_for_test(
            0,
            u64::try_from(total_blocks).expect("test block count must fit u64"),
            remote[..MAX_SYNC_BLOCKS_PER_MESSAGE].to_vec(),
        )
        .expect("first chunk must be accepted");
    assert_eq!(
        first_result,
        Some(u64::try_from(MAX_SYNC_BLOCKS_PER_MESSAGE).expect("test chunk size must fit u64"))
    );

    let result = node.apply_chain_chunk_without_persisting_for_test(
        u64::try_from(MAX_SYNC_BLOCKS_PER_MESSAGE - 1).expect("test start index must fit u64"),
        u64::try_from(total_blocks).expect("test block count must fit u64"),
        remote[MAX_SYNC_BLOCKS_PER_MESSAGE..].to_vec(),
    );

    assert!(result.is_err());
    assert_eq!(node.blockchain.height(), 1);
}

#[test]
fn node_applies_a_chain_in_multiple_chunks() {
    let total_blocks = MAX_SYNC_BLOCKS_PER_MESSAGE + 1;
    let remote = remote_chain(total_blocks);
    let remote_tip_hash = remote
        .last()
        .expect("remote chain must contain a tip")
        .hash
        .clone();
    let mut node = chain_sync_node();

    let next_index = node
        .apply_chain_chunk_without_persisting_for_test(
            0,
            u64::try_from(total_blocks).expect("test block count must fit u64"),
            remote[..MAX_SYNC_BLOCKS_PER_MESSAGE].to_vec(),
        )
        .expect("first chunk must be accepted");
    assert_eq!(
        next_index,
        Some(u64::try_from(MAX_SYNC_BLOCKS_PER_MESSAGE).expect("test chunk size must fit u64"))
    );

    let completed = node
        .apply_chain_chunk_without_persisting_for_test(
            u64::try_from(MAX_SYNC_BLOCKS_PER_MESSAGE).expect("test start index must fit u64"),
            u64::try_from(total_blocks).expect("test block count must fit u64"),
            remote[MAX_SYNC_BLOCKS_PER_MESSAGE..].to_vec(),
        )
        .expect("final chunk must be accepted");

    assert_eq!(completed, None);
    assert_eq!(node.blockchain.height(), total_blocks);
    assert_eq!(
        node.blockchain
            .chain
            .last()
            .expect("local chain must contain a tip")
            .hash,
        remote_tip_hash
    );
}

#[test]
fn node_rejects_a_chunk_containing_a_tampered_equal_length_chain() {
    let mut remote = remote_chain(1);
    remote[0].timestamp += 1;
    assert!(!remote[0].is_hash_valid());
    let mut node = chain_sync_node();

    let result = node.apply_chain_chunk_without_persisting_for_test(0, 1, remote);

    assert!(
        result.is_err(),
        "an equal-length remote chain must be validated before it is treated as synchronized"
    );
    assert_eq!(node.blockchain.height(), 1);
}

#[test]
fn completed_sync_matches_the_remote_total_block_count() {
    let total_blocks = MAX_SYNC_BLOCKS_PER_MESSAGE * 2 + 1;
    let total_blocks_u64 = u64::try_from(total_blocks).expect("test block count must fit u64");
    let remote = remote_chain(total_blocks);
    let mut node = chain_sync_node();

    let first = node
        .apply_chain_chunk_without_persisting_for_test(
            0,
            total_blocks_u64,
            remote[..MAX_SYNC_BLOCKS_PER_MESSAGE].to_vec(),
        )
        .expect("first chunk must be accepted");
    assert_eq!(first, Some(256));

    let second = node
        .apply_chain_chunk_without_persisting_for_test(
            256,
            total_blocks_u64,
            remote[256..512].to_vec(),
        )
        .expect("second chunk must be accepted");
    assert_eq!(second, Some(512));

    let completed = node
        .apply_chain_chunk_without_persisting_for_test(
            512,
            total_blocks_u64,
            remote[512..].to_vec(),
        )
        .expect("final chunk must be accepted");

    assert_eq!(completed, None);
    assert_eq!(
        u64::try_from(node.blockchain.height()).expect("local height must fit u64"),
        total_blocks_u64
    );
}
