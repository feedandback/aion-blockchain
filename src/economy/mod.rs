#[derive(Debug, Clone)]
pub struct Economy {
    // Micro unit
    // 1 AION = 1.000.000 unit
    pub total_supply: u64,

    pub max_supply: u64,

    // Block reward
    pub block_reward: u64,

    // Minimum transaction fee
    pub minimum_fee: u64,

    // Fee dağılımı
    pub validator_fee_percent: u64,

    pub treasury_fee_percent: u64,

    pub burn_fee_percent: u64,
}

impl Economy {
    pub fn new() -> Self {
        Self {
            // Genesis başlangıç arzı
            total_supply: 0,

            // 100 milyon AION
            max_supply: 100_000_000 * 1_000_000,

            // Validator ödülü
            block_reward: 10 * 1_000_000,

            // Minimum fee
            // 0.001 AION
            minimum_fee: 1_000,

            validator_fee_percent: 70,

            treasury_fee_percent: 20,

            burn_fee_percent: 10,
        }
    }

    // ==========================
    // TRANSACTION FEE
    // ==========================

    pub fn calculate_fee(
        &self,
        amount: u64,
    ) -> u64 {
        let percentage_fee =
            amount / 1000;

        percentage_fee.max(
            self.minimum_fee,
        )
    }

    // ==========================
    // FEE DOĞRULAMA
    // ==========================

    pub fn validate_fee(
        &self,
        amount: u64,
        fee: u64,
    ) -> bool {
        fee == self.calculate_fee(amount)
    }

    // ==========================
    // FEE DAĞITIMI
    // ==========================

    pub fn distribute_fee(
        &self,
        fee: u64,
    ) -> (u64, u64, u64) {
        let validator =
            fee
                * self.validator_fee_percent
                / 100;

        let treasury =
            fee
                * self.treasury_fee_percent
                / 100;

        let burn =
            fee
                * self.burn_fee_percent
                / 100;

        (
            validator,
            treasury,
            burn,
        )
    }

    // ==========================
    // MINT KONTROLÜ
    // ==========================

    pub fn can_mint(
        &self,
        amount: u64,
    ) -> bool {
        match self
            .total_supply
            .checked_add(amount)
        {
            Some(new_supply) => {
                new_supply
                    <= self.max_supply
            }

            None => false,
        }
    }

    // ==========================
    // MINT
    // ==========================

    pub fn mint(
        &mut self,
        amount: u64,
    ) -> Result<(), String> {
        if !self.can_mint(amount) {
            return Err(
                "Maksimum arz aşıldı"
                    .into(),
            );
        }

        self.total_supply += amount;

        Ok(())
    }

    // ==========================
    // BURN
    // ==========================

    pub fn burn(
        &mut self,
        amount: u64,
    ) {
        if self.total_supply >= amount {
            self.total_supply -= amount;
        }
    }

    // ==========================
    // VALIDATOR REWARD
    // ==========================

    pub fn reward_validator(
        &mut self,
    ) -> Result<u64, String> {
        let reward =
            self.block_reward;

        self.mint(reward)?;

        Ok(reward)
    }

    // ==========================
    // SUPPLY
    // ==========================

    pub fn supply(&self) -> u64 {
        self.total_supply
    }
}