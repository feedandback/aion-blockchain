use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kybernetes::bootstrap::canonical_bootstrap;
use kybernetes::chain::Blockchain;
use kybernetes::consensus::Consensus;
use kybernetes::core::Transaction;
use kybernetes::network::tcp::TcpTransport;
use kybernetes::network::{Network, NetworkMessage, ONE_SHOT_CLIENT_LISTEN_ADDRESS};
use kybernetes::node::Node;
use kybernetes::protocol::GENESIS_TIMESTAMP;
use kybernetes::runtime::{NodeRole, NodeRuntime, RuntimeConfig};
use kybernetes::state::State;
use kybernetes::storage::Storage;
use kybernetes::validator::{ValidatorIdentity, ValidatorKeystore};
use kybernetes::wallet::Wallet;

const PASSWORD: &str = "correct horse battery staple";
const WRONG_PASSWORD: &str = "wrong horse battery staple";
const INITIAL_SUPPLY: u64 = 20_000_000;
static TEMP_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        let unique = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must follow unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kybernetes-{label}-{}-{timestamp}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("isolated test directory must be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let system_temp = std::env::temp_dir();
        if self.path.starts_with(&system_temp) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn consensus_with(wallets: &[&Wallet]) -> Consensus {
    let mut consensus = Consensus::new();
    for wallet in wallets {
        assert!(consensus.add_validator(wallet.address().to_string(), 1));
    }
    consensus
}

fn free_loopback_address() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("free loopback port must be allocated");
    let address = listener
        .local_addr()
        .expect("allocated loopback address must be readable");
    drop(listener);
    address.to_string()
}

async fn wait_for_listener(address: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if tokio::net::TcpStream::connect(address).await.is_ok() {
                return;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("runtime listener must become reachable");
}

fn provisioned_identity(
    data_directory: &Path,
    wallet: &Wallet,
    consensus: &Consensus,
    fingerprint: &str,
) -> ValidatorIdentity {
    let keystore = ValidatorKeystore::at(data_directory);
    keystore
        .provision(PASSWORD, &wallet.private_key_hex(), consensus, fingerprint)
        .expect("authorized test validator must be provisioned");
    keystore
        .load_authorized(PASSWORD, consensus, fingerprint)
        .expect("test validator keystore must decrypt")
        .expect("test validator identity must exist")
}

#[test]
fn validator_private_key_in_the_validator_set_is_accepted() {
    let validator = Wallet::new();
    let consensus = consensus_with(&[&validator]);

    let identity = ValidatorIdentity::from_private_key(&validator.private_key_hex(), &consensus)
        .expect("validator-set private key must be accepted");

    assert_eq!(identity.address(), validator.address());
}

#[test]
fn private_key_outside_the_validator_set_is_rejected() {
    let directory = TestDirectory::new("unauthorized-validator");
    let validator = Wallet::new();
    let outsider = Wallet::new();
    let consensus = consensus_with(&[&validator]);
    let keystore = ValidatorKeystore::at(directory.path());

    assert!(ValidatorIdentity::from_private_key(&outsider.private_key_hex(), &consensus).is_err());
    assert!(
        keystore
            .provision(
                PASSWORD,
                &outsider.private_key_hex(),
                &consensus,
                "test-genesis-fingerprint",
            )
            .is_err()
    );
    assert!(
        !keystore
            .exists()
            .expect("unauthorized provisioning path must remain inspectable")
    );
}

#[test]
fn node_starts_as_observer_without_a_validator_key() {
    let directory = TestDirectory::new("observer-start");
    let runtime = NodeRuntime::initialize_at(directory.path(), None)
        .expect("clean node must initialize without validator key");

    assert_eq!(runtime.role(), NodeRole::Observer);
    assert_eq!(runtime.node().blockchain.height(), 1);
    assert!(
        !ValidatorKeystore::at(directory.path())
            .exists()
            .expect("validator keystore path must be inspectable")
    );
    assert!(
        Storage::load_blockchain_from(directory.path())
            .expect("observer chain must be readable")
            .is_some()
    );
}

#[test]
fn observer_node_cannot_produce_a_block() {
    let directory = TestDirectory::new("observer-produce");
    let mut runtime = NodeRuntime::initialize_at(directory.path(), None)
        .expect("observer runtime must initialize");
    let original_height = runtime.node().blockchain.height();

    let error = runtime
        .try_produce_block(GENESIS_TIMESTAMP + 1)
        .expect_err("observer runtime must reject block production");
    assert!(error.contains("Observer node"));
    assert_eq!(runtime.node().blockchain.height(), original_height);
}

#[test]
fn validator_node_produces_only_when_its_validator_is_selected() {
    let directory = TestDirectory::new("selected-validator");
    let validator_a = Wallet::new();
    let validator_b = Wallet::new();
    let sender = Wallet::new();
    let recipient = Wallet::new();
    let validator_a_key = validator_a.private_key_hex();
    let validator_b_key = validator_b.private_key_hex();
    let sender_key = sender.private_key_hex();
    let recipient_address = recipient.address().to_string();
    let consensus = consensus_with(&[&validator_a, &validator_b]);
    let canonical = canonical_bootstrap().expect("canonical bootstrap must be valid");
    let genesis = canonical.blockchain.chain[0].clone();
    let selected_address = consensus
        .select_validator_from_hash(&genesis.hash)
        .expect("a test validator must be selected")
        .address
        .clone();
    let (selected_key, nonselected_key) = if selected_address == validator_a.address() {
        (validator_a_key, validator_b_key)
    } else {
        (validator_b_key, validator_a_key)
    };

    let build_runtime = |data_directory: &Path, validator_key: &str| {
        let mut blockchain = Blockchain::new(genesis.clone());
        blockchain
            .economy
            .mint(INITIAL_SUPPLY)
            .expect("test genesis supply must mint");
        let mut state = State::new();
        let sender = Wallet::from_private_key_hex(&sender_key)
            .expect("test sender private key must restore");
        state.create_account(sender.address().to_string(), INITIAL_SUPPLY);
        state.create_account(recipient_address.clone(), 0);
        state.create_account(validator_a.address().to_string(), 0);
        state.create_account(validator_b.address().to_string(), 0);
        let mut node = Node::new_with_data_directory(
            blockchain,
            state,
            consensus.clone(),
            data_directory.to_path_buf(),
        );
        let amount = 1_000_000;
        let fee = node.blockchain.economy.calculate_fee(amount);
        let mut transaction = Transaction::new(
            sender.address().to_string(),
            sender.public_key_hex(),
            recipient_address.clone(),
            amount,
            fee,
            0,
        );
        transaction.sign(sender.sign(&transaction.message()));
        assert!(node.add_transaction(transaction));
        Storage::save_blockchain_to(data_directory, &node.blockchain.chain)
            .expect("test genesis must be persisted before block production");
        let identity = ValidatorIdentity::from_private_key(validator_key, &consensus)
            .expect("test validator identity must be authorized");
        NodeRuntime::from_node(node, Some(identity)).expect("validator runtime must initialize")
    };

    let nonselected_directory = directory.path().join("nonselected");
    let mut nonselected = build_runtime(&nonselected_directory, &nonselected_key);
    assert!(
        nonselected
            .try_produce_block(GENESIS_TIMESTAMP + 1)
            .is_err()
    );
    assert_eq!(nonselected.node().blockchain.height(), 1);

    let selected_directory = directory.path().join("selected");
    let mut selected = build_runtime(&selected_directory, &selected_key);
    let produced = selected
        .try_produce_block(GENESIS_TIMESTAMP + 1)
        .expect("selected validator must produce a block");
    assert_eq!(produced.validator, selected_address);
    assert_eq!(selected.node().blockchain.height(), 2);
    assert!(
        Storage::load_blockchain_from(&selected_directory)
            .expect("produced chain must be readable")
            .is_some()
    );
}

#[test]
fn encrypted_validator_keystore_opens_with_the_correct_password() {
    let directory = TestDirectory::new("keystore-correct-password");
    let validator = Wallet::new();
    let consensus = consensus_with(&[&validator]);
    let fingerprint = "test-genesis-fingerprint";
    let private_key = validator.private_key_hex();
    let identity = provisioned_identity(directory.path(), &validator, &consensus, fingerprint);
    let encrypted_file = std::fs::read_to_string(ValidatorKeystore::at(directory.path()).path())
        .expect("validator keystore must be readable as envelope JSON");
    let envelope: serde_json::Value =
        serde_json::from_str(&encrypted_file).expect("keystore envelope must be valid JSON");

    assert_eq!(identity.address(), validator.address());
    assert!(!encrypted_file.contains(&private_key));
    assert_eq!(envelope["version"], 1);
    assert_eq!(envelope["kdf"], "argon2id");
    assert_eq!(envelope["kdf_memory_kib"], 19_456);
    assert_eq!(envelope["kdf_iterations"], 2);
    assert_eq!(envelope["kdf_parallelism"], 1);
    assert_eq!(envelope["cipher"], "chacha20poly1305");
    assert_eq!(
        envelope["salt_hex"]
            .as_str()
            .expect("salt must be encoded as hex")
            .len(),
        32
    );
    assert_eq!(
        envelope["nonce_hex"]
            .as_str()
            .expect("nonce must be encoded as hex")
            .len(),
        24
    );
}

#[test]
fn encrypted_validator_keystore_rejects_the_wrong_password() {
    let directory = TestDirectory::new("keystore-wrong-password");
    let validator = Wallet::new();
    let consensus = consensus_with(&[&validator]);
    let fingerprint = "test-genesis-fingerprint";
    let keystore = ValidatorKeystore::at(directory.path());
    keystore
        .provision(
            PASSWORD,
            &validator.private_key_hex(),
            &consensus,
            fingerprint,
        )
        .expect("test validator must be provisioned");

    assert!(
        keystore
            .load_authorized(WRONG_PASSWORD, &consensus, fingerprint)
            .is_err()
    );
}

#[test]
fn validator_keystores_are_isolated_between_data_directories() {
    let root = TestDirectory::new("keystore-isolation");
    let directory_a = root.path().join("node-a");
    let directory_b = root.path().join("node-b");
    let validator_a = Wallet::new();
    let validator_b = Wallet::new();
    let consensus = consensus_with(&[&validator_a, &validator_b]);
    let fingerprint = "test-genesis-fingerprint";
    let keystore_a = ValidatorKeystore::at(&directory_a);
    let keystore_b = ValidatorKeystore::at(&directory_b);
    keystore_a
        .provision(
            PASSWORD,
            &validator_a.private_key_hex(),
            &consensus,
            fingerprint,
        )
        .expect("node A validator must be provisioned");
    keystore_b
        .provision(
            WRONG_PASSWORD,
            &validator_b.private_key_hex(),
            &consensus,
            fingerprint,
        )
        .expect("node B validator must be provisioned");
    let identity_a = keystore_a
        .load_authorized(PASSWORD, &consensus, fingerprint)
        .expect("node A keystore must decrypt with node A password")
        .expect("node A identity must exist");
    let identity_b = keystore_b
        .load_authorized(WRONG_PASSWORD, &consensus, fingerprint)
        .expect("node B keystore must decrypt with node B password")
        .expect("node B identity must exist");

    assert_ne!(directory_a, directory_b);
    assert_ne!(
        ValidatorKeystore::at(&directory_a).path(),
        ValidatorKeystore::at(&directory_b).path()
    );
    assert_eq!(identity_a.address(), validator_a.address());
    assert_eq!(identity_b.address(), validator_b.address());
    assert_ne!(identity_a.address(), identity_b.address());
    assert!(
        keystore_a
            .load_authorized(WRONG_PASSWORD, &consensus, fingerprint)
            .is_err()
    );
    assert!(
        keystore_b
            .load_authorized(PASSWORD, &consensus, fingerprint)
            .is_err()
    );
}

#[tokio::test]
async fn runtime_one_shot_transaction_returns_acceptance_ack() {
    let directory = TestDirectory::new("runtime-transaction-ack");
    let validator = Wallet::new();
    let sender = Wallet::new();
    let recipient = Wallet::new();
    let consensus = consensus_with(&[&validator]);
    let canonical = canonical_bootstrap().expect("canonical bootstrap must be valid");
    let genesis = canonical.blockchain.chain[0].clone();

    let mut blockchain = Blockchain::new(genesis);
    blockchain
        .economy
        .mint(INITIAL_SUPPLY)
        .expect("test supply must mint");

    let mut state = State::new();
    state.create_account(sender.address().to_string(), INITIAL_SUPPLY);
    state.create_account(recipient.address().to_string(), 0);
    state.create_account(validator.address().to_string(), 0);

    let node = Node::new_with_data_directory(
        blockchain,
        state,
        consensus,
        directory.path().to_path_buf(),
    );
    let fee = node.blockchain.economy.calculate_fee(1);
    let mut transaction = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        1,
        fee,
        0,
    );
    transaction.sign(sender.sign(&transaction.message()));

    let transaction_id = transaction.id.clone();
    let runtime =
        NodeRuntime::from_node(node, None).expect("observer runtime must initialize");
    let listen_address = free_loopback_address();
    let server_address = listen_address.clone();

    let server = tokio::spawn(async move {
        runtime
            .run(RuntimeConfig {
                listen_address: server_address,
                peers: Vec::new(),
            })
            .await
    });

    wait_for_listener(&listen_address).await;

    let response = TcpTransport::send_authenticated_request(
        &listen_address,
        &sender,
        ONE_SHOT_CLIENT_LISTEN_ADDRESS,
        &NetworkMessage::Transaction(transaction),
    )
    .await
    .expect("runtime must return a transaction acknowledgement");

    assert!(Network::validate_transaction_ack(
        &response,
        &transaction_id
    ));
    match response {
        NetworkMessage::TransactionAck {
            transaction_id: acknowledged_id,
            accepted,
            reason,
        } => {
            assert_eq!(acknowledged_id, transaction_id);
            assert!(accepted);
            assert_eq!(reason, None);
        }
        _ => panic!("runtime must reply with TransactionAck"),
    }

    server.abort();
    let _ = server.await;
}

#[test]
fn runtime_rebuilds_state_and_nonce_from_persisted_chain() {
    let directory = TestDirectory::new("runtime-restart-replay");
    let validator_a = Wallet::new();
    let validator_b = Wallet::new();
    let sender = Wallet::new();
    let recipient = Wallet::new();
    let consensus = consensus_with(&[&validator_a, &validator_b]);
    let canonical = canonical_bootstrap().expect("canonical bootstrap must be valid");
    let genesis = canonical.blockchain.chain[0].clone();

    let selected_address = consensus
        .select_validator_from_hash(&genesis.hash)
        .expect("a test validator must be selected")
        .address
        .clone();
    let selected_key = if selected_address == validator_a.address() {
        validator_a.private_key_hex()
    } else {
        validator_b.private_key_hex()
    };

    let mut blockchain = Blockchain::new(genesis.clone());
    blockchain
        .economy
        .mint(INITIAL_SUPPLY)
        .expect("test genesis supply must mint");

    let mut state = State::new();
    state.create_account(sender.address().to_string(), INITIAL_SUPPLY);
    state.create_account(recipient.address().to_string(), 0);
    state.create_account(validator_a.address().to_string(), 0);
    state.create_account(validator_b.address().to_string(), 0);

    let mut node = Node::new_with_data_directory(
        blockchain,
        state,
        consensus.clone(),
        directory.path().to_path_buf(),
    );

    let amount = 1_000_000;
    let fee = node.blockchain.economy.calculate_fee(amount);
    let mut transaction = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        amount,
        fee,
        0,
    );
    transaction.sign(sender.sign(&transaction.message()));
    assert!(node.add_transaction(transaction));

    Storage::save_blockchain_to(directory.path(), &node.blockchain.chain)
        .expect("test genesis must be persisted before block production");

    let identity = ValidatorIdentity::from_private_key(&selected_key, &consensus)
        .expect("selected validator identity must be authorized");
    let mut runtime =
        NodeRuntime::from_node(node, Some(identity)).expect("validator runtime must initialize");

    let produced = runtime
        .try_produce_block(GENESIS_TIMESTAMP + 1)
        .expect("selected validator must produce the persisted block");

    let stored_chain = Storage::load_blockchain_from(directory.path())
        .expect("persisted chain must be readable")
        .expect("persisted chain must exist");
    assert_eq!(stored_chain.len(), 2);
    assert_eq!(
        stored_chain
            .last()
            .expect("persisted chain must contain the produced block")
            .hash,
        produced.hash
    );

    let mut restarted_blockchain = Blockchain::new(genesis);
    restarted_blockchain
        .economy
        .mint(INITIAL_SUPPLY)
        .expect("restart base supply must mint");

    let mut restarted_state = State::new();
    restarted_state.create_account(sender.address().to_string(), INITIAL_SUPPLY);
    restarted_state.create_account(recipient.address().to_string(), 0);
    restarted_state.create_account(validator_a.address().to_string(), 0);
    restarted_state.create_account(validator_b.address().to_string(), 0);

    let mut restarted_node = Node::new_with_data_directory(
        restarted_blockchain,
        restarted_state,
        consensus,
        directory.path().to_path_buf(),
    );
    restarted_node
        .restore_chain_from_storage(stored_chain)
        .expect("persisted chain must replay after restart");

    assert_eq!(restarted_node.blockchain.height(), 2);
    assert_eq!(
        restarted_node
            .blockchain
            .chain
            .last()
            .expect("replayed chain must have a tip")
            .hash,
        produced.hash
    );
    assert_eq!(restarted_node.state.balance_of(recipient.address()), amount);
    assert_eq!(restarted_node.state.nonce_of(sender.address()), 1);

    let second_amount = 1;
    let second_fee = restarted_node
        .blockchain
        .economy
        .calculate_fee(second_amount);
    let mut second_transaction = Transaction::new(
        sender.address().to_string(),
        sender.public_key_hex(),
        recipient.address().to_string(),
        second_amount,
        second_fee,
        restarted_node.state.nonce_of(sender.address()),
    );
    second_transaction.sign(sender.sign(&second_transaction.message()));
    assert!(restarted_node.add_transaction(second_transaction));

    let restarted_runtime = NodeRuntime::from_node(restarted_node, None)
        .expect("replayed node must remain usable as a runtime");
    assert_eq!(restarted_runtime.role(), NodeRole::Observer);
    assert_eq!(restarted_runtime.node().blockchain.height(), 2);
    assert_eq!(
        restarted_runtime.node().state.nonce_of(sender.address()),
        1
    );
}