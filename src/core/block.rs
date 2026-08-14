use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub index: u64,
    pub timestamp: u64,
    pub previous_hash: String,
    pub hash: String,
    pub validator: String,
    pub validator_public_key: String,
    pub validator_signature: Option<String>,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn new(
        index: u64,
        timestamp: u64,
        previous_hash: String,
        validator: String,
        validator_public_key: String,
        transactions: Vec<Transaction>,
    ) -> Self {
        let mut block = Self {
            index,
            timestamp,
            previous_hash,
            hash: String::new(),
            validator,
            validator_public_key,
            validator_signature: None,
            transactions,
        };

        block.hash = block.calculate_hash();

        block
    }

    fn append_bytes(buffer: &mut Vec<u8>, value: &[u8]) {
        let length = value.len() as u64;

        buffer.extend_from_slice(&length.to_be_bytes());

        buffer.extend_from_slice(value);
    }

    fn append_string(buffer: &mut Vec<u8>, value: &str) {
        Self::append_bytes(buffer, value.as_bytes());
    }

    pub fn calculate_hash(&self) -> String {
        let mut data = Vec::new();

        data.extend_from_slice(b"AION_BLOCK_V1");

        data.extend_from_slice(&self.index.to_be_bytes());

        data.extend_from_slice(&self.timestamp.to_be_bytes());

        Self::append_string(&mut data, &self.previous_hash);

        Self::append_string(&mut data, &self.validator);

        Self::append_string(&mut data, &self.validator_public_key);

        let transaction_count = self.transactions.len() as u64;

        data.extend_from_slice(&transaction_count.to_be_bytes());

        for transaction in &self.transactions {
            Self::append_string(&mut data, &transaction.id);

            let transaction_message = transaction.message();

            Self::append_bytes(&mut data, &transaction_message);

            match &transaction.signature {
                Some(signature) => {
                    data.push(1);

                    Self::append_string(&mut data, signature);
                }

                None => {
                    data.push(0);
                }
            }
        }

        let mut hasher = Sha256::new();

        hasher.update(&data);

        format!("{:x}", hasher.finalize())
    }

    pub fn sign(&mut self, signature: String) {
        self.validator_signature = Some(signature);
    }

    pub fn is_signed(&self) -> bool {
        self.validator_signature.is_some()
    }

    pub fn is_hash_valid(&self) -> bool {
        self.hash == self.calculate_hash()
    }
}
