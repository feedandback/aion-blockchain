pub mod tcp;

pub const ONE_SHOT_CLIENT_LISTEN_ADDRESS: &str = "127.0.0.1:0";

use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use rand_core::{
    OsRng,
    RngCore,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::core::{
    Block,
    Transaction,
};
use crate::wallet::Wallet;
use crate::protocol::{
    is_fixed_hex,
    ADDRESS_HEX_LENGTH,
    MAX_NETWORK_INBOX_MESSAGES,
    MAX_NETWORK_MESSAGE_BYTES,
    MAX_HANDSHAKE_AGE_SECONDS,
    MAX_NETWORK_MESSAGE_HISTORY,
    MAX_NETWORK_PEERS,
    MAX_PEER_ADDRESS_LENGTH,
    MAX_SYNC_BLOCKS_PER_MESSAGE,
    NETWORK_ID,
    NETWORK_PROTOCOL_VERSION,
};

#[allow(dead_code)]
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
)]
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
}

#[derive(
    Debug,
    Clone,
)]
pub struct PeerIdentity {
    pub node_id: String,
    pub listen_address: String,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
)]
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

    pub peer_identities:
        Vec<PeerIdentity>,

    pub messages:
        Vec<NetworkMessage>,

    pub inbox:
        Vec<NetworkMessage>,
}

#[allow(dead_code)]
impl Network {
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),

            peer_identities:
                Vec::new(),

            messages: Vec::new(),

            inbox: Vec::new(),
        }
    }

    pub fn current_timestamp(
    ) -> u64 {
        SystemTime::now()
            .duration_since(
                UNIX_EPOCH,
            )
            .unwrap_or_default()
            .as_secs()
    }

    fn handshake_timestamp_valid(
        timestamp: u64,
    ) -> bool {
        let now =
            Self::current_timestamp();

        now.abs_diff(
            timestamp,
        ) <= MAX_HANDSHAKE_AGE_SECONDS
    }

    pub fn generate_handshake_challenge(
    ) -> String {
        let mut bytes =
            [0u8; 32];

        OsRng.fill_bytes(
            &mut bytes,
        );

        hex::encode(
            bytes,
        )
    }

    pub fn handshake_challenge(
        message: &NetworkMessage,
    ) -> Option<&str> {
        match message {
            NetworkMessage::Handshake {
                challenge,
                ..
            } => {
                Some(
                    challenge,
                )
            }

            _ => None,
        }
    }

    pub fn protocol_version(
    ) -> u32 {
        NETWORK_PROTOCOL_VERSION
    }

    pub fn create_handshake(
        wallet: &Wallet,
        listen_address: String,
    ) -> NetworkMessage {
        let node_id =
            wallet
                .node_id()
                .to_string();

        let public_key =
            wallet
                .public_key_hex();

        let timestamp =
            Self::current_timestamp();

        let challenge =
            Self::generate_handshake_challenge();

        let signature =
            wallet
                .sign_node_handshake(
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
            network_id:
                NETWORK_ID.to_string(),
            protocol_version:
                NETWORK_PROTOCOL_VERSION,
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
        let node_id =
            wallet
                .node_id()
                .to_string();

        let public_key =
            wallet
                .public_key_hex();

        let timestamp =
            Self::current_timestamp();

        let signature =
            wallet
                .sign_node_handshake_ack(
                    NETWORK_ID,
                    NETWORK_PROTOCOL_VERSION,
                    timestamp,
                    &challenge,
                    accepted,
                );

        NetworkMessage::HandshakeAck {
            node_id,
            public_key,
            network_id:
                NETWORK_ID.to_string(),
            protocol_version:
                NETWORK_PROTOCOL_VERSION,
            timestamp,
            challenge,
            accepted,
            signature,
        }
    }

    pub fn validate_handshake_ack(
        message: &NetworkMessage,
        expected_challenge: &str,
    ) -> bool {
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
                    && challenge
                        == expected_challenge
                    && Self::handshake_ack_fields_valid(
                        node_id,
                        network_id,
                        *protocol_version,
                    )
                    && Self::handshake_timestamp_valid(
                        *timestamp,
                    )
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

    fn push_message_history(
        &mut self,
        message: NetworkMessage,
    ) {
        if self.messages.len()
            >= MAX_NETWORK_MESSAGE_HISTORY
        {
            self.messages.remove(0);
        }

        self.messages.push(message);
    }

    fn push_inbox(
        &mut self,
        message: NetworkMessage,
    ) {
        if self.inbox.len()
            >= MAX_NETWORK_INBOX_MESSAGES
        {
            self.inbox.remove(0);
        }

        self.inbox.push(message);
    }

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
        is_fixed_hex(
            node_id,
            ADDRESS_HEX_LENGTH,
        )
            && is_fixed_hex(
                public_key,
                ADDRESS_HEX_LENGTH,
            )
            && !listen_address.is_empty()
            && listen_address.len()
                <= MAX_PEER_ADDRESS_LENGTH
            && network_id
                == NETWORK_ID
            && protocol_version
                == NETWORK_PROTOCOL_VERSION
            && Self::handshake_timestamp_valid(
                timestamp,
            )
            && is_fixed_hex(
                challenge,
                64,
            )
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

    fn handshake_ack_fields_valid(
        node_id: &str,
        network_id: &str,
        protocol_version: u32,
    ) -> bool {
        is_fixed_hex(
            node_id,
            ADDRESS_HEX_LENGTH,
        )
            && network_id
                == NETWORK_ID
            && protocol_version
                == NETWORK_PROTOCOL_VERSION
    }

    fn message_within_limits(
        message: &NetworkMessage,
    ) -> bool {
        let serialized_size_ok =
            serde_json::to_vec(
                message,
            )
            .map(
                |bytes| {
                    bytes.len()
                        <= MAX_NETWORK_MESSAGE_BYTES
                },
            )
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
            } => {
                Self::handshake_fields_valid(
                    node_id,
                    public_key,
                    listen_address,
                    network_id,
                    *protocol_version,
                    *timestamp,
                    challenge,
                    signature,
                )
            }

            NetworkMessage::HandshakeAck {
                node_id,
                network_id,
                protocol_version,
                challenge,
                ..
            } => {
                Self::handshake_ack_fields_valid(
                    node_id,
                    network_id,
                    *protocol_version,
                )
                    && is_fixed_hex(
                        challenge,
                        64,
                    )
            }

            NetworkMessage::ChainChunkResponse {
                blocks,
                ..
            } => {
                blocks.len()
                    <= MAX_SYNC_BLOCKS_PER_MESSAGE
            }

            NetworkMessage::ChainChunkRequest {
                ..
            } => true,

            _ => true,
        }
    }

    pub fn validate_handshake(
        message: &NetworkMessage,
    ) -> bool {
        matches!(
            message,
            NetworkMessage::Handshake {
                ..
            }
        ) && Self::message_within_limits(
            message,
        )
    }

    fn print_message_summary(
        message: &NetworkMessage,
    ) {
        match message {
            NetworkMessage::Handshake {
                node_id,
                listen_address,
                network_id,
                protocol_version,
                ..
            } => {
                println!(
                    "📡 Network mesajı yayınlandı: Handshake node={} address={} network={} protocol={}",
                    node_id,
                    listen_address,
                    network_id,
                    protocol_version
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
                    "📡 Network mesajı yayınlandı: HandshakeAck node={} network={} protocol={} accepted={}",
                    node_id,
                    network_id,
                    protocol_version,
                    accepted
                );
            }

            NetworkMessage::Transaction(
                transaction,
            ) => {
                println!(
                    "📡 Network mesajı yayınlandı: Transaction {}",
                    transaction.id
                );
            }

            NetworkMessage::Block(
                block,
            ) => {
                println!(
                    "📡 Network mesajı yayınlandı: Block {}",
                    block.index
                );
            }

            NetworkMessage::SyncRequest => {
                println!(
                    "📡 Network mesajı yayınlandı: SyncRequest"
                );
            }

            NetworkMessage::ChainChunkRequest {
                start_index,
            } => {
                println!(
                    "📡 Network mesajı yayınlandı: ChainChunkRequest (start: {})",
                    start_index
                );
            }

            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    "📡 Network mesajı yayınlandı: ChainChunkResponse (start: {}, chunk: {}, total: {})",
                    start_index,
                    blocks.len(),
                    total_blocks
                );
            }
        }
    }

    pub fn add_peer(
        &mut self,
        address: String,
    ) -> bool {
        if address.is_empty()
            || address
                == ONE_SHOT_CLIENT_LISTEN_ADDRESS
            || address.len()
                > MAX_PEER_ADDRESS_LENGTH
        {
            return false;
        }

        if self.peers.len()
            >= MAX_NETWORK_PEERS
        {
            return false;
        }

        if self.peers.contains(
            &address,
        ) {
            return false;
        }

        self.peers.push(address);

        true
    }

    pub fn add_peer_identity(
        &mut self,
        node_id: String,
        listen_address: String,
    ) -> bool {
        matches!(
            self.register_peer_identity(
                node_id,
                listen_address,
            ),
            PeerRegistrationOutcome::Registered
        )
    }

    pub fn register_peer_identity(
        &mut self,
        node_id: String,
        listen_address: String,
    ) -> PeerRegistrationOutcome {
        if !is_fixed_hex(
            &node_id,
            ADDRESS_HEX_LENGTH,
        )
            || listen_address.is_empty()
            || listen_address.len()
                > MAX_PEER_ADDRESS_LENGTH
        {
            return PeerRegistrationOutcome::InvalidIdentity;
        }

        if listen_address
            == ONE_SHOT_CLIENT_LISTEN_ADDRESS
        {
            return PeerRegistrationOutcome::OneShotClient;
        }

        if self
            .peer_identities
            .iter()
            .any(
                |peer| {
                    peer.node_id
                        == node_id
                        && peer.listen_address
                            == listen_address
                },
            )
        {
            return PeerRegistrationOutcome::AlreadyRegistered;
        }

        if self
            .peer_identities
            .iter()
            .any(
                |peer| {
                    peer.node_id
                        == node_id
                },
            )
        {
            return PeerRegistrationOutcome::NodeIdConflict;
        }

        if self
            .peer_identities
            .iter()
            .any(
                |peer| {
                    peer.listen_address
                        == listen_address
                },
            )
        {
            return PeerRegistrationOutcome::ListenAddressConflict;
        }

        if self.peer_identities.len()
            >= MAX_NETWORK_PEERS
            || (!self.peers.contains(
                &listen_address,
            ) && self.peers.len()
                >= MAX_NETWORK_PEERS)
        {
            return PeerRegistrationOutcome::PeerLimitReached;
        }

        if !self.peers.contains(
            &listen_address,
        ) {
            self.peers.push(
                listen_address.clone(),
            );
        }

        self.peer_identities.push(
            PeerIdentity {
                node_id,
                listen_address,
            },
        );

        PeerRegistrationOutcome::Registered
    }

    pub fn has_peer_node_id(
        &self,
        node_id: &str,
    ) -> bool {
        self.peer_identities
            .iter()
            .any(
                |peer| {
                    peer.node_id
                        == node_id
                },
            )
    }

    pub fn peer_count(
        &self,
    ) -> usize {
        self.peers.len()
    }

    pub fn identified_peer_count(
        &self,
    ) -> usize {
        self.peer_identities
            .len()
    }

    pub async fn broadcast_to_peers(
        &mut self,
        wallet: &Wallet,
        listen_address: &str,
        message: NetworkMessage,
    ) -> (
        usize,
        usize,
    ) {
        if !Self::message_within_limits(
            &message,
        ) {
            println!(
                "❌ Network mesajı reddedildi: Mesaj boyutu veya protokol limiti aşıldı"
            );

            return (
                0,
                0,
            );
        }

        Self::print_message_summary(
            &message,
        );

        let peer_addresses =
            self.peers.clone();

        println!(
            "Gerçek P2P broadcast peer sayısı: {}",
            peer_addresses.len()
        );

        let result =
            tcp::TcpTransport::broadcast_authenticated(
                &peer_addresses,
                wallet,
                listen_address,
                &message,
            )
            .await;

        self.push_message_history(
            message.clone(),
        );

        self.push_inbox(
            message,
        );

        result
    }

    pub fn broadcast(
        &mut self,
        message: NetworkMessage,
    ) {
        println!();

        if !Self::message_within_limits(
            &message,
        ) {
            println!(
                "❌ Network mesajı reddedildi: Mesaj boyutu veya protokol limiti aşıldı"
            );

            return;
        }

        Self::print_message_summary(
            &message,
        );

        println!(
            "Bağlı node sayısı: {}",
            self.peers.len()
        );

        self.push_message_history(
            message.clone(),
        );

        self.push_inbox(message);
    }

    pub fn receive(
        &mut self,
        message: NetworkMessage,
    ) {
        if !Self::message_within_limits(
            &message,
        ) {
            if matches!(
                &message,
                NetworkMessage::Handshake {
                    ..
                }
            ) {
                println!(
                    "❌ Handshake reddedildi: alan, imza, network, protocol veya timestamp geçersiz"
                );
            } else {
                println!(
                    "❌ Network mesajı reddedildi: Mesaj boyutu veya protokol limiti aşıldı"
                );
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
                    self.register_peer_identity(
                        node_id.clone(),
                        listen_address.clone(),
                    );

                match registration {
                    PeerRegistrationOutcome::Registered => {
                        println!(
                            "📥 Handshake alındı. Node: {} Network: {} Protocol: {} Peer kaydedildi",
                            node_id,
                            network_id,
                            protocol_version
                        );
                    }

                    PeerRegistrationOutcome::AlreadyRegistered => {
                        println!(
                            "📥 Handshake alındı. Node: {} Network: {} Protocol: {} Peer zaten kayıtlı",
                            node_id,
                            network_id,
                            protocol_version
                        );
                    }

                    PeerRegistrationOutcome::OneShotClient => {
                        println!(
                            "📥 Authenticated one-shot client; peer registry kaydı oluşturulmadı"
                        );
                    }

                    PeerRegistrationOutcome::NodeIdConflict => {
                        println!(
                            "❌ Peer kaydı reddedildi: node kimliği farklı listen address ile kayıtlı"
                        );
                    }

                    PeerRegistrationOutcome::ListenAddressConflict => {
                        println!(
                            "❌ Peer kaydı reddedildi: listen address farklı node kimliğiyle kayıtlı"
                        );
                    }

                    PeerRegistrationOutcome::PeerLimitReached => {
                        println!(
                            "❌ Peer kaydı reddedildi: peer limiti dolu"
                        );
                    }

                    PeerRegistrationOutcome::InvalidIdentity => {
                        println!(
                            "❌ Peer kaydı reddedildi: peer kimliği veya listen address geçersiz"
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
                    "📥 Handshake ACK alındı. Node: {} Network: {} Protocol: {} Kabul: {}",
                    node_id,
                    network_id,
                    protocol_version,
                    accepted
                );
            }

            NetworkMessage::Transaction(
                transaction,
            ) => {
                println!(
                    "📥 Yeni transaction alındı: {} KBN",
                    transaction.amount
                );
            }

            NetworkMessage::Block(
                block,
            ) => {
                println!(
                    "📥 Yeni block alındı. Index: {}",
                    block.index
                );
            }

            NetworkMessage::SyncRequest => {
                println!(
                    "📥 Blockchain senkronizasyon isteği geldi."
                );
            }

            NetworkMessage::ChainChunkRequest {
                start_index,
            } => {
                println!(
                    "📥 Chain chunk isteği geldi. Başlangıç index: {}",
                    start_index
                );
            }

            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    "📥 Chain chunk cevabı geldi. Start: {}, chunk: {}, total: {}",
                    start_index,
                    blocks.len(),
                    total_blocks
                );
            }
        }

        self.push_message_history(
            message,
        );
    }

    pub fn fetch_messages(
        &mut self,
    ) -> Vec<NetworkMessage> {
        std::mem::take(
            &mut self.inbox,
        )
    }

    pub fn message_count(
        &self,
    ) -> usize {
        self.messages.len()
    }
}
