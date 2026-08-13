use kybernetes::network::{Network, NetworkMessage};
use kybernetes::protocol::{MAX_HANDSHAKE_AGE_SECONDS, NETWORK_ID, NETWORK_PROTOCOL_VERSION};
use kybernetes::wallet::Wallet;

const PRIVATE_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const LISTEN_ADDRESS: &str = "127.0.0.1:41001";

fn wallet() -> Wallet {
    Wallet::from_private_key_hex(PRIVATE_KEY).expect("test private key must be valid")
}

fn signed_handshake(
    wallet: &Wallet,
    network_id: &str,
    protocol_version: u32,
    timestamp: u64,
    challenge: &str,
) -> NetworkMessage {
    let public_key = wallet.public_key_hex();
    let signature = wallet.sign_node_handshake(
        LISTEN_ADDRESS,
        network_id,
        protocol_version,
        timestamp,
        challenge,
    );

    assert!(Wallet::verify_node_handshake(
        wallet.node_id(),
        &public_key,
        LISTEN_ADDRESS,
        network_id,
        protocol_version,
        timestamp,
        challenge,
        &signature,
    ));

    NetworkMessage::Handshake {
        node_id: wallet.node_id().to_string(),
        public_key,
        listen_address: LISTEN_ADDRESS.to_string(),
        network_id: network_id.to_string(),
        protocol_version,
        timestamp,
        challenge: challenge.to_string(),
        signature,
    }
}

fn assert_handshake_rejected(message: NetworkMessage) {
    let mut network = Network::new();
    network.receive(message);

    assert_eq!(network.peer_count(), 0);
    assert_eq!(network.identified_peer_count(), 0);
    assert_eq!(network.message_count(), 0);
}

#[test]
fn network_accepts_a_valid_signed_handshake() {
    let wallet = wallet();
    let challenge = "ab".repeat(32);
    let message = signed_handshake(
        &wallet,
        NETWORK_ID,
        NETWORK_PROTOCOL_VERSION,
        Network::current_timestamp(),
        &challenge,
    );
    let mut network = Network::new();

    network.receive(message);

    assert_eq!(network.peer_count(), 1);
    assert_eq!(network.identified_peer_count(), 1);
    assert!(network.has_peer_node_id(wallet.node_id()));
    assert_eq!(network.message_count(), 1);
}

#[test]
fn network_rejects_a_handshake_for_the_wrong_network_id() {
    let wallet = wallet();
    let challenge = "bc".repeat(32);
    let message = signed_handshake(
        &wallet,
        "kybernetes-testnet-v1",
        NETWORK_PROTOCOL_VERSION,
        Network::current_timestamp(),
        &challenge,
    );

    assert_handshake_rejected(message);
}

#[test]
fn network_rejects_a_handshake_for_the_wrong_protocol_version() {
    let wallet = wallet();
    let challenge = "cd".repeat(32);
    let wrong_version = NETWORK_PROTOCOL_VERSION
        .checked_add(1)
        .expect("test protocol version must not overflow");
    let message = signed_handshake(
        &wallet,
        NETWORK_ID,
        wrong_version,
        Network::current_timestamp(),
        &challenge,
    );

    assert_handshake_rejected(message);
}

#[test]
fn network_rejects_a_stale_handshake() {
    let wallet = wallet();
    let challenge = "de".repeat(32);
    let stale_timestamp =
        Network::current_timestamp().saturating_sub(MAX_HANDSHAKE_AGE_SECONDS + 10);
    let message = signed_handshake(
        &wallet,
        NETWORK_ID,
        NETWORK_PROTOCOL_VERSION,
        stale_timestamp,
        &challenge,
    );

    assert_handshake_rejected(message);
}

#[test]
fn handshake_ack_rejects_the_wrong_challenge() {
    let wallet = wallet();
    let actual_challenge = "ef".repeat(32);
    let expected_challenge = "f0".repeat(32);
    let acknowledgement = Network::create_handshake_ack(&wallet, true, actual_challenge.clone());

    assert!(Network::validate_handshake_ack(
        &acknowledgement,
        &actual_challenge
    ));
    assert!(!Network::validate_handshake_ack(
        &acknowledgement,
        &expected_challenge
    ));
}

#[test]
fn network_rejects_a_tampered_handshake_signature() {
    let wallet = wallet();
    let challenge = "12".repeat(32);
    let mut message = signed_handshake(
        &wallet,
        NETWORK_ID,
        NETWORK_PROTOCOL_VERSION,
        Network::current_timestamp(),
        &challenge,
    );

    match &mut message {
        NetworkMessage::Handshake { signature, .. } => {
            let replacement = if signature.starts_with('0') { "1" } else { "0" };
            signature.replace_range(..1, replacement);
        }
        _ => panic!("fixture must be a handshake"),
    }

    assert_handshake_rejected(message);
}
