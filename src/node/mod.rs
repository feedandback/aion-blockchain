use std::path::{Path, PathBuf};

use crate::chain::{Blockchain, Mempool};
use crate::consensus::Consensus;
use crate::core::{Block, Transaction};
use crate::economy::Economy;
use crate::network::{Network, NetworkMessage};
use crate::protocol::{
    ADDRESS_HEX_LENGTH, HASH_HEX_LENGTH, MAX_SYNC_BLOCKS_PER_MESSAGE,
    MAX_TOTAL_TRANSACTIONS_PER_BLOCK, PUBLIC_KEY_HEX_LENGTH, SIGNATURE_HEX_LENGTH, SYSTEM_ADDRESS,
    SYSTEM_PUBLIC_KEY, SYSTEM_REWARD_SIGNATURE, is_fixed_hex,
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

    // During chain sync, state and economy
    // are rebuilt from genesis.
    genesis_state: State,
    genesis_economy: Economy,

    storage_directory: PathBuf,

    // For chunked blockchain synchronization
    // temporary remote chain buffer.
    sync_buffer: Vec<Block>,
    sync_expected_total: Option<u64>,

    #[cfg(test)]
    #[allow(dead_code)]
    skip_chain_sync_persistence_for_test: bool,
}

impl Node {
    pub fn new(blockchain: Blockchain, state: State, consensus: Consensus) -> Self {
        Self::new_with_data_directory(blockchain, state, consensus, Storage::data_directory())
    }

    pub fn new_with_data_directory(
        blockchain: Blockchain,
        state: State,
        consensus: Consensus,
        storage_directory: PathBuf,
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
            storage_directory,
            sync_buffer: Vec::new(),
            sync_expected_total: None,
            #[cfg(test)]
            skip_chain_sync_persistence_for_test: false,
        }
    }

    pub fn storage_directory(&self) -> &Path {
        &self.storage_directory
    }

    pub fn restore_chain_from_storage(&mut self, chain: Vec<Block>) -> Result<(), String> {
        self.synchronize_chain(chain)
    }

    // ==========================
    // PROTOCOL FIELD FORMAT
    // ==========================

    fn validate_transaction_field_sizes(&self, transaction: &Transaction) -> Result<(), String> {
        if !is_fixed_hex(&transaction.id, HASH_HEX_LENGTH) {
            return Err("Transaction ID format is invalid".into());
        }

        if transaction.coinbase {
            if transaction.from != SYSTEM_ADDRESS || transaction.public_key != SYSTEM_PUBLIC_KEY {
                return Err("Coinbase SYSTEM fields are invalid".into());
            }

            if !is_fixed_hex(&transaction.to, ADDRESS_HEX_LENGTH) {
                return Err("Coinbase recipient address format is invalid".into());
            }

            match transaction.signature.as_ref() {
                Some(signature) if signature == SYSTEM_REWARD_SIGNATURE => {}

                _ => {
                    return Err("Coinbase signature format is invalid".into());
                }
            }

            return Ok(());
        }

        if !is_fixed_hex(&transaction.from, ADDRESS_HEX_LENGTH)
            || !is_fixed_hex(&transaction.to, ADDRESS_HEX_LENGTH)
        {
            return Err("Transaction address format is invalid".into());
        }

        if !is_fixed_hex(&transaction.public_key, PUBLIC_KEY_HEX_LENGTH) {
            return Err("Transaction public key format is invalid".into());
        }

        let signature = transaction
            .signature
            .as_ref()
            .ok_or("Transaction signature is missing")?;

        if !is_fixed_hex(signature, SIGNATURE_HEX_LENGTH) {
            return Err("Transaction signature format is invalid".into());
        }

        Ok(())
    }

    // ==========================
    // BASE TRANSACTION VALIDATION
    // ==========================

    fn validate_transaction_base(
        &self,
        transaction: &Transaction,
        economy: &Economy,
    ) -> Result<(), String> {
        self.validate_transaction_field_sizes(transaction)?;

        if transaction.coinbase {
            return Err("Coinbase transaction cannot be added to the mempool".into());
        }

        if transaction.from == SYSTEM_ADDRESS {
            return Err("Normal transaction cannot originate from the SYSTEM address".into());
        }

        if transaction.reward_marker != 0 {
            return Err("Normal transaction reward marker must be zero".into());
        }

        if transaction.id != transaction.calculate_id() {
            return Err("Transaction ID is invalid".into());
        }

        if !transaction.is_signed() {
            return Err("Transaction signature is missing".into());
        }

        let derived_address = Wallet::address_from_public_key(&transaction.public_key)
            .ok_or("Transaction public key is invalid")?;

        if derived_address != transaction.from {
            return Err("Transaction sender address does not match the public key".into());
        }

        let signature = transaction
            .signature
            .as_ref()
            .ok_or("Transaction signature was not found")?;

        if !Wallet::verify(&transaction.public_key, &transaction.message(), signature) {
            return Err("Transaction signature is invalid".into());
        }

        if transaction.amount == 0 {
            return Err("Transaction amount cannot be zero".into());
        }

        if !economy.validate_fee(transaction.amount, transaction.fee) {
            let expected_fee = economy.calculate_fee(transaction.amount);

            return Err(format!(
                "Transaction fee is invalid. Expected: {}, Received: {}",
                expected_fee, transaction.fee
            ));
        }

        Ok(())
    }

    // ==========================
    // IN-BLOCK TRANSACTION VALIDATION
    // ==========================

    fn validate_transaction(
        &self,
        transaction: &Transaction,
        state: &State,
        economy: &Economy,
    ) -> Result<(), String> {
        self.validate_transaction_base(transaction, economy)?;

        let total_cost = transaction
            .amount
            .checked_add(transaction.fee)
            .ok_or("Transaction total amount overflow")?;

        let balance = state.balance_of(&transaction.from);

        if balance < total_cost {
            return Err("Insufficient balance for transaction".into());
        }

        let expected_nonce = state.nonce_of(&transaction.from);

        if transaction.nonce != expected_nonce {
            return Err(format!(
                "Transaction nonce is invalid. Expected: {}, Received: {}",
                expected_nonce, transaction.nonce
            ));
        }

        Ok(())
    }

    // ==========================
    // MEMPOOL TRANSACTION VALIDATION
    // ==========================

    fn validate_transaction_for_mempool(
        &self,
        transaction: &Transaction,
        state: &State,
        economy: &Economy,
        mempool: &Mempool,
    ) -> Result<(), String> {
        self.validate_transaction_base(transaction, economy)?;

        let state_nonce = state.nonce_of(&transaction.from);

        let expected_nonce = mempool.next_nonce(&transaction.from, state_nonce)?;

        if transaction.nonce != expected_nonce {
            return Err(format!(
                "Transaction nonce is invalid. Expected: {}, Received: {}",
                expected_nonce, transaction.nonce
            ));
        }

        let transaction_cost = transaction
            .amount
            .checked_add(transaction.fee)
            .ok_or("Transaction total amount overflow")?;

        let pending_cost = mempool.pending_cost(&transaction.from)?;

        let total_reserved_cost = pending_cost
            .checked_add(transaction_cost)
            .ok_or("Mempool reserved total overflow")?;

        let balance = state.balance_of(&transaction.from);

        if balance < total_reserved_cost {
            return Err("Insufficient available balance for transaction".into());
        }

        Ok(())
    }

    // ==========================
    // ADD TRANSACTION
    // ==========================

    fn insert_transaction_into_mempool(&mut self, transaction: Transaction) -> Result<(), String> {
        self.validate_transaction_for_mempool(
            &transaction,
            &self.state,
            &self.blockchain.economy,
            &self.mempool,
        )?;

        if !self.mempool.add_transaction(transaction) {
            return Err("Transaction could not be added to mempool: limit or duplicate".into());
        }

        Ok(())
    }

    pub(crate) fn receive_transaction_with_result(
        &mut self,
        transaction: Transaction,
    ) -> Result<(), String> {
        self.insert_transaction_into_mempool(transaction)
    }

    pub fn add_transaction(&mut self, transaction: Transaction) -> bool {
        match self.insert_transaction_into_mempool(transaction.clone()) {
            Ok(()) => {
                self.network
                    .broadcast(NetworkMessage::Transaction(transaction));
                true
            }
            Err(error) => {
                println!(" Transaction rejected: {}", error);
                false
            }
        }
    }

    // ==========================
    // NETWORK MESSAGE
    // ==========================

    pub fn receive_message(&mut self, message: NetworkMessage) {
        match message {
            message @ NetworkMessage::Handshake { .. } => {
                self.network.receive(message);
            }

            message @ NetworkMessage::HandshakeAck { .. } => {
                self.network.receive(message);
            }

            message @ NetworkMessage::TransactionAck { .. } => {
                self.network.receive(message);
            }

            message @ NetworkMessage::AccountStateRequest { .. }
            | message @ NetworkMessage::AccountStateResponse { .. } => {
                self.network.receive(message);
            }
            NetworkMessage::Transaction(transaction) => {
                match self.insert_transaction_into_mempool(transaction) {
                    Ok(()) => {
                        println!(" Network transaction validated and added to mempool");
                    }

                    Err(error) => {
                        println!(" Network transaction rejected: {}", error);
                    }
                }
            }

            NetworkMessage::Block(block) => {
                println!(" Network block received. Index: {}", block.index);

                let already_known = self
                    .blockchain
                    .chain
                    .iter()
                    .any(|known_block| known_block.hash == block.hash);

                if already_known {
                    println!("Block already known: {}", block.hash);
                } else {
                    let accepted = self.receive_block(block);

                    println!("Block accepted: {}", accepted);
                }
            }

            NetworkMessage::SyncRequest => {
                self.network.receive(NetworkMessage::SyncRequest);
            }

            NetworkMessage::ChainChunkRequest { start_index } => {
                let start = match usize::try_from(start_index) {
                    Ok(start) => start,

                    Err(_) => {
                        println!(" Chain chunk request rejected: start index invalid");

                        return;
                    }
                };

                let total_blocks = self.blockchain.chain.len();

                if start > total_blocks {
                    println!(" Chain chunk request rejected: start index exceeds chain length");

                    return;
                }

                let end = start
                    .saturating_add(MAX_SYNC_BLOCKS_PER_MESSAGE)
                    .min(total_blocks);

                let blocks = self.blockchain.chain[start..end].to_vec();

                println!(
                    " Chain chunk request. Start: {}, sent: {}, total: {}",
                    start,
                    blocks.len(),
                    total_blocks
                );

                self.network.broadcast(NetworkMessage::ChainChunkResponse {
                    start_index,
                    total_blocks: total_blocks as u64,
                    blocks,
                });
            }

            NetworkMessage::ChainChunkResponse {
                start_index,
                total_blocks,
                blocks,
            } => {
                println!(
                    " Chain chunk response. Start: {}, chunk: {}, total: {}",
                    start_index,
                    blocks.len(),
                    total_blocks
                );

                match self.accept_chain_chunk(start_index, total_blocks, blocks) {
                    Ok(Some(next_index)) => {
                        self.network.broadcast(NetworkMessage::ChainChunkRequest {
                            start_index: next_index,
                        });
                    }

                    Ok(None) => {
                        println!(" Chunked blockchain synchronization completed.");
                    }

                    Err(error) => {
                        println!(" Chain chunk rejected: {}", error);
                    }
                }
            }
        }
    }

    // ==========================
    // VALIDATE + APPLY BLOCK
    // ==========================

    fn validate_and_apply_block(
        &self,
        block: Block,
        current_blockchain: &Blockchain,
        current_state: &State,
    ) -> Result<(Blockchain, State), String> {
        // ==========================
        // BLOCK TRANSACTION LIMIT
        // ==========================

        if block.transactions.len() > MAX_TOTAL_TRANSACTIONS_PER_BLOCK {
            return Err("Block transaction limit exceeded".into());
        }

        if !is_fixed_hex(&block.previous_hash, HASH_HEX_LENGTH)
            || !is_fixed_hex(&block.hash, HASH_HEX_LENGTH)
        {
            return Err("Block hash format is invalid".into());
        }

        if !is_fixed_hex(&block.validator, ADDRESS_HEX_LENGTH) {
            return Err("Block validator address format is invalid".into());
        }

        if !is_fixed_hex(&block.validator_public_key, PUBLIC_KEY_HEX_LENGTH) {
            return Err("Block validator public key format is invalid".into());
        }

        let validator_signature = block
            .validator_signature
            .as_ref()
            .ok_or("Block validator signature is missing")?;

        if !is_fixed_hex(validator_signature, SIGNATURE_HEX_LENGTH) {
            return Err("Block validator signature format is invalid".into());
        }

        for transaction in &block.transactions {
            self.validate_transaction_field_sizes(transaction)?;
        }

        // ==========================
        // BLOCK HASH
        // ==========================

        if !block.is_hash_valid() {
            return Err("Block hash is invalid".into());
        }

        // ==========================
        // BLOCK CHAIN LINKAGE
        // ==========================

        let mut candidate_blockchain = current_blockchain.clone();

        candidate_blockchain.add_received_block(block.clone())?;

        // ==========================
        // SELECTED VALIDATOR
        // ==========================

        let expected_validator = self
            .consensus
            .select_validator_from_hash(&block.previous_hash)
            .ok_or("Validator could not be selected")?
            .address
            .clone();

        if block.validator != expected_validator {
            return Err("Selected validator for this block is different".into());
        }

        // ==========================
        // VALIDATOR PUBLIC KEY
        // ==========================

        let derived_validator_address =
            Wallet::address_from_public_key(&block.validator_public_key)
                .ok_or("Validator public key is invalid")?;

        if derived_validator_address != block.validator {
            return Err("Validator address does not match the public key".into());
        }

        // ==========================
        // VALIDATOR CONSENSUS
        // ==========================

        if !self.consensus.is_validator_allowed(&block.validator) {
            return Err("Validator is not registered in the consensus network".into());
        }

        // ==========================
        // VALIDATOR SIGNATURE
        // ==========================

        let validator_signature = block
            .validator_signature
            .as_ref()
            .ok_or("Validator signature is missing")?;

        if !Wallet::verify(
            &block.validator_public_key,
            block.hash.as_bytes(),
            validator_signature,
        ) {
            return Err("Validator signature is invalid".into());
        }

        // ==========================
        // TRANSACTIONS
        // ==========================

        if block.transactions.is_empty() {
            return Err("Block transaction list is empty".into());
        }

        let mut rebuilt_state = current_state.clone();

        let mut rebuilt_economy = current_blockchain.economy.clone();

        let mut validator_fee_total = 0u64;
        let mut liquidity_fee_total = 0u64;
        let mut treasury_fee_total = 0u64;
        let mut burn_fee_total = 0u64;

        let mut coinbase: Option<Transaction> = None;

        for (position, transaction) in block.transactions.iter().enumerate() {
            // ==========================
            // TX ID
            // ==========================

            if transaction.id != transaction.calculate_id() {
                return Err("Transaction ID inside block is invalid".into());
            }

            // ==========================
            // COINBASE
            // ==========================

            if transaction.coinbase {
                if coinbase.is_some() {
                    return Err("Block contains more than one coinbase".into());
                }

                if position != block.transactions.len() - 1 {
                    return Err("Coinbase must be the last transaction".into());
                }

                if transaction.from != SYSTEM_ADDRESS {
                    return Err("Coinbase sender must be SYSTEM".into());
                }

                if transaction.public_key != SYSTEM_PUBLIC_KEY {
                    return Err("Coinbase public key is invalid".into());
                }

                if transaction.signature.as_deref() != Some(SYSTEM_REWARD_SIGNATURE) {
                    return Err("Coinbase signature is invalid".into());
                }

                if transaction.to != block.validator {
                    return Err("Coinbase reward does not go to the validator address".into());
                }

                if transaction.fee != 0 {
                    return Err("Coinbase fee must be zero".into());
                }

                if transaction.nonce != 0 {
                    return Err("Coinbase nonce must be zero".into());
                }

                if transaction.reward_marker != block.index as u128 {
                    return Err("Coinbase reward marker does not match block index".into());
                }

                if transaction.amount != rebuilt_economy.block_reward {
                    return Err("Coinbase reward amount is invalid".into());
                }

                coinbase = Some(transaction.clone());

                continue;
            }

            // ==========================
            // NORMAL TRANSACTION
            // ==========================

            if transaction.reward_marker != 0 {
                return Err("Normal transaction reward marker must be zero".into());
            }

            self.validate_transaction(transaction, &rebuilt_state, &rebuilt_economy)?;

            rebuilt_state.apply_transaction(transaction)?;

            let (validator_fee, liquidity_fee, treasury_fee, burn_fee) =
                rebuilt_economy.distribute_fee(transaction.fee);

            validator_fee_total = validator_fee_total
                .checked_add(validator_fee)
                .ok_or("Validator fee overflow")?;

            liquidity_fee_total = liquidity_fee_total
                .checked_add(liquidity_fee)
                .ok_or("Liquidity Reserve fee overflow")?;

            treasury_fee_total = treasury_fee_total
                .checked_add(treasury_fee)
                .ok_or("Treasury fee overflow")?;

            burn_fee_total = burn_fee_total
                .checked_add(burn_fee)
                .ok_or("Burn fee overflow")?;
        }

        // ==========================
        // COINBASE VAR MI?
        // ==========================

        let coinbase = coinbase.ok_or("Coinbase transaction was not found")?;

        // ==========================
        // VALIDATOR ACCOUNT
        // ==========================

        if !rebuilt_state.accounts.contains_key(&block.validator) {
            rebuilt_state.create_account(block.validator.clone(), 0);
        }

        // ==========================
        // FEE DISTRIBUTION
        // ==========================

        rebuilt_state.add_balance(&block.validator, validator_fee_total)?;

        rebuilt_state.add_treasury(treasury_fee_total)?;

        rebuilt_economy.add_liquidity_reserve(liquidity_fee_total)?;

        // ==========================
        // BURN
        // ==========================

        rebuilt_state.burn(burn_fee_total)?;

        rebuilt_economy.burn(burn_fee_total);

        // ==========================
        // COINBASE MINT
        // ==========================

        rebuilt_economy.mint(coinbase.amount)?;

        // ==========================
        // COINBASE STATE
        // ==========================

        rebuilt_state.apply_transaction(&coinbase)?;

        // ==========================
        // ECONOMY COMMIT
        // ==========================

        candidate_blockchain.economy = rebuilt_economy;

        Ok((candidate_blockchain, rebuilt_state))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn validate_and_apply_block_for_test(
        &self,
        block: Block,
    ) -> Result<(Blockchain, State), String> {
        self.validate_and_apply_block(block, &self.blockchain, &self.state)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn apply_chain_chunk_without_persisting_for_test(
        &mut self,
        start_index: u64,
        total_blocks: u64,
        blocks: Vec<Block>,
    ) -> Result<Option<u64>, String> {
        let previous_setting = self.skip_chain_sync_persistence_for_test;

        self.skip_chain_sync_persistence_for_test = true;

        let result = self.apply_chain_chunk(start_index, total_blocks, blocks);

        self.skip_chain_sync_persistence_for_test = previous_setting;

        result
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn receive_message_without_persisting_for_test(&mut self, message: NetworkMessage) {
        let previous_setting = self.skip_chain_sync_persistence_for_test;

        self.skip_chain_sync_persistence_for_test = true;

        self.receive_message(message);

        self.skip_chain_sync_persistence_for_test = previous_setting;
    }

    // ==========================
    // CLEAN MEMPOOL
    // ==========================

    fn remove_confirmed_transactions(&mut self, block: &Block) {
        self.mempool.transactions.retain(|pending_transaction| {
            !block.transactions.iter().any(|confirmed_transaction| {
                !confirmed_transaction.coinbase
                    && confirmed_transaction.id == pending_transaction.id
            })
        });
    }

    // ==========================
    // BLOCK AL
    // ==========================

    pub fn receive_block(&mut self, block: Block) -> bool {
        let confirmed_block = block.clone();

        match self.validate_and_apply_block(block, &self.blockchain, &self.state) {
            Ok((candidate_blockchain, rebuilt_state)) => {
                if let Err(error) = Storage::save_blockchain_to(
                    &self.storage_directory,
                    &candidate_blockchain.chain,
                ) {
                    println!(
                        " Block could not be written to persistent storage: {}",
                        error
                    );

                    return false;
                }

                self.blockchain = candidate_blockchain;

                self.state = rebuilt_state;

                self.remove_confirmed_transactions(&confirmed_block);

                println!(" New block validated, added to the chain, and state updated.");

                println!(" Confirmed transactions removed from mempool.");

                true
            }

            Err(error) => {
                println!(" Block rejected: {}", error);

                false
            }
        }
    }

    pub(crate) fn apply_block_to_sync_candidate(&mut self, block: Block) -> Result<(), String> {
        let confirmed_block = block.clone();
        let (candidate_blockchain, rebuilt_state) =
            self.validate_and_apply_block(block, &self.blockchain, &self.state)?;

        self.blockchain = candidate_blockchain;
        self.state = rebuilt_state;
        self.remove_confirmed_transactions(&confirmed_block);

        Ok(())
    }

    pub(crate) fn sync_candidate_snapshot(&self) -> Self {
        Self::new_with_data_directory(
            self.blockchain.clone(),
            self.state.clone(),
            self.consensus.clone(),
            PathBuf::new(),
        )
    }

    pub(crate) fn adopt_validated_sync_candidate(
        &mut self,
        candidate_blockchain: Blockchain,
        candidate_state: State,
    ) -> Result<(), String> {
        let local_genesis = self
            .blockchain
            .chain
            .first()
            .ok_or("Local genesis was not found")?;
        let candidate_genesis = candidate_blockchain
            .chain
            .first()
            .ok_or("Candidate genesis was not found")?;

        if candidate_genesis.hash != local_genesis.hash {
            return Err("Candidate genesis does not match local genesis".into());
        }
        if candidate_blockchain.chain.len() < self.blockchain.chain.len() {
            return Err("Candidate blockchain is shorter than the current chain".into());
        }
        if candidate_blockchain.chain.len() == self.blockchain.chain.len() {
            let candidate_tip = candidate_blockchain
                .chain
                .last()
                .ok_or("Candidate blockchain is empty")?;
            let local_tip = self
                .blockchain
                .chain
                .last()
                .ok_or("Local blockchain is empty")?;
            if candidate_tip.hash != local_tip.hash {
                return Err("Different equal-length candidate fork rejected".into());
            }
            return Ok(());
        }

        Storage::save_blockchain_to(&self.storage_directory, &candidate_blockchain.chain).map_err(
            |error| {
                format!(
                    "Synchronized blockchain could not be written to persistent storage: {error}"
                )
            },
        )?;

        self.blockchain = candidate_blockchain;
        self.state = candidate_state;
        self.mempool = Mempool::new();

        Ok(())
    }

    pub(crate) fn verify_equal_chain(&mut self, chain: Vec<Block>) -> Result<(), String> {
        if chain.len() != self.blockchain.chain.len() {
            return Err(
                "Remote chain to validate is not the same length as the local chain".into(),
            );
        }
        self.synchronize_chain(chain)
    }

    // ==========================
    // CHAIN SYNC
    // ==========================

    pub fn apply_chain_chunk(
        &mut self,
        start_index: u64,
        total_blocks: u64,
        blocks: Vec<Block>,
    ) -> Result<Option<u64>, String> {
        self.accept_chain_chunk(start_index, total_blocks, blocks)
    }

    fn accept_chain_chunk(
        &mut self,
        start_index: u64,
        total_blocks: u64,
        blocks: Vec<Block>,
    ) -> Result<Option<u64>, String> {
        if blocks.len() > MAX_SYNC_BLOCKS_PER_MESSAGE {
            return Err("Sync chunk block limit exceeded".into());
        }

        let total = usize::try_from(total_blocks)
            .map_err(|_| "Sync total block count is invalid".to_string())?;

        let start =
            usize::try_from(start_index).map_err(|_| "Sync start index is invalid".to_string())?;

        if total == 0 {
            return Err("Sync total block count cannot be zero".into());
        }

        if start > total {
            return Err("Sync start index exceeds total block count".into());
        }

        let end = start
            .checked_add(blocks.len())
            .ok_or("Sync chunk index overflow")?;

        if end > total {
            return Err("Sync chunk exceeds total block count".into());
        }

        if start == 0 {
            self.sync_buffer.clear();
            self.sync_expected_total = Some(total_blocks);
        } else {
            if self.sync_expected_total != Some(total_blocks) {
                return Err("Sync total block count changed".into());
            }

            if self.sync_buffer.len() != start {
                return Err("Sync chunk order is invalid".into());
            }
        }

        for (offset, block) in blocks.iter().enumerate() {
            let expected_index = start
                .checked_add(offset)
                .ok_or("Sync block index overflow")? as u64;

            if block.index != expected_index {
                return Err(format!(
                    "Sync block index order is invalid. Expected: {}, Received: {}",
                    expected_index, block.index
                ));
            }
        }

        if blocks.is_empty() && start < total {
            return Err("Sync chunk is unexpectedly empty".into());
        }

        self.sync_buffer.extend(blocks);

        if self.sync_buffer.len() < total {
            return Ok(Some(self.sync_buffer.len() as u64));
        }

        if self.sync_buffer.len() != total {
            return Err("Sync buffer length is invalid".into());
        }

        let assembled_chain = std::mem::take(&mut self.sync_buffer);

        self.sync_expected_total = None;

        self.synchronize_chain(assembled_chain)?;

        Ok(None)
    }

    fn synchronize_chain(&mut self, chain: Vec<Block>) -> Result<(), String> {
        if chain.is_empty() {
            return Err("Incoming blockchain is empty".into());
        }

        if chain.len() < self.blockchain.chain.len() {
            return Err("Incoming blockchain is shorter than the current chain".into());
        }

        // ==========================
        // EQUAL-LENGTH FORK CHECK
        // ==========================
        //
        // If there are two different chains at the same height
        // the remote chain must not silently
        // overwrite the local chain.
        //
        // Same height + same tip hash:
        // already synchronized.
        //
        // Same height + different tip hash:
        // it is rejected as a fork.

        if chain.len() == self.blockchain.chain.len() {
            let local_tip_hash = self
                .blockchain
                .chain
                .last()
                .ok_or("Local blockchain is empty")?
                .hash
                .clone();

            let remote_tip_hash = chain
                .last()
                .ok_or("Incoming blockchain is empty")?
                .hash
                .clone();

            if remote_tip_hash == local_tip_hash {
                let remote_blockchain = Blockchain {
                    chain,
                    economy: self.genesis_economy.clone(),
                };

                if !remote_blockchain.is_valid() {
                    return Err("Equal-length blockchain is invalid".into());
                }

                let local_genesis_hash = self
                    .blockchain
                    .chain
                    .first()
                    .ok_or("Local genesis was not found")?
                    .hash
                    .as_str();

                let remote_genesis_hash = remote_blockchain
                    .chain
                    .first()
                    .ok_or("Incoming genesis was not found")?
                    .hash
                    .as_str();

                if remote_genesis_hash != local_genesis_hash {
                    return Err("Genesis blocks do not match".into());
                }

                return Ok(());
            }

            return Err("Different equal-length blockchain fork rejected".into());
        }

        let local_genesis = self
            .blockchain
            .chain
            .first()
            .ok_or("Local genesis was not found")?;

        let remote_genesis = chain.first().ok_or("Incoming genesis was not found")?;

        // ==========================
        // GENESIS CHECK
        // ==========================

        if remote_genesis.index != 0 {
            return Err("Genesis index is invalid".into());
        }

        if remote_genesis.previous_hash != "0" {
            return Err("Genesis previous hash is invalid".into());
        }

        if remote_genesis.hash != remote_genesis.calculate_hash() {
            return Err("Genesis hash is invalid".into());
        }

        if remote_genesis.hash != local_genesis.hash {
            return Err("Genesis blocks do not match".into());
        }

        // ==========================
        // REBUILD FROM GENESIS
        // ==========================

        let mut rebuilt_blockchain = Blockchain::new(remote_genesis.clone());

        rebuilt_blockchain.economy = self.genesis_economy.clone();

        let mut rebuilt_state = self.genesis_state.clone();

        // ==========================
        // REPLAY ALL BLOCKS
        // ==========================

        for block in chain.into_iter().skip(1) {
            let (next_blockchain, next_state) =
                self.validate_and_apply_block(block, &rebuilt_blockchain, &rebuilt_state)?;

            rebuilt_blockchain = next_blockchain;

            rebuilt_state = next_state;
        }

        // ==========================
        // PERSISTENCE + ATOMIC COMMIT
        // ==========================

        #[cfg(not(test))]
        Storage::save_blockchain_to(&self.storage_directory, &rebuilt_blockchain.chain).map_err(
            |error| {
                format!(
                    "Synchronized blockchain could not be written to persistent storage: {}",
                    error
                )
            },
        )?;

        #[cfg(test)]
        if !self.skip_chain_sync_persistence_for_test {
            Storage::save_blockchain_to(&self.storage_directory, &rebuilt_blockchain.chain)
                .map_err(|error| {
                    format!(
                        "Synchronized blockchain could not be written to persistent storage: {}",
                        error
                    )
                })?;
        }

        self.blockchain = rebuilt_blockchain;

        self.state = rebuilt_state;

        self.mempool = Mempool::new();

        Ok(())
    }

    // ==========================
    // NETWORK SYNC
    // ==========================

    pub fn sync_network(&self) {
        println!(" Network message synchronization started.");

        println!("Network message count: {}", self.network.message_count());
    }

    // ==========================
    // CHAIN REQUEST
    // ==========================

    pub fn request_chain(&mut self) {
        println!(" Current blockchain is being requested in chunks.");

        self.sync_buffer.clear();
        self.sync_expected_total = None;

        self.network
            .broadcast(NetworkMessage::ChainChunkRequest { start_index: 0 });
    }

    // ==========================
    // PRODUCE BLOCK
    // ==========================

    pub fn produce_block(
        &mut self,
        timestamp: u64,
        validator_wallet: &Wallet,
    ) -> Result<Block, String> {
        if !self
            .consensus
            .is_validator_allowed(validator_wallet.address())
        {
            return Err("Validator is not registered in the consensus network".into());
        }

        let previous_hash = self
            .blockchain
            .chain
            .last()
            .ok_or("Blockchain is empty")?
            .hash
            .clone();

        let selected_validator = self
            .consensus
            .select_validator_from_hash(&previous_hash)
            .ok_or("Validator could not be selected")?;

        if selected_validator.address != validator_wallet.address() {
            return Err("Selected validator for this block is different".into());
        }

        // ==========================
        // NODE-LEVEL ATOMIC COPIES
        // ==========================

        let mut candidate_blockchain = self.blockchain.clone();

        let mut candidate_state = self.state.clone();

        let mut candidate_mempool = Mempool {
            transactions: self.mempool.transactions.clone(),
        };

        // ==========================
        // PRODUCE BLOCK
        // ==========================

        let block = candidate_blockchain.create_block_from_mempool(
            timestamp,
            validator_wallet,
            &mut candidate_mempool,
            &mut candidate_state,
        )?;

        // ==========================
        // PERSISTENCE + ATOMIC COMMIT
        // ==========================

        Storage::save_blockchain_to(&self.storage_directory, &candidate_blockchain.chain).map_err(
            |error| {
                format!(
                    "Produced blockchain could not be written to persistent storage: {}",
                    error
                )
            },
        )?;

        self.blockchain = candidate_blockchain;

        self.state = candidate_state;

        self.mempool = candidate_mempool;

        Ok(block)
    }

    // ==========================
    // SELECT VALIDATOR
    // ==========================

    pub fn select_validator(&self, seed: &str) -> Option<String> {
        self.consensus
            .select_validator_from_hash(seed)
            .map(|validator| validator.address.clone())
    }

    // ==========================
    // PEER
    // ==========================

    pub fn add_peer(&mut self, peer: String) -> bool {
        self.network.add_peer(peer)
    }

    pub fn peer_count(&self) -> usize {
        self.network.peer_count()
    }
}
