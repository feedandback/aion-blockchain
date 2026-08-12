mod chain;
mod consensus;
mod core;
mod economy;
mod network;
mod node;
mod protocol;
mod state;
mod storage;
mod wallet;

use crate::chain::Blockchain;
use crate::consensus::Consensus;
use crate::core::{Block, Transaction};
use crate::network::tcp::TcpTransport;
use crate::network::{Network, NetworkMessage};
use crate::node::Node;
use crate::state::State;
use crate::storage::Storage;
use crate::wallet::Wallet;

#[tokio::main]
async fn main() {
    let arguments:
        Vec<String> =
        std::env::args()
            .collect();

    if arguments.len() >= 3
        && arguments[1] == "listen"
    {
        let listen_address =
            arguments[2].clone();

        let listener =
            TcpTransport::bind(
                &listen_address,
            )
            .await
            .expect(
                "TCP listener başlatılamadı",
            );

        println!(
            "AION peer dinleniyor: {}",
            listen_address
        );

        println!(
            "🌐 AION TCP listener aktif: {}",
            listen_address
        );

        let (
            mut stream,
            peer_address,
        ) =
            TcpTransport::accept_connection(
                &listener,
            )
            .await
            .expect(
                "TCP bağlantısı kabul edilemedi",
            );

        let message =
            TcpTransport::read_message(
                &mut stream,
            )
            .await
            .expect(
                "TCP mesajı okunamadı",
            );

        println!(
            "✅ Gerçek TCP mesajı alındı: {:?}",
            message
        );

        println!(
            "Peer: {}",
            peer_address
        );

        let handshake_challenge =
            Network::handshake_challenge(
                &message,
            )
            .unwrap_or("")
            .to_string();

        let mut test_network =
            Network::new();

        test_network.receive(
            message,
        );

        let accepted =
            test_network
                .identified_peer_count()
                == 1;

        println!(
            "✅ Tanımlanan peer sayısı: {}",
            test_network
                .identified_peer_count()
        );

        let listener_wallet =
            Wallet::new();

        let handshake_ack =
            Network::create_handshake_ack(
                &listener_wallet,
                accepted,
                handshake_challenge,
            );

        TcpTransport::send_message(
            &mut stream,
            &handshake_ack,
        )
        .await
        .expect(
            "HandshakeAck gönderilemedi",
        );

        println!(
            "✅ HandshakeAck gönderildi. Kabul: {}",
            accepted
        );

        return;
    }

    if arguments.len() >= 3
        && arguments[1]
            == "send-stale-handshake"
    {
        let peer_address =
            arguments[2].clone();

        let test_wallet =
            Wallet::new();

        let listen_address =
            "127.0.0.1:7005"
                .to_string();

        let protocol_version =
            Network::protocol_version();

        let timestamp =
            Network::current_timestamp()
                .saturating_sub(
                    crate::protocol::MAX_HANDSHAKE_AGE_SECONDS
                        + 1,
                );

        let public_key =
            test_wallet
                .public_key_hex();

        let challenge =
            Network::generate_handshake_challenge();

        let signature =
            test_wallet
                .sign_node_handshake(
                    &listen_address,
                    crate::protocol::NETWORK_ID,
                    protocol_version,
                    timestamp,
                    &challenge,
                );

        let stale_handshake =
            NetworkMessage::Handshake {
                node_id:
                    test_wallet
                        .node_id()
                        .to_string(),
                public_key,
                listen_address,
                network_id:
                    crate::protocol::NETWORK_ID
                        .to_string(),
                protocol_version,
                timestamp,
                challenge,
                signature,
            };

        let response =
            TcpTransport::send_and_receive(
                &peer_address,
                &stale_handshake,
            )
            .await
            .expect(
                "Eski handshake testi başarısız",
            );

        println!(
            "✅ Eski timestamp handshake gönderildi: {}",
            timestamp
        );

        println!(
            "Handshake cevabı: {:?}",
            response
        );

        match response {
            NetworkMessage::HandshakeAck {
                accepted,
                ..
            } => {
                println!(
                    "✅ Eski handshake reddedildi mi: {}",
                    !accepted
                );
            }

            _ => {
                panic!(
                    "Beklenen HandshakeAck alınmadı"
                );
            }
        }

        return;
    }

    if arguments.len() >= 3
        && arguments[1]
            == "send-wrong-version"
    {
        let peer_address =
            arguments[2].clone();

        let test_wallet =
            Wallet::new();

        let listen_address =
            "127.0.0.1:7004"
                .to_string();

        let protocol_version =
            Network::protocol_version()
                + 1;

        let public_key =
            test_wallet
                .public_key_hex();

        let timestamp =
            Network::current_timestamp();

        let challenge =
            Network::generate_handshake_challenge();

        let signature =
            test_wallet
                .sign_node_handshake(
                    &listen_address,
                    crate::protocol::NETWORK_ID,
                    protocol_version,
                    timestamp,
                    &challenge,
                );

        let wrong_version_handshake =
            NetworkMessage::Handshake {
                node_id:
                    test_wallet
                        .node_id()
                        .to_string(),
                public_key,
                listen_address,
                network_id:
                    crate::protocol::NETWORK_ID
                        .to_string(),
                protocol_version,
                timestamp,
                challenge,
                signature,
            };

        TcpTransport::send_to(
            &peer_address,
            &wrong_version_handshake,
        )
        .await
        .expect(
            "Yanlış protokol sürümü handshake gönderilemedi",
        );

        println!(
            "✅ Yanlış protokol sürümü handshake test mesajı gönderildi: {}",
            peer_address
        );

        return;
    }

    if arguments.len() >= 3
        && arguments[1]
            == "send-wrong-network"
    {
        let peer_address =
            arguments[2].clone();

        let test_wallet =
            Wallet::new();

        let listen_address =
            "127.0.0.1:7003"
                .to_string();

        let wrong_network_id =
            "wrong-aion-network"
                .to_string();

        let protocol_version =
            Network::protocol_version();

        let public_key =
            test_wallet
                .public_key_hex();

        let timestamp =
            Network::current_timestamp();

        let challenge =
            Network::generate_handshake_challenge();

        let signature =
            test_wallet
                .sign_node_handshake(
                    &listen_address,
                    &wrong_network_id,
                    protocol_version,
                    timestamp,
                    &challenge,
                );

        let wrong_handshake =
            NetworkMessage::Handshake {
                node_id:
                    test_wallet
                        .node_id()
                        .to_string(),
                public_key,
                listen_address,
                network_id:
                    wrong_network_id,
                protocol_version,
                timestamp,
                challenge,
                signature,
            };

        TcpTransport::send_to(
            &peer_address,
            &wrong_handshake,
        )
        .await
        .expect(
            "Yanlış network handshake gönderilemedi",
        );

        println!(
            "✅ Yanlış Network ID handshake test mesajı gönderildi: {}",
            peer_address
        );

        return;
    }

    if arguments.len() >= 3
        && arguments[1] == "send"
    {
        let peer_address =
            arguments[2].clone();

        let test_wallet =
            Wallet::new();

        let handshake =
            Network::create_handshake(
                &test_wallet,
                "127.0.0.1:7002"
                    .to_string(),
            );

        let expected_challenge =
            Network::handshake_challenge(
                &handshake,
            )
            .expect(
                "Handshake challenge oluşturulamadı",
            )
            .to_string();

        let response =
            TcpTransport::send_and_receive(
                &peer_address,
                &handshake,
            )
            .await
            .expect(
                "Gerçek TCP handshake veya ACK işlemi başarısız",
            );

        println!(
            "✅ Handshake gerçek TCP üzerinden gönderildi: {}",
            peer_address
        );

        println!(
            "✅ Handshake cevabı alındı: {:?}",
            response
        );

        let handshake_ack_valid =
            Network::validate_handshake_ack(
                &response,
                &expected_challenge,
            );

        println!(
            "✅ HandshakeAck doğrulandı mı: {}",
            handshake_ack_valid
        );

        if !handshake_ack_valid {
            panic!(
                "HandshakeAck geçersiz"
            );
        }

        return;
    }

    let wallet_password =
        std::env::var(
            "AION_WALLET_PASSWORD",
        )
        .expect(
            "AION_WALLET_PASSWORD ortam değişkeni tanımlı değil",
        );

    // ==========================
    // GERÇEK TCP LOOPBACK TESTİ
    // ==========================

    let tcp_listener =
        TcpTransport::bind(
            "127.0.0.1:0",
        )
        .await
        .expect(
            "TCP test listener başlatılamadı",
        );

    let tcp_address =
        tcp_listener
            .local_addr()
            .expect(
                "TCP test adresi alınamadı",
            )
            .to_string();

    let tcp_server =
        tokio::spawn(
            async move {
                TcpTransport::accept_one(
                    &tcp_listener,
                )
                .await
            },
        );

    TcpTransport::send_to(
        &tcp_address,
        &NetworkMessage::SyncRequest,
    )
    .await
    .expect(
        "TCP test mesajı gönderilemedi",
    );

    let (
        tcp_received_message,
        tcp_peer,
    ) =
        tcp_server
            .await
            .expect(
                "TCP test görevi tamamlanamadı",
            )
            .expect(
                "TCP test mesajı alınamadı",
            );

    let tcp_loopback_ok =
        matches!(
            tcp_received_message,
            NetworkMessage::SyncRequest
        );

    println!(
        "🌐 TCP P2P loopback mesaj testi başarılı mı: {}",
        tcp_loopback_ok
    );

    println!(
        "TCP test peer: {}",
        tcp_peer
    );

    // ==========================
    // KALICI WALLET YÜKLEME
    // ==========================

    let (
        alice_wallet,
        bob_wallet,
    ) = match Storage::load_wallet_private_keys(
        &wallet_password,
    ) {
        Ok(Some((
            alice_private_key,
            bob_private_key,
        ))) => {
            let alice_wallet =
                Wallet::from_private_key_hex(
                    &alice_private_key,
                )
                .expect(
                    "Diskteki Alice private key geçersiz",
                );

            let bob_wallet =
                Wallet::from_private_key_hex(
                    &bob_private_key,
                )
                .expect(
                    "Diskteki Bob private key geçersiz",
                );

            println!(
                "🔐 Wallet anahtarları diskten yüklendi."
            );

            (
                alice_wallet,
                bob_wallet,
            )
        }

        Ok(None) => {
            let alice_wallet =
                Wallet::new();

            let bob_wallet =
                Wallet::new();

            Storage::save_wallet_private_keys(
                &wallet_password,
                &alice_wallet
                    .private_key_hex(),
                &bob_wallet
                    .private_key_hex(),
            )
            .expect(
                "Wallet private key'leri diske kaydedilemedi",
            );

            println!(
                "🔐 Yeni wallet anahtarları oluşturuldu ve diske kaydedildi: {}",
                Storage::wallets_path()
                    .display()
            );

            (
                alice_wallet,
                bob_wallet,
            )
        }

        Err(error) => {
            panic!(
                "Wallet dosyası güvenli şekilde yüklenemedi: {}",
                error
            );
        }
    };

    // ==========================
    // WALLET PRIVATE KEY GERİ YÜKLEME TESTİ
    // ==========================

    let alice_private_key =
        alice_wallet
            .private_key_hex();

    let restored_alice_wallet =
        Wallet::from_private_key_hex(
            &alice_private_key,
        )
        .expect(
            "Alice wallet private key'den geri yüklenemedi",
        );

    println!(
        "Wallet private key'den aynı adres geri yüklendi mi: {}",
        restored_alice_wallet.address()
            == alice_wallet.address()
    );

    let node_identity_timestamp =
        Network::current_timestamp();

    let node_identity_challenge =
        Network::generate_handshake_challenge();

    let node_identity_signature =
        alice_wallet
            .sign_node_handshake(
                "127.0.0.1:7002",
                crate::protocol::NETWORK_ID,
                Network::protocol_version(),
                node_identity_timestamp,
                &node_identity_challenge,
            );

    let signed_node_identity_ok =
        Wallet::verify_node_handshake(
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
        "🔐 İmzalı node kimliği doğrulandı mı: {}",
        signed_node_identity_ok
    );

    if !signed_node_identity_ok {
        panic!(
            "İmzalı node kimliği doğrulaması başarısız"
        );
    }

    // ==========================
    // GENESIS SUPPLY
    // ==========================

    let genesis_supply = 1000 * 1_000_000;

    // ==========================
    // CONSENSUS
    // ==========================

    let mut consensus = Consensus::new();

    consensus.add_validator(
        alice_wallet.address().to_string(),
        700,
    );

    consensus.add_validator(
        bob_wallet.address().to_string(),
        300,
    );

    println!(
        "Validator sayısı: {}",
        consensus.validator_count()
    );

    println!(
        "Toplam stake: {} AION",
        consensus.total_stake()
    );

    // ==========================
    // ALICE STATE
    // ==========================

    let mut state = State::new();

    state.create_account(
        alice_wallet.address().to_string(),
        genesis_supply,
    );

    state.create_account(
        bob_wallet.address().to_string(),
        0,
    );

    // ==========================
    // GENESIS
    // ==========================

    let genesis = Block::new(
        0,
        1754690000,
        String::from("0"),
        String::from("GENESIS"),
        String::new(),
        Vec::new(),
    );

    let mut blockchain =
        Blockchain::new(genesis);

    blockchain
        .economy
        .mint(genesis_supply)
        .expect(
            "Genesis arzı oluşturulamadı",
        );

    // ==========================
    // ALICE NODE
    // ==========================

    let mut alice_node =
        Node::new(
            blockchain,
            state,
            consensus,
        );

    // ==========================
    // BOB NODE
    // ==========================

    let mut bob_consensus =
        Consensus::new();

    bob_consensus.add_validator(
        alice_wallet.address().to_string(),
        700,
    );

    bob_consensus.add_validator(
        bob_wallet.address().to_string(),
        300,
    );

    let mut bob_state =
        State::new();

    bob_state.create_account(
        alice_wallet.address().to_string(),
        genesis_supply,
    );

    bob_state.create_account(
        bob_wallet.address().to_string(),
        0,
    );

    let bob_genesis = Block::new(
        0,
        1754690000,
        String::from("0"),
        String::from("GENESIS"),
        String::new(),
        Vec::new(),
    );

    let mut bob_blockchain =
        Blockchain::new(
            bob_genesis,
        );

    bob_blockchain
        .economy
        .mint(genesis_supply)
        .expect(
            "Bob genesis arzı oluşturulamadı",
        );

    let mut bob_node =
        Node::new(
            bob_blockchain,
            bob_state,
            bob_consensus,
        );

    // ==========================
    // BLOCKCHAIN RESTART RESTORE
    // ==========================

    match Storage::load_blockchain() {
        Ok(Some(saved_chain)) => {
            let saved_block_count =
                saved_chain.len();

            let saved_tip_hash =
                saved_chain
                    .last()
                    .map(
                        |block| {
                            block.hash.clone()
                        },
                    )
                    .expect(
                        "Diskteki blockchain boş",
                    );

            alice_node
                .restore_chain_from_storage(
                    saved_chain.clone(),
                )
                .expect(
                    "Alice Node blockchain'i diskten geri yükleyemedi",
                );

            bob_node
                .restore_chain_from_storage(
                    saved_chain,
                )
                .expect(
                    "Bob Node blockchain'i diskten geri yükleyemedi",
                );

            println!(
                "📂 Blockchain açılışta diskten yüklendi."
            );

            println!(
                "Yüklenen blok sayısı: {}",
                saved_block_count
            );

            println!(
                "Tip hash aynı mı: {}",
                alice_node
                    .blockchain
                    .chain
                    .last()
                    .map(
                        |block| {
                            block.hash.as_str()
                        },
                    )
                    == Some(
                        saved_tip_hash
                            .as_str(),
                    )
            );

            println!(
                "Alice yüklenen bakiye: {} AION",
                alice_node
                    .state
                    .balance_of(
                        alice_wallet.address(),
                    )
                    / 1_000_000
            );

            println!(
                "Bob yüklenen bakiye: {} AION",
                alice_node
                    .state
                    .balance_of(
                        bob_wallet.address(),
                    )
                    / 1_000_000
            );

            println!(
                "Yüklenen toplam arz: {} AION",
                alice_node
                    .blockchain
                    .economy
                    .supply()
                    / 1_000_000
            );

            println!(
                "Alice Blockchain geçerli mi: {}",
                alice_node
                    .blockchain
                    .is_valid()
            );

            println!(
                "Bob Blockchain geçerli mi: {}",
                bob_node
                    .blockchain
                    .is_valid()
            );

            println!(
                "✅ Node restart sonrası blockchain başarıyla devam ettirildi."
            );

            // ==========================
            // RESTART SONRASI YENİ BLOCK
            // ==========================

            let restart_amount =
                1_000_000;

            let restart_fee =
                alice_node
                    .blockchain
                    .economy
                    .calculate_fee(
                        restart_amount,
                    );

            let restart_nonce =
                alice_node
                    .state
                    .nonce_of(
                        alice_wallet.address(),
                    );

            let mut restart_transaction =
                Transaction::new(
                    alice_wallet
                        .address()
                        .to_string(),
                    alice_wallet
                        .public_key_hex(),
                    bob_wallet
                        .address()
                        .to_string(),
                    restart_amount,
                    restart_fee,
                    restart_nonce,
                );

            restart_transaction.sign(
                alice_wallet.sign(
                    &restart_transaction
                        .message(),
                ),
            );

            let restart_tx_added =
                alice_node
                    .add_transaction(
                        restart_transaction
                            .clone(),
                    );

            bob_node.receive_message(
                NetworkMessage::Transaction(
                    restart_transaction,
                ),
            );

            println!(
                "Restart sonrası transaction eklendi mi: {}",
                restart_tx_added
            );

            let restart_previous_block =
                alice_node
                    .blockchain
                    .chain
                    .last()
                    .expect(
                        "Restart sonrası blockchain boş",
                    )
                    .clone();

            let restart_validator_address =
                alice_node
                    .select_validator(
                        &restart_previous_block
                            .hash,
                    )
                    .expect(
                        "Restart sonrası validator seçilemedi",
                    );

            let restart_validator_wallet =
                if restart_validator_address
                    == alice_wallet.address()
                {
                    &alice_wallet
                } else {
                    &bob_wallet
                };

            let restart_timestamp =
                restart_previous_block
                    .timestamp
                    .checked_add(100)
                    .expect(
                        "Restart block timestamp overflow",
                    );

            let restart_block =
                alice_node
                    .produce_block(
                        restart_timestamp,
                        restart_validator_wallet,
                    )
                    .expect(
                        "Restart sonrası yeni block üretilemedi",
                    );

            let restart_block_index =
                restart_block.index;

            bob_node.receive_message(
                NetworkMessage::Block(
                    restart_block,
                ),
            );

            let expected_block_count =
                saved_block_count
                    .checked_add(1)
                    .expect(
                        "Block count overflow",
                    );

            let continuation_ok =
                alice_node
                    .blockchain
                    .chain
                    .len()
                    == expected_block_count
                    && bob_node
                        .blockchain
                        .chain
                        .len()
                        == expected_block_count
                    && alice_node
                        .blockchain
                        .is_valid()
                    && bob_node
                        .blockchain
                        .is_valid();

            println!(
                "Restart sonrası üretilen block index: {}",
                restart_block_index
            );

            println!(
                "Restart sonrası zincir {} bloktan {} bloğa devam etti mi: {}",
                saved_block_count,
                expected_block_count,
                continuation_ok
            );

            Storage::save_blockchain(
                &alice_node
                    .blockchain
                    .chain,
            )
            .expect(
                "Restart sonrası blockchain diske kaydedilemedi",
            );

            println!(
                "💾 Devam eden blockchain tekrar diske kaydedildi."
            );

            return;
        }

        Ok(None) => {
            println!(
                "📂 Kayıtlı blockchain bulunamadı. Genesis ile yeni zincir başlatılıyor."
            );
        }

        Err(error) => {
            panic!(
                "Blockchain dosyası güvenli şekilde yüklenemedi: {}",
                error
            );
        }
    }

    // ==========================
    // PEER NETWORK
    // ==========================

    alice_node.add_peer(
        String::from("BOB_NODE"),
    );

    bob_node.add_peer(
        String::from("ALICE_NODE"),
    );

    println!();

    println!(
        "Alice Node peer sayısı: {}",
        alice_node.peer_count()
    );

    println!(
        "Bob Node peer sayısı: {}",
        bob_node.peer_count()
    );

    // ==========================
    // SYNC TEST
    // ==========================

    alice_node
        .network
        .broadcast(
            NetworkMessage::SyncRequest,
        );

    bob_node.receive_message(
        NetworkMessage::SyncRequest,
    );

    alice_node.sync_network();

    // ==========================
    // İLK VALIDATOR SEÇİMİ
    // ==========================

    let latest_hash =
        alice_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .hash
            .clone();

    let selected_validator_address =
        match alice_node
            .select_validator(
                &latest_hash,
            )
        {
            Some(address) => {
                println!();

                println!(
                    "Seçilen validator: {}",
                    address
                );

                address
            }

            None => {
                println!(
                    "Validator seçilemedi."
                );

                return;
            }
        };

    let selected_validator_wallet =
        if selected_validator_address
            == alice_wallet.address()
        {
            &alice_wallet
        } else {
            &bob_wallet
        };

    println!();

    println!(
        "Alice başlangıç bakiyesi: {} AION",
        alice_node
            .state
            .balance_of(
                alice_wallet.address(),
            )
            / 1_000_000
    );

    println!(
        "Bob başlangıç bakiyesi: {} AION",
        alice_node
            .state
            .balance_of(
                bob_wallet.address(),
            )
            / 1_000_000
    );

    println!(
        "Genesis toplam arzı: {} AION",
        alice_node
            .blockchain
            .economy
            .supply()
            / 1_000_000
    );

    // ==========================
    // İLK TRANSACTION
    // ==========================

    let first_amount =
        50_000_000;

    let first_fee =
        alice_node
            .blockchain
            .economy
            .calculate_fee(
                first_amount,
            );

    println!(
        "İlk transaction fee: {} microAION",
        first_fee
    );

    let mut transaction =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            first_amount,
            first_fee,
            0,
        );

    let signature =
        alice_wallet.sign(
            &transaction.message(),
        );

    transaction.sign(
        signature,
    );

    let added =
        alice_node
            .add_transaction(
                transaction.clone(),
            );

    println!();

    println!(
        "Alice Node mempool'a eklendi mi: {}",
        added
    );

    println!(
        "Alice mempool: {}",
        alice_node.mempool.len()
    );

    // ==========================
    // TRANSACTION BOB'A GÖNDER
    // ==========================

    bob_node.receive_message(
        NetworkMessage::Transaction(
            transaction,
        ),
    );

    println!(
        "Bob mempool: {}",
        bob_node.mempool.len()
    );

    // ==========================
    // İLK BLOCK
    // ==========================

    let block_created =
        alice_node.produce_block(
            1754690100,
            selected_validator_wallet,
        );

    match block_created {
        Ok(block) => {
            println!(
                "✅ İlk block oluşturuldu."
            );

            println!(
                "📡 Block Bob Node'a gönderiliyor..."
            );

            bob_node.receive_message(
                NetworkMessage::Block(
                    block,
                ),
            );
        }

        Err(error) => {
            println!(
                "❌ Blok oluşturma hatası: {}",
                error
            );

            return;
        }
    }

    // ==========================
    // İKİNCİ TRANSACTION
    // ==========================

    println!();

    println!(
        "========== İKİNCİ BLOCK TEST =========="
    );

    let second_amount =
        10_000_000;

    let second_fee =
        alice_node
            .blockchain
            .economy
            .calculate_fee(
                second_amount,
            );

    println!(
        "İkinci transaction fee: {} microAION",
        second_fee
    );

    let mut second_transaction =
        Transaction::new(
            bob_wallet
                .address()
                .to_string(),
            bob_wallet
                .public_key_hex(),
            alice_wallet
                .address()
                .to_string(),
            second_amount,
            second_fee,
            0,
        );

    let second_signature =
        bob_wallet.sign(
            &second_transaction
                .message(),
        );

    second_transaction.sign(
        second_signature,
    );

    let added_second =
        alice_node
            .add_transaction(
                second_transaction.clone(),
            );

    println!(
        "İkinci transaction eklendi mi: {}",
        added_second
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            second_transaction,
        ),
    );

    // ==========================
    // İKİNCİ VALIDATOR SEÇİMİ
    // ==========================

    let second_latest_hash =
        alice_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .hash
            .clone();

    let second_validator_address =
        match alice_node
            .select_validator(
                &second_latest_hash,
            )
        {
            Some(address) => {
                println!(
                    "İkinci blok validator: {}",
                    address
                );

                address
            }

            None => {
                println!(
                    "İkinci validator seçilemedi."
                );

                return;
            }
        };

    let second_validator_wallet =
        if second_validator_address
            == alice_wallet.address()
        {
            &alice_wallet
        } else {
            &bob_wallet
        };

    // ==========================
    // İKİNCİ BLOCK
    // ==========================

    let block_two =
        alice_node.produce_block(
            1754690200,
            second_validator_wallet,
        );

    match block_two {
        Ok(block) => {
            println!(
                "✅ İkinci block oluşturuldu."
            );

            println!(
                "📡 İkinci block Bob Node'a gönderiliyor..."
            );

            bob_node.receive_message(
                NetworkMessage::Block(
                    block,
                ),
            );
        }

        Err(error) => {
            println!(
                "❌ İkinci block üretilemedi: {}",
                error
            );
        }
    }

    // ==========================
    // NONCE KUYRUK TESTİ
    // ==========================

    println!();

    println!(
        "========== NONCE KUYRUK TESTİ =========="
    );

    // Alice'in ilk işlemi nonce 0 ile
    // zaten blok 1'de işlendi.
    // Bu yüzden sıradaki nonce değerleri:
    // 1, 2 ve 3.

    let queue_amount_1 =
        1_000_000;

    let queue_fee_1 =
        alice_node
            .blockchain
            .economy
            .calculate_fee(
                queue_amount_1,
            );

    let mut queue_transaction_1 =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            queue_amount_1,
            queue_fee_1,
            1,
        );

    let queue_signature_1 =
        alice_wallet.sign(
            &queue_transaction_1
                .message(),
        );

    queue_transaction_1.sign(
        queue_signature_1,
    );

    let queue_added_1 =
        alice_node
            .add_transaction(
                queue_transaction_1.clone(),
            );

    println!(
        "Nonce 1 transaction eklendi mi: {}",
        queue_added_1
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            queue_transaction_1,
        ),
    );

    let queue_amount_2 =
        2_000_000;

    let queue_fee_2 =
        alice_node
            .blockchain
            .economy
            .calculate_fee(
                queue_amount_2,
            );

    let mut queue_transaction_2 =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            queue_amount_2,
            queue_fee_2,
            2,
        );

    let queue_signature_2 =
        alice_wallet.sign(
            &queue_transaction_2
                .message(),
        );

    queue_transaction_2.sign(
        queue_signature_2,
    );

    let queue_added_2 =
        alice_node
            .add_transaction(
                queue_transaction_2.clone(),
            );

    println!(
        "Nonce 2 transaction eklendi mi: {}",
        queue_added_2
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            queue_transaction_2,
        ),
    );

    let queue_amount_3 =
        3_000_000;

    let queue_fee_3 =
        alice_node
            .blockchain
            .economy
            .calculate_fee(
                queue_amount_3,
            );

    let mut queue_transaction_3 =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            queue_amount_3,
            queue_fee_3,
            3,
        );

    let queue_signature_3 =
        alice_wallet.sign(
            &queue_transaction_3
                .message(),
        );

    queue_transaction_3.sign(
        queue_signature_3,
    );

    let queue_added_3 =
        alice_node
            .add_transaction(
                queue_transaction_3.clone(),
            );

    println!(
        "Nonce 3 transaction eklendi mi: {}",
        queue_added_3
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            queue_transaction_3,
        ),
    );

    println!(
        "Alice nonce test mempool: {}",
        alice_node.mempool.len()
    );

    println!(
        "Bob nonce test mempool: {}",
        bob_node.mempool.len()
    );

    // ==========================
    // ÜÇÜNCÜ VALIDATOR SEÇİMİ
    // ==========================

    let third_latest_hash =
        alice_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .hash
            .clone();

    let third_validator_address =
        match alice_node
            .select_validator(
                &third_latest_hash,
            )
        {
            Some(address) => {
                println!(
                    "Üçüncü blok validator: {}",
                    address
                );

                address
            }

            None => {
                println!(
                    "Üçüncü validator seçilemedi."
                );

                return;
            }
        };

    let third_validator_wallet =
        if third_validator_address
            == alice_wallet.address()
        {
            &alice_wallet
        } else {
            &bob_wallet
        };

    // ==========================
    // ÜÇÜNCÜ BLOCK
    // ==========================

    let block_three =
        alice_node.produce_block(
            1754690300,
            third_validator_wallet,
        );

    match block_three {
        Ok(block) => {
            println!(
                "✅ Üçüncü block oluşturuldu."
            );

            let normal_transaction_count =
                block
                    .transactions
                    .iter()
                    .filter(
                        |transaction| {
                            !transaction.coinbase
                        },
                    )
                    .count();

            println!(
                "Üçüncü blok normal transaction sayısı: {}",
                normal_transaction_count
            );

            println!(
                "📡 Üçüncü block Bob Node'a gönderiliyor..."
            );

            bob_node.receive_message(
                NetworkMessage::Block(
                    block,
                ),
            );

            println!(
                "Alice mempool blok sonrası: {}",
                alice_node.mempool.len()
            );

            println!(
                "Bob mempool blok sonrası: {}",
                bob_node.mempool.len()
            );
        }

        Err(error) => {
            println!(
                "❌ Üçüncü block üretilemedi: {}",
                error
            );
        }
    }

    println!();

    println!(
        "Alice chain uzunluğu: {}",
        alice_node
            .blockchain
            .height()
    );

    println!(
        "Bob chain uzunluğu: {}",
        bob_node
            .blockchain
            .height()
    );

    // ==========================
    // CHAIN SYNC TEST
    // ==========================

    println!();

    println!(
        "📡 Bob güncel chain istiyor..."
    );

    bob_node.request_chain();

    let alice_chain =
        alice_node
            .blockchain
            .chain
            .clone();

    bob_node.receive_message(
        NetworkMessage::ChainChunkResponse {
            start_index: 0,
            total_blocks:
                alice_chain.len()
                    as u64,
            blocks: alice_chain,
        },
    );

    // ==========================
    // EŞİT UZUNLUKTA SAHTE FORK TESTİ
    // ==========================

    println!();

    println!(
        "========== SAHTE FORK TESTİ =========="
    );

    let bob_tip_before_fork =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .hash
            .clone();

    let mut fake_fork_chain =
        alice_node
            .blockchain
            .chain
            .clone();

    if let Some(fake_tip) =
        fake_fork_chain.last_mut()
    {
        fake_tip.timestamp =
            fake_tip
                .timestamp
                .checked_add(1)
                .expect(
                    "Fake fork timestamp overflow",
                );

        fake_tip.hash =
            fake_tip.calculate_hash();
    }

    let fake_tip_hash =
        fake_fork_chain
            .last()
            .unwrap()
            .hash
            .clone();

    println!(
        "Bob mevcut tip hash: {}",
        bob_tip_before_fork
    );

    println!(
        "Sahte fork tip hash: {}",
        fake_tip_hash
    );

    bob_node.receive_message(
        NetworkMessage::ChainChunkResponse {
            start_index: 0,
            total_blocks:
                fake_fork_chain.len()
                    as u64,
            blocks: fake_fork_chain,
        },
    );

    let bob_tip_after_fork =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .hash
            .clone();

    println!(
        "Bob fork sonrası tip hash: {}",
        bob_tip_after_fork
    );

    println!(
        "Sahte fork reddedildi mi: {}",
        bob_tip_before_fork
            == bob_tip_after_fork
    );

    // ==========================
    // DUPLICATE TRANSACTION ID TESTİ
    // ==========================

    println!();

    println!(
        "========== DUPLICATE TRANSACTION ID TESTİ =========="
    );

    let bob_chain_len_before_duplicate_attack =
        bob_node
            .blockchain
            .chain
            .len();

    let bob_tip_before_duplicate_attack =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .clone();

    let mut duplicate_transaction_block =
        bob_tip_before_duplicate_attack
            .clone();

    duplicate_transaction_block.index =
        bob_tip_before_duplicate_attack
            .index
            .checked_add(1)
            .expect(
                "Duplicate transaction block index overflow",
            );

    duplicate_transaction_block.previous_hash =
        bob_tip_before_duplicate_attack
            .hash
            .clone();

    duplicate_transaction_block.timestamp =
        bob_tip_before_duplicate_attack
            .timestamp
            .checked_add(1)
            .expect(
                "Duplicate transaction block timestamp overflow",
            );

    duplicate_transaction_block.hash =
        duplicate_transaction_block
            .calculate_hash();

    bob_node.receive_message(
        NetworkMessage::Block(
            duplicate_transaction_block,
        ),
    );

    let bob_chain_len_after_duplicate_attack =
        bob_node
            .blockchain
            .chain
            .len();

    println!(
        "Aynı transaction ID tekrar kullanımı reddedildi mi: {}",
        bob_chain_len_before_duplicate_attack
            == bob_chain_len_after_duplicate_attack
    );

    // ==========================
    // TIMESTAMP SALDIRI TESTİ
    // ==========================

    println!();

    println!(
        "========== TIMESTAMP SALDIRI TESTİ =========="
    );

    let bob_chain_len_before_timestamp_attack =
        bob_node
            .blockchain
            .chain
            .len();

    let bob_tip_before_timestamp_attack =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .clone();

    let mut fake_timestamp_block =
        bob_tip_before_timestamp_attack
            .clone();

    fake_timestamp_block.index =
        bob_tip_before_timestamp_attack
            .index
            .checked_add(1)
            .expect(
                "Fake timestamp block index overflow",
            );

    fake_timestamp_block.previous_hash =
        bob_tip_before_timestamp_attack
            .hash
            .clone();

    // Bilerek önceki blokla AYNI timestamp.
    // Yeni kural bunu reddetmeli.
    fake_timestamp_block.timestamp =
        bob_tip_before_timestamp_attack
            .timestamp;

    // Bu test yalnızca timestamp kuralını ölçsün.
    // Önceki bloktan klonlanan transaction'ları temizliyoruz.
    fake_timestamp_block.transactions.clear();

    fake_timestamp_block.hash =
        fake_timestamp_block
            .calculate_hash();

    println!(
        "Önceki block timestamp: {}",
        bob_tip_before_timestamp_attack
            .timestamp
    );

    println!(
        "Sahte block timestamp: {}",
        fake_timestamp_block
            .timestamp
    );

    bob_node.receive_message(
        NetworkMessage::Block(
            fake_timestamp_block,
        ),
    );

    let bob_chain_len_after_timestamp_attack =
        bob_node
            .blockchain
            .chain
            .len();

    let timestamp_attack_rejected =
        bob_chain_len_before_timestamp_attack
            == bob_chain_len_after_timestamp_attack;

    println!(
        "Timestamp saldırısı reddedildi mi: {}",
        timestamp_attack_rejected
    );

    // ==========================
    // GELECEK TIMESTAMP SALDIRI TESTİ
    // ==========================

    println!();

    println!(
        "========== GELECEK TIMESTAMP SALDIRI TESTİ =========="
    );

    let bob_chain_len_before_future_attack =
        bob_node
            .blockchain
            .chain
            .len();

    let bob_tip_before_future_attack =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .clone();

    let mut fake_future_block =
        bob_tip_before_future_attack
            .clone();

    fake_future_block.index =
        bob_tip_before_future_attack
            .index
            .checked_add(1)
            .expect(
                "Future block index overflow",
            );

    fake_future_block.previous_hash =
        bob_tip_before_future_attack
            .hash
            .clone();

    // Bilerek çok ileri bir gelecek zamanı.
    fake_future_block.timestamp =
        4_000_000_000;

    // Bu test yalnızca gelecek timestamp kuralını ölçsün.
    fake_future_block.transactions.clear();

    fake_future_block.hash =
        fake_future_block
            .calculate_hash();

    println!(
        "Sahte gelecek timestamp: {}",
        fake_future_block.timestamp
    );

    bob_node.receive_message(
        NetworkMessage::Block(
            fake_future_block,
        ),
    );

    let bob_chain_len_after_future_attack =
        bob_node
            .blockchain
            .chain
            .len();

    println!(
        "Gelecek timestamp saldırısı reddedildi mi: {}",
        bob_chain_len_before_future_attack
            == bob_chain_len_after_future_attack
    );

    // ==========================
    // BLOCK TRANSACTION LİMİT TESTİ
    // ==========================

    println!();

    println!(
        "========== BLOCK TRANSACTION LİMİT TESTİ =========="
    );

    alice_node
        .mempool
        .transactions
        .clear();

    let test_start_nonce =
        alice_node
            .state
            .nonce_of(
                alice_wallet.address(),
            );

    let limit_test_amount =
        1u64;

    let limit_test_fee =
        alice_node
            .blockchain
            .economy
            .calculate_fee(
                limit_test_amount,
            );

    for offset in 0..1001u64 {
        let nonce =
            test_start_nonce
                .checked_add(
                    offset,
                )
                .expect(
                    "Limit test nonce overflow",
                );

        let mut limit_transaction =
            Transaction::new(
                alice_wallet
                    .address()
                    .to_string(),
                alice_wallet
                    .public_key_hex(),
                bob_wallet
                    .address()
                    .to_string(),
                limit_test_amount,
                limit_test_fee,
                nonce,
            );

        let limit_signature =
            alice_wallet.sign(
                &limit_transaction
                    .message(),
            );

        limit_transaction.sign(
            limit_signature,
        );

        alice_node
            .mempool
            .transactions
            .push(
                limit_transaction,
            );
    }

    let selected_transactions =
        alice_node
            .mempool
            .take_valid_transactions(
                &alice_node.state,
            );

    println!(
        "Blok için seçilen normal transaction: {}",
        selected_transactions.len()
    );

    println!(
        "Mempool'da kalan transaction: {}",
        alice_node.mempool.len()
    );

    println!(
        "1000 transaction blok limiti çalışıyor mu: {}",
        selected_transactions.len() == 1000
            && alice_node.mempool.len() == 1
    );

    alice_node
        .mempool
        .transactions
        .clear();

    // ==========================
    // MEMPOOL KAPASİTE TESTİ
    // ==========================

    println!();

    println!(
        "========== MEMPOOL KAPASİTE TESTİ =========="
    );

    let mut capacity_probe =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            1,
            limit_test_fee,
            test_start_nonce,
        );

    let capacity_signature =
        alice_wallet.sign(
            &capacity_probe
                .message(),
        );

    capacity_probe.sign(
        capacity_signature,
    );

    alice_node
        .mempool
        .transactions =
        vec![
            capacity_probe.clone();
            10_000
        ];

    let accepted_over_capacity =
        alice_node
            .mempool
            .add_transaction(
                capacity_probe,
            );

    println!(
        "Mempool mevcut transaction: {}",
        alice_node.mempool.len()
    );

    println!(
        "10001. transaction reddedildi mi: {}",
        !accepted_over_capacity
            && alice_node.mempool.len()
                == 10_000
    );

    alice_node
        .mempool
        .transactions
        .clear();

    // ==========================
    // DIŞARIDAN AŞIRI BÜYÜK BLOCK TESTİ
    // ==========================

    println!();

    println!(
        "========== DIŞARIDAN AŞIRI BÜYÜK BLOCK TESTİ =========="
    );

    let bob_chain_len_before_oversized_block =
        bob_node
            .blockchain
            .chain
            .len();

    let bob_tip_for_oversized_block =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .clone();

    let mut oversized_block =
        bob_tip_for_oversized_block
            .clone();

    oversized_block.index =
        bob_tip_for_oversized_block
            .index
            .checked_add(1)
            .expect(
                "Oversized block index overflow",
            );

    oversized_block.previous_hash =
        bob_tip_for_oversized_block
            .hash
            .clone();

    oversized_block.timestamp =
        bob_tip_for_oversized_block
            .timestamp
            .checked_add(1)
            .expect(
                "Oversized block timestamp overflow",
            );

    let filler_transaction =
        bob_tip_for_oversized_block
            .transactions
            .iter()
            .find(
                |transaction| {
                    !transaction.coinbase
                },
            )
            .expect(
                "Oversized block testi için normal transaction bulunamadı",
            )
            .clone();

    while oversized_block
        .transactions
        .len()
        < 1002
    {
        oversized_block
            .transactions
            .push(
                filler_transaction.clone(),
            );
    }

    oversized_block.hash =
        oversized_block
            .calculate_hash();

    println!(
        "Sahte block transaction sayısı: {}",
        oversized_block
            .transactions
            .len()
    );

    bob_node.receive_message(
        NetworkMessage::Block(
            oversized_block,
        ),
    );

    let bob_chain_len_after_oversized_block =
        bob_node
            .blockchain
            .chain
            .len();

    println!(
        "1002 transaction'lı sahte block reddedildi mi: {}",
        bob_chain_len_before_oversized_block
            == bob_chain_len_after_oversized_block
    );

    // ==========================
    // AŞIRI BÜYÜK TRANSACTION ALANI TESTİ
    // ==========================

    println!();

    println!(
        "========== AŞIRI BÜYÜK TRANSACTION ALANI TESTİ =========="
    );

    let bob_mempool_before_large_field =
        bob_node.mempool.len();

    let large_field_fee =
        bob_node
            .blockchain
            .economy
            .calculate_fee(
                1_000_000,
            );

    let mut large_field_transaction =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            1_000_000,
            large_field_fee,
            alice_node
                .state
                .nonce_of(
                    alice_wallet.address(),
                ),
        );

    let large_field_signature =
        alice_wallet.sign(
            &large_field_transaction
                .message(),
        );

    large_field_transaction.sign(
        large_field_signature,
    );

    // 128 karakter sınırını bilerek aşıyoruz.
    large_field_transaction.to =
        "X".repeat(129);

    println!(
        "Sahte alıcı adres uzunluğu: {}",
        large_field_transaction
            .to
            .len()
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            large_field_transaction,
        ),
    );

    let bob_mempool_after_large_field =
        bob_node.mempool.len();

    println!(
        "Aşırı büyük transaction alanı reddedildi mi: {}",
        bob_mempool_before_large_field
            == bob_mempool_after_large_field
    );

    // ==========================
    // AŞIRI BÜYÜK BLOCK ALANI TESTİ
    // ==========================

    println!();

    println!(
        "========== AŞIRI BÜYÜK BLOCK ALANI TESTİ =========="
    );

    let bob_chain_len_before_large_block_field =
        bob_node
            .blockchain
            .chain
            .len();

    let bob_tip_for_large_block_field =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .clone();

    let mut large_block_field =
        bob_tip_for_large_block_field
            .clone();

    large_block_field.index =
        bob_tip_for_large_block_field
            .index
            .checked_add(1)
            .expect(
                "Large block field index overflow",
            );

    large_block_field.previous_hash =
        bob_tip_for_large_block_field
            .hash
            .clone();

    large_block_field.timestamp =
        bob_tip_for_large_block_field
            .timestamp
            .checked_add(1)
            .expect(
                "Large block field timestamp overflow",
            );

    // 128 karakter sınırını bilerek aşıyoruz.
    large_block_field.validator =
        "V".repeat(129);

    println!(
        "Sahte validator adres uzunluğu: {}",
        large_block_field
            .validator
            .len()
    );

    bob_node.receive_message(
        NetworkMessage::Block(
            large_block_field,
        ),
    );

    let bob_chain_len_after_large_block_field =
        bob_node
            .blockchain
            .chain
            .len();

    println!(
        "Aşırı büyük block alanı reddedildi mi: {}",
        bob_chain_len_before_large_block_field
            == bob_chain_len_after_large_block_field
    );

    // ==========================
    // DOĞRUDAN BLOCKCHAIN KATMANI TESTİ
    // ==========================

    println!();

    println!(
        "========== DOĞRUDAN BLOCKCHAIN KATMANI TESTİ =========="
    );

    let bob_chain_len_before_direct_test =
        bob_node
            .blockchain
            .chain
            .len();

    let bob_tip_for_direct_test =
        bob_node
            .blockchain
            .chain
            .last()
            .unwrap()
            .clone();

    let mut direct_large_field_block =
        bob_tip_for_direct_test
            .clone();

    direct_large_field_block.index =
        bob_tip_for_direct_test
            .index
            .checked_add(1)
            .expect(
                "Direct blockchain test index overflow",
            );

    direct_large_field_block.previous_hash =
        bob_tip_for_direct_test
            .hash
            .clone();

    direct_large_field_block.timestamp =
        bob_tip_for_direct_test
            .timestamp
            .checked_add(1)
            .expect(
                "Direct blockchain test timestamp overflow",
            );

    direct_large_field_block.validator =
        "V".repeat(129);

    let direct_blockchain_result =
        bob_node
            .blockchain
            .add_received_block(
                direct_large_field_block,
            );

    match &direct_blockchain_result {
        Ok(()) => {
            println!(
                "❌ Blockchain katmanı sahte block'u kabul etti"
            );
        }

        Err(error) => {
            println!(
                "✅ Blockchain katmanı reddetti: {}",
                error
            );
        }
    }

    let bob_chain_len_after_direct_test =
        bob_node
            .blockchain
            .chain
            .len();

    println!(
        "Node bypass edilince koruma çalışıyor mu: {}",
        direct_blockchain_result
            .is_err()
            && bob_chain_len_before_direct_test
                == bob_chain_len_after_direct_test
    );

    // ==========================
    // KISA / GEÇERSİZ ADRES FORMAT TESTİ
    // ==========================

    println!();

    println!(
        "========== KISA / GEÇERSİZ ADRES FORMAT TESTİ =========="
    );

    let bob_mempool_before_short_address =
        bob_node.mempool.len();

    let short_address_fee =
        bob_node
            .blockchain
            .economy
            .calculate_fee(
                1_000_000,
            );

    let mut short_address_transaction =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            1_000_000,
            short_address_fee,
            alice_node
                .state
                .nonce_of(
                    alice_wallet.address(),
                ),
        );

    let short_address_signature =
        alice_wallet.sign(
            &short_address_transaction
                .message(),
        );

    short_address_transaction.sign(
        short_address_signature,
    );

    short_address_transaction.to =
        "1234567890abcdef1234"
            .to_string();

    println!(
        "Sahte kısa alıcı adres uzunluğu: {}",
        short_address_transaction
            .to
            .len()
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            short_address_transaction,
        ),
    );

    let bob_mempool_after_short_address =
        bob_node.mempool.len();

    println!(
        "Kısa/geçersiz adres reddedildi mi: {}",
        bob_mempool_before_short_address
            == bob_mempool_after_short_address
    );

    // ==========================
    // DOĞRUDAN MEMPOOL KATMANI TESTİ
    // ==========================

    println!();

    println!(
        "========== DOĞRUDAN MEMPOOL KATMANI TESTİ =========="
    );

    let direct_mempool_before =
        bob_node.mempool.len();

    let direct_mempool_fee =
        bob_node
            .blockchain
            .economy
            .calculate_fee(
                1_000_000,
            );

    let mut direct_mempool_transaction =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            1_000_000,
            direct_mempool_fee,
            alice_node
                .state
                .nonce_of(
                    alice_wallet.address(),
                ),
        );

    let direct_mempool_signature =
        alice_wallet.sign(
            &direct_mempool_transaction
                .message(),
        );

    direct_mempool_transaction.sign(
        direct_mempool_signature,
    );

    direct_mempool_transaction.to =
        "1234567890abcdef1234"
            .to_string();

    let direct_mempool_result =
        bob_node
            .mempool
            .add_transaction(
                direct_mempool_transaction,
            );

    let direct_mempool_after =
        bob_node.mempool.len();

    println!(
        "Doğrudan mempool kabul etti mi: {}",
        direct_mempool_result
    );

    println!(
        "Node bypass edilince mempool koruması çalışıyor mu: {}",
        !direct_mempool_result
            && direct_mempool_before
                == direct_mempool_after
    );

    // ==========================
    // 64 KARAKTER AMA HEX OLMAYAN ADRES TESTİ
    // ==========================

    println!();

    println!(
        "========== 64 KARAKTER AMA HEX OLMAYAN ADRES TESTİ =========="
    );

    let bob_mempool_before_non_hex =
        bob_node.mempool.len();

    let non_hex_fee =
        bob_node
            .blockchain
            .economy
            .calculate_fee(
                1_000_000,
            );

    let mut non_hex_transaction =
        Transaction::new(
            alice_wallet
                .address()
                .to_string(),
            alice_wallet
                .public_key_hex(),
            bob_wallet
                .address()
                .to_string(),
            1_000_000,
            non_hex_fee,
            alice_node
                .state
                .nonce_of(
                    alice_wallet.address(),
                ),
        );

    let non_hex_signature =
        alice_wallet.sign(
            &non_hex_transaction
                .message(),
        );

    non_hex_transaction.sign(
        non_hex_signature,
    );

    non_hex_transaction.to =
        format!(
            "{}Z",
            "a".repeat(63),
        );

    println!(
        "Sahte adres uzunluğu: {}",
        non_hex_transaction
            .to
            .len()
    );

    bob_node.receive_message(
        NetworkMessage::Transaction(
            non_hex_transaction.clone(),
        ),
    );

    let node_rejected_non_hex =
        bob_node.mempool.len()
            == bob_mempool_before_non_hex;

    println!(
        "Node hex olmayan adresi reddetti mi: {}",
        node_rejected_non_hex
    );

    let direct_mempool_result_non_hex =
        bob_node
            .mempool
            .add_transaction(
                non_hex_transaction,
            );

    println!(
        "Mempool hex olmayan adresi reddetti mi: {}",
        !direct_mempool_result_non_hex
            && bob_node.mempool.len()
                == bob_mempool_before_non_hex
    );

    // ==========================
    // 257 BLOCK CHUNK SYNC TESTİ
    // ==========================

    println!();

    println!(
        "========== 257 BLOCK CHUNK SYNC TESTİ =========="
    );

    let oversized_chunk_block =
        bob_node
            .blockchain
            .chain
            .last()
            .expect(
                "Bob blockchain boş",
            )
            .clone();

    let oversized_chunk =
        vec![
            oversized_chunk_block;
            257
        ];

    let chunk_network_message_count_before =
        bob_node
            .network
            .message_count();

    bob_node.network.receive(
        NetworkMessage::ChainChunkResponse {
            start_index: 0,
            total_blocks: 257,
            blocks:
                oversized_chunk
                    .clone(),
        },
    );

    let chunk_network_message_count_after =
        bob_node
            .network
            .message_count();

    println!(
        "Network 257 block chunk mesajını reddetti mi: {}",
        chunk_network_message_count_before
            == chunk_network_message_count_after
    );

    let bob_chain_len_before_chunk_attack =
        bob_node
            .blockchain
            .chain
            .len();

    bob_node.receive_message(
        NetworkMessage::ChainChunkResponse {
            start_index: 0,
            total_blocks: 257,
            blocks:
                oversized_chunk,
        },
    );

    let bob_chain_len_after_chunk_attack =
        bob_node
            .blockchain
            .chain
            .len();

    println!(
        "Node 257 block chunk mesajını reddetti mi: {}",
        bob_chain_len_before_chunk_attack
            == bob_chain_len_after_chunk_attack
    );

    // ==========================
    // SON DURUM
    // ==========================

    println!();

    println!(
        "Alice Node blockchain blok sayısı: {}",
        alice_node
            .blockchain
            .chain
            .len()
    );

    println!(
        "Bob Node blockchain blok sayısı: {}",
        bob_node
            .blockchain
            .chain
            .len()
    );

    println!(
        "Alice final bakiye: {} AION",
        alice_node
            .state
            .balance_of(
                alice_wallet.address(),
            )
            / 1_000_000
    );

    println!(
        "Bob final bakiye: {} AION",
        alice_node
            .state
            .balance_of(
                bob_wallet.address(),
            )
            / 1_000_000
    );

    println!(
        "Treasury: {} microAION",
        alice_node
            .state
            .treasury()
    );

    println!(
        "Yakılan miktar: {} microAION",
        alice_node
            .state
            .burned()
    );

    println!(
        "Alice Blockchain geçerli mi: {}",
        alice_node
            .blockchain
            .is_valid()
    );

    println!(
        "Bob Blockchain geçerli mi: {}",
        bob_node
            .blockchain
            .is_valid()
    );

    println!(
        "Toplam arz: {} AION",
        alice_node
            .blockchain
            .economy
            .supply()
            / 1_000_000
    );

    // ==========================
    // BLOCKCHAIN KALICI KAYIT
    // ==========================

    println!();

    match Storage::save_blockchain(
        &alice_node
            .blockchain
            .chain,
    ) {
        Ok(()) => {
            println!(
                "💾 Blockchain diske kaydedildi: {}",
                Storage::blockchain_path()
                    .display()
            );

            match Storage::load_blockchain() {
                Ok(Some(loaded_chain)) => {
                    let same_length =
                        loaded_chain.len()
                            == alice_node
                                .blockchain
                                .chain
                                .len();

                    let same_tip =
                        loaded_chain
                            .last()
                            .map(
                                |block| {
                                    block.hash.as_str()
                                },
                            )
                            == alice_node
                                .blockchain
                                .chain
                                .last()
                                .map(
                                    |block| {
                                        block.hash.as_str()
                                    },
                                );

                    println!(
                        "📂 Blockchain diskten tekrar okundu. Blok sayısı: {}",
                        loaded_chain.len()
                    );

                    println!(
                        "Diskten yüklenen blockchain aynı mı: {}",
                        same_length
                            && same_tip
                    );
                }

                Ok(None) => {
                    println!(
                        "❌ Blockchain dosyası bulunamadı"
                    );
                }

                Err(error) => {
                    println!(
                        "❌ Blockchain diskten okunamadı: {}",
                        error
                    );
                }
            }
        }

        Err(error) => {
            println!(
                "❌ Blockchain diske kaydedilemedi: {}",
                error
            );
        }
    }
}