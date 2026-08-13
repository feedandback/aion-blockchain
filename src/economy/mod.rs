#[derive(Debug, Clone)]
pub struct Economy {
    // Micro unit
    // 1 KBN = 1.000.000 unit
    pub total_supply: u64,

    pub max_supply: u64,

    // Block reward
    pub block_reward: u64,

    // Minimum transaction fee
    pub minimum_fee: u64,

    // Network fee'lerinden biriken Liquidity Reserve
    pub liquidity_reserve: u64,

    // Fee dağılımı
    #[allow(dead_code)]
    pub validator_fee_percent: u64,

    pub liquidity_fee_percent: u64,

    pub treasury_fee_percent: u64,

    #[allow(dead_code)]
    pub burn_fee_percent: u64,
}

impl Economy {
    pub fn new() -> Self {
        Self {
            // Genesis başlangıç arzı
            total_supply: 0,

            // 100 milyon KBN
            max_supply: 100_000_000 * 1_000_000,

            // Validator ödülü
            block_reward: 10 * 1_000_000,

            // Minimum fee
            // 10 microKBN
            minimum_fee: 10,

            liquidity_reserve: 0,

            validator_fee_percent: 15,

            liquidity_fee_percent: 80,

            treasury_fee_percent: 5,

            burn_fee_percent: 0,
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
            amount / 100_000;

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
    ) -> (u64, u64, u64, u64) {
        let liquidity =
            u64::try_from(
                u128::from(fee)
                    * u128::from(
                        self.liquidity_fee_percent,
                    )
                    / 100,
            )
            .expect(
                "Liquidity Reserve fee u64 aralığını aşıyor",
            );

        let treasury =
            u64::try_from(
                u128::from(fee)
                    * u128::from(
                        self.treasury_fee_percent,
                    )
                    / 100,
            )
            .expect(
                "Treasury fee u64 aralığını aşıyor",
            );

        let burn = 0;

        let validator =
            fee.checked_sub(
                liquidity,
            )
            .and_then(
                |remaining| {
                    remaining.checked_sub(
                        treasury,
                    )
                },
            )
            .expect(
                "Liquidity Reserve ve treasury payları toplam fee'yi aşıyor",
            );

        (
            validator,
            liquidity,
            treasury,
            burn,
        )
    }

    pub fn add_liquidity_reserve(
        &mut self,
        amount: u64,
    ) -> Result<(), String> {
        self.liquidity_reserve =
            self.liquidity_reserve
                .checked_add(amount)
                .ok_or(
                    "Liquidity Reserve overflow",
                )?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn liquidity_reserve(
        &self,
    ) -> u64 {
        self.liquidity_reserve
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
