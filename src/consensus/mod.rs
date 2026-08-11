use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Validator {
    pub address: String,
    pub stake: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Consensus {
    pub validators: Vec<Validator>,
}

#[allow(dead_code)]
impl Consensus {
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    pub fn add_validator(&mut self, address: String, stake: u64) -> bool {
        if stake == 0 {
            return false;
        }

        if self.validators.iter().any(|v| v.address == address) {
            return false;
        }

        self.validators.push(Validator { address, stake });

        true
    }

    pub fn increase_stake(
        &mut self,
        address: &str,
        amount: u64,
    ) -> Result<(), String> {
        let validator = self
            .validators
            .iter_mut()
            .find(|v| v.address == address)
            .ok_or("Validator bulunamadı")?;

        validator.stake += amount;

        Ok(())
    }

    pub fn decrease_stake(
        &mut self,
        address: &str,
        amount: u64,
    ) -> Result<(), String> {
        let validator = self
            .validators
            .iter_mut()
            .find(|v| v.address == address)
            .ok_or("Validator bulunamadı")?;

        if validator.stake < amount {
            return Err("Yetersiz stake".into());
        }

        validator.stake -= amount;

        Ok(())
    }

    pub fn get_validator(&self, address: &str) -> Option<&Validator> {
        self.validators.iter().find(|v| v.address == address)
    }

    pub fn total_stake(&self) -> u64 {
        self.validators.iter().map(|v| v.stake).sum()
    }

    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    pub fn select_validator(&self, value: u64) -> Option<&Validator> {
        let total = self.total_stake();

        if total == 0 {
            return None;
        }

        let target = value % total;

        let mut current = 0;

        for validator in &self.validators {
            current += validator.stake;

            if target < current {
                return Some(validator);
            }
        }

        None
    }

    pub fn select_validator_from_hash(
        &self,
        previous_hash: &str,
    ) -> Option<&Validator> {
        let mut hasher = Sha256::new();

        hasher.update(previous_hash.as_bytes());

        let result = hasher.finalize();

        let mut bytes = [0u8; 8];

        bytes.copy_from_slice(&result[..8]);

        let number = u64::from_be_bytes(bytes);

        self.select_validator(number)
    }

    pub fn is_validator_allowed(&self, address: &str) -> bool {
        self.validators
            .iter()
            .any(|v| v.address == address)
    }
}