use crate::chain::{Blockchain, Mempool};
use crate::consensus::Consensus;
use crate::core::{Block, Transaction};
use crate::economy::Economy;
use crate::network::{Network, NetworkMessage};
use crate::protocol::{
    is_fixed_hex,
    ADDRESS_HEX_LENGTH,
    HASH_HEX_LENGTH,
    MAX_TOTAL_TRANSACTIONS_PER_BLOCK,
    MAX_SYNC_BLOCKS_PER_MESSAGE,
    PUBLIC_KEY_HEX_LENGTH,
    SIGNATURE_HEX_LENGTH,
    SYSTEM_ADDRESS,
    SYSTEM_PUBLIC_KEY,
    SYSTEM_REWARD_SIGNATURE,
};
use crate::state::State;
use crate::storage::Storage;
use crate::wallet::Wallet;

#[derive(Debug)]
pub struct Node {
    pub blockchain: Blockchain,
    pub state: State,
    pub mempool: Mempool,
    pub consensus: Consensus,
    pub network: Network,

    // Chain sync sırasında state ve economy
    // genesis'ten yeniden oluşturulur.
    genesis_state: State,
    genesis_economy: Economy,

    // Parçalı blockchain senkronizasyonu için
    // geçici uzak zincir tamponu.
    sync_buffer: Vec<Block>,
    sync_expected_total: Option<u64>,
}

impl Node {
    pub fn new(
        blockchain: Blockchain,
        state: State,
        consensus: Consensus,
    ) -> Self {
        let genesis_state = state.clone();
        let genesis_economy = blockchain.economy.clone();

        Self {
            blockchain,
            state,
            mempool: Mempool::new(),
            consensus,
            network: Network::new(),
            genesis_state,
            genesis_economy,
            sync_buffer: Vec::new(),
            sync_expected_total: None,
        }
    }

    pub fn restore_chain_from_storage(
        &mut self,
        chain: Vec<Block>,
    ) -> Result<(), String> {
        self.synchronize_chain(
            chain,
        )
    }

    // ==========================
    // PROTOKOL ALAN FORMATI
    // ==========================

    fn validate_transaction_field_sizes(
        &self,
        transaction: &Transaction,
    ) -> Result<(), String> {
        if !is_fixed_hex(
            &transaction.id,
            HASH_HEX_LENGTH,
        ) {
            return Err(
                "Transaction ID formatı geçersiz"
                    .into(),
            );
        }

        if transaction.coinbase {
            if transaction.from != SYSTEM_ADDRESS
                || transaction.public_key
                    != SYSTEM_PUBLIC_KEY
            {
                return Err(
                    "Coinbase SYSTEM alanları geçersiz"
                        .into(),
                );
            }

            if !is_fixed_hex(
                &transaction.to,
                ADDRESS_HEX_LENGTH,
            ) {
                return Err(
                    "Coinbase alıcı adres formatı geçersiz"
                        .into(),
                );
            }

            match transaction.signature.as_ref() {
                Some(signature)
                    if signature
                        == SYSTEM_REWARD_SIGNATURE => {}

                _ => {
                    return Err(
                        "Coinbase imza formatı geçersiz"
                            .into(),
                    );
                }
            }

            return Ok(());
        }

        if !is_fixed_hex(
            &transaction.from,
            ADDRESS_HEX_LENGTH,
        )
            || !is_fixed_hex(
                &transaction.to,
                ADDRESS_HEX_LENGTH,
            )
        {
            return Err(
                "Transaction adres formatı geçersiz"
                    .into(),
            );
        }

        if !is_fixed_hex(
            &transaction.public_key,
            PUBLIC_KEY_HEX_LENGTH,
        ) {
            return Err(
                "Transaction public key formatı geçersiz"
                    .into(),
            );
        }

        let signature =
            transaction
                .signature
                .as_ref()
                .ok_or(
                    "Transaction imzası yok",
                )?;

        if !is_fixed_hex(
            signature,
            SIGNATURE_HEX_LENGTH,
        ) {
            return Err(
                "Transaction imza formatı geçersiz"
                    .into(),
            );
        }

        Ok(())
    }

    // ==========================
    // TEMEL TRANSACTION DOĞRULAMA
    // ==========================

    fn validate_transaction_base(
        &self,
        transaction: &Transaction,
        economy: &Economy,
    ) -> Result<(), String> {
        self.validate_transaction_field_sizes(
            transaction,
        )?;

        if transaction.coinbase {
            return Err(
                "Coinbase transaction mempool'a eklenemez".into(),
            );
        }

        if transaction.from == SYSTEM_ADDRESS {
            return Err(
                "Normal transaction SYSTEM adresinden gelemez".into(),
            );
        }

        if transaction.reward_marker != 0 {
            return Err(
                "Normal transaction reward marker sıfır olmalı".into(),
            );
        }

        if transaction.id != transaction.calculate_id() {
            return Err(
                "Transaction ID geçersiz".into(),
            );
        }

        if !transaction.is_signed() {
            return Err(
                "Transaction imzası yok".into(),
            );
        }

        let derived_address =
            Wallet::address_from_public_key(
                &transaction.public_key,
            )
            .ok_or(
                "Transaction public key geçersiz",
            )?;

        if derived_address != transaction.from {
            return Err(
                "Transaction gönderen adresi public key ile uyuşmuyor"
                    .into(),
            );
        }

        let signature = transaction
            .signature
            .as_ref()
            .ok_or(
                "Transaction imzası bulunamadı",
            )?;

        if !Wallet::verify(
            &transaction.public_key,
            &transaction.message(),
            signature,
        ) {
            return Err(
                "Transaction imzası geçersiz".into(),
            );
        }

        if transaction.amount == 0 {
            return Err(
                "Transaction miktarı sıfır olamaz".into(),
            );
        }

        if !economy.validate_fee(
            transaction.amount,
            transaction.fee,
        ) {
            let expected_fee =
                economy.calculate_fee(
                    transaction.amount,
                );

            return Err(format!(
                "Transaction fee hatalı. Beklenen: {}, Gelen: {}",
                expected_fee,
                transaction.fee
            ));
        }

        Ok(())
    }

    // ==========================
    // BLOCK İÇİ TRANSACTION DOĞRULAMA
    // ==========================

    fn validate_transaction(
        &self,
        transaction: &Transaction,
        state: &State,
        economy: &Economy,
    ) -> Result<(), String> {
        self.validate_transaction_base(
            transaction,
            economy,
        )?;

        let total_cost = transaction
            .amount
            .checked_add(transaction.fee)
            .ok_or(
                "Transaction toplam tutarı overflow",
            )?;

        let balance =
            state.balance_of(&transaction.from);

        if balance < total_cost {
            return Err(
                "Transaction için yetersiz bakiye"
                    .into(),
            );
        }

        let expected_nonce =
            state.nonce_of(&transaction.from);

        if transaction.nonce != expected_nonce {
            return Err(format!(
                "Transaction nonce hatalı. Beklenen: {}, Gelen: {}",
                expected_nonce,
                transaction.nonce
            ));
        }

        Ok(())
    }

    // ==========================
    // MEMPOOL TRANSACTION DOĞRULAMA
    // ==========================

    fn validate_transaction_for_mempool(
        &self,
        transaction: &Transaction,
        state: &State,
        economy: &Economy,
        mempool: &Mempool,
    ) -> Result<(), String> {
        self.validate_transaction_base(
            transaction,
            economy,
        )?;

        let state_nonce =
            state.nonce_of(&transaction.from);

        let expected_nonce =
            mempool.next_nonce(
                &transaction.from,
                state_nonce,
            )?;

        if transaction.nonce != expected_nonce {
            return Err(format!(
                "Transaction nonce hatalı. Beklenen: {}, Gelen: {}",
                expected_nonce,
                transaction.nonce
            ));
        }

        let transaction_cost =
            transaction
                .amount
                .checked_add(
                    transaction.fee,
                )
                .ok_or(
                    "Transaction toplam tutarı overflow",
                )?;

        let pending_cost =
            mempool.pending_cost(
                &transaction.from,
            )?;

        let total_reserved_cost =
            pending_cost
                .checked_add(
                    transaction_cost,
                )
                .ok_or(
                    "Mempool rezerv toplamı overflow",
                )?;

        let balance =
            state.balance_of(
                &transaction.from,
            );

        if balance < total_reserved_cost {
            return Err(
                "Transaction için kullanılabilir bakiye yetersiz"
                    .into(),
            );
        }

        Ok(())
    }

    // ==========================
    // TRANSACTION EKLE
    // ==========================

    pub fn add_transaction(
        &mut self,
        transaction: Transaction,
    ) -> bool {
        if let Err(error) =
            self.validate_transaction_for_mempool(
                &transaction,
                &self.state,
                &self.blockchain.economy,
                &self.mempool,
            )
        {
            println!(
                "❌ Transaction reddedildi: {}",
                error
            );

            return false;
        }

        let added =
            self.mempool
                .add_transaction(
                    transaction.clone(),
                );

        if added {
            self.network.broadcast(
                NetworkMessage::Transaction(
                    transaction,
                ),
            );
        }

        added
    }

    // ==========================
    // NETWORK MESAJI
    // ==========================

    pub fn receive_message(
        &mut self,
        message: NetworkMessage,
    ) {
        match message {
            message @ NetworkMessage::Handshake {
                ..
            } => {
                self.network.receive(
                    message,
                );
            }

            message @ NetworkMessage::HandshakeAck {
                ..
            } => {
                self.network.receive(
                    message,
                );
            }

            NetworkMessage::Transaction(
                transaction,
            ) => {
                match self.validate_transaction_for_mempool(
                    &transaction,
                    &self.state,
                    &self.blockchain.economy,
                    &self.mempool,
                ) {
                    Ok(()) => {
                        let added =
                            self.mempool
                                .add_transaction(
                                    transaction,
                                );

                        println!(
                            "📥 Network transaction doğrulandı. Mempool'a eklendi: {}",
                            added
                        );
                    }

                    Err(error) => {
                        println!(
                            "❌ Network transaction reddedildi: {}",
                            error
                        );
                    }
                }
            }

            NetworkMessage::Block(block) => {
                println!(
                    "📥 Network block alındı. Index: {}",
                    block.index
                );

                let accepted =
                    self.receive_block(block);

                println!(
                    "Block kabul edildi mi: {}",
                    accepted
                );
            }

            NetworkMessage::SyncRequest => {
                self.network.receive(
                    NetworkMessage::SyncRequest,
                );
            }

            NetworkMessage::ChainChunkRequest {
                start_index,
            } => {
                let start =
                    match usize::try_from(
                        start_index,
                    ) {
                        Ok(start) => start,

                        Err(_) => {
                            println!(
                                "❌ Chain chunk isteği reddedildi: Başlangıç index geçersiz"
                            );

                            return;
                        }
                    };

                let total_blocks =
                    self.blockchain
                        .chain
                        .len();

                if start > total_blocks {
                    println!(
                        "❌ Chain chunk isteği reddedildi: Başlangıç index zincir uzunluğunu aşıyor"
                    );

                    return;
                }

                let end =
                    start
                        .saturating_add(
                            MAX_SYNC_BLOCKS_PER_MESSAGE,
                        )
                        .min(total_blocks);

                let blocks =
                    self.blockchain
                        .chain[start..end]
                        .to_vec();

                println!(
                    "📥 Chain chunk isteği. Start: {}, gönderilen: {}, total: {}",
                    start,
                    blocks.len(),
                    total_blocks
                );

                self.network.broadcast(
                    NetworkMessage::ChainChunkResponse {
                        start_index,
                        total_blocks:
                            total_blocks as u64,
                        blocks,
                    },
                );
            }

            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    "📥 Chain chunk cevabı. Start: {}, chunk: {}, total: {}",
                    start_index,
                    blocks.len(),
                    total_blocks
                );

                match self.accept_chain_chunk(
                    start_index,
                    total_blocks,
                    blocks,
                ) {
                    Ok(Some(next_index)) => {
                        self.network.broadcast(
                            NetworkMessage::ChainChunkRequest {
                                start_index:
                                    next_index,
                            },
                        );
                    }

                    Ok(None) => {
                        println!(
                            "✅ Parçalı blockchain senkronizasyonu tamamlandı."
                        );
                    }

                    Err(error) => {
                        println!(
                            "❌ Chain chunk reddedildi: {}",
                            error
                        );
                    }
                }
            }
        }
    }

    // ==========================
    // BLOCK DOĞRULA + UYGULA
    // ==========================

    fn validate_and_apply_block(
        &self,
        block: Block,
        current_blockchain: &Blockchain,
        current_state: &State,
    ) -> Result<(Blockchain, State), String> {
        // ==========================
        // BLOCK TRANSACTION LİMİTİ
        // ==========================

        if block.transactions.len()
            > MAX_TOTAL_TRANSACTIONS_PER_BLOCK
        {
            return Err(
                "Block transaction limiti aşıldı"
                    .into(),
            );
        }

        if !is_fixed_hex(
            &block.previous_hash,
            HASH_HEX_LENGTH,
        )
            || !is_fixed_hex(
                &block.hash,
                HASH_HEX_LENGTH,
            )
        {
            return Err(
                "Block hash formatı geçersiz"
                    .into(),
            );
        }

        if !is_fixed_hex(
            &block.validator,
            ADDRESS_HEX_LENGTH,
        ) {
            return Err(
                "Block validator adres formatı geçersiz"
                    .into(),
            );
        }

        if !is_fixed_hex(
            &block.validator_public_key,
            PUBLIC_KEY_HEX_LENGTH,
        ) {
            return Err(
                "Block validator public key formatı geçersiz"
                    .into(),
            );
        }

        let validator_signature =
            block
                .validator_signature
                .as_ref()
                .ok_or(
                    "Block validator imzası yok",
                )?;

        if !is_fixed_hex(
            validator_signature,
            SIGNATURE_HEX_LENGTH,
        ) {
            return Err(
                "Block validator imza formatı geçersiz"
                    .into(),
            );
        }

        for transaction in
            &block.transactions
        {
            self.validate_transaction_field_sizes(
                transaction,
            )?;
        }

        // ==========================
        // BLOCK HASH
        // ==========================

        if !block.is_hash_valid() {
            return Err(
                "Block hash geçersiz".into(),
            );
        }

        // ==========================
        // BLOCK ZİNCİR BAĞLANTISI
        // ==========================

        let mut candidate_blockchain =
            current_blockchain.clone();

        candidate_blockchain
            .add_received_block(
                block.clone(),
            )?;

        // ==========================
        // SEÇİLEN VALIDATOR
        // ==========================

        let expected_validator = self
            .consensus
            .select_validator_from_hash(
                &block.previous_hash,
            )
            .ok_or(
                "Validator seçilemedi",
            )?
            .address
            .clone();

        if block.validator != expected_validator {
            return Err(
                "Bu blok için seçilen validator farklı"
                    .into(),
            );
        }

        // ==========================
        // VALIDATOR PUBLIC KEY
        // ==========================

        let derived_validator_address =
            Wallet::address_from_public_key(
                &block.validator_public_key,
            )
            .ok_or(
                "Validator public key geçersiz",
            )?;

        if derived_validator_address
            != block.validator
        {
            return Err(
                "Validator adresi public key ile uyuşmuyor"
                    .into(),
            );
        }

        // ==========================
        // VALIDATOR CONSENSUS
        // ==========================

        if !self
            .consensus
            .is_validator_allowed(
                &block.validator,
            )
        {
            return Err(
                "Validator consensus ağında kayıtlı değil"
                    .into(),
            );
        }

        // ==========================
        // VALIDATOR SIGNATURE
        // ==========================

        let validator_signature =
            block.validator_signature
                .as_ref()
                .ok_or(
                    "Validator imzası yok",
                )?;

        if !Wallet::verify(
            &block.validator_public_key,
            block.hash.as_bytes(),
            validator_signature,
        ) {
            return Err(
                "Validator imzası geçersiz".into(),
            );
        }

        // ==========================
        // TRANSACTION'LAR
        // ==========================

        if block.transactions.is_empty() {
            return Err(
                "Block transaction listesi boş"
                    .into(),
            );
        }

        let mut rebuilt_state =
            current_state.clone();

        let mut rebuilt_economy =
            current_blockchain
                .economy
                .clone();

        let mut validator_fee_total = 0u64;
        let mut treasury_fee_total = 0u64;
        let mut burn_fee_total = 0u64;

        let mut coinbase:
            Option<Transaction> = None;

        for (
            position,
            transaction,
        ) in block
            .transactions
            .iter()
            .enumerate()
        {
            // ==========================
            // TX ID
            // ==========================

            if transaction.id
                != transaction.calculate_id()
            {
                return Err(
                    "Block içindeki transaction ID geçersiz"
                        .into(),
                );
            }

            // ==========================
            // COINBASE
            // ==========================

            if transaction.coinbase {
                if coinbase.is_some() {
                    return Err(
                        "Block içinde birden fazla coinbase var"
                            .into(),
                    );
                }

                if position
                    != block.transactions.len() - 1
                {
                    return Err(
                        "Coinbase son transaction olmalı"
                            .into(),
                    );
                }

                if transaction.from != SYSTEM_ADDRESS {
                    return Err(
                        "Coinbase gönderen SYSTEM olmalı"
                            .into(),
                    );
                }

                if transaction.public_key
                    != SYSTEM_PUBLIC_KEY
                {
                    return Err(
                        "Coinbase public key geçersiz"
                            .into(),
                    );
                }

                if transaction.signature
                    .as_deref()
                    != Some(SYSTEM_REWARD_SIGNATURE)
                {
                    return Err(
                        "Coinbase imzası geçersiz"
                            .into(),
                    );
                }

                if transaction.to
                    != block.validator
                {
                    return Err(
                        "Coinbase ödülü validator adresine gitmiyor"
                            .into(),
                    );
                }

                if transaction.fee != 0 {
                    return Err(
                        "Coinbase fee sıfır olmalı"
                            .into(),
                    );
                }

                if transaction.nonce != 0 {
                    return Err(
                        "Coinbase nonce sıfır olmalı"
                            .into(),
                    );
                }

                if transaction.reward_marker
                    != block.index as u128
                {
                    return Err(
                        "Coinbase reward marker block index ile uyuşmuyor"
                            .into(),
                    );
                }

                if transaction.amount
                    != rebuilt_economy
                        .block_reward
                {
                    return Err(
                        "Coinbase ödül miktarı hatalı"
                            .into(),
                    );
                }

                coinbase =
                    Some(transaction.clone());

                continue;
            }

            // ==========================
            // NORMAL TRANSACTION
            // ==========================

            if transaction.reward_marker != 0 {
                return Err(
                    "Normal transaction reward marker sıfır olmalı"
                        .into(),
                );
            }

            self.validate_transaction(
                transaction,
                &rebuilt_state,
                &rebuilt_economy,
            )?;

            rebuilt_state
                .apply_transaction(
                    transaction,
                )?;

            let (
                validator_fee,
                treasury_fee,
                burn_fee,
            ) = rebuilt_economy
                .distribute_fee(
                    transaction.fee,
                );

            validator_fee_total =
                validator_fee_total
                    .checked_add(
                        validator_fee,
                    )
                    .ok_or(
                        "Validator fee overflow",
                    )?;

            treasury_fee_total =
                treasury_fee_total
                    .checked_add(
                        treasury_fee,
                    )
                    .ok_or(
                        "Treasury fee overflow",
                    )?;

            burn_fee_total =
                burn_fee_total
                    .checked_add(
                        burn_fee,
                    )
                    .ok_or(
                        "Burn fee overflow",
                    )?;
        }

        // ==========================
        // COINBASE VAR MI?
        // ==========================

        let coinbase =
            coinbase.ok_or(
                "Coinbase transaction bulunamadı",
            )?;

        // ==========================
        // VALIDATOR HESABI
        // ==========================

        if !rebuilt_state
            .accounts
            .contains_key(
                &block.validator,
            )
        {
            rebuilt_state.create_account(
                block.validator.clone(),
                0,
            );
        }

        // ==========================
        // FEE DAĞITIMI
        // ==========================

        rebuilt_state.add_balance(
            &block.validator,
            validator_fee_total,
        )?;

        rebuilt_state.add_treasury(
            treasury_fee_total,
        )?;

        // ==========================
        // BURN
        // ==========================

        rebuilt_state.burn(
            burn_fee_total,
        )?;

        rebuilt_economy.burn(
            burn_fee_total,
        );

        // ==========================
        // COINBASE MINT
        // ==========================

        rebuilt_economy.mint(
            coinbase.amount,
        )?;

        // ==========================
        // COINBASE STATE
        // ==========================

        rebuilt_state
            .apply_transaction(
                &coinbase,
            )?;

        // ==========================
        // ECONOMY COMMIT
        // ==========================

        candidate_blockchain.economy =
            rebuilt_economy;

        Ok((
            candidate_blockchain,
            rebuilt_state,
        ))
    }

    // ==========================
    // MEMPOOL TEMİZLE
    // ==========================

    fn remove_confirmed_transactions(
        &mut self,
        block: &Block,
    ) {
        self.mempool.transactions.retain(
            |pending_transaction| {
                !block
                    .transactions
                    .iter()
                    .any(
                        |confirmed_transaction| {
                            !confirmed_transaction.coinbase
                                && confirmed_transaction.id
                                    == pending_transaction.id
                        },
                    )
            },
        );
    }

    // ==========================
    // BLOCK AL
    // ==========================

    pub fn receive_block(
        &mut self,
        block: Block,
    ) -> bool {
        let confirmed_block =
            block.clone();

        match self.validate_and_apply_block(
            block,
            &self.blockchain,
            &self.state,
        ) {
            Ok((
                candidate_blockchain,
                rebuilt_state,
            )) => {
                if let Err(error) =
                    Storage::save_blockchain(
                        &candidate_blockchain
                            .chain,
                    )
                {
                    println!(
                        "❌ Block kalıcı kayda yazılamadı: {}",
                        error
                    );

                    return false;
                }

                self.blockchain =
                    candidate_blockchain;

                self.state =
                    rebuilt_state;

                self.remove_confirmed_transactions(
                    &confirmed_block,
                );

                println!(
                    "✅ Yeni block doğrulandı, zincire eklendi ve state güncellendi."
                );

                println!(
                    "🧹 Onaylanan transaction'lar mempool'dan temizlendi."
                );

                true
            }

            Err(error) => {
                println!(
                    "❌ Block reddedildi: {}",
                    error
                );

                false
            }
        }
    }

    // ==========================
    // CHAIN SYNC
    // ==========================

    fn accept_chain_chunk(
        &mut self,
        start_index: u64,
        total_blocks: u64,
        blocks: Vec<Block>,
    ) -> Result<Option<u64>, String> {
        if blocks.len()
            > MAX_SYNC_BLOCKS_PER_MESSAGE
        {
            return Err(
                "Sync chunk block limiti aşıldı"
                    .into(),
            );
        }

        let total =
            usize::try_from(
                total_blocks,
            )
            .map_err(
                |_| {
                    "Sync toplam block sayısı geçersiz"
                        .to_string()
                },
            )?;

        let start =
            usize::try_from(
                start_index,
            )
            .map_err(
                |_| {
                    "Sync başlangıç index geçersiz"
                        .to_string()
                },
            )?;

        if total == 0 {
            return Err(
                "Sync toplam block sayısı sıfır olamaz"
                    .into(),
            );
        }

        if start > total {
            return Err(
                "Sync başlangıç index toplam block sayısını aşıyor"
                    .into(),
            );
        }

        let end =
            start
                .checked_add(
                    blocks.len(),
                )
                .ok_or(
                    "Sync chunk index overflow",
                )?;

        if end > total {
            return Err(
                "Sync chunk toplam block sayısını aşıyor"
                    .into(),
            );
        }

        if start == 0 {
            self.sync_buffer.clear();
            self.sync_expected_total =
                Some(total_blocks);
        } else {
            if self.sync_expected_total
                != Some(total_blocks)
            {
                return Err(
                    "Sync toplam block sayısı değişti"
                        .into(),
                );
            }

            if self.sync_buffer.len()
                != start
            {
                return Err(
                    "Sync chunk sırası geçersiz"
                        .into(),
                );
            }
        }

        for (offset, block)
            in blocks.iter().enumerate()
        {
            let expected_index =
                start
                    .checked_add(
                        offset,
                    )
                    .ok_or(
                        "Sync block index overflow",
                    )? as u64;

            if block.index
                != expected_index
            {
                return Err(
                    format!(
                        "Sync block index sırası geçersiz. Beklenen: {}, Gelen: {}",
                        expected_index,
                        block.index
                    ),
                );
            }
        }

        if blocks.is_empty()
            && start < total
        {
            return Err(
                "Sync chunk beklenmedik şekilde boş"
                    .into(),
            );
        }

        self.sync_buffer.extend(
            blocks,
        );

        if self.sync_buffer.len()
            < total
        {
            return Ok(
                Some(
                    self.sync_buffer.len()
                        as u64,
                ),
            );
        }

        if self.sync_buffer.len()
            != total
        {
            return Err(
                "Sync buffer uzunluğu geçersiz"
                    .into(),
            );
        }

        let assembled_chain =
            std::mem::take(
                &mut self.sync_buffer,
            );

        self.sync_expected_total =
            None;

        self.synchronize_chain(
            assembled_chain,
        )?;

        Ok(None)
    }

    fn synchronize_chain(
        &mut self,
        chain: Vec<Block>,
    ) -> Result<(), String> {
        if chain.is_empty() {
            return Err(
                "Gelen blockchain boş".into(),
            );
        }

        if chain.len()
            < self.blockchain.chain.len()
        {
            return Err(
                "Gelen blockchain mevcut zincirden kısa"
                    .into(),
            );
        }

        // ==========================
        // EŞİT UZUNLUKTA FORK KONTROLÜ
        // ==========================
        //
        // Aynı yükseklikte iki farklı zincir varsa
        // uzak zincirin yerel zinciri sessizce
        // ezmesine izin vermiyoruz.
        //
        // Aynı yükseklik + aynı tip hash:
        // zaten senkron.
        //
        // Aynı yükseklik + farklı tip hash:
        // fork olarak reddedilir.

        if chain.len()
            == self.blockchain.chain.len()
        {
            let local_tip_hash =
                self.blockchain
                    .chain
                    .last()
                    .ok_or(
                        "Yerel blockchain boş",
                    )?
                    .hash
                    .clone();

            let remote_tip_hash =
                chain
                    .last()
                    .ok_or(
                        "Gelen blockchain boş",
                    )?
                    .hash
                    .clone();

            if remote_tip_hash
                == local_tip_hash
            {
                return Ok(());
            }

            return Err(
                "Eşit uzunlukta farklı blockchain fork'u reddedildi"
                    .into(),
            );
        }

        let local_genesis =
            self.blockchain
                .chain
                .first()
                .ok_or(
                    "Yerel genesis bulunamadı",
                )?;

        let remote_genesis =
            chain
                .first()
                .ok_or(
                    "Gelen genesis bulunamadı",
                )?;

        // ==========================
        // GENESIS KONTROLÜ
        // ==========================

        if remote_genesis.index != 0 {
            return Err(
                "Genesis index geçersiz".into(),
            );
        }

        if remote_genesis.previous_hash
            != "0"
        {
            return Err(
                "Genesis previous hash geçersiz"
                    .into(),
            );
        }

        if remote_genesis.hash
            != remote_genesis.calculate_hash()
        {
            return Err(
                "Genesis hash geçersiz".into(),
            );
        }

        if remote_genesis.hash
            != local_genesis.hash
        {
            return Err(
                "Genesis blokları uyuşmuyor"
                    .into(),
            );
        }

        // ==========================
        // GENESIS'TEN YENİDEN KUR
        // ==========================

        let mut rebuilt_blockchain =
            Blockchain::new(
                remote_genesis.clone(),
            );

        rebuilt_blockchain.economy =
            self.genesis_economy.clone();

        let mut rebuilt_state =
            self.genesis_state.clone();

        // ==========================
        // TÜM BLOKLARI TEKRAR OYNAT
        // ==========================

        for block in
            chain.into_iter().skip(1)
        {
            let (
                next_blockchain,
                next_state,
            ) = self
                .validate_and_apply_block(
                    block,
                    &rebuilt_blockchain,
                    &rebuilt_state,
                )?;

            rebuilt_blockchain =
                next_blockchain;

            rebuilt_state =
                next_state;
        }

        // ==========================
        // KALICI KAYIT + ATOMİK COMMIT
        // ==========================

        Storage::save_blockchain(
            &rebuilt_blockchain
                .chain,
        )
        .map_err(
            |error| {
                format!(
                    "Senkronize blockchain kalıcı kayda yazılamadı: {}",
                    error
                )
            },
        )?;

        self.blockchain =
            rebuilt_blockchain;

        self.state =
            rebuilt_state;

        self.mempool =
            Mempool::new();

        Ok(())
    }

    // ==========================
    // NETWORK SYNC
    // ==========================

    pub fn sync_network(&self) {
        println!(
            "🔄 Network mesaj senkronizasyonu başlatıldı."
        );

        println!(
            "Network mesaj sayısı: {}",
            self.network.message_count()
        );
    }

    // ==========================
    // CHAIN REQUEST
    // ==========================

    pub fn request_chain(&mut self) {
        println!(
            "📡 Güncel blockchain parçalı olarak talep ediliyor."
        );

        self.sync_buffer.clear();
        self.sync_expected_total =
            None;

        self.network.broadcast(
            NetworkMessage::ChainChunkRequest {
                start_index: 0,
            },
        );
    }

    // ==========================
    // BLOCK ÜRET
    // ==========================

    pub fn produce_block(
        &mut self,
        timestamp: u64,
        validator_wallet: &Wallet,
    ) -> Result<Block, String> {
        if !self
            .consensus
            .is_validator_allowed(
                validator_wallet.address(),
            )
        {
            return Err(
                "Validator consensus ağında kayıtlı değil"
                    .into(),
            );
        }

        let previous_hash = self
            .blockchain
            .chain
            .last()
            .ok_or(
                "Blockchain boş",
            )?
            .hash
            .clone();

        let selected_validator =
            self.consensus
                .select_validator_from_hash(
                    &previous_hash,
                )
                .ok_or(
                    "Validator seçilemedi",
                )?;

        if selected_validator.address
            != validator_wallet.address()
        {
            return Err(
                "Bu blok için seçilen validator farklı"
                    .into(),
            );
        }

        // ==========================
        // NODE SEVİYESİNDE ATOMİK KOPYALAR
        // ==========================

        let mut candidate_blockchain =
            self.blockchain.clone();

        let mut candidate_state =
            self.state.clone();

        let mut candidate_mempool =
            Mempool {
                transactions:
                    self.mempool
                        .transactions
                        .clone(),
            };

        // ==========================
        // BLOCK ÜRET
        // ==========================

        let block =
            candidate_blockchain
                .create_block_from_mempool(
                    timestamp,
                    validator_wallet,
                    &mut candidate_mempool,
                    &mut candidate_state,
                )?;

        // ==========================
        // KALICI KAYIT + ATOMİK COMMIT
        // ==========================

        Storage::save_blockchain(
            &candidate_blockchain
                .chain,
        )
        .map_err(
            |error| {
                format!(
                    "Üretilen blockchain kalıcı kayda yazılamadı: {}",
                    error
                )
            },
        )?;

        self.blockchain =
            candidate_blockchain;

        self.state =
            candidate_state;

        self.mempool =
            candidate_mempool;

        Ok(block)
    }

    // ==========================
    // VALIDATOR SEÇ
    // ==========================

    pub fn select_validator(
        &self,
        seed: &str,
    ) -> Option<String> {
        self.consensus
            .select_validator_from_hash(
                seed,
            )
            .map(
                |validator| {
                    validator
                        .address
                        .clone()
                },
            )
    }

    // ==========================
    // PEER
    // ==========================

    pub fn add_peer(
        &mut self,
        peer: String,
    ) -> bool {
        self.network.add_peer(peer)
    }

    pub fn peer_count(&self) -> usize {
        self.network.peer_count()
    }
}