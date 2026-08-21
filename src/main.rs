mod bootstrap;
mod chain;
mod consensus;
mod core;
mod economy;
mod network;
mod node;
mod protocol;
mod runtime;
mod state;
mod storage;
mod transaction_submission;
mod user_wallet;
mod validator;
mod wallet;

use std::io::{IsTerminal, Read};

use crate::bootstrap::canonical_bootstrap;
use crate::core::Transaction;
use crate::network::tcp::TcpTransport;
use crate::network::{Network, NetworkMessage};
use crate::node::Node;
use crate::protocol::MAX_SYNC_BLOCKS_PER_MESSAGE;
use crate::runtime::{
    NODE_LISTEN_ADDRESS_ENV, NODE_PEERS_ENV, NodeRuntime, RuntimeConfig, VALIDATOR_PASSWORD_ENV,
};
use crate::storage::Storage;
use crate::transaction_submission::{
    query_account_state_from_node, submit_from_active_validator, submit_from_user_wallet,
};
use crate::user_wallet::UserWalletKeystore;
use crate::validator::{ValidatorCandidateKeystore, ValidatorKeystore};
use crate::wallet::Wallet;

const WALLET_PASSWORD_ENV: &str = "KYBERNETES_WALLET_PASSWORD";

const LEGACY_WALLET_PASSWORD_ENV: &str = "AION_WALLET_PASSWORD";

const DATA_DIRECTORY_ENV: &str = "KYBERNETES_DATA_DIR";

#[tokio::main]
async fn main() {
    let arguments: Vec<String> = std::env::args().collect();

    if arguments.get(1).map(String::as_str) == Some("validator") {
        if arguments.len() != 3 {
            eprintln!(
                "Usage: kybernetes validator generate-candidate | candidate-address | activate-candidate"
            );
            std::process::exit(1);
        }

        let password = match std::env::var(VALIDATOR_PASSWORD_ENV) {
            Ok(password) if !password.trim().is_empty() => password,
            _ => {
                eprintln!(
                    "{} environment variable must be defined",
                    VALIDATOR_PASSWORD_ENV
                );
                std::process::exit(1);
            }
        };
        let candidate_keystore = ValidatorCandidateKeystore::configured();

        match arguments[2].as_str() {
            "generate-candidate" => match candidate_keystore.generate(&password) {
                Ok(address) => println!("Candidate validator address: {}", address),
                Err(error) => {
                    eprintln!("Candidate validator could not be generated: {error}");
                    std::process::exit(1);
                }
            },
            "candidate-address" => match candidate_keystore.load(&password) {
                Ok(Some(candidate)) => {
                    println!("Candidate validator address: {}", candidate.address())
                }
                Ok(None) => {
                    eprintln!("Validator candidate keystore was not found");
                    std::process::exit(1);
                }
                Err(_) => {
                    eprintln!("Candidate validator address could not be read");
                    std::process::exit(1);
                }
            },
            "activate-candidate" => {
                let bootstrap = match canonical_bootstrap() {
                    Ok(bootstrap) => bootstrap,
                    Err(error) => {
                        eprintln!("Canonical bootstrap could not be created: {error}");
                        std::process::exit(1);
                    }
                };
                match candidate_keystore.activate(
                    &password,
                    &bootstrap.consensus,
                    &bootstrap.genesis_fingerprint,
                ) {
                    Ok(activation) => {
                        println!("Active validator address: {}", activation.address());
                        if !activation.candidate_removed() {
                            eprintln!(
                                "Warning: active keystore was created; candidate file could not be removed and remains encrypted"
                            );
                        }
                    }
                    Err(error) => {
                        eprintln!("Candidate validator could not be activated: {error}");
                        std::process::exit(1);
                    }
                }
            }
            _ => {
                eprintln!(
                    "Usage: kybernetes validator generate-candidate | candidate-address | activate-candidate"
                );
                std::process::exit(1);
            }
        }

        return;
    }

    if arguments.get(1).map(String::as_str) == Some("provision-validator") {
        let password = match std::env::var(VALIDATOR_PASSWORD_ENV) {
            Ok(password) if !password.trim().is_empty() => password,
            _ => {
                eprintln!(
                    "{} environment variable must be defined",
                    VALIDATOR_PASSWORD_ENV
                );
                std::process::exit(1);
            }
        };
        let stdin = std::io::stdin();
        if stdin.is_terminal() {
            eprintln!(
                "Validator private key must not be echoed in the terminal; pipe it to stdin from a restricted-access file"
            );
            std::process::exit(1);
        }
        let mut private_key = String::new();
        if let Err(error) = stdin.take(129).read_to_string(&mut private_key) {
            eprintln!("Validator private key could not be read from stdin: {error}");
            std::process::exit(1);
        }
        if private_key.len() > 128 || private_key.trim().len() != 64 {
            eprintln!("Validator private key must be exactly 32-byte hex");
            std::process::exit(1);
        }

        let bootstrap = match canonical_bootstrap() {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                eprintln!("Canonical bootstrap could not be created: {error}");
                std::process::exit(1);
            }
        };
        let keystore = ValidatorKeystore::configured();
        match keystore.provision(
            &password,
            private_key.trim(),
            &bootstrap.consensus,
            &bootstrap.genesis_fingerprint,
        ) {
            Ok(address) => println!(
                "Validator keystore created: {} ({})",
                address,
                keystore.path().display()
            ),
            Err(error) => {
                eprintln!("Validator provisioning failed: {error}");
                std::process::exit(1);
            }
        }

        return;
    }

    if arguments.get(1).map(String::as_str) == Some("wallet")
        && arguments.get(2).map(String::as_str) == Some("send")
    {
        if arguments.len() != 6 {
            eprintln!(
                "Usage: kybernetes wallet send <peer_address> <recipient_address> <amount_microkbn>"
            );
            std::process::exit(1);
        }

        let password = match std::env::var(WALLET_PASSWORD_ENV) {
            Ok(password) if !password.trim().is_empty() => password,
            _ => {
                eprintln!("KYBERNETES_WALLET_PASSWORD environment variable must be defined");
                std::process::exit(1);
            }
        };

        let data_directory = match std::env::var(DATA_DIRECTORY_ENV) {
            Ok(directory) if !directory.trim().is_empty() => std::path::PathBuf::from(directory),
            _ => {
                eprintln!("KYBERNETES_DATA_DIR environment variable must be defined");
                std::process::exit(1);
            }
        };

        let amount_micro_kbn = match arguments[5].parse::<u64>() {
            Ok(amount) => amount,
            Err(_) => {
                eprintln!("Amount must be a valid unsigned integer in microKBN");
                std::process::exit(1);
            }
        };

        match submit_from_user_wallet(
            &data_directory,
            &password,
            &arguments[3],
            &arguments[4],
            amount_micro_kbn,
        )
        .await
        {
            Ok(transaction) => {
                println!("User wallet transaction accepted");
                println!("Transaction ID: {}", transaction.id);
                println!("From: {}", transaction.from);
                println!("To: {}", transaction.to);
            }

            Err(error) => {
                eprintln!("User wallet transaction failed: {error}");
                std::process::exit(1);
            }
        }

        return;
    }
    if arguments.get(1).map(String::as_str) == Some("wallet")
        && arguments.get(2).map(String::as_str) == Some("balance")
    {
        if arguments.len() != 4 {
            eprintln!("Usage: kybernetes wallet balance <peer_address>");
            std::process::exit(1);
        }

        let peer_address = &arguments[3];

        let password = match std::env::var(WALLET_PASSWORD_ENV) {
            Ok(password) if !password.trim().is_empty() => password,
            _ => {
                eprintln!("KYBERNETES_WALLET_PASSWORD environment variable must be defined");
                std::process::exit(1);
            }
        };

        let data_directory = match std::env::var(DATA_DIRECTORY_ENV) {
            Ok(directory) if !directory.trim().is_empty() => std::path::PathBuf::from(directory),
            _ => {
                eprintln!("KYBERNETES_DATA_DIR environment variable must be defined");
                std::process::exit(1);
            }
        };

        let wallet = match UserWalletKeystore::at(&data_directory).load(&password) {
            Ok(Some(wallet)) => wallet,
            Ok(None) => {
                eprintln!("User wallet keystore was not found");
                std::process::exit(1);
            }
            Err(error) => {
                eprintln!("User wallet could not be opened: {error}");
                std::process::exit(1);
            }
        };

        let address = wallet.address().to_string();

        match query_account_state_from_node(peer_address, &address).await {
            Ok((address, balance, nonce, tip_index, tip_hash)) => {
                println!("Address: {}", address);
                println!("Balance (microKBN): {}", balance);
                println!("Nonce: {}", nonce);
                println!("Node tip index: {}", tip_index);
                println!("Node tip hash: {}", tip_hash);
            }

            Err(error) => {
                eprintln!("User wallet balance could not be queried from node: {error}");
                std::process::exit(1);
            }
        }

        return;
    }
    if arguments.get(1).map(String::as_str) == Some("wallet") {
        if arguments.len() != 3 {
            eprintln!(
                "Usage: kybernetes wallet create | address | balance <peer_address> | send <peer_address> <recipient_address> <amount_microkbn>"
            );
            std::process::exit(1);
        }

        let password = match std::env::var(WALLET_PASSWORD_ENV) {
            Ok(password) if !password.trim().is_empty() => password,
            _ => {
                eprintln!("KYBERNETES_WALLET_PASSWORD environment variable must be defined");
                std::process::exit(1);
            }
        };

        let keystore = UserWalletKeystore::at(Storage::data_directory());

        match arguments[2].as_str() {
            "create" => match keystore.create(&password) {
                Ok(wallet) => {
                    println!("Kybernetes user wallet created");
                    println!("Address: {}", wallet.address());
                    println!("Public key: {}", wallet.public_key_hex());
                    println!("Keystore: {}", keystore.path().display());
                }
                Err(error) => {
                    eprintln!("User wallet could not be created: {error}");
                    std::process::exit(1);
                }
            },

            "address" => match keystore.load(&password) {
                Ok(Some(wallet)) => {
                    println!("Address: {}", wallet.address());
                    println!("Public key: {}", wallet.public_key_hex());
                }
                Ok(None) => {
                    eprintln!("User wallet does not exist");
                    std::process::exit(1);
                }
                Err(error) => {
                    eprintln!("User wallet could not be opened: {error}");
                    std::process::exit(1);
                }
            },

            _ => {
                eprintln!(
                    "Usage: kybernetes wallet create | address | balance <peer_address> | send <peer_address> <recipient_address> <amount_microkbn>"
                );
                std::process::exit(1);
            }
        }

        return;
    }
    if arguments.get(1).map(String::as_str) == Some("transaction") {
        if arguments.len() != 6 || arguments.get(2).map(String::as_str) != Some("submit") {
            eprintln!(
                "Usage: kybernetes transaction submit <peer_address> <recipient_address> <amount_microkbn>"
            );
            std::process::exit(1);
        }

        let peer_address = match arguments[3].parse::<std::net::SocketAddr>() {
            Ok(address) if address.port() != 0 => address.to_string(),
            _ => {
                eprintln!("Peer address must be a valid non-zero socket address");
                std::process::exit(1);
            }
        };
        let amount_micro_kbn = match arguments[5].parse::<u64>() {
            Ok(amount) if amount > 0 => amount,
            Err(_) => {
                eprintln!("Amount microKBN must be a positive u64");
                std::process::exit(1);
            }
            Ok(_) => {
                eprintln!("Amount microKBN must be greater than zero");
                std::process::exit(1);
            }
        };
        let password = match std::env::var(VALIDATOR_PASSWORD_ENV) {
            Ok(password) if !password.trim().is_empty() => password,
            _ => {
                eprintln!(
                    "{} environment variable must be defined",
                    VALIDATOR_PASSWORD_ENV
                );
                std::process::exit(1);
            }
        };
        let data_directory = match std::env::var(DATA_DIRECTORY_ENV) {
            Ok(directory) if !directory.trim().is_empty() => std::path::PathBuf::from(directory),
            _ => {
                eprintln!(
                    "{} environment variable must be defined",
                    DATA_DIRECTORY_ENV
                );
                std::process::exit(1);
            }
        };

        match submit_from_active_validator(
            &data_directory,
            &password,
            &peer_address,
            &arguments[4],
            amount_micro_kbn,
        )
        .await
        {
            Ok(transaction) => {
                println!("Transaction accepted by node");
                println!("Transaction ID: {}", transaction.id);
                println!("Sender address: {}", transaction.from);
                println!("Recipient address: {}", transaction.to);
                println!("Amount (microKBN): {}", transaction.amount);
                println!("Calculated fee (microKBN): {}", transaction.fee);
                println!("Nonce: {}", transaction.nonce);
                println!("Target peer: {peer_address}");
            }
            Err(error) => {
                eprintln!("Transaction was not accepted: {error}");
                std::process::exit(1);
            }
        }

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "chunk-sync-listen" {
        let listen_address = arguments[2].clone();

        let listener = TcpTransport::bind(&listen_address)
            .await
            .expect("Multi-chunk listener could not be started");

        let listener_wallet = Wallet::new();

        println!(" Multi-chain-chunk listener active: {}", listen_address);

        let (mut stream, peer_address, _handshake) =
            TcpTransport::accept_authenticated(&listener, &listener_wallet)
                .await
                .expect("Authenticated multi-chunk connection could not be established");

        println!(" Multi-chunk peer authenticated: {}", peer_address);

        let stored_chain = Storage::load_blockchain()
            .expect("Stored blockchain could not be read")
            .expect("Blockchain was not found on disk");

        let total_blocks =
            u64::try_from(stored_chain.len()).expect("Total block count exceeds the u64 range");

        let test_chunk_size = 4usize.min(MAX_SYNC_BLOCKS_PER_MESSAGE);

        let mut served_chunk_count = 0usize;

        loop {
            let request = TcpTransport::read_message(&mut stream)
                .await
                .expect("Multi ChainChunkRequest could not be read");

            let start_index = match request {
                NetworkMessage::ChainChunkRequest { start_index } => start_index,

                _ => {
                    panic!("Expected message was not ChainChunkRequest");
                }
            };

            let start =
                usize::try_from(start_index).expect("Chunk start index exceeds the usize range");

            let blocks = if start >= stored_chain.len() {
                Vec::new()
            } else {
                let end = start
                    .saturating_add(test_chunk_size)
                    .min(stored_chain.len());

                stored_chain[start..end].to_vec()
            };

            let sent_block_count = blocks.len();

            let response = NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            };

            TcpTransport::send_message(&mut stream, &response)
                .await
                .expect("Multi ChainChunkResponse could not be sent");

            served_chunk_count += 1;

            println!(
                " Chunk #{} sent. Start: {} Blocks sent: {} Total chain: {}",
                served_chunk_count, start_index, sent_block_count, total_blocks
            );

            let end_index = start.saturating_add(sent_block_count);

            if end_index >= stored_chain.len() {
                break;
            }
        }

        println!(
            " Multi-chunk serving completed. Total chunks: {}",
            served_chunk_count
        );

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "chunk-sync" {
        let peer_address = arguments[2].clone();

        let sync_bootstrap =
            canonical_bootstrap().expect("Canonical sync genesis could not be created");

        println!(" Sync node started with the fixed Kybernetes genesis configuration.");

        println!(" Sync does not require a local validator private key.");

        let mut sync_node = Node::new(
            sync_bootstrap.blockchain,
            sync_bootstrap.state,
            sync_bootstrap.consensus,
        );

        let requester_wallet = Wallet::new();

        let mut stream =
            TcpTransport::connect_authenticated(&peer_address, &requester_wallet, "127.0.0.1:7005")
                .await
                .expect("Authenticated connection for multi-chunk sync could not be established");

        let mut next_start = 0u64;

        let mut expected_total = None;

        let mut received_chunk_count = 0usize;

        loop {
            let request = NetworkMessage::ChainChunkRequest {
                start_index: next_start,
            };

            TcpTransport::send_message(&mut stream, &request)
                .await
                .expect("Multi ChainChunkRequest could not be sent");

            let response = TcpTransport::read_message(&mut stream)
                .await
                .expect("Multi ChainChunkResponse could not be read");

            let (start_index, total_blocks, blocks) = match response {
                NetworkMessage::ChainChunkResponse {
                    start_index,
                    total_blocks,
                    blocks,
                } => (start_index, total_blocks, blocks),

                _ => {
                    panic!("Expected message was not ChainChunkResponse");
                }
            };

            if start_index != next_start {
                panic!(
                    "Chunk start index does not match. Expected: {}, Received: {}",
                    next_start, start_index
                );
            }

            if total_blocks == 0 {
                panic!("Total block count cannot be zero");
            }

            if blocks.is_empty() {
                panic!("Received an empty chunk before synchronization completed");
            }

            if blocks.len() > MAX_SYNC_BLOCKS_PER_MESSAGE {
                panic!("Incoming chunk exceeds the protocol limit");
            }

            match expected_total {
                Some(current_total) if current_total != total_blocks => {
                    panic!("Total block count changed during synchronization");
                }

                None => {
                    expected_total = Some(total_blocks);
                }

                _ => {}
            }

            received_chunk_count += 1;

            println!(
                " Chunk #{} received. Start: {} Blocks: {} Total: {}",
                received_chunk_count,
                start_index,
                blocks.len(),
                total_blocks
            );

            let next_chunk = sync_node
                .apply_chain_chunk(start_index, total_blocks, blocks)
                .expect("Incoming multi-blockchain chunk failed Node validation");

            match next_chunk {
                Some(next_index) => {
                    println!(
                        " Next chunk will be requested automatically: {}",
                        next_index
                    );

                    next_start = next_index;
                }

                None => {
                    break;
                }
            }
        }

        let total_blocks =
            expected_total.expect("Synchronization total block information could not be obtained");

        let synchronized = sync_node.blockchain.chain.len() == total_blocks as usize;

        println!(
            " Multi-chunk synchronization completed. Chunks received: {}",
            received_chunk_count
        );

        println!(
            " Synchronized chain block count: {}",
            sync_node.blockchain.chain.len()
        );

        println!(
            " Automatic multi-chunk sync in the same TCP session succeeded: {}",
            synchronized && received_chunk_count > 1
        );

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "chunk-listen" {
        let listen_address = arguments[2].clone();

        let listener = TcpTransport::bind(&listen_address)
            .await
            .expect("Chunk test listener could not be started");

        let listener_wallet = Wallet::new();

        println!(" Chain chunk test listener active: {}", listen_address);

        let (mut stream, peer_address, _handshake, request) =
            TcpTransport::accept_authenticated_request(&listener, &listener_wallet)
                .await
                .expect("Authenticated chunk request could not be received");

        println!(" Authenticated chunk request received: {:?}", request);

        println!("Peer: {}", peer_address);

        let start_index = match request {
            NetworkMessage::ChainChunkRequest { start_index } => start_index,

            _ => {
                panic!("Expected message was not ChainChunkRequest");
            }
        };

        let stored_chain =
            Storage::load_blockchain()
                .expect(
                    "Stored blockchain could not be read",
                )
                .expect(
                    "Blockchain was not found on disk. Run a normal node first to create the blockchain.",
                );

        let total_blocks =
            u64::try_from(stored_chain.len()).expect("Total block count exceeds the u64 range");

        let start =
            usize::try_from(start_index).expect("Chunk start index exceeds the usize range");

        let blocks = if start >= stored_chain.len() {
            Vec::new()
        } else {
            let end = start
                .saturating_add(MAX_SYNC_BLOCKS_PER_MESSAGE)
                .min(stored_chain.len());

            stored_chain[start..end].to_vec()
        };

        let sent_block_count = blocks.len();

        let response = NetworkMessage::ChainChunkResponse {
            start_index,
            total_blocks,
            blocks,
        };

        TcpTransport::send_message(&mut stream, &response)
            .await
            .expect("ChainChunkResponse could not be sent");

        println!(
            " Real ChainChunkResponse sent. Start: {} Total chain: {} Blocks sent: {}",
            start_index, total_blocks, sent_block_count
        );

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "chunk-request" {
        let peer_address = arguments[2].clone();

        let requester_wallet = Wallet::new();

        let request = NetworkMessage::ChainChunkRequest { start_index: 0 };

        let response = TcpTransport::send_authenticated_request(
            &peer_address,
            &requester_wallet,
            "127.0.0.1:7004",
            &request,
        )
        .await
        .expect("Authenticated ChainChunkRequest/Response failed");

        println!(" ChainChunkResponse received: {:?}", response);

        let valid_response = match &response {
            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    " Received real blockchain chunk: start={} total_blocks={} received_blocks={}",
                    start_index,
                    total_blocks,
                    blocks.len()
                );

                *start_index == 0
                    && *total_blocks > 0
                    && !blocks.is_empty()
                    && blocks.len() <= MAX_SYNC_BLOCKS_PER_MESSAGE
                    && blocks
                        .first()
                        .map(|block| block.index == 0)
                        .unwrap_or(false)
            }

            _ => false,
        };

        println!(
            " Real on-disk blockchain received over TCP: {}",
            valid_response
        );

        if !valid_response {
            panic!("Incoming ChainChunkResponse failed basic validation");
        }

        let (start_index, total_blocks, blocks) = match response {
            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => (start_index, total_blocks, blocks),

            _ => {
                unreachable!("Response was already validated as ChainChunkResponse");
            }
        };

        let sync_bootstrap =
            canonical_bootstrap().expect("Canonical sync genesis could not be created");

        let mut sync_node = Node::new(
            sync_bootstrap.blockchain,
            sync_bootstrap.state,
            sync_bootstrap.consensus,
        );

        let next_chunk = sync_node
            .apply_chain_chunk(start_index, total_blocks, blocks)
            .expect("Incoming blockchain chunk failed Node validation");

        println!(" Incoming blockchain passed Node validation.");

        println!(
            " Synchronized local chain block count: {}",
            sync_node.blockchain.chain.len()
        );

        println!(" Next chunk required: {}", next_chunk.is_some());

        println!(
            " Blockchain validated and applied to persistent storage: {}",
            next_chunk.is_none() && sync_node.blockchain.chain.len() == total_blocks as usize
        );

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "listen" {
        let listen_address = arguments[2].clone();

        let listener_wallet = std::sync::Arc::new(Wallet::new());

        let (sender, mut receiver) = tokio::sync::mpsc::channel(32);

        let listener_address = listen_address.clone();

        let server_wallet = listener_wallet.clone();

        let server_task = tokio::spawn(async move {
            TcpTransport::run_authenticated_listener(&listener_address, server_wallet, sender).await
        });

        println!(
            "Kybernetes persistent P2P listener started: {}",
            listen_address
        );

        let mut network = Network::new();

        let mut handshake_received = false;

        let mut sync_received = false;

        while !handshake_received || !sync_received {
            let (message, peer_address) = receiver
                .recv()
                .await
                .expect("P2P message channel closed unexpectedly");

            println!(" Main node received P2P message: {:?}", message);

            println!("Peer: {}", peer_address);

            match &message {
                NetworkMessage::Handshake { .. } => {
                    network.receive(message);

                    handshake_received = true;

                    println!(
                        " main network configured peer count: {}",
                        network.identified_peer_count()
                    );
                }

                NetworkMessage::SyncRequest => {
                    network.receive(message);

                    sync_received = true;

                    println!(" main node SyncRequest processed.");
                }

                _ => {
                    network.receive(message);
                }
            }
        }

        println!(
            " Authenticated peer registered with main Network: {}",
            network.identified_peer_count() == 1
        );

        server_task.abort();

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "send-stale-handshake" {
        let peer_address = arguments[2].clone();

        let test_wallet = Wallet::new();

        let listen_address = "127.0.0.1:7005".to_string();

        let protocol_version = Network::protocol_version();

        let timestamp = Network::current_timestamp()
            .saturating_sub(crate::protocol::MAX_HANDSHAKE_AGE_SECONDS + 1);

        let public_key = test_wallet.public_key_hex();

        let challenge = Network::generate_handshake_challenge();

        let signature = test_wallet.sign_node_handshake(
            &listen_address,
            crate::protocol::NETWORK_ID,
            protocol_version,
            timestamp,
            &challenge,
        );

        let stale_handshake = NetworkMessage::Handshake {
            node_id: test_wallet.node_id().to_string(),
            public_key,
            listen_address,
            network_id: crate::protocol::NETWORK_ID.to_string(),
            protocol_version,
            timestamp,
            challenge,
            signature,
        };

        let response = TcpTransport::send_and_receive(&peer_address, &stale_handshake)
            .await
            .expect("Stale handshake test failed");

        println!(" Stale timestamp handshake sent: {}", timestamp);

        println!("Handshake response: {:?}", response);

        match response {
            NetworkMessage::HandshakeAck { accepted, .. } => {
                println!(" Stale handshake rejected: {}", !accepted);
            }

            _ => {
                panic!("Expected HandshakeAck was not received");
            }
        }

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "send-wrong-version" {
        let peer_address = arguments[2].clone();

        let test_wallet = Wallet::new();

        let listen_address = "127.0.0.1:7004".to_string();

        let protocol_version = Network::protocol_version() + 1;

        let public_key = test_wallet.public_key_hex();

        let timestamp = Network::current_timestamp();

        let challenge = Network::generate_handshake_challenge();

        let signature = test_wallet.sign_node_handshake(
            &listen_address,
            crate::protocol::NETWORK_ID,
            protocol_version,
            timestamp,
            &challenge,
        );

        let wrong_version_handshake = NetworkMessage::Handshake {
            node_id: test_wallet.node_id().to_string(),
            public_key,
            listen_address,
            network_id: crate::protocol::NETWORK_ID.to_string(),
            protocol_version,
            timestamp,
            challenge,
            signature,
        };

        TcpTransport::send_to(&peer_address, &wrong_version_handshake)
            .await
            .expect("Handshake with wrong protocol version could not be sent");

        println!(
            " Wrong protocol version handshake test message sent: {}",
            peer_address
        );

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "send-wrong-network" {
        let peer_address = arguments[2].clone();

        let test_wallet = Wallet::new();

        let listen_address = "127.0.0.1:7003".to_string();

        let wrong_network_id = "wrong-kybernetes-network".to_string();

        let protocol_version = Network::protocol_version();

        let public_key = test_wallet.public_key_hex();

        let timestamp = Network::current_timestamp();

        let challenge = Network::generate_handshake_challenge();

        let signature = test_wallet.sign_node_handshake(
            &listen_address,
            &wrong_network_id,
            protocol_version,
            timestamp,
            &challenge,
        );

        let wrong_handshake = NetworkMessage::Handshake {
            node_id: test_wallet.node_id().to_string(),
            public_key,
            listen_address,
            network_id: wrong_network_id,
            protocol_version,
            timestamp,
            challenge,
            signature,
        };

        TcpTransport::send_to(&peer_address, &wrong_handshake)
            .await
            .expect("Wrong network handshake could not be sent");

        println!(
            " Wrong Network ID handshake test message sent: {}",
            peer_address
        );

        return;
    }

    if arguments.len() >= 4 && arguments[1] == "broadcast" {
        let peer_addresses = vec![arguments[2].clone(), arguments[3].clone()];

        let test_wallet = Wallet::new();

        let mut network = Network::new();

        for peer_address in peer_addresses {
            let added = network.add_peer(peer_address);

            if !added {
                panic!("Broadcast peer registration could not be added");
            }
        }

        let (success_count, failure_count) = network
            .broadcast_to_peers(&test_wallet, "127.0.0.1:7003", NetworkMessage::SyncRequest)
            .await;

        println!(
            " Network broadcast successful peer count: {}",
            success_count
        );

        println!(" Network broadcast failed peer count: {}", failure_count);

        println!(
            " Network layer two-peer broadcast test succeeded: {}",
            success_count == 2 && failure_count == 0
        );

        println!("Network message history: {}", network.message_count());

        return;
    }

    if arguments.len() >= 3 && arguments[1] == "send" {
        let peer_address = arguments[2].clone();

        let test_wallet = Wallet::new();

        TcpTransport::send_authenticated_message(
            &peer_address,
            &test_wallet,
            "127.0.0.1:7002",
            &NetworkMessage::SyncRequest,
        )
        .await
        .expect("Authenticated P2P message could not be sent");

        println!(
            " SyncRequest sent over authenticated real TCP: {}",
            peer_address
        );

        return;
    }

    if arguments.len() == 1 || arguments.get(1).map(String::as_str) == Some("node") {
        let listen_address = arguments
            .get(2)
            .cloned()
            .or_else(|| std::env::var(NODE_LISTEN_ADDRESS_ENV).ok())
            .filter(|address| !address.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1:7000".to_string());
        let mut peers = arguments
            .iter()
            .skip(3)
            .filter(|peer| !peer.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>();

        if let Ok(configured_peers) = std::env::var(NODE_PEERS_ENV) {
            peers.extend(
                configured_peers
                    .split(',')
                    .map(str::trim)
                    .filter(|peer| !peer.is_empty())
                    .map(str::to_string),
            );
        }
        peers.sort();
        peers.dedup();

        let validator_password = std::env::var(VALIDATOR_PASSWORD_ENV)
            .ok()
            .filter(|password| !password.trim().is_empty());
        let runtime = match NodeRuntime::initialize_at(
            Storage::data_directory(),
            validator_password.as_deref(),
        ) {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Kybernetes node runtime could not be started: {error}");
                std::process::exit(1);
            }
        };

        if let Err(error) = runtime
            .run(RuntimeConfig {
                listen_address,
                peers,
            })
            .await
        {
            eprintln!("Kybernetes node runtime stopped: {error}");
            std::process::exit(1);
        }

        return;
    }

    if arguments.get(1).map(String::as_str) != Some("demo") {
        eprintln!(
            "Usage: kybernetes [node [listen_address] [peer...]] | wallet create | wallet address | wallet balance <peer_address> | wallet send <peer_address> <recipient_address> <amount_microkbn> | transaction submit <peer_address> <recipient_address> <amount_microkbn> | validator generate-candidate | validator candidate-address | validator activate-candidate | provision-validator | demo | legacy test CLI mode"
        );
        return;
    }

    let wallet_password = std::env::var(WALLET_PASSWORD_ENV)
        .or_else(|_| std::env::var(LEGACY_WALLET_PASSWORD_ENV))
        .expect("KYBERNETES_WALLET_PASSWORD environment variable is not defined");

    // ==========================
    // REAL TCP LOOPBACK TEST
    // ==========================

    let tcp_listener = TcpTransport::bind("127.0.0.1:0")
        .await
        .expect("TCP test listener could not be started");

    let tcp_address = tcp_listener
        .local_addr()
        .expect("TCP test address could not be obtained")
        .to_string();

    let tcp_server = tokio::spawn(async move { TcpTransport::accept_one(&tcp_listener).await });

    TcpTransport::send_to(&tcp_address, &NetworkMessage::SyncRequest)
        .await
        .expect("TCP test message could not be sent");

    let (tcp_received_message, tcp_peer) = tcp_server
        .await
        .expect("TCP test task could not be completed")
        .expect("TCP test message could not be received");

    let tcp_loopback_ok = matches!(tcp_received_message, NetworkMessage::SyncRequest);

    println!(
        " TCP P2P loopback message test succeeded: {}",
        tcp_loopback_ok
    );

    println!("TCP test peer: {}", tcp_peer);

    // ==========================
    // PERSISTENT WALLET LOADING
    // ==========================

    let (alice_wallet, bob_wallet) = match Storage::load_wallet_private_keys(&wallet_password) {
        Ok(Some((alice_private_key, bob_private_key))) => {
            let alice_wallet = Wallet::from_private_key_hex(&alice_private_key)
                .expect("Stored Alice private key is invalid");

            let bob_wallet = Wallet::from_private_key_hex(&bob_private_key)
                .expect("Stored Bob private key is invalid");

            println!(" Wallet keys from disk loaded.");

            (alice_wallet, bob_wallet)
        }

        Ok(None) => {
            let alice_wallet = Wallet::new();

            let bob_wallet = Wallet::new();

            Storage::save_wallet_private_keys(
                &wallet_password,
                &alice_wallet.private_key_hex(),
                &bob_wallet.private_key_hex(),
            )
            .expect("Wallet private keys could not be saved to disk");

            println!(
                " New wallet keys created and saved to disk: {}",
                Storage::wallets_path().display()
            );

            (alice_wallet, bob_wallet)
        }

        Err(error) => {
            panic!("Wallet file could not be loaded safely: {}", error);
        }
    };

    // ==========================
    // WALLET PRIVATE KEY RESTORE TEST
    // ==========================

    let alice_private_key = alice_wallet.private_key_hex();

    let restored_alice_wallet = Wallet::from_private_key_hex(&alice_private_key)
        .expect("Alice wallet could not be restored from private key");

    println!(
        "Same address restored from wallet private key: {}",
        restored_alice_wallet.address() == alice_wallet.address()
    );

    let node_identity_timestamp = Network::current_timestamp();

    let node_identity_challenge = Network::generate_handshake_challenge();

    let node_identity_signature = alice_wallet.sign_node_handshake(
        "127.0.0.1:7002",
        crate::protocol::NETWORK_ID,
        Network::protocol_version(),
        node_identity_timestamp,
        &node_identity_challenge,
    );

    let signed_node_identity_ok = Wallet::verify_node_handshake(
        alice_wallet.node_id(),
        &alice_wallet.public_key_hex(),
        "127.0.0.1:7002",
        crate::protocol::NETWORK_ID,
        Network::protocol_version(),
        node_identity_timestamp,
        &node_identity_challenge,
        &node_identity_signature,
    );

    println!(
        " Signed node identity verified: {}",
        signed_node_identity_ok
    );

    if !signed_node_identity_ok {
        panic!("Signed node identity verification failed");
    }

    let alice_bootstrap = canonical_bootstrap().expect("Canonical genesis could not be created");

    println!(
        "Validator count: {}",
        alice_bootstrap.consensus.validator_count()
    );

    println!(
        "Total stake: {} KBN",
        alice_bootstrap.consensus.total_stake()
    );

    // ==========================
    // ALICE NODE
    // ==========================

    let mut alice_node = Node::new(
        alice_bootstrap.blockchain,
        alice_bootstrap.state,
        alice_bootstrap.consensus,
    );

    // ==========================
    // BOB NODE
    // ==========================

    let bob_bootstrap = canonical_bootstrap().expect("Bob canonical genesis could not be created");

    let mut bob_node = Node::new(
        bob_bootstrap.blockchain,
        bob_bootstrap.state,
        bob_bootstrap.consensus,
    );

    // ==========================
    // BLOCKCHAIN RESTART RESTORE
    // ==========================

    match Storage::load_blockchain() {
        Ok(Some(saved_chain)) => {
            let saved_block_count = saved_chain.len();

            let saved_tip_hash = saved_chain
                .last()
                .map(|block| block.hash.clone())
                .expect("Stored blockchain is empty");

            alice_node
                .restore_chain_from_storage(saved_chain.clone())
                .expect("Alice Node could not restore blockchain from disk");

            bob_node
                .restore_chain_from_storage(saved_chain)
                .expect("Bob Node could not restore blockchain from disk");

            println!(" Blockchain loaded from disk at startup.");

            println!("Loaded block count: {}", saved_block_count);

            println!(
                "Tip hash matches: {}",
                alice_node
                    .blockchain
                    .chain
                    .last()
                    .map(|block| { block.hash.as_str() },)
                    == Some(saved_tip_hash.as_str(),)
            );

            println!(
                "Alice loaded balance: {} KBN",
                alice_node.state.balance_of(alice_wallet.address(),) / 1_000_000
            );

            println!(
                "Bob loaded balance: {} KBN",
                alice_node.state.balance_of(bob_wallet.address(),) / 1_000_000
            );

            println!(
                "Loaded total supply: {} KBN",
                alice_node.blockchain.economy.supply() / 1_000_000
            );

            println!(
                "Alice blockchain valid: {}",
                alice_node.blockchain.is_valid()
            );

            println!("Bob blockchain valid: {}", bob_node.blockchain.is_valid());

            println!(" Blockchain resumed successfully after node restart.");

            // ==========================
            // NEW BLOCK AFTER RESTART
            // ==========================

            let restart_amount = 1_000_000;

            let restart_fee = alice_node.blockchain.economy.calculate_fee(restart_amount);

            let restart_nonce = alice_node.state.nonce_of(alice_wallet.address());

            let mut restart_transaction = Transaction::new(
                alice_wallet.address().to_string(),
                alice_wallet.public_key_hex(),
                bob_wallet.address().to_string(),
                restart_amount,
                restart_fee,
                restart_nonce,
            );

            restart_transaction.sign(alice_wallet.sign(&restart_transaction.message()));

            let restart_tx_added = alice_node.add_transaction(restart_transaction.clone());

            bob_node.receive_message(NetworkMessage::Transaction(restart_transaction));

            println!("Transaction added after restart: {}", restart_tx_added);

            let restart_previous_block = alice_node
                .blockchain
                .chain
                .last()
                .expect("Blockchain is empty after restart")
                .clone();

            let restart_validator_address = alice_node
                .select_validator(&restart_previous_block.hash)
                .expect("Validator could not be selected after restart");

            let restart_validator_wallet = if restart_validator_address == alice_wallet.address() {
                &alice_wallet
            } else {
                &bob_wallet
            };

            let restart_timestamp = restart_previous_block
                .timestamp
                .checked_add(100)
                .expect("Restart block timestamp overflow");

            let restart_block = alice_node
                .produce_block(restart_timestamp, restart_validator_wallet)
                .expect("New block could not be produced after restart");

            let restart_block_index = restart_block.index;

            bob_node.receive_message(NetworkMessage::Block(restart_block));

            let expected_block_count = saved_block_count
                .checked_add(1)
                .expect("Block count overflow");

            let continuation_ok = alice_node.blockchain.chain.len() == expected_block_count
                && bob_node.blockchain.chain.len() == expected_block_count
                && alice_node.blockchain.is_valid()
                && bob_node.blockchain.is_valid();

            println!(
                "Block index produced after restart: {}",
                restart_block_index
            );

            println!(
                "Chain continued from {} blocks to {} blocks after restart: {}",
                saved_block_count, expected_block_count, continuation_ok
            );

            Storage::save_blockchain(&alice_node.blockchain.chain)
                .expect("Blockchain could not be saved to disk after restart");

            println!(" Resumed blockchain saved to disk again.");

            return;
        }

        Ok(None) => {
            println!(" Stored blockchain was not found. Starting a new chain from genesis.");
        }

        Err(error) => {
            panic!("Blockchain file could not be loaded safely: {}", error);
        }
    }

    // ==========================
    // PEER NETWORK
    // ==========================

    alice_node.add_peer(String::from("BOB_NODE"));

    bob_node.add_peer(String::from("ALICE_NODE"));

    println!();

    println!("Alice Node peer count: {}", alice_node.peer_count());

    println!("Bob Node peer count: {}", bob_node.peer_count());

    // ==========================
    // SYNC TEST
    // ==========================

    alice_node.network.broadcast(NetworkMessage::SyncRequest);

    bob_node.receive_message(NetworkMessage::SyncRequest);

    alice_node.sync_network();

    // ==========================
    // FIRST VALIDATOR SELECTION
    // ==========================

    let latest_hash = alice_node.blockchain.chain.last().unwrap().hash.clone();

    let selected_validator_address = match alice_node.select_validator(&latest_hash) {
        Some(address) => {
            println!();

            println!("Selected validator: {}", address);

            address
        }

        None => {
            println!("Validator could not be selected.");

            return;
        }
    };

    let selected_validator_wallet = if selected_validator_address == alice_wallet.address() {
        &alice_wallet
    } else {
        &bob_wallet
    };

    println!();

    println!(
        "Alice initial balance: {} KBN",
        alice_node.state.balance_of(alice_wallet.address(),) / 1_000_000
    );

    println!(
        "Bob initial balance: {} KBN",
        alice_node.state.balance_of(bob_wallet.address(),) / 1_000_000
    );

    println!(
        "Genesis total supply: {} KBN",
        alice_node.blockchain.economy.supply() / 1_000_000
    );

    // ==========================
    // FIRST TRANSACTION
    // ==========================

    let first_amount = 50_000_000;

    let first_fee = alice_node.blockchain.economy.calculate_fee(first_amount);

    println!("First transaction fee: {} microKBN", first_fee);

    let mut transaction = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        first_amount,
        first_fee,
        0,
    );

    let signature = alice_wallet.sign(&transaction.message());

    transaction.sign(signature);

    let added = alice_node.add_transaction(transaction.clone());

    println!();

    println!("Added to Alice Node mempool: {}", added);

    println!("Alice mempool: {}", alice_node.mempool.len());

    // ==========================
    // SEND TRANSACTION TO BOB
    // ==========================

    bob_node.receive_message(NetworkMessage::Transaction(transaction));

    println!("Bob mempool: {}", bob_node.mempool.len());

    // ==========================
    // FIRST BLOCK
    // ==========================

    let block_created = alice_node.produce_block(1754690100, selected_validator_wallet);

    match block_created {
        Ok(block) => {
            println!(" First block created.");

            println!(" Block is being sent to Bob Node...");

            bob_node.receive_message(NetworkMessage::Block(block));
        }

        Err(error) => {
            println!(" Block creation error: {}", error);

            return;
        }
    }

    // ==========================
    // SECOND TRANSACTION
    // ==========================

    println!();

    println!("========== SECOND BLOCK TEST ==========");

    let second_amount = 10_000_000;

    let second_fee = alice_node.blockchain.economy.calculate_fee(second_amount);

    println!("Second transaction fee: {} microKBN", second_fee);

    let mut second_transaction = Transaction::new(
        bob_wallet.address().to_string(),
        bob_wallet.public_key_hex(),
        alice_wallet.address().to_string(),
        second_amount,
        second_fee,
        0,
    );

    let second_signature = bob_wallet.sign(&second_transaction.message());

    second_transaction.sign(second_signature);

    let added_second = alice_node.add_transaction(second_transaction.clone());

    println!("Second transaction added: {}", added_second);

    bob_node.receive_message(NetworkMessage::Transaction(second_transaction));

    // ==========================
    // SECOND VALIDATOR SELECTION
    // ==========================

    let second_latest_hash = alice_node.blockchain.chain.last().unwrap().hash.clone();

    let second_validator_address = match alice_node.select_validator(&second_latest_hash) {
        Some(address) => {
            println!("Second block validator: {}", address);

            address
        }

        None => {
            println!("Second validator could not be selected.");

            return;
        }
    };

    let second_validator_wallet = if second_validator_address == alice_wallet.address() {
        &alice_wallet
    } else {
        &bob_wallet
    };

    // ==========================
    // SECOND BLOCK
    // ==========================

    let block_two = alice_node.produce_block(1754690200, second_validator_wallet);

    match block_two {
        Ok(block) => {
            println!(" Second block created.");

            println!(" Second block is being sent to Bob Node...");

            bob_node.receive_message(NetworkMessage::Block(block));
        }

        Err(error) => {
            println!(" Second block could not be produced: {}", error);
        }
    }

    // ==========================
    // NONCE QUEUE TEST
    // ==========================

    println!();

    println!("========== NONCE QUEUE TEST ==========");

    // Alice first transaction used nonce 0
    // and was already processed in block 1.
    // Therefore the next nonce values are:
    // 1, 2, and 3.

    let queue_amount_1 = 1_000_000;

    let queue_fee_1 = alice_node.blockchain.economy.calculate_fee(queue_amount_1);

    let mut queue_transaction_1 = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        queue_amount_1,
        queue_fee_1,
        1,
    );

    let queue_signature_1 = alice_wallet.sign(&queue_transaction_1.message());

    queue_transaction_1.sign(queue_signature_1);

    let queue_added_1 = alice_node.add_transaction(queue_transaction_1.clone());

    println!("Nonce 1 transaction added: {}", queue_added_1);

    bob_node.receive_message(NetworkMessage::Transaction(queue_transaction_1));

    let queue_amount_2 = 2_000_000;

    let queue_fee_2 = alice_node.blockchain.economy.calculate_fee(queue_amount_2);

    let mut queue_transaction_2 = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        queue_amount_2,
        queue_fee_2,
        2,
    );

    let queue_signature_2 = alice_wallet.sign(&queue_transaction_2.message());

    queue_transaction_2.sign(queue_signature_2);

    let queue_added_2 = alice_node.add_transaction(queue_transaction_2.clone());

    println!("Nonce 2 transaction added: {}", queue_added_2);

    bob_node.receive_message(NetworkMessage::Transaction(queue_transaction_2));

    let queue_amount_3 = 3_000_000;

    let queue_fee_3 = alice_node.blockchain.economy.calculate_fee(queue_amount_3);

    let mut queue_transaction_3 = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        queue_amount_3,
        queue_fee_3,
        3,
    );

    let queue_signature_3 = alice_wallet.sign(&queue_transaction_3.message());

    queue_transaction_3.sign(queue_signature_3);

    let queue_added_3 = alice_node.add_transaction(queue_transaction_3.clone());

    println!("Nonce 3 transaction added: {}", queue_added_3);

    bob_node.receive_message(NetworkMessage::Transaction(queue_transaction_3));

    println!("Alice nonce test mempool: {}", alice_node.mempool.len());

    println!("Bob nonce test mempool: {}", bob_node.mempool.len());

    // ==========================
    // THIRD VALIDATOR SELECTION
    // ==========================

    let third_latest_hash = alice_node.blockchain.chain.last().unwrap().hash.clone();

    let third_validator_address = match alice_node.select_validator(&third_latest_hash) {
        Some(address) => {
            println!("Third block validator: {}", address);

            address
        }

        None => {
            println!("Third validator could not be selected.");

            return;
        }
    };

    let third_validator_wallet = if third_validator_address == alice_wallet.address() {
        &alice_wallet
    } else {
        &bob_wallet
    };

    // ==========================
    // THIRD BLOCK
    // ==========================

    let block_three = alice_node.produce_block(1754690300, third_validator_wallet);

    match block_three {
        Ok(block) => {
            println!(" Third block created.");

            let normal_transaction_count = block
                .transactions
                .iter()
                .filter(|transaction| !transaction.coinbase)
                .count();

            println!(
                "Third block normal transaction count: {}",
                normal_transaction_count
            );

            println!(" Third block is being sent to Bob Node...");

            bob_node.receive_message(NetworkMessage::Block(block));

            println!("Alice mempool after block: {}", alice_node.mempool.len());

            println!("Bob mempool after block: {}", bob_node.mempool.len());
        }

        Err(error) => {
            println!(" Third block could not be produced: {}", error);
        }
    }

    println!();

    println!("Alice chain length: {}", alice_node.blockchain.height());

    println!("Bob chain length: {}", bob_node.blockchain.height());

    // ==========================
    // CHAIN SYNC TEST
    // ==========================

    println!();

    println!(" Bob is requesting the current chain...");

    bob_node.request_chain();

    let alice_chain = alice_node.blockchain.chain.clone();

    bob_node.receive_message(NetworkMessage::ChainChunkResponse {
        start_index: 0,
        total_blocks: alice_chain.len() as u64,
        blocks: alice_chain,
    });

    // ==========================
    // EQUAL-LENGTH FAKE FORK TEST
    // ==========================

    println!();

    println!("========== FAKE FORK TEST ==========");

    let bob_tip_before_fork = bob_node.blockchain.chain.last().unwrap().hash.clone();

    let mut fake_fork_chain = alice_node.blockchain.chain.clone();

    if let Some(fake_tip) = fake_fork_chain.last_mut() {
        fake_tip.timestamp = fake_tip
            .timestamp
            .checked_add(1)
            .expect("Fake fork timestamp overflow");

        fake_tip.hash = fake_tip.calculate_hash();
    }

    let fake_tip_hash = fake_fork_chain.last().unwrap().hash.clone();

    println!("Bob current tip hash: {}", bob_tip_before_fork);

    println!("Fake fork tip hash: {}", fake_tip_hash);

    bob_node.receive_message(NetworkMessage::ChainChunkResponse {
        start_index: 0,
        total_blocks: fake_fork_chain.len() as u64,
        blocks: fake_fork_chain,
    });

    let bob_tip_after_fork = bob_node.blockchain.chain.last().unwrap().hash.clone();

    println!("Bob tip hash after fork: {}", bob_tip_after_fork);

    println!(
        "Fake fork rejected: {}",
        bob_tip_before_fork == bob_tip_after_fork
    );

    // ==========================
    // DUPLICATE TRANSACTION ID TEST
    // ==========================

    println!();

    println!("========== DUPLICATE TRANSACTION ID TEST ==========");

    let bob_chain_len_before_duplicate_attack = bob_node.blockchain.chain.len();

    let bob_tip_before_duplicate_attack = bob_node.blockchain.chain.last().unwrap().clone();

    let mut duplicate_transaction_block = bob_tip_before_duplicate_attack.clone();

    duplicate_transaction_block.index = bob_tip_before_duplicate_attack
        .index
        .checked_add(1)
        .expect("Duplicate transaction block index overflow");

    duplicate_transaction_block.previous_hash = bob_tip_before_duplicate_attack.hash.clone();

    duplicate_transaction_block.timestamp = bob_tip_before_duplicate_attack
        .timestamp
        .checked_add(1)
        .expect("Duplicate transaction block timestamp overflow");

    duplicate_transaction_block.hash = duplicate_transaction_block.calculate_hash();

    bob_node.receive_message(NetworkMessage::Block(duplicate_transaction_block));

    let bob_chain_len_after_duplicate_attack = bob_node.blockchain.chain.len();

    println!(
        "Reuse of the same transaction ID rejected: {}",
        bob_chain_len_before_duplicate_attack == bob_chain_len_after_duplicate_attack
    );

    // ==========================
    // TIMESTAMP ATTACK TEST
    // ==========================

    println!();

    println!("========== TIMESTAMP ATTACK TEST ==========");

    let bob_chain_len_before_timestamp_attack = bob_node.blockchain.chain.len();

    let bob_tip_before_timestamp_attack = bob_node.blockchain.chain.last().unwrap().clone();

    let mut fake_timestamp_block = bob_tip_before_timestamp_attack.clone();

    fake_timestamp_block.index = bob_tip_before_timestamp_attack
        .index
        .checked_add(1)
        .expect("Fake timestamp block index overflow");

    fake_timestamp_block.previous_hash = bob_tip_before_timestamp_attack.hash.clone();

    // Intentionally uses the SAME timestamp as the previous block.
    // The new rule should reject this.
    fake_timestamp_block.timestamp = bob_tip_before_timestamp_attack.timestamp;

    // This test isolates the timestamp rule.
    // Clear transactions cloned from the previous block.
    fake_timestamp_block.transactions.clear();

    fake_timestamp_block.hash = fake_timestamp_block.calculate_hash();

    println!(
        "Previous block timestamp: {}",
        bob_tip_before_timestamp_attack.timestamp
    );

    println!("Fake block timestamp: {}", fake_timestamp_block.timestamp);

    bob_node.receive_message(NetworkMessage::Block(fake_timestamp_block));

    let bob_chain_len_after_timestamp_attack = bob_node.blockchain.chain.len();

    let timestamp_attack_rejected =
        bob_chain_len_before_timestamp_attack == bob_chain_len_after_timestamp_attack;

    println!("Timestamp attack rejected: {}", timestamp_attack_rejected);

    // ==========================
    // FUTURE TIMESTAMP ATTACK TEST
    // ==========================

    println!();

    println!("========== FUTURE TIMESTAMP ATTACK TEST ==========");

    let bob_chain_len_before_future_attack = bob_node.blockchain.chain.len();

    let bob_tip_before_future_attack = bob_node.blockchain.chain.last().unwrap().clone();

    let mut fake_future_block = bob_tip_before_future_attack.clone();

    fake_future_block.index = bob_tip_before_future_attack
        .index
        .checked_add(1)
        .expect("Future block index overflow");

    fake_future_block.previous_hash = bob_tip_before_future_attack.hash.clone();

    // Intentionally uses a timestamp far in the future.
    fake_future_block.timestamp = 4_000_000_000;

    // This test isolates the future timestamp rule.
    fake_future_block.transactions.clear();

    fake_future_block.hash = fake_future_block.calculate_hash();

    println!("Fake future timestamp: {}", fake_future_block.timestamp);

    bob_node.receive_message(NetworkMessage::Block(fake_future_block));

    let bob_chain_len_after_future_attack = bob_node.blockchain.chain.len();

    println!(
        "Future timestamp attack rejected: {}",
        bob_chain_len_before_future_attack == bob_chain_len_after_future_attack
    );

    // ==========================
    // BLOCK TRANSACTION LIMIT TEST
    // ==========================

    println!();

    println!("========== BLOCK TRANSACTION LIMIT TEST ==========");

    alice_node.mempool.transactions.clear();

    let test_start_nonce = alice_node.state.nonce_of(alice_wallet.address());

    let limit_test_amount = 1u64;

    let limit_test_fee = alice_node
        .blockchain
        .economy
        .calculate_fee(limit_test_amount);

    for offset in 0..1001u64 {
        let nonce = test_start_nonce
            .checked_add(offset)
            .expect("Limit test nonce overflow");

        let mut limit_transaction = Transaction::new(
            alice_wallet.address().to_string(),
            alice_wallet.public_key_hex(),
            bob_wallet.address().to_string(),
            limit_test_amount,
            limit_test_fee,
            nonce,
        );

        let limit_signature = alice_wallet.sign(&limit_transaction.message());

        limit_transaction.sign(limit_signature);

        alice_node.mempool.transactions.push(limit_transaction);
    }

    let selected_transactions = alice_node
        .mempool
        .take_valid_transactions(&alice_node.state);

    println!(
        "Normal transactions selected for block: {}",
        selected_transactions.len()
    );

    println!(
        "Transactions remaining in mempool: {}",
        alice_node.mempool.len()
    );

    println!(
        "1000-transaction block limit works: {}",
        selected_transactions.len() == 1000 && alice_node.mempool.len() == 1
    );

    alice_node.mempool.transactions.clear();

    // ==========================
    // MEMPOOL CAPACITY TEST
    // ==========================

    println!();

    println!("========== MEMPOOL CAPACITY TEST ==========");

    let mut capacity_probe = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        1,
        limit_test_fee,
        test_start_nonce,
    );

    let capacity_signature = alice_wallet.sign(&capacity_probe.message());

    capacity_probe.sign(capacity_signature);

    alice_node.mempool.transactions = vec![capacity_probe.clone(); 10_000];

    let accepted_over_capacity = alice_node.mempool.add_transaction(capacity_probe);

    println!("Current mempool transactions: {}", alice_node.mempool.len());

    println!(
        "10001st transaction rejected: {}",
        !accepted_over_capacity && alice_node.mempool.len() == 10_000
    );

    alice_node.mempool.transactions.clear();

    // ==========================
    // EXTERNAL OVERSIZED BLOCK TEST
    // ==========================

    println!();

    println!("========== EXTERNAL OVERSIZED BLOCK TEST ==========");

    let bob_chain_len_before_oversized_block = bob_node.blockchain.chain.len();

    let bob_tip_for_oversized_block = bob_node.blockchain.chain.last().unwrap().clone();

    let mut oversized_block = bob_tip_for_oversized_block.clone();

    oversized_block.index = bob_tip_for_oversized_block
        .index
        .checked_add(1)
        .expect("Oversized block index overflow");

    oversized_block.previous_hash = bob_tip_for_oversized_block.hash.clone();

    oversized_block.timestamp = bob_tip_for_oversized_block
        .timestamp
        .checked_add(1)
        .expect("Oversized block timestamp overflow");

    let filler_transaction = bob_tip_for_oversized_block
        .transactions
        .iter()
        .find(|transaction| !transaction.coinbase)
        .expect("Normal transaction for oversized block test was not found")
        .clone();

    while oversized_block.transactions.len() < 1002 {
        oversized_block
            .transactions
            .push(filler_transaction.clone());
    }

    oversized_block.hash = oversized_block.calculate_hash();

    println!(
        "Fake block transaction count: {}",
        oversized_block.transactions.len()
    );

    bob_node.receive_message(NetworkMessage::Block(oversized_block));

    let bob_chain_len_after_oversized_block = bob_node.blockchain.chain.len();

    println!(
        "Fake block with 1002 transactions rejected: {}",
        bob_chain_len_before_oversized_block == bob_chain_len_after_oversized_block
    );

    // ==========================
    // OVERSIZED TRANSACTION FIELD TEST
    // ==========================

    println!();

    println!("========== OVERSIZED TRANSACTION FIELD TEST ==========");

    let bob_mempool_before_large_field = bob_node.mempool.len();

    let large_field_fee = bob_node.blockchain.economy.calculate_fee(1_000_000);

    let mut large_field_transaction = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        1_000_000,
        large_field_fee,
        alice_node.state.nonce_of(alice_wallet.address()),
    );

    let large_field_signature = alice_wallet.sign(&large_field_transaction.message());

    large_field_transaction.sign(large_field_signature);

    // Intentionally exceed the 128-character limit.
    large_field_transaction.to = "X".repeat(129);

    println!(
        "Fake recipient address length: {}",
        large_field_transaction.to.len()
    );

    bob_node.receive_message(NetworkMessage::Transaction(large_field_transaction));

    let bob_mempool_after_large_field = bob_node.mempool.len();

    println!(
        "Oversized transaction field rejected: {}",
        bob_mempool_before_large_field == bob_mempool_after_large_field
    );

    // ==========================
    // OVERSIZED BLOCK FIELD TEST
    // ==========================

    println!();

    println!("========== OVERSIZED BLOCK FIELD TEST ==========");

    let bob_chain_len_before_large_block_field = bob_node.blockchain.chain.len();

    let bob_tip_for_large_block_field = bob_node.blockchain.chain.last().unwrap().clone();

    let mut large_block_field = bob_tip_for_large_block_field.clone();

    large_block_field.index = bob_tip_for_large_block_field
        .index
        .checked_add(1)
        .expect("Large block field index overflow");

    large_block_field.previous_hash = bob_tip_for_large_block_field.hash.clone();

    large_block_field.timestamp = bob_tip_for_large_block_field
        .timestamp
        .checked_add(1)
        .expect("Large block field timestamp overflow");

    // Intentionally exceed the 128-character limit.
    large_block_field.validator = "V".repeat(129);

    println!(
        "Fake validator address length: {}",
        large_block_field.validator.len()
    );

    bob_node.receive_message(NetworkMessage::Block(large_block_field));

    let bob_chain_len_after_large_block_field = bob_node.blockchain.chain.len();

    println!(
        "Oversized block field rejected: {}",
        bob_chain_len_before_large_block_field == bob_chain_len_after_large_block_field
    );

    // ==========================
    // DIRECT BLOCKCHAIN LAYER TEST
    // ==========================

    println!();

    println!("========== DIRECT BLOCKCHAIN LAYER TEST ==========");

    let bob_chain_len_before_direct_test = bob_node.blockchain.chain.len();

    let bob_tip_for_direct_test = bob_node.blockchain.chain.last().unwrap().clone();

    let mut direct_large_field_block = bob_tip_for_direct_test.clone();

    direct_large_field_block.index = bob_tip_for_direct_test
        .index
        .checked_add(1)
        .expect("Direct blockchain test index overflow");

    direct_large_field_block.previous_hash = bob_tip_for_direct_test.hash.clone();

    direct_large_field_block.timestamp = bob_tip_for_direct_test
        .timestamp
        .checked_add(1)
        .expect("Direct blockchain test timestamp overflow");

    direct_large_field_block.validator = "V".repeat(129);

    let direct_blockchain_result = bob_node
        .blockchain
        .add_received_block(direct_large_field_block);

    match &direct_blockchain_result {
        Ok(()) => {
            println!(" Blockchain layer accepted the fake block");
        }

        Err(error) => {
            println!(" Blockchain layer rejected: {}", error);
        }
    }

    let bob_chain_len_after_direct_test = bob_node.blockchain.chain.len();

    println!(
        "Protection works when Node is bypassed: {}",
        direct_blockchain_result.is_err()
            && bob_chain_len_before_direct_test == bob_chain_len_after_direct_test
    );

    // ==========================
    // SHORT / INVALID ADDRESS FORMAT TEST
    // ==========================

    println!();

    println!("========== SHORT / INVALID ADDRESS FORMAT TEST ==========");

    let bob_mempool_before_short_address = bob_node.mempool.len();

    let short_address_fee = bob_node.blockchain.economy.calculate_fee(1_000_000);

    let mut short_address_transaction = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        1_000_000,
        short_address_fee,
        alice_node.state.nonce_of(alice_wallet.address()),
    );

    let short_address_signature = alice_wallet.sign(&short_address_transaction.message());

    short_address_transaction.sign(short_address_signature);

    short_address_transaction.to = "1234567890abcdef1234".to_string();

    println!(
        "Fake short recipient address length: {}",
        short_address_transaction.to.len()
    );

    bob_node.receive_message(NetworkMessage::Transaction(short_address_transaction));

    let bob_mempool_after_short_address = bob_node.mempool.len();

    println!(
        "Short/invalid address rejected: {}",
        bob_mempool_before_short_address == bob_mempool_after_short_address
    );

    // ==========================
    // DIRECT MEMPOOL LAYER TEST
    // ==========================

    println!();

    println!("========== DIRECT MEMPOOL LAYER TEST ==========");

    let direct_mempool_before = bob_node.mempool.len();

    let direct_mempool_fee = bob_node.blockchain.economy.calculate_fee(1_000_000);

    let mut direct_mempool_transaction = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        1_000_000,
        direct_mempool_fee,
        alice_node.state.nonce_of(alice_wallet.address()),
    );

    let direct_mempool_signature = alice_wallet.sign(&direct_mempool_transaction.message());

    direct_mempool_transaction.sign(direct_mempool_signature);

    direct_mempool_transaction.to = "1234567890abcdef1234".to_string();

    let direct_mempool_result = bob_node.mempool.add_transaction(direct_mempool_transaction);

    let direct_mempool_after = bob_node.mempool.len();

    println!("Direct mempool accepted: {}", direct_mempool_result);

    println!(
        "Mempool protection works when Node is bypassed: {}",
        !direct_mempool_result && direct_mempool_before == direct_mempool_after
    );

    // ==========================
    // 64-CHARACTER NON-HEX ADDRESS TEST
    // ==========================

    println!();

    println!("========== 64-CHARACTER NON-HEX ADDRESS TEST ==========");

    let bob_mempool_before_non_hex = bob_node.mempool.len();

    let non_hex_fee = bob_node.blockchain.economy.calculate_fee(1_000_000);

    let mut non_hex_transaction = Transaction::new(
        alice_wallet.address().to_string(),
        alice_wallet.public_key_hex(),
        bob_wallet.address().to_string(),
        1_000_000,
        non_hex_fee,
        alice_node.state.nonce_of(alice_wallet.address()),
    );

    let non_hex_signature = alice_wallet.sign(&non_hex_transaction.message());

    non_hex_transaction.sign(non_hex_signature);

    non_hex_transaction.to = format!("{}Z", "a".repeat(63),);

    println!("Fake address length: {}", non_hex_transaction.to.len());

    bob_node.receive_message(NetworkMessage::Transaction(non_hex_transaction.clone()));

    let node_rejected_non_hex = bob_node.mempool.len() == bob_mempool_before_non_hex;

    println!("Node rejected non-hex address: {}", node_rejected_non_hex);

    let direct_mempool_result_non_hex = bob_node.mempool.add_transaction(non_hex_transaction);

    println!(
        "Mempool rejected non-hex address: {}",
        !direct_mempool_result_non_hex && bob_node.mempool.len() == bob_mempool_before_non_hex
    );

    // ==========================
    // 257 BLOCK CHUNK SYNC TEST
    // ==========================

    println!();

    println!("========== 257 BLOCK CHUNK SYNC TEST ==========");

    let oversized_chunk_block = bob_node
        .blockchain
        .chain
        .last()
        .expect("Bob blockchain is empty")
        .clone();

    let oversized_chunk = vec![oversized_chunk_block; 257];

    let chunk_network_message_count_before = bob_node.network.message_count();

    bob_node
        .network
        .receive(NetworkMessage::ChainChunkResponse {
            start_index: 0,
            total_blocks: 257,
            blocks: oversized_chunk.clone(),
        });

    let chunk_network_message_count_after = bob_node.network.message_count();

    println!(
        "Network rejected 257-block chunk message: {}",
        chunk_network_message_count_before == chunk_network_message_count_after
    );

    let bob_chain_len_before_chunk_attack = bob_node.blockchain.chain.len();

    bob_node.receive_message(NetworkMessage::ChainChunkResponse {
        start_index: 0,
        total_blocks: 257,
        blocks: oversized_chunk,
    });

    let bob_chain_len_after_chunk_attack = bob_node.blockchain.chain.len();

    println!(
        "Node rejected 257-block chunk message: {}",
        bob_chain_len_before_chunk_attack == bob_chain_len_after_chunk_attack
    );

    // ==========================
    // SON DURUM
    // ==========================

    println!();

    println!(
        "Alice Node blockchain block count: {}",
        alice_node.blockchain.chain.len()
    );

    println!(
        "Bob Node blockchain block count: {}",
        bob_node.blockchain.chain.len()
    );

    println!(
        "Alice final balance: {} KBN",
        alice_node.state.balance_of(alice_wallet.address(),) / 1_000_000
    );

    println!(
        "Bob final balance: {} KBN",
        alice_node.state.balance_of(bob_wallet.address(),) / 1_000_000
    );

    println!("Treasury: {} microKBN", alice_node.state.treasury());

    println!("Burned amount: {} microKBN", alice_node.state.burned());

    println!(
        "Alice blockchain valid: {}",
        alice_node.blockchain.is_valid()
    );

    println!("Bob blockchain valid: {}", bob_node.blockchain.is_valid());

    println!(
        "Total supply: {} KBN",
        alice_node.blockchain.economy.supply() / 1_000_000
    );

    // ==========================
    // BLOCKCHAIN PERSISTENCE
    // ==========================

    println!();

    match Storage::save_blockchain(&alice_node.blockchain.chain) {
        Ok(()) => {
            println!(
                " Blockchain saved to disk: {}",
                Storage::blockchain_path().display()
            );

            match Storage::load_blockchain() {
                Ok(Some(loaded_chain)) => {
                    let same_length = loaded_chain.len() == alice_node.blockchain.chain.len();

                    let same_tip = loaded_chain.last().map(|block| block.hash.as_str())
                        == alice_node
                            .blockchain
                            .chain
                            .last()
                            .map(|block| block.hash.as_str());

                    println!(
                        " Blockchain read back from disk. Block count: {}",
                        loaded_chain.len()
                    );

                    println!(
                        "Blockchain loaded from disk matches: {}",
                        same_length && same_tip
                    );
                }

                Ok(None) => {
                    println!(" Blockchain file was not found");
                }

                Err(error) => {
                    println!(" Blockchain from disk could not be read: {}", error);
                }
            }
        }

        Err(error) => {
            println!(" Blockchain could not be saved to disk: {}", error);
        }
    }
}
