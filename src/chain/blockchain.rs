use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::{Block, Transaction};
use crate::protocol::{
    is_fixed_hex,
    ADDRESS_HEX_LENGTH,
    GENESIS_PREVIOUS_HASH,
    GENESIS_VALIDATOR,
    HASH_HEX_LENGTH,
    MAX_FUTURE_DRIFT_SECONDS,
    MAX_TOTAL_TRANSACTIONS_PER_BLOCK,
    PUBLIC_KEY_HEX_LENGTH,
    SIGNATURE_HEX_LENGTH,
    SYSTEM_ADDRESS,
    SYSTEM_PUBLIC_KEY,
    SYSTEM_REWARD_SIGNATURE,
};
use crate::economy::Economy;
use crate::state::State;
use crate::wallet::Wallet;

use super::Mempool;

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub economy: Economy,
}

impl Blockchain {
    

    fn validate_transaction_field_sizes(
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

    fn validate_block_field_sizes(
        block: &Block,
    ) -> Result<(), String> {
        if block.transactions.len()
            > MAX_TOTAL_TRANSACTIONS_PER_BLOCK
        {
            return Err(
                "Block transaction limiti aşıldı"
                    .into(),
            );
        }

        if !is_fixed_hex(
            &block.hash,
            HASH_HEX_LENGTH,
        ) {
            return Err(
                "Block hash formatı geçersiz"
                    .into(),
            );
        }

        // Genesis mevcut Kybernetes protokolünde özel formattadır.
        if block.index == 0 {
            if block.previous_hash != GENESIS_PREVIOUS_HASH
                || block.validator != GENESIS_VALIDATOR
                || !block.validator_public_key.is_empty()
                || block.validator_signature.is_some()
                || !block.transactions.is_empty()
            {
                return Err(
                    "Genesis block formatı geçersiz"
                        .into(),
                );
            }

            return Ok(());
        }

        if !is_fixed_hex(
            &block.previous_hash,
            HASH_HEX_LENGTH,
        ) {
            return Err(
                "Block previous hash formatı geçersiz"
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
            Self::validate_transaction_field_sizes(
                transaction,
            )?;
        }

        Ok(())
    }

    fn validate_transaction_id_uniqueness(
        &self,
        block: &Block,
    ) -> Result<(), String> {
        let mut seen_ids:
            HashSet<&str> =
            self.chain
                .iter()
                .flat_map(
                    |existing_block| {
                        existing_block
                            .transactions
                            .iter()
                    },
                )
                .map(
                    |transaction| {
                        transaction.id.as_str()
                    },
                )
                .collect();

        for transaction in
            &block.transactions
        {
            if !seen_ids.insert(
                transaction.id.as_str(),
            ) {
                return Err(format!(
                    "Transaction ID daha önce kullanılmış. TX: {}",
                    transaction.id
                ));
            }
        }

        Ok(())
    }

    fn current_unix_timestamp(
    ) -> Result<u64, String> {
        SystemTime::now()
            .duration_since(
                UNIX_EPOCH,
            )
            .map(
                |duration| {
                    duration.as_secs()
                },
            )
            .map_err(
                |_| {
                    "Sistem zamanı UNIX epoch öncesinde"
                        .into()
                },
            )
    }

    fn validate_future_timestamp(
        timestamp: u64,
    ) -> Result<(), String> {
        let now =
            Self::current_unix_timestamp()?;

        let maximum_allowed =
            now.checked_add(
                MAX_FUTURE_DRIFT_SECONDS,
            )
            .ok_or(
                "Timestamp üst sınırı overflow",
            )?;

        if timestamp > maximum_allowed {
            return Err(
                "Block timestamp izin verilen gelecek zaman sınırını aşıyor"
                    .into(),
            );
        }

        Ok(())
    }

    pub fn new(
        genesis: Block,
    ) -> Self {
        Self {
            chain: vec![genesis],
            economy: Economy::new(),
        }
    }

    // ==========================
    // BLOCK ÜRET
    // ==========================

    pub fn create_block_from_mempool(
        &mut self,
        timestamp: u64,
        validator_wallet: &Wallet,
        mempool: &mut Mempool,
        state: &mut State,
    ) -> Result<Block, String> {
        if mempool.is_empty() {
            return Err(
                "Mempool boş".into(),
            );
        }

        // ==========================
        // TIMESTAMP KONTROLÜ
        // ==========================

        let last_block =
            self.chain
                .last()
                .ok_or(
                    "Blockchain boş",
                )?;

        if timestamp
            <= last_block.timestamp
        {
            return Err(
                "Yeni block timestamp önceki block timestamp'inden büyük olmalı"
                    .into(),
            );
        }

        Self::validate_future_timestamp(
            timestamp,
        )?;

        // ==========================
        // GEÇİCİ KOPYALAR
        // ==========================

        let mut candidate_state =
            state.clone();

        let mut candidate_economy =
            self.economy.clone();

        let mut candidate_mempool =
            Mempool {
                transactions:
                    mempool.transactions.clone(),
            };

        // ==========================
        // GEÇERLİ TRANSACTION'LAR
        // ==========================

        let mut transactions =
            candidate_mempool
                .take_valid_transactions(
                    &candidate_state,
                );

        if transactions.is_empty() {
            return Err(
                "Geçerli transaction yok".into(),
            );
        }

        // ==========================
        // FEE KONTROLÜ
        // ==========================

        for transaction
            in &transactions
        {
            if !candidate_economy
                .validate_fee(
                    transaction.amount,
                    transaction.fee,
                )
            {
                return Err(format!(
                    "Transaction fee geçersiz. TX: {}",
                    transaction.id
                ));
            }
        }

        // ==========================
        // NORMAL TRANSACTION'LAR
        // ==========================

        candidate_state
            .apply_transactions_atomically(
                &transactions,
            )?;

        // ==========================
        // FEE DAĞITIMI
        // ==========================

        let mut validator_fee_total =
            0u64;

        let mut liquidity_fee_total =
            0u64;

        let mut treasury_fee_total =
            0u64;

        let mut burn_fee_total =
            0u64;

        for transaction
            in &transactions
        {
            let (
                validator_fee,
                liquidity_fee,
                treasury_fee,
                burn_fee,
            ) = candidate_economy
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

            liquidity_fee_total =
                liquidity_fee_total
                    .checked_add(
                        liquidity_fee,
                    )
                    .ok_or(
                        "Liquidity Reserve fee overflow",
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
        // VALIDATOR HESABI
        // ==========================

        if !candidate_state
            .accounts
            .contains_key(
                validator_wallet.address(),
            )
        {
            candidate_state
                .create_account(
                    validator_wallet
                        .address()
                        .to_string(),
                    0,
                );
        }

        // ==========================
        // VALIDATOR FEE
        // ==========================

        candidate_state
            .add_balance(
                validator_wallet.address(),
                validator_fee_total,
            )?;

        // ==========================
        // TREASURY
        // ==========================

        candidate_state
            .add_treasury(
                treasury_fee_total,
            )?;

        // ==========================
        // LIQUIDITY RESERVE
        // ==========================

        candidate_economy
            .add_liquidity_reserve(
                liquidity_fee_total,
            )?;

        // ==========================
        // BURN
        // ==========================

        candidate_state
            .burn(
                burn_fee_total,
            )?;

        candidate_economy
            .burn(
                burn_fee_total,
            );

        // ==========================
        // BLOCK BİLGİLERİ
        // ==========================

        let index =
            self.chain.len() as u64;

        let previous_hash =
            last_block
                .hash
                .clone();

        // ==========================
        // COINBASE REWARD
        // ==========================

        let reward =
            candidate_economy
                .reward_validator()?;

        let coinbase =
            Transaction::new_coinbase(
                validator_wallet
                    .address()
                    .to_string(),
                reward,
                index,
            );

        candidate_state
            .apply_transaction(
                &coinbase,
            )?;

        transactions.push(
            coinbase,
        );

        // ==========================
        // BLOCK OLUŞTUR
        // ==========================

        let mut block =
            Block::new(
                index,
                timestamp,
                previous_hash,
                validator_wallet
                    .address()
                    .to_string(),
                validator_wallet
                    .public_key_hex(),
                transactions.clone(),
            );

        // ==========================
        // BLOCK İMZALA
        // ==========================

        let signature =
            validator_wallet.sign(
                block.hash.as_bytes(),
            );

        block.sign(
            signature,
        );

        Self::validate_block_field_sizes(
            &block,
        )?;

        self.validate_transaction_id_uniqueness(
            &block,
        )?;

        // ==========================
        // ONAYLANAN TX ID'LERİ
        // ==========================

        let confirmed_transaction_ids:
            Vec<String> =
            transactions
                .iter()
                .filter(
                    |transaction| {
                        !transaction.coinbase
                    },
                )
                .map(
                    |transaction| {
                        transaction.id.clone()
                    },
                )
                .collect();

        // ==========================
        // ATOMİK COMMIT
        // ==========================

        *state =
            candidate_state;

        self.economy =
            candidate_economy;

        mempool
            .transactions
            .retain(
                |pending_transaction| {
                    !confirmed_transaction_ids
                        .iter()
                        .any(
                            |confirmed_id| {
                                confirmed_id
                                    == &pending_transaction.id
                            },
                        )
                },
            );

        self.chain.push(
            block.clone(),
        );

        println!(
            "💰 Toplam KBN arzı: {}",
            self.economy
                .supply()
                / 1_000_000
        );

        Ok(block)
    }

    // ==========================
    // GELEN BLOCK EKLE
    // ==========================

    pub fn add_received_block(
        &mut self,
        block: Block,
    ) -> Result<(), String> {
        Self::validate_block_field_sizes(
            &block,
        )?;

        self.validate_transaction_id_uniqueness(
            &block,
        )?;

        let last_block =
            self.chain
                .last()
                .ok_or(
                    "Blockchain boş",
                )?;

        if block.previous_hash
            != last_block.hash
        {
            return Err(
                "Önceki hash uyuşmuyor"
                    .into(),
            );
        }

        if block.timestamp
            <= last_block.timestamp
        {
            return Err(
                "Block timestamp önceki block timestamp'inden büyük olmalı"
                    .into(),
            );
        }

        Self::validate_future_timestamp(
            block.timestamp,
        )?;

        if block.hash
            != block.calculate_hash()
        {
            return Err(
                "Block hash geçersiz"
                    .into(),
            );
        }

        if block.index
            != self.chain.len() as u64
        {
            return Err(
                "Block index hatalı"
                    .into(),
            );
        }

        self.chain.push(
            block,
        );

        Ok(())
    }

    // ==========================
    // BLOCKCHAIN HEIGHT
    // ==========================

    pub fn height(
        &self,
    ) -> usize {
        self.chain.len()
    }

    // ==========================
    // BLOCKCHAIN DOĞRULAMA
    // ==========================

    pub fn is_valid(
        &self,
    ) -> bool {
        let mut seen_transaction_ids:
            HashSet<&str> =
            HashSet::new();

        for (
            position,
            block,
        ) in self
            .chain
            .iter()
            .enumerate()
        {
            if Self::validate_block_field_sizes(
                block,
            )
            .is_err()
            {
                return false;
            }

            // ==========================
            // INDEX
            // ==========================

            if block.index
                != position as u64
            {
                return false;
            }

            // ==========================
            // TRANSACTION ID
            // ==========================

            for transaction
                in &block.transactions
            {
                if !seen_transaction_ids.insert(
                    transaction.id.as_str(),
                ) {
                    return false;
                }

                if transaction.id
                    != transaction
                        .calculate_id()
                {
                    return false;
                }

                if transaction.coinbase
                    && transaction.reward_marker
                        != block.index as u128
                {
                    return false;
                }
            }

            // ==========================
            // HASH
            // ==========================

            if block.hash
                != block.calculate_hash()
            {
                return false;
            }

            // ==========================
            // GENESIS
            // ==========================

            if position == 0 {
                if block.previous_hash
                    != "0"
                {
                    return false;
                }

                continue;
            }

            let previous =
                &self.chain[
                    position - 1
                ];

            // ==========================
            // TIMESTAMP
            // ==========================

            if block.timestamp
                <= previous.timestamp
            {
                return false;
            }

            // ==========================
            // ZİNCİR BAĞLANTISI
            // ==========================

            if block.previous_hash
                != previous.hash
            {
                return false;
            }

            // ==========================
            // BLOCK İMZASI
            // ==========================

            if !block.is_signed() {
                return false;
            }

            // ==========================
            // VALIDATOR ADRESİ
            // ==========================

            let derived_validator_address =
                match Wallet::
                    address_from_public_key(
                        &block
                            .validator_public_key,
                    )
                {
                    Some(address) => {
                        address
                    }

                    None => {
                        return false;
                    }
                };

            if derived_validator_address
                != block.validator
            {
                return false;
            }

            // ==========================
            // VALIDATOR SIGNATURE
            // ==========================

            let validator_signature =
                match &block
                    .validator_signature
                {
                    Some(signature) => {
                        signature
                    }

                    None => {
                        return false;
                    }
                };

            if !Wallet::verify(
                &block.validator_public_key,
                block.hash.as_bytes(),
                validator_signature,
            ) {
                return false;
            }
        }

        true
    }
}
