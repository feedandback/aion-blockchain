use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::{Mutex, Semaphore};

use crate::bootstrap::canonical_bootstrap;
use crate::core::Block;
use crate::network::tcp::TcpTransport;
use crate::network::{Network, NetworkMessage, ONE_SHOT_CLIENT_LISTEN_ADDRESS};
use crate::node::Node;
use crate::protocol::{MAX_CONCURRENT_NETWORK_CONNECTIONS, MAX_SYNC_BLOCKS_PER_MESSAGE};
use crate::storage::Storage;
use crate::validator::{ValidatorIdentity, ValidatorKeystore};
use crate::wallet::Wallet;

pub const VALIDATOR_PASSWORD_ENV: &str = "KYBERNETES_VALIDATOR_PASSWORD";
pub const NODE_LISTEN_ADDRESS_ENV: &str = "KYBERNETES_LISTEN_ADDRESS";
pub const NODE_PEERS_ENV: &str = "KYBERNETES_PEERS";
const CHAIN_SYNC_INTERVAL: Duration = Duration::from_secs(30);
const CHAIN_SYNC_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const BLOCK_PRODUCTION_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    Observer,
    Validator,
}

pub struct RuntimeConfig {
    pub listen_address: String,
    pub peers: Vec<String>,
}

pub struct NodeRuntime {
    node: Node,
    validator_identity: Option<ValidatorIdentity>,
    validator_keystore_locked: bool,
    data_directory: PathBuf,
    chain_sync_active: bool,
}

struct ChainChunk {
    start_index: u64,
    total_blocks: u64,
    blocks: Vec<Block>,
}

enum RuntimeAction {
    Reply(NetworkMessage),
    Broadcast(NetworkMessage),
}

struct ProcessOutcome {
    actions: Vec<RuntimeAction>,
}

#[allow(dead_code)]
impl NodeRuntime {
    pub fn initialize_at(
        data_directory: impl Into<PathBuf>,
        validator_password: Option<&str>,
    ) -> Result<Self, String> {
        let data_directory = data_directory.into();
        let bootstrap = canonical_bootstrap()?;
        let keystore = ValidatorKeystore::at(&data_directory);
        let validator_keystore_locked = keystore.exists()? && validator_password.is_none();
        let validator_identity = match validator_password {
            Some(password) => keystore.load_authorized(
                password,
                &bootstrap.consensus,
                &bootstrap.genesis_fingerprint,
            )?,
            None => None,
        };

        let mut node = Node::new_with_data_directory(
            bootstrap.blockchain,
            bootstrap.state,
            bootstrap.consensus,
            data_directory.clone(),
        );

        match Storage::load_blockchain_from(&data_directory)? {
            Some(chain) => node.restore_chain_from_storage(chain)?,
            None => Storage::save_blockchain_to(&data_directory, &node.blockchain.chain)?,
        }

        Ok(Self {
            node,
            validator_identity,
            validator_keystore_locked,
            data_directory,
            chain_sync_active: false,
        })
    }

    pub fn from_node(
        node: Node,
        validator_identity: Option<ValidatorIdentity>,
    ) -> Result<Self, String> {
        if let Some(identity) = validator_identity.as_ref() {
            if !node.consensus.is_validator_allowed(identity.address()) {
                return Err("Runtime validator kimliği node consensus setinde yok".into());
            }
        }

        let data_directory = node.storage_directory().to_path_buf();

        Ok(Self {
            node,
            validator_identity,
            validator_keystore_locked: false,
            data_directory,
            chain_sync_active: false,
        })
    }

    pub fn role(&self) -> NodeRole {
        if self.validator_identity.is_some() {
            NodeRole::Validator
        } else {
            NodeRole::Observer
        }
    }

    pub fn validator_address(&self) -> Option<&str> {
        self.validator_identity
            .as_ref()
            .map(ValidatorIdentity::address)
    }

    pub fn validator_keystore_locked(&self) -> bool {
        self.validator_keystore_locked
    }

    pub fn data_directory(&self) -> &Path {
        &self.data_directory
    }

    pub fn node(&self) -> &Node {
        &self.node
    }

    pub fn try_produce_block(&mut self, timestamp: u64) -> Result<Block, String> {
        if self.chain_sync_active {
            return Err("Aktif chain sync sırasında block üretilemez".into());
        }

        let identity = self
            .validator_identity
            .as_ref()
            .ok_or("Observer node block üretemez; validator key yüklenmemiş")?;
        self.node.produce_block(timestamp, identity.wallet())
    }

    pub async fn run(self, config: RuntimeConfig) -> Result<(), String> {
        let RuntimeConfig {
            listen_address,
            peers,
        } = config;
        if listen_address
            == ONE_SHOT_CLIENT_LISTEN_ADDRESS
        {
            return Err(
                "Production node listener one-shot client address kullanamaz"
                    .into(),
            );
        }
        let listener = TcpTransport::bind(&listen_address).await?;
        let p2p_identity = Arc::new(Wallet::new());
        let runtime = Arc::new(Mutex::new(self));
        let connection_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_NETWORK_CONNECTIONS));
        let mut sync_peers = Vec::new();

        {
            let mut locked = runtime.lock().await;
            for peer in peers {
                if locked.node.add_peer(peer.clone()) {
                    sync_peers.push(peer);
                }
            }
            locked.chain_sync_active = !sync_peers.is_empty();

            println!("Kybernetes node runtime başlatıldı: {listen_address}");
            println!("Data directory: {}", locked.data_directory.display());
            match locked.role() {
                NodeRole::Observer => {
                    println!("Node rolü: observer/full node");
                    if locked.validator_keystore_locked {
                        println!(
                            "Validator keystore mevcut fakat {} tanımlı değil; observer modu aktif.",
                            VALIDATOR_PASSWORD_ENV
                        );
                    }
                }
                NodeRole::Validator => println!(
                    "Node rolü: validator ({})",
                    locked.validator_address().unwrap_or("unknown")
                ),
            }
        }

        if !sync_peers.is_empty() {
            let peer_runtime = runtime.clone();
            let peer_identity = p2p_identity.clone();
            let sync_listen_address = listen_address.clone();
            tokio::spawn(async move {
                loop {
                    {
                        let mut locked = peer_runtime.lock().await;
                        locked.chain_sync_active = true;
                    }

                    let mut synchronized = false;
                    for peer in &sync_peers {
                        match Self::sync_from_peer(
                            peer_runtime.clone(),
                            peer_identity.clone(),
                            sync_listen_address.clone(),
                            peer.clone(),
                        )
                        .await
                        {
                            Ok(()) => synchronized = true,
                            Err(error) => {
                                println!("Peer sync başarısız ({peer}): {error}");
                            }
                        }
                    }

                    {
                        let mut locked = peer_runtime.lock().await;
                        locked.chain_sync_active = !synchronized;
                    }

                    tokio::time::sleep(if synchronized {
                        CHAIN_SYNC_INTERVAL
                    } else {
                        CHAIN_SYNC_RETRY_INTERVAL
                    })
                    .await;
                }
            });
        }

        let mut production_tick = tokio::time::interval(BLOCK_PRODUCTION_POLL_INTERVAL);
        production_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                shutdown = tokio::signal::ctrl_c() => {
                    shutdown.map_err(|error| format!("Shutdown sinyali dinlenemedi: {error}"))?;
                    println!("Kybernetes node runtime kapatılıyor.");
                    return Ok(());
                }
                _ = production_tick.tick() => {
                    let block = {
                        let mut locked = runtime.lock().await;
                        locked.try_produce_pending_block()
                    };

                    match block {
                        Ok(Some(block)) => {
                            Self::broadcast_message(
                                &runtime,
                                &p2p_identity,
                                &listen_address,
                                &NetworkMessage::Block(block),
                            )
                            .await;
                        }
                        Ok(None) => {}
                        Err(error) => println!("Validator block üretim denemesi başarısız: {error}"),
                    }
                }
                accepted = TcpTransport::accept_connection(&listener) => {
                    match accepted {
                        Ok((stream, peer_address)) => {
                            let permit = match connection_limit.clone().try_acquire_owned() {
                                Ok(permit) => permit,
                                Err(_) => {
                                    println!("P2P bağlantısı reddedildi: eşzamanlı bağlantı limiti dolu");
                                    drop(stream);
                                    continue;
                                }
                            };
                            let session_runtime = runtime.clone();
                            let session_identity = p2p_identity.clone();
                            let session_listen_address = listen_address.clone();
                            tokio::spawn(async move {
                                let _permit = permit;
                                match TcpTransport::authenticate_incoming(
                                    stream,
                                    peer_address.clone(),
                                    &session_identity,
                                )
                                .await
                                {
                                    Ok((stream, _, handshake)) => {
                                        if let Err(error) = Self::handle_session(
                                            session_runtime,
                                            session_identity,
                                            session_listen_address,
                                            stream,
                                            handshake,
                                        )
                                        .await
                                        {
                                            println!("P2P session kapandı ({peer_address}): {error}");
                                        }
                                    }
                                    Err(error) => {
                                        println!("P2P bağlantısı reddedildi ({peer_address}): {error}");
                                    }
                                }
                            });
                        }
                        Err(error) => {
                            println!("P2P bağlantısı reddedildi: {error}");
                        }
                    }
                }
            }
        }
    }

    async fn handle_session(
        runtime: Arc<Mutex<Self>>,
        p2p_identity: Arc<Wallet>,
        listen_address: String,
        mut stream: TcpStream,
        handshake: NetworkMessage,
    ) -> Result<(), String> {
        {
            let mut locked = runtime.lock().await;
            locked.node.receive_message(handshake);
        }

        loop {
            let Some(message) =
                TcpTransport::read_message_or_eof(
                    &mut stream,
                )
                .await?
            else {
                return Ok(());
            };

            if matches!(&message, NetworkMessage::ChainChunkResponse { .. }) {
                return Err("Beklenmeyen chain chunk response reddedildi".into());
            }
            let outcome = {
                let mut locked = runtime.lock().await;
                locked.process_message(message)?
            };
            Self::execute_actions(
                &runtime,
                &p2p_identity,
                &listen_address,
                &mut stream,
                outcome.actions,
            )
            .await?;
        }
    }

    async fn sync_from_peer(
        runtime: Arc<Mutex<Self>>,
        p2p_identity: Arc<Wallet>,
        listen_address: String,
        peer: String,
    ) -> Result<(), String> {
        let mut stream =
            TcpTransport::connect_authenticated(&peer, &p2p_identity, &listen_address).await?;
        let status = Self::request_chunk(&mut stream, 0).await?;
        let (local_height, local_genesis_hash, local_tip_hash) = {
            let locked = runtime.lock().await;
            let local_height = u64::try_from(locked.node.blockchain.chain.len())
                .map_err(|_| "Yerel blockchain uzunluğu u64 aralığını aşıyor")?;
            let local_genesis_hash = locked
                .node
                .blockchain
                .chain
                .first()
                .ok_or("Yerel blockchain boş")?
                .hash
                .clone();
            let local_tip_hash = locked
                .node
                .blockchain
                .chain
                .last()
                .ok_or("Yerel blockchain boş")?
                .hash
                .clone();
            (local_height, local_genesis_hash, local_tip_hash)
        };

        let remote_genesis = status
            .blocks
            .first()
            .ok_or("Remote status chunk genesis içermiyor")?;
        if remote_genesis.index != 0
            || remote_genesis.hash != remote_genesis.calculate_hash()
            || remote_genesis.hash != local_genesis_hash
        {
            return Err("Remote genesis canonical genesis ile uyuşmuyor".into());
        }

        if status.total_blocks < local_height {
            return Ok(());
        }

        if status.total_blocks == local_height {
            let mut remote_chain = status.blocks;
            let mut next_index = u64::try_from(remote_chain.len())
                .map_err(|_| "Remote chain uzunluğu u64 aralığını aşıyor")?;
            while next_index < status.total_blocks {
                let chunk = Self::request_chunk(&mut stream, next_index).await?;
                if chunk.total_blocks != status.total_blocks {
                    return Err("Chain sync sırasında remote toplam block sayısı değişti".into());
                }
                remote_chain.extend(chunk.blocks);
                next_index = u64::try_from(remote_chain.len())
                    .map_err(|_| "Remote chain uzunluğu u64 aralığını aşıyor")?;
            }
            let remote_tip_hash = remote_chain
                .last()
                .ok_or("Remote blockchain boş")?
                .hash
                .clone();
            if remote_tip_hash != local_tip_hash {
                return Err("Eşit uzunlukta farklı remote fork reddedildi".into());
            }
            let mut locked = runtime.lock().await;
            locked.node.verify_equal_chain(remote_chain)?;
            return Ok(());
        }

        let anchor_index = local_height.checked_sub(1).ok_or("Yerel blockchain boş")?;
        let anchor_chunk = if anchor_index < status.blocks.len() as u64 {
            None
        } else {
            let chunk = Self::request_chunk(&mut stream, anchor_index).await?;
            if chunk.total_blocks < status.total_blocks {
                return Err("Chain sync sırasında remote toplam block sayısı küçüldü".into());
            }
            Some(chunk)
        };
        let anchor_blocks = anchor_chunk
            .as_ref()
            .map(|chunk| chunk.blocks.as_slice())
            .unwrap_or(status.blocks.as_slice());
        let anchor_offset = usize::try_from(
            anchor_index
                .checked_sub(anchor_chunk.as_ref().map_or(0, |chunk| chunk.start_index))
                .ok_or("Remote anchor index geriye taştı")?,
        )
        .map_err(|_| "Remote anchor offset geçersiz")?;
        let remote_anchor = anchor_blocks
            .get(anchor_offset)
            .ok_or("Remote chain yerel tip anchor block'unu içermiyor")?;

        if remote_anchor.hash == local_tip_hash {
            let anchor_suffix = anchor_blocks
                .iter()
                .skip(anchor_offset + 1)
                .cloned()
                .collect::<Vec<_>>();
            return Self::extend_from_common_tip(
                &runtime,
                &mut stream,
                status,
                local_height,
                anchor_suffix,
            )
            .await;
        }

        Self::replace_from_full_chain_stream(runtime, &mut stream, status).await
    }

    async fn request_chunk(
        stream: &mut TcpStream,
        requested_start: u64,
    ) -> Result<ChainChunk, String> {
        TcpTransport::send_message(
            stream,
            &NetworkMessage::ChainChunkRequest {
                start_index: requested_start,
            },
        )
        .await?;
        let message = TcpTransport::read_message(stream).await?;
        let chunk = match message {
            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => ChainChunk {
                start_index,
                total_blocks,
                blocks,
            },
            _ => return Err("Chain sync oturumunda beklenmeyen mesaj reddedildi".into()),
        };
        Self::validate_chunk(&chunk, requested_start)?;
        Ok(chunk)
    }

    fn validate_chunk(chunk: &ChainChunk, expected_start: u64) -> Result<(), String> {
        if chunk.start_index != expected_start {
            return Err("Remote chain chunk start_index değeri beklenenle uyuşmuyor".into());
        }
        if chunk.total_blocks == 0 || chunk.start_index > chunk.total_blocks {
            return Err("Remote chain chunk toplam block sayısı geçersiz".into());
        }
        if chunk.blocks.len() > MAX_SYNC_BLOCKS_PER_MESSAGE {
            return Err("Remote chain chunk 256 block limitini aşıyor".into());
        }
        let block_count = u64::try_from(chunk.blocks.len())
            .map_err(|_| "Remote chain chunk uzunluğu geçersiz")?;
        let end = chunk
            .start_index
            .checked_add(block_count)
            .ok_or("Remote chain chunk index overflow")?;
        if end > chunk.total_blocks || (chunk.blocks.is_empty() && end < chunk.total_blocks) {
            return Err("Remote chain chunk uzunluğu toplam block sayısıyla uyumsuz".into());
        }
        for (offset, block) in chunk.blocks.iter().enumerate() {
            let expected_index = chunk
                .start_index
                .checked_add(offset as u64)
                .ok_or("Remote block index overflow")?;
            if block.index != expected_index {
                return Err("Remote chain chunk block sırası geçersiz".into());
            }
        }
        Ok(())
    }

    async fn extend_from_common_tip(
        runtime: &Arc<Mutex<Self>>,
        stream: &mut TcpStream,
        status: ChainChunk,
        local_height: u64,
        anchor_suffix: Vec<Block>,
    ) -> Result<(), String> {
        let (mut candidate, original_height) = {
            let locked = runtime.lock().await;
            (
                locked.node.sync_candidate_snapshot(),
                locked.node.blockchain.chain.len(),
            )
        };
        for block in anchor_suffix {
            candidate.apply_block_to_sync_candidate(block)?;
        }
        let mut next_index = u64::try_from(candidate.blockchain.chain.len())
            .map_err(|_| "Candidate blockchain uzunluğu u64 aralığını aşıyor")?;
        if next_index < local_height {
            return Err("Candidate blockchain yerel tip gerisinde kaldı".into());
        }
        while next_index < status.total_blocks {
            let chunk = Self::request_chunk(stream, next_index).await?;
            if chunk.total_blocks < status.total_blocks {
                return Err("Chain sync sırasında remote toplam block sayısı küçüldü".into());
            }
            let remaining = usize::try_from(status.total_blocks - next_index)
                .map_err(|_| "Remote kalan block sayısı geçersiz")?;
            for block in chunk.blocks.into_iter().take(remaining) {
                candidate.apply_block_to_sync_candidate(block)?;
            }
            next_index = u64::try_from(candidate.blockchain.chain.len())
                .map_err(|_| "Candidate blockchain uzunluğu u64 aralığını aşıyor")?;
        }

        let mut locked = runtime.lock().await;
        if locked.node.blockchain.chain.len() != original_height {
            return Err("Chain sync sırasında yerel tip değişti".into());
        }
        locked
            .node
            .adopt_validated_sync_candidate(candidate.blockchain, candidate.state)
    }

    async fn replace_from_full_chain_stream(
        runtime: Arc<Mutex<Self>>,
        stream: &mut TcpStream,
        first_chunk: ChainChunk,
    ) -> Result<(), String> {
        let original_height = {
            let locked = runtime.lock().await;
            locked.node.blockchain.chain.len()
        };
        let bootstrap = canonical_bootstrap()?;
        let mut candidate = Node::new_with_data_directory(
            bootstrap.blockchain,
            bootstrap.state,
            bootstrap.consensus,
            PathBuf::new(),
        );
        let candidate_genesis_hash = candidate
            .blockchain
            .chain
            .first()
            .ok_or("Canonical candidate genesis bulunamadı")?
            .hash
            .clone();
        let remote_genesis = first_chunk
            .blocks
            .first()
            .ok_or("Remote full-chain chunk genesis içermiyor")?;
        if remote_genesis.hash != candidate_genesis_hash
            || remote_genesis.hash != remote_genesis.calculate_hash()
        {
            return Err("Remote full-chain genesis geçersiz".into());
        }

        for block in first_chunk.blocks.into_iter().skip(1) {
            candidate.apply_block_to_sync_candidate(block)?;
        }
        let mut next_index = u64::try_from(candidate.blockchain.chain.len())
            .map_err(|_| "Candidate blockchain uzunluğu u64 aralığını aşıyor")?;
        while next_index < first_chunk.total_blocks {
            let chunk = Self::request_chunk(stream, next_index).await?;
            if chunk.total_blocks < first_chunk.total_blocks {
                return Err("Chain sync sırasında remote toplam block sayısı küçüldü".into());
            }
            let remaining = usize::try_from(first_chunk.total_blocks - next_index)
                .map_err(|_| "Remote kalan block sayısı geçersiz")?;
            for block in chunk.blocks.into_iter().take(remaining) {
                candidate.apply_block_to_sync_candidate(block)?;
            }
            next_index = u64::try_from(candidate.blockchain.chain.len())
                .map_err(|_| "Candidate blockchain uzunluğu u64 aralığını aşıyor")?;
        }

        let mut locked = runtime.lock().await;
        if locked.node.blockchain.chain.len() != original_height {
            return Err("Chain sync sırasında yerel tip değişti".into());
        }
        locked
            .node
            .adopt_validated_sync_candidate(candidate.blockchain, candidate.state)
    }

    async fn execute_actions(
        runtime: &Arc<Mutex<Self>>,
        p2p_identity: &Wallet,
        listen_address: &str,
        stream: &mut TcpStream,
        actions: Vec<RuntimeAction>,
    ) -> Result<(), String> {
        for action in actions {
            match action {
                RuntimeAction::Reply(message) => {
                    TcpTransport::send_message(stream, &message).await?;
                }
                RuntimeAction::Broadcast(message) => {
                    Self::broadcast_message(runtime, p2p_identity, listen_address, &message).await;
                }
            }
        }

        Ok(())
    }

    async fn broadcast_message(
        runtime: &Arc<Mutex<Self>>,
        p2p_identity: &Wallet,
        listen_address: &str,
        message: &NetworkMessage,
    ) {
        let peers = {
            let locked = runtime.lock().await;
            locked.node.network.peers.clone()
        };
        let _ =
            TcpTransport::broadcast_authenticated(&peers, p2p_identity, listen_address, message)
                .await;
    }

    fn process_message(&mut self, message: NetworkMessage) -> Result<ProcessOutcome, String> {
        let mut outcome = ProcessOutcome {
            actions: Vec::new(),
        };

        match message {
            NetworkMessage::ChainChunkRequest { start_index } => {
                outcome.actions.push(RuntimeAction::Reply(
                    self.chain_chunk_response(start_index)?,
                ));
            }
            NetworkMessage::SyncRequest => {
                outcome
                    .actions
                    .push(RuntimeAction::Reply(self.chain_chunk_response(0)?));
            }
            NetworkMessage::ChainChunkResponse { .. } => {
                return Err("Beklenmeyen chain chunk response reddedildi".into());
            }
            NetworkMessage::Transaction(transaction) => {
                let transaction_id = if crate::protocol::is_fixed_hex(
                    &transaction.id,
                    crate::protocol::HASH_HEX_LENGTH,
                ) {
                    transaction.id.clone()
                } else {
                    "<invalid-id>".to_string()
                };

                match self
                    .node
                    .receive_transaction_with_result(
                        transaction.clone(),
                    )
                {
                    Ok(()) => {
                        println!(
                            "Transaction accepted: {transaction_id}"
                        );
                        outcome.actions.push(
                            RuntimeAction::Broadcast(
                                NetworkMessage::Transaction(
                                    transaction,
                                ),
                            ),
                        );
                        self.append_pending_block_action(
                            &mut outcome,
                        );
                    }
                    Err(error) => {
                        println!(
                            "Transaction rejected: {transaction_id} - {error}"
                        );
                    }
                }
            }
            NetworkMessage::Block(block) => {
                let previous_height = self.node.blockchain.height();
                self.node
                    .receive_message(NetworkMessage::Block(block.clone()));
                if self.node.blockchain.height() > previous_height {
                    outcome
                        .actions
                        .push(RuntimeAction::Broadcast(NetworkMessage::Block(block)));
                    self.append_pending_block_action(&mut outcome);
                }
            }
            message @ NetworkMessage::Handshake { .. }
            | message @ NetworkMessage::HandshakeAck { .. } => {
                self.node.receive_message(message);
            }
        }

        Ok(outcome)
    }

    fn append_pending_block_action(&mut self, outcome: &mut ProcessOutcome) {
        match self.try_produce_pending_block() {
            Ok(Some(block)) => outcome
                .actions
                .push(RuntimeAction::Broadcast(NetworkMessage::Block(block))),
            Ok(None) => {}
            Err(error) => println!("Validator block üretim denemesi başarısız: {error}"),
        }
    }

    fn chain_chunk_response(&self, start_index: u64) -> Result<NetworkMessage, String> {
        let start = usize::try_from(start_index)
            .map_err(|_| "Chain chunk başlangıç index geçersiz".to_string())?;
        let total_blocks = self.node.blockchain.chain.len();

        if start > total_blocks {
            return Err("Chain chunk başlangıcı zincir uzunluğunu aşıyor".into());
        }

        let end = start
            .saturating_add(MAX_SYNC_BLOCKS_PER_MESSAGE)
            .min(total_blocks);
        let blocks = self.node.blockchain.chain[start..end].to_vec();

        Ok(NetworkMessage::ChainChunkResponse {
            start_index,
            total_blocks: u64::try_from(total_blocks)
                .map_err(|_| "Blockchain uzunluğu u64 aralığını aşıyor".to_string())?,
            blocks,
        })
    }

    fn try_produce_pending_block(&mut self) -> Result<Option<Block>, String> {
        let identity = match self.validator_identity.as_ref() {
            Some(identity) => identity,
            None => return Ok(None),
        };

        if self.chain_sync_active {
            return Ok(None);
        }

        if self.node.mempool.is_empty() {
            return Ok(None);
        }

        let tip = self.node.blockchain.chain.last().ok_or("Blockchain boş")?;
        let selected = self
            .node
            .select_validator(&tip.hash)
            .ok_or("Validator seçilemedi")?;

        if selected != identity.address() {
            return Ok(None);
        }

        let timestamp = Network::current_timestamp().max(
            tip.timestamp
                .checked_add(1)
                .ok_or("Block timestamp overflow")?,
        );
        self.node
            .produce_block(timestamp, identity.wallet())
            .map(Some)
    }
}
