use crate::chain::Blockchain;
use crate::consensus::Consensus;
use crate::core::Block;
use crate::protocol::{
    GENESIS_PREVIOUS_HASH, GENESIS_SUPPLY_MICRO_KBN, GENESIS_TIMESTAMP, GENESIS_VALIDATOR,
    GENESIS_VALIDATOR_A_ADDRESS, GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN,
    GENESIS_VALIDATOR_A_STAKE, GENESIS_VALIDATOR_B_ADDRESS,
    GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN, GENESIS_VALIDATOR_B_STAKE,
};
use crate::state::State;

#[derive(Debug)]
pub struct CanonicalBootstrap {
    pub blockchain: Blockchain,
    pub state: State,
    pub consensus: Consensus,
}

pub fn canonical_bootstrap() -> Result<CanonicalBootstrap, String> {
    let allocated_supply = GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN
        .checked_add(GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN)
        .ok_or("Canonical genesis allocation overflow")?;

    if allocated_supply != GENESIS_SUPPLY_MICRO_KBN {
        return Err("Canonical genesis allocation toplam arza eşit değil".into());
    }

    let mut consensus = Consensus::new();

    if !consensus.add_validator(
        GENESIS_VALIDATOR_A_ADDRESS.to_string(),
        GENESIS_VALIDATOR_A_STAKE,
    ) {
        return Err("Canonical genesis validator A eklenemedi".into());
    }

    if !consensus.add_validator(
        GENESIS_VALIDATOR_B_ADDRESS.to_string(),
        GENESIS_VALIDATOR_B_STAKE,
    ) {
        return Err("Canonical genesis validator B eklenemedi".into());
    }

    let mut state = State::new();
    state.create_account(
        GENESIS_VALIDATOR_A_ADDRESS.to_string(),
        GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN,
    );
    state.create_account(
        GENESIS_VALIDATOR_B_ADDRESS.to_string(),
        GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN,
    );

    let genesis = Block::new(
        0,
        GENESIS_TIMESTAMP,
        GENESIS_PREVIOUS_HASH.to_string(),
        GENESIS_VALIDATOR.to_string(),
        String::new(),
        Vec::new(),
    );

    let mut blockchain = Blockchain::new(genesis);
    blockchain.economy.mint(GENESIS_SUPPLY_MICRO_KBN)?;

    Ok(CanonicalBootstrap {
        blockchain,
        state,
        consensus,
    })
}
