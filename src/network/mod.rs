pub mod tcp;

use serde::{
    Deserialize,
    Serialize,
};

use crate::core::{
    Block,
    Transaction,
};
use crate::protocol::{
    is_fixed_hex,
    ADDRESS_HEX_LENGTH,
    MAX_NETWORK_INBOX_MESSAGES,
    MAX_NETWORK_MESSAGE_BYTES,
    MAX_NETWORK_MESSAGE_HISTORY,
    MAX_NETWORK_PEERS,
    MAX_PEER_ADDRESS_LENGTH,
    MAX_SYNC_BLOCKS_PER_MESSAGE,
};

const NETWORK_PROTOCOL_VERSION: u32 = 1;

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
        listen_address: String,
        protocol_version: u32,
    },

    HandshakeAck {
        node_id: String,
        protocol_version: u32,
        accepted: bool,
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

    pub fn protocol_version(
    ) -> u32 {
        NETWORK_PROTOCOL_VERSION
    }

    pub fn create_handshake(
        node_id: String,
        listen_address: String,
    ) -> NetworkMessage {
        NetworkMessage::Handshake {
            node_id,
            listen_address,
            protocol_version:
                NETWORK_PROTOCOL_VERSION,
        }
    }

    pub fn create_handshake_ack(
        node_id: String,
        accepted: bool,
    ) -> NetworkMessage {
        NetworkMessage::HandshakeAck {
            node_id,
            protocol_version:
                NETWORK_PROTOCOL_VERSION,
            accepted,
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
        listen_address: &str,
        protocol_version: u32,
    ) -> bool {
        is_fixed_hex(
            node_id,
            ADDRESS_HEX_LENGTH,
        )
            && !listen_address.is_empty()
            && listen_address.len()
                <= MAX_PEER_ADDRESS_LENGTH
            && protocol_version
                == NETWORK_PROTOCOL_VERSION
    }

    fn handshake_ack_fields_valid(
        node_id: &str,
        protocol_version: u32,
    ) -> bool {
        is_fixed_hex(
            node_id,
            ADDRESS_HEX_LENGTH,
        )
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
                listen_address,
                protocol_version,
            } => {
                Self::handshake_fields_valid(
                    node_id,
                    listen_address,
                    *protocol_version,
                )
            }

            NetworkMessage::HandshakeAck {
                node_id,
                protocol_version,
                ..
            } => {
                Self::handshake_ack_fields_valid(
                    node_id,
                    *protocol_version,
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

    fn print_message_summary(
        message: &NetworkMessage,
    ) {
        match message {
            NetworkMessage::Handshake {
                node_id,
                listen_address,
                protocol_version,
            } => {
                println!(
                    "📡 Network mesajı yayınlandı: Handshake node={} address={} protocol={}",
                    node_id,
                    listen_address,
                    protocol_version
                );
            }

            NetworkMessage::HandshakeAck {
                node_id,
                protocol_version,
                accepted,
            } => {
                println!(
                    "📡 Network mesajı yayınlandı: HandshakeAck node={} protocol={} accepted={}",
                    node_id,
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
        if !Self::handshake_fields_valid(
            &node_id,
            &listen_address,
            NETWORK_PROTOCOL_VERSION,
        ) {
            return false;
        }

        if self.peer_identities.len()
            >= MAX_NETWORK_PEERS
        {
            return false;
        }

        if self
            .peer_identities
            .iter()
            .any(
                |peer| {
                    peer.node_id
                        == node_id
                        || peer.listen_address
                            == listen_address
                },
            )
        {
            return false;
        }

        if !self.peers.contains(
            &listen_address,
        ) {
            if !self.add_peer(
                listen_address.clone(),
            ) {
                return false;
            }
        }

        self.peer_identities.push(
            PeerIdentity {
                node_id,
                listen_address,
            },
        );

        true
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
            println!(
                "❌ Network mesajı reddedildi: Mesaj boyutu veya protokol limiti aşıldı"
            );

            return;
        }

        match &message {
            NetworkMessage::Handshake {
                node_id,
                listen_address,
                protocol_version,
            } => {
                let registered =
                    self.add_peer_identity(
                        node_id.clone(),
                        listen_address.clone(),
                    );

                println!(
                    "📥 Handshake alındı. Node: {} Protocol: {} Kayıt: {}",
                    node_id,
                    protocol_version,
                    registered
                );
            }

            NetworkMessage::HandshakeAck {
                node_id,
                protocol_version,
                accepted,
            } => {
                println!(
                    "📥 Handshake ACK alındı. Node: {} Protocol: {} Kabul: {}",
                    node_id,
                    protocol_version,
                    accepted
                );
            }

            NetworkMessage::Transaction(
                transaction,
            ) => {
                println!(
                    "📥 Yeni transaction alındı: {} AION",
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