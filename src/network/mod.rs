pub mod tcp;

pub const ONE_SHOT_CLIENT_LISTEN_ADDRESS: &str = "127.0.0.1:0";
const MAX_TRANSACTION_ACK_REASON_LENGTH: usize = 512;

use std::time::{SystemTime, UNIX_EPOCH};

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::core::{Block, Transaction};
use crate::protocol::{
    ADDRESS_HEX_LENGTH, HASH_HEX_LENGTH, MAX_HANDSHAKE_AGE_SECONDS, MAX_NETWORK_INBOX_MESSAGES,
    MAX_NETWORK_MESSAGE_BYTES, MAX_NETWORK_MESSAGE_HISTORY, MAX_NETWORK_PEERS,
    MAX_PEER_ADDRESS_LENGTH, MAX_SYNC_BLOCKS_PER_MESSAGE, NETWORK_ID, NETWORK_PROTOCOL_VERSION,
    is_fixed_hex,
};
use crate::wallet::Wallet;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    Handshake {
        node_id: String,
        public_key: String,
        listen_address: String,
        network_id: String,
        protocol_version: u32,
        timestamp: u64,
        challenge: String,
        signature: String,
    },

    HandshakeAck {
        node_id: String,
        public_key: String,
        network_id: String,
        protocol_version: u32,
        timestamp: u64,
        challenge: String,
        accepted: bool,
        signature: String,
    },

    Transaction(Transaction),

    TransactionAck {
        transaction_id: String,
        accepted: bool,
        reason: Option<String>,
    },

    Block(Block),

    SyncRequest,

    ChainChunkRequest {
        start_index: u64,
    },

    ChainChunkResponse {
        start_index: u64,
        total_blocks: u64,
        blocks: Vec<Block>,
    },

    AccountStateRequest {
        address: String,
    },

    AccountStateResponse {
        address: String,
        balance: u64,
        nonce: u64,
        tip_index: u64,
        tip_hash: String,
    },
}

#[derive(Debug, Clone)]
pub struct PeerIdentity {
    pub node_id: String,
    pub listen_address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRegistrationOutcome {
    Registered,
    AlreadyRegistered,
    OneShotClient,
    NodeIdConflict,
    ListenAddressConflict,
    PeerLimitReached,
    InvalidIdentity,
}

#[derive(Debug, Default)]
pub struct Network {
    pub peers: Vec<String>,

    pub peer_identities: Vec<PeerIdentity>,

    pub messages: Vec<NetworkMessage>,

    pub inbox: Vec<NetworkMessage>,
}

#[allow(dead_code)]
impl Network {
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),

            peer_identities: Vec::new(),

            messages: Vec::new(),

            inbox: Vec::new(),
        }
    }

    pub fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn handshake_timestamp_valid(timestamp: u64) -> bool {
        let now = Self::current_timestamp();

        now.abs_diff(timestamp) <= MAX_HANDSHAKE_AGE_SECONDS
    }

    pub fn generate_handshake_challenge() -> String {
        let mut bytes = [0u8; 32];

        OsRng.fill_bytes(&mut bytes);

        hex::encode(bytes)
    }

    pub fn handshake_challenge(message: &NetworkMessage) -> Option<&str> {
        match message {
            NetworkMessage::Handshake { challenge, .. } => Some(challenge),

            _ => None,
        }
    }

    pub fn protocol_version() -> u32 {
        NETWORK_PROTOCOL_VERSION
    }

    pub fn create_handshake(wallet: &Wallet, listen_address: String) -> NetworkMessage {
        let node_id = wallet.node_id().to_string();

        let public_key = wallet.public_key_hex();

        let timestamp = Self::current_timestamp();

        let challenge = Self::generate_handshake_challenge();

        let signature = wallet.sign_node_handshake(
            &listen_address,
            NETWORK_ID,
            NETWORK_PROTOCOL_VERSION,
            timestamp,
            &challenge,
        );

        NetworkMessage::Handshake {
            node_id,
            public_key,
            listen_address,
            network_id: NETWORK_ID.to_string(),
            protocol_version: NETWORK_PROTOCOL_VERSION,
            timestamp,
            challenge,
            signature,
        }
    }

    pub fn create_handshake_ack(
        wallet: &Wallet,
        accepted: bool,
        challenge: String,
    ) -> NetworkMessage {
        let node_id = wallet.node_id().to_string();

        let public_key = wallet.public_key_hex();

        let timestamp = Self::current_timestamp();

        let signature = wallet.sign_node_handshake_ack(
            NETWORK_ID,
            NETWORK_PROTOCOL_VERSION,
            timestamp,
            &challenge,
            accepted,
        );

        NetworkMessage::HandshakeAck {
            node_id,
            public_key,
            network_id: NETWORK_ID.to_string(),
            protocol_version: NETWORK_PROTOCOL_VERSION,
            timestamp,
            challenge,
            accepted,
            signature,
        }
    }

    pub fn validate_handshake_ack(message: &NetworkMessage, expected_challenge: &str) -> bool {
        match message {
            NetworkMessage::HandshakeAck {
                node_id,
                public_key,
                network_id,
                protocol_version,
                timestamp,
                challenge,
                accepted,
                signature,
            } => {
                *accepted
                    && challenge == expected_challenge
                    && Self::handshake_ack_fields_valid(node_id, network_id, *protocol_version)
                    && Self::handshake_timestamp_valid(*timestamp)
                    && Wallet::verify_node_handshake_ack(
                        node_id,
                        public_key,
                        network_id,
                        *protocol_version,
                        *timestamp,
                        challenge,
                        *accepted,
                        signature,
                    )
            }

            _ => false,
        }
    }

    fn push_message_history(&mut self, message: NetworkMessage) {
        if self.messages.len() >= MAX_NETWORK_MESSAGE_HISTORY {
            self.messages.remove(0);
        }

        self.messages.push(message);
    }

    fn push_inbox(&mut self, message: NetworkMessage) {
        if self.inbox.len() >= MAX_NETWORK_INBOX_MESSAGES {
            self.inbox.remove(0);
        }

        self.inbox.push(message);
    }

    // This validator intentionally mirrors all signed handshake fields one-to-one.
    #[allow(clippy::too_many_arguments)]
    fn handshake_fields_valid(
        node_id: &str,
        public_key: &str,
        listen_address: &str,
        network_id: &str,
        protocol_version: u32,
        timestamp: u64,
        challenge: &str,
        signature: &str,
    ) -> bool {
        is_fixed_hex(node_id, ADDRESS_HEX_LENGTH)
            && is_fixed_hex(public_key, ADDRESS_HEX_LENGTH)
            && !listen_address.is_empty()
            && listen_address.len() <= MAX_PEER_ADDRESS_LENGTH
            && network_id == NETWORK_ID
            && protocol_version == NETWORK_PROTOCOL_VERSION
            && Self::handshake_timestamp_valid(timestamp)
            && is_fixed_hex(challenge, 64)
            && Wallet::verify_node_handshake(
                node_id,
                public_key,
                listen_address,
                network_id,
                protocol_version,
                timestamp,
                challenge,
                signature,
            )
    }

    fn handshake_ack_fields_valid(node_id: &str, network_id: &str, protocol_version: u32) -> bool {
        is_fixed_hex(node_id, ADDRESS_HEX_LENGTH)
            && network_id == NETWORK_ID
            && protocol_version == NETWORK_PROTOCOL_VERSION
    }

    fn transaction_ack_fields_valid(
        transaction_id: &str,
        accepted: bool,
        reason: &Option<String>,
    ) -> bool {
        if !is_fixed_hex(transaction_id, HASH_HEX_LENGTH) {
            return false;
        }

        match (accepted, reason.as_deref()) {
            (true, None) => true,

            (false, Some(reason)) => {
                !reason.is_empty() && reason.len() <= MAX_TRANSACTION_ACK_REASON_LENGTH
            }

            _ => false,
        }
    }

    fn message_within_limits(message: &NetworkMessage) -> bool {
        let serialized_size_ok = serde_json::to_vec(message)
            .map(|bytes| bytes.len() <= MAX_NETWORK_MESSAGE_BYTES)
            .unwrap_or(false);

        if !serialized_size_ok {
            return false;
        }

        match message {
            NetworkMessage::Handshake {
                node_id,
                public_key,
                listen_address,
                network_id,
                protocol_version,
                timestamp,
                challenge,
                signature,
            } => Self::handshake_fields_valid(
                node_id,
                public_key,
                listen_address,
                network_id,
                *protocol_version,
                *timestamp,
                challenge,
                signature,
            ),

            NetworkMessage::HandshakeAck {
                node_id,
                network_id,
                protocol_version,
                challenge,
                ..
            } => {
                Self::handshake_ack_fields_valid(node_id, network_id, *protocol_version)
                    && is_fixed_hex(challenge, 64)
            }

            NetworkMessage::TransactionAck {
                transaction_id,
                accepted,
                reason,
            } => Self::transaction_ack_fields_valid(transaction_id, *accepted, reason),

            NetworkMessage::AccountStateRequest { address } => {
                is_fixed_hex(address, ADDRESS_HEX_LENGTH)
            }

            NetworkMessage::AccountStateResponse {
                address, tip_hash, ..
            } => {
                is_fixed_hex(address, ADDRESS_HEX_LENGTH) && is_fixed_hex(tip_hash, HASH_HEX_LENGTH)
            }

            NetworkMessage::ChainChunkResponse { blocks, .. } => {
                blocks.len() <= MAX_SYNC_BLOCKS_PER_MESSAGE
            }

            NetworkMessage::ChainChunkRequest { .. } => true,

            _ => true,
        }
    }

    pub fn validate_handshake(message: &NetworkMessage) -> bool {
        matches!(message, NetworkMessage::Handshake { .. }) && Self::message_within_limits(message)
    }

    pub fn validate_transaction_ack(
        message: &NetworkMessage,
        expected_transaction_id: &str,
    ) -> bool {
        matches!(
            message,
            NetworkMessage::TransactionAck {
                transaction_id,
                ..
            } if transaction_id
                == expected_transaction_id
        ) && Self::message_within_limits(message)
    }

    pub fn validate_account_state_response(
        message: &NetworkMessage,
        expected_address: &str,
    ) -> bool {
        matches!(
            message,
            NetworkMessage::AccountStateResponse { address, .. }
                if address == expected_address
        ) && Self::message_within_limits(message)
    }

    fn print_message_summary(message: &NetworkMessage) {
        match message {
            NetworkMessage::Handshake {
                node_id,
                listen_address,
                network_id,
                protocol_version,
                ..
            } => {
                println!(
                    "Network message broadcast: Handshake node={} address={} network={} protocol={}",
                    node_id, listen_address, network_id, protocol_version
                );
            }

            NetworkMessage::HandshakeAck {
                node_id,
                network_id,
                protocol_version,
                accepted,
                ..
            } => {
                println!(
                    "Network message broadcast: HandshakeAck node={} network={} protocol={} accepted={}",
                    node_id, network_id, protocol_version, accepted
                );
            }

            NetworkMessage::Transaction(transaction) => {
                println!("Network message broadcast: Transaction {}", transaction.id);
            }

            NetworkMessage::TransactionAck {
                transaction_id,
                accepted,
                ..
            } => {
                println!(
                    "Network message broadcast: TransactionAck {} accepted={}",
                    transaction_id, accepted
                );
            }

            NetworkMessage::AccountStateRequest { address } => {
                println!(
                    "Network message broadcast: AccountStateRequest address={}",
                    address
                );
            }

            NetworkMessage::AccountStateResponse {
                address,
                balance,
                nonce,
                tip_index,
                ..
            } => {
                println!(
                    "Network message broadcast: AccountStateResponse address={} balance={} nonce={} tip={}",
                    address, balance, nonce, tip_index
                );
            }
            NetworkMessage::Block(block) => {
                println!("Network message broadcast: Block {}", block.index);
            }

            NetworkMessage::SyncRequest => {
                println!("Network message broadcast: SyncRequest");
            }

            NetworkMessage::ChainChunkRequest { start_index } => {
                println!(
                    "Network message broadcast: ChainChunkRequest (start: {})",
                    start_index
                );
            }

            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    "Network message broadcast: ChainChunkResponse (start: {}, chunk: {}, total: {})",
                    start_index,
                    blocks.len(),
                    total_blocks
                );
            }
        }
    }

    pub fn add_peer(&mut self, address: String) -> bool {
        if address.is_empty()
            || address == ONE_SHOT_CLIENT_LISTEN_ADDRESS
            || address.len() > MAX_PEER_ADDRESS_LENGTH
        {
            return false;
        }

        if self.peers.len() >= MAX_NETWORK_PEERS {
            return false;
        }

        if self.peers.contains(&address) {
            return false;
        }

        self.peers.push(address);

        true
    }

    pub fn add_peer_identity(&mut self, node_id: String, listen_address: String) -> bool {
        matches!(
            self.register_peer_identity(node_id, listen_address,),
            PeerRegistrationOutcome::Registered
        )
    }

    pub fn register_peer_identity(
        &mut self,
        node_id: String,
        listen_address: String,
    ) -> PeerRegistrationOutcome {
        if !is_fixed_hex(&node_id, ADDRESS_HEX_LENGTH)
            || listen_address.is_empty()
            || listen_address.len() > MAX_PEER_ADDRESS_LENGTH
        {
            return PeerRegistrationOutcome::InvalidIdentity;
        }

        if listen_address == ONE_SHOT_CLIENT_LISTEN_ADDRESS {
            return PeerRegistrationOutcome::OneShotClient;
        }

        if self
            .peer_identities
            .iter()
            .any(|peer| peer.node_id == node_id && peer.listen_address == listen_address)
        {
            return PeerRegistrationOutcome::AlreadyRegistered;
        }

        if self
            .peer_identities
            .iter()
            .any(|peer| peer.node_id == node_id)
        {
            return PeerRegistrationOutcome::NodeIdConflict;
        }

        if self
            .peer_identities
            .iter()
            .any(|peer| peer.listen_address == listen_address)
        {
            return PeerRegistrationOutcome::ListenAddressConflict;
        }

        if self.peer_identities.len() >= MAX_NETWORK_PEERS
            || (!self.peers.contains(&listen_address) && self.peers.len() >= MAX_NETWORK_PEERS)
        {
            return PeerRegistrationOutcome::PeerLimitReached;
        }

        if !self.peers.contains(&listen_address) {
            self.peers.push(listen_address.clone());
        }

        self.peer_identities.push(PeerIdentity {
            node_id,
            listen_address,
        });

        PeerRegistrationOutcome::Registered
    }

    pub fn has_peer_node_id(&self, node_id: &str) -> bool {
        self.peer_identities
            .iter()
            .any(|peer| peer.node_id == node_id)
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn identified_peer_count(&self) -> usize {
        self.peer_identities.len()
    }

    pub async fn broadcast_to_peers(
        &mut self,
        wallet: &Wallet,
        listen_address: &str,
        message: NetworkMessage,
    ) -> (usize, usize) {
        if !Self::message_within_limits(&message) {
            println!("Network message rejected: message size or protocol limit exceeded");

            return (0, 0);
        }

        Self::print_message_summary(&message);

        let peer_addresses = self.peers.clone();

        println!("Actual P2P broadcast peer count: {}", peer_addresses.len());

        let result = tcp::TcpTransport::broadcast_authenticated(
            &peer_addresses,
            wallet,
            listen_address,
            &message,
        )
        .await;

        self.push_message_history(message.clone());

        self.push_inbox(message);

        result
    }

    pub fn broadcast(&mut self, message: NetworkMessage) {
        println!();

        if !Self::message_within_limits(&message) {
            println!("Network message rejected: message size or protocol limit exceeded");

            return;
        }

        Self::print_message_summary(&message);

        println!("Connected node count: {}", self.peers.len());

        self.push_message_history(message.clone());

        self.push_inbox(message);
    }

    pub fn receive(&mut self, message: NetworkMessage) {
        if !Self::message_within_limits(&message) {
            if matches!(&message, NetworkMessage::Handshake { .. }) {
                println!(
                    "Handshake rejected: fields, signature, network, protocol, or timestamp are invalid"
                );
            } else {
                println!("Network message rejected: message size or protocol limit exceeded");
            }

            return;
        }

        match &message {
            NetworkMessage::Handshake {
                node_id,
                listen_address,
                network_id,
                protocol_version,
                ..
            } => {
                let registration =
                    self.register_peer_identity(node_id.clone(), listen_address.clone());

                match registration {
                    PeerRegistrationOutcome::Registered => {
                        println!(
                            "Handshake received. Node: {} Network: {} Protocol: {} Peer registered",
                            node_id, network_id, protocol_version
                        );
                    }

                    PeerRegistrationOutcome::AlreadyRegistered => {
                        println!(
                            "Handshake received. Node: {} Network: {} Protocol: {} Peer already registered",
                            node_id, network_id, protocol_version
                        );
                    }

                    PeerRegistrationOutcome::OneShotClient => {
                        println!("Authenticated one-shot client; no peer registry entry created");
                    }

                    PeerRegistrationOutcome::NodeIdConflict => {
                        println!(
                            "Peer registration rejected: node identity is already registered with a different listen address"
                        );
                    }

                    PeerRegistrationOutcome::ListenAddressConflict => {
                        println!(
                            "Peer registration rejected: listen address is already registered with a different node identity"
                        );
                    }

                    PeerRegistrationOutcome::PeerLimitReached => {
                        println!("Peer registration rejected: peer limit reached");
                    }

                    PeerRegistrationOutcome::InvalidIdentity => {
                        println!(
                            "Peer registration rejected: peer identity or listen address is invalid"
                        );
                    }
                }
            }

            NetworkMessage::HandshakeAck {
                node_id,
                network_id,
                protocol_version,
                accepted,
                ..
            } => {
                println!(
                    "Handshake ACK received. Node: {} Network: {} Protocol: {} Accepted: {}",
                    node_id, network_id, protocol_version, accepted
                );
            }

            NetworkMessage::AccountStateRequest { address } => {
                println!("Account state request received. Address: {}", address);
            }

            NetworkMessage::AccountStateResponse {
                address,
                balance,
                nonce,
                tip_index,
                ..
            } => {
                println!(
                    "Account state response received. Address: {} Balance: {} Nonce: {} Tip: {}",
                    address, balance, nonce, tip_index
                );
            }
            NetworkMessage::Transaction(transaction) => {
                println!("New transaction received: {} KBN", transaction.amount);
            }

            NetworkMessage::TransactionAck {
                transaction_id,
                accepted,
                ..
            } => {
                println!(
                    "Transaction ACK received. Transaction: {} Accepted: {}",
                    transaction_id, accepted
                );
            }

            NetworkMessage::Block(block) => {
                println!("New block received. Index: {}", block.index);
            }

            NetworkMessage::SyncRequest => {
                println!("Blockchain synchronization request received.");
            }

            NetworkMessage::ChainChunkRequest { start_index } => {
                println!("Chain chunk request received. Start index: {}", start_index);
            }

            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    "Chain chunk response received. Start: {}, chunk: {}, total: {}",
                    start_index,
                    blocks.len(),
                    total_blocks
                );
            }
        }

        self.push_message_history(message);
    }

    pub fn fetch_messages(&mut self) -> Vec<NetworkMessage> {
        std::mem::take(&mut self.inbox)
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod account_state_network_tests {
    use super::*;

    fn address() -> String {
        "11".repeat(32)
    }

    fn tip_hash() -> String {
        "22".repeat(32)
    }

    #[test]
    fn valid_account_state_response_is_accepted() {
        let address = address();

        let response = NetworkMessage::AccountStateResponse {
            address: address.clone(),
            balance: 123_456,
            nonce: 7,
            tip_index: 42,
            tip_hash: tip_hash(),
        };

        assert!(Network::validate_account_state_response(
            &response, &address
        ));
    }

    #[test]
    fn account_state_response_for_different_address_is_rejected() {
        let expected_address = address();

        let response = NetworkMessage::AccountStateResponse {
            address: "33".repeat(32),
            balance: 123_456,
            nonce: 7,
            tip_index: 42,
            tip_hash: tip_hash(),
        };

        assert!(!Network::validate_account_state_response(
            &response,
            &expected_address
        ));
    }

    #[test]
    fn malformed_account_state_messages_are_rejected() {
        let invalid_request = NetworkMessage::AccountStateRequest {
            address: "invalid".to_string(),
        };

        assert!(!Network::message_within_limits(&invalid_request));

        let address = address();

        let invalid_response = NetworkMessage::AccountStateResponse {
            address: address.clone(),
            balance: 0,
            nonce: 0,
            tip_index: 0,
            tip_hash: "invalid".to_string(),
        };

        assert!(!Network::validate_account_state_response(
            &invalid_response,
            &address
        ));
    }
}
