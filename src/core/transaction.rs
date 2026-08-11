use serde::{
    Deserialize,
    Serialize,
};
use sha2::{Digest, Sha256};

#[allow(dead_code)]
pub const COIN_UNIT: u64 = 1_000_000;

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
)]
pub struct Transaction {
    pub id: String,
    pub from: String,
    pub public_key: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
    pub signature: Option<String>,
    pub coinbase: bool,
    pub reward_marker: u128,
}

impl Transaction {
    pub fn new(
        from: String,
        public_key: String,
        to: String,
        amount: u64,
        fee: u64,
        nonce: u64,
    ) -> Self {
        let mut tx = Self {
            id: String::new(),
            from,
            public_key,
            to,
            amount,
            fee,
            nonce,
            signature: None,
            coinbase: false,
            reward_marker: 0,
        };

        tx.id = tx.calculate_id();

        tx
    }

    pub fn new_coinbase(
        validator: String,
        reward: u64,
        block_index: u64,
    ) -> Self {
        let mut tx = Self {
            id: String::new(),
            from: "SYSTEM".to_string(),
            public_key: "SYSTEM".to_string(),
            to: validator,
            amount: reward,
            fee: 0,
            nonce: 0,
            signature: Some(
                "SYSTEM_REWARD".to_string(),
            ),
            coinbase: true,
            reward_marker: block_index as u128,
        };

        tx.id = tx.calculate_id();

        tx
    }

    pub fn calculate_id(
        &self,
    ) -> String {
        let mut hasher = Sha256::new();

        hasher.update(
            self.message(),
        );

        format!(
            "{:x}",
            hasher.finalize()
        )
    }

    fn append_string(
        buffer: &mut Vec<u8>,
        value: &str,
    ) {
        let bytes =
            value.as_bytes();

        let length =
            bytes.len() as u64;

        buffer.extend_from_slice(
            &length.to_be_bytes(),
        );

        buffer.extend_from_slice(
            bytes,
        );
    }

    pub fn message(
        &self,
    ) -> Vec<u8> {
        let mut message =
            Vec::new();

        message.extend_from_slice(
            b"AION_TX_V1",
        );

        Self::append_string(
            &mut message,
            &self.from,
        );

        Self::append_string(
            &mut message,
            &self.public_key,
        );

        Self::append_string(
            &mut message,
            &self.to,
        );

        message.extend_from_slice(
            &self.amount.to_be_bytes(),
        );

        message.extend_from_slice(
            &self.fee.to_be_bytes(),
        );

        message.extend_from_slice(
            &self.nonce.to_be_bytes(),
        );

        message.push(
            if self.coinbase {
                1
            } else {
                0
            },
        );

        message.extend_from_slice(
            &self.reward_marker
                .to_be_bytes(),
        );

        message
    }

    pub fn sign(
        &mut self,
        signature: String,
    ) {
        self.signature =
            Some(signature);
    }

    pub fn is_signed(
        &self,
    ) -> bool {
        self.signature.is_some()
    }
}