pub mod tcp;

use serde::{
    Deserialize,
    Serialize,
};

use crate::core::{Block, Transaction};
use crate::protocol::{
    MAX_NETWORK_INBOX_MESSAGES,
    MAX_NETWORK_MESSAGE_BYTES,
    MAX_NETWORK_MESSAGE_HISTORY,
    MAX_NETWORK_PEERS,
    MAX_PEER_ADDRESS_LENGTH,
    MAX_SYNC_BLOCKS_PER_MESSAGE,
};

#[allow(dead_code)]
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
)]
pub enum NetworkMessage {
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

#[derive(Debug, Default)]
pub struct Network {
    pub peers: Vec<String>,

    pub messages: Vec<NetworkMessage>,

    pub inbox: Vec<NetworkMessage>,
}

#[allow(dead_code)]
impl Network {
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),

            messages: Vec::new(),

            inbox: Vec::new(),
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

    pub fn peer_count(
        &self,
    ) -> usize {
        self.peers.len()
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
                "❌ Network mesajı reddedildi: Mesaj boyutu veya sync block limiti aşıldı"
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
                "❌ Network mesajı reddedildi: Mesaj boyutu veya sync block limiti aşıldı"
            );

            return;
        }

        match &message {
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