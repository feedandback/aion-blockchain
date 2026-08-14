use crate::core::Transaction;
use crate::protocol::{
    ADDRESS_HEX_LENGTH, HASH_HEX_LENGTH, MAX_MEMPOOL_TRANSACTIONS,
    MAX_NORMAL_TRANSACTIONS_PER_BLOCK, PUBLIC_KEY_HEX_LENGTH, SIGNATURE_HEX_LENGTH, SYSTEM_ADDRESS,
    is_fixed_hex,
};
use crate::state::State;
use crate::wallet::Wallet;

#[derive(Debug, Default)]
pub struct Mempool {
    pub transactions: Vec<Transaction>,
}

impl Mempool {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    fn has_valid_protocol_format(transaction: &Transaction) -> bool {
        if !is_fixed_hex(&transaction.id, HASH_HEX_LENGTH) {
            return false;
        }

        if !is_fixed_hex(&transaction.from, ADDRESS_HEX_LENGTH) {
            return false;
        }

        if !is_fixed_hex(&transaction.to, ADDRESS_HEX_LENGTH) {
            return false;
        }

        if !is_fixed_hex(&transaction.public_key, PUBLIC_KEY_HEX_LENGTH) {
            return false;
        }

        let signature = match transaction.signature.as_ref() {
            Some(signature) => signature,
            None => return false,
        };

        if !is_fixed_hex(signature, SIGNATURE_HEX_LENGTH) {
            return false;
        }

        true
    }

    pub fn add_transaction(&mut self, transaction: Transaction) -> bool {
        if self.transactions.len() >= MAX_MEMPOOL_TRANSACTIONS {
            return false;
        }

        if transaction.coinbase {
            return false;
        }

        if transaction.from == SYSTEM_ADDRESS {
            return false;
        }

        if transaction.amount == 0 {
            return false;
        }

        if transaction.reward_marker != 0 {
            return false;
        }

        // Cheap protocol format checks,
        // before hash and cryptographic operations.
        if !Self::has_valid_protocol_format(&transaction) {
            return false;
        }

        if transaction.id != transaction.calculate_id() {
            return false;
        }

        if !transaction.is_signed() {
            return false;
        }

        let signature = match transaction.signature.as_ref() {
            Some(signature) => signature,
            None => return false,
        };

        let derived = match Wallet::address_from_public_key(&transaction.public_key) {
            Some(address) => address,
            None => return false,
        };

        if derived != transaction.from {
            return false;
        }

        if !Wallet::verify(&transaction.public_key, &transaction.message(), signature) {
            return false;
        }

        if self
            .transactions
            .iter()
            .any(|existing| existing.id == transaction.id)
        {
            return false;
        }

        if self.transactions.iter().any(|existing| {
            existing.from == transaction.from && existing.nonce == transaction.nonce
        }) {
            return false;
        }

        self.transactions.push(transaction);

        true
    }

    pub fn pending_for_sender(&self, address: &str) -> Vec<&Transaction> {
        let mut transactions: Vec<&Transaction> = self
            .transactions
            .iter()
            .filter(|transaction| transaction.from == address)
            .collect();

        transactions.sort_by_key(|transaction| transaction.nonce);

        transactions
    }

    pub fn next_nonce(&self, address: &str, state_nonce: u64) -> Result<u64, String> {
        let mut expected = state_nonce;

        let pending = self.pending_for_sender(address);

        for transaction in pending {
            if transaction.nonce < expected {
                continue;
            }

            if transaction.nonce > expected {
                break;
            }

            expected = expected.checked_add(1).ok_or("Nonce overflow")?;
        }

        Ok(expected)
    }

    pub fn pending_cost(&self, address: &str) -> Result<u64, String> {
        let mut total = 0u64;

        for transaction in self
            .transactions
            .iter()
            .filter(|transaction| transaction.from == address)
        {
            let cost = transaction
                .amount
                .checked_add(transaction.fee)
                .ok_or("Transaction cost overflow")?;

            total = total
                .checked_add(cost)
                .ok_or("Mempool total cost overflow")?;
        }

        Ok(total)
    }

    pub fn take_valid_transactions(&mut self, state: &State) -> Vec<Transaction> {
        let mut temporary_state = state.clone();

        let mut valid_transactions = Vec::new();

        let mut pending_transactions = std::mem::take(&mut self.transactions);

        loop {
            if pending_transactions.is_empty() {
                break;
            }

            let mut next_pending = Vec::new();

            let mut progress = false;

            for transaction in pending_transactions {
                if valid_transactions.len() >= MAX_NORMAL_TRANSACTIONS_PER_BLOCK {
                    next_pending.push(transaction);

                    continue;
                }

                match temporary_state.apply_transaction(&transaction) {
                    Ok(()) => {
                        valid_transactions.push(transaction);

                        progress = true;
                    }

                    Err(_) => {
                        next_pending.push(transaction);
                    }
                }
            }

            if valid_transactions.len() >= MAX_NORMAL_TRANSACTIONS_PER_BLOCK {
                self.transactions = next_pending;

                break;
            }

            if !progress {
                self.transactions = next_pending;

                break;
            }

            pending_transactions = next_pending;
        }

        valid_transactions
    }

    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}
