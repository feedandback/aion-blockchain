use sha2::{Digest, Sha256};

use crate::chain::Blockchain;
use crate::consensus::Consensus;
use crate::core::Block;
use crate::economy::Economy;
use crate::protocol::{
    GENESIS_PREVIOUS_HASH, GENESIS_SUPPLY_MICRO_KBN, GENESIS_TIMESTAMP, GENESIS_VALIDATOR,
    GENESIS_VALIDATOR_A_ADDRESS, GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN,
    GENESIS_VALIDATOR_A_STAKE, GENESIS_VALIDATOR_B_ADDRESS,
    GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN, GENESIS_VALIDATOR_B_STAKE, NETWORK_ID,
    NETWORK_PROTOCOL_VERSION,
};
use crate::state::State;

const GENESIS_FINGERPRINT_DOMAIN: &str = "KYBERNETES_GENESIS_CONFIG_V1";

#[derive(Debug)]
pub struct CanonicalBootstrap {
    pub blockchain: Blockchain,
    pub state: State,
    pub consensus: Consensus,
    #[allow(dead_code)]
    pub genesis_fingerprint: String,
}

#[derive(Debug, Clone)]
struct GenesisValidatorConfiguration {
    address: String,
    stake: u64,
}

#[derive(Debug, Clone)]
struct GenesisAllocation {
    address: String,
    balance: u64,
}

#[derive(Debug, Clone)]
struct GenesisConfiguration {
    network_id: String,
    network_protocol_version: u32,
    timestamp: u64,
    validators: Vec<GenesisValidatorConfiguration>,
    allocations: Vec<GenesisAllocation>,
    genesis_supply: u64,
    max_supply: u64,
    block_reward: u64,
    minimum_fee: u64,
    fee_divisor: u64,
    validator_fee_percent: u64,
    liquidity_fee_percent: u64,
    treasury_fee_percent: u64,
    burn_fee_percent: u64,
    initial_liquidity_reserve: u64,
    initial_treasury_balance: u64,
    initial_burned_amount: u64,
}

pub fn canonical_bootstrap() -> Result<CanonicalBootstrap, String> {
    build_bootstrap(canonical_genesis_configuration())
}

fn canonical_genesis_configuration() -> GenesisConfiguration {
    let economy = Economy::new();

    GenesisConfiguration {
        network_id: NETWORK_ID.to_string(),
        network_protocol_version: NETWORK_PROTOCOL_VERSION,
        timestamp: GENESIS_TIMESTAMP,
        validators: vec![
            GenesisValidatorConfiguration {
                address: GENESIS_VALIDATOR_A_ADDRESS.to_string(),
                stake: GENESIS_VALIDATOR_A_STAKE,
            },
            GenesisValidatorConfiguration {
                address: GENESIS_VALIDATOR_B_ADDRESS.to_string(),
                stake: GENESIS_VALIDATOR_B_STAKE,
            },
        ],
        allocations: vec![
            GenesisAllocation {
                address: GENESIS_VALIDATOR_A_ADDRESS.to_string(),
                balance: GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN,
            },
            GenesisAllocation {
                address: GENESIS_VALIDATOR_B_ADDRESS.to_string(),
                balance: GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN,
            },
        ],
        genesis_supply: GENESIS_SUPPLY_MICRO_KBN,
        max_supply: economy.max_supply,
        block_reward: economy.block_reward,
        minimum_fee: economy.minimum_fee,
        fee_divisor: economy.fee_divisor,
        validator_fee_percent: economy.validator_fee_percent,
        liquidity_fee_percent: economy.liquidity_fee_percent,
        treasury_fee_percent: economy.treasury_fee_percent,
        burn_fee_percent: economy.burn_fee_percent,
        initial_liquidity_reserve: economy.liquidity_reserve,
        initial_treasury_balance: 0,
        initial_burned_amount: 0,
    }
}

fn build_bootstrap(configuration: GenesisConfiguration) -> Result<CanonicalBootstrap, String> {
    if configuration.fee_divisor == 0 {
        return Err("Canonical transaction fee divisor sıfır olamaz".into());
    }

    let configured_fee_percent = configuration
        .validator_fee_percent
        .checked_add(configuration.liquidity_fee_percent)
        .and_then(|total| total.checked_add(configuration.treasury_fee_percent))
        .and_then(|total| total.checked_add(configuration.burn_fee_percent))
        .ok_or("Canonical fee yüzdeleri overflow")?;

    if configured_fee_percent != 100 {
        return Err("Canonical fee yüzdeleri geçersiz".into());
    }

    let allocated_supply =
        configuration
            .allocations
            .iter()
            .try_fold(0u64, |total, allocation| {
                total
                    .checked_add(allocation.balance)
                    .ok_or("Canonical genesis allocation overflow")
            })?;

    if allocated_supply != configuration.genesis_supply {
        return Err("Canonical genesis allocation toplam arza eşit değil".into());
    }

    let mut consensus = Consensus::new();
    for validator in &configuration.validators {
        if !consensus.add_validator(validator.address.clone(), validator.stake) {
            return Err("Canonical genesis validator eklenemedi".into());
        }
    }

    let mut state = State::new();
    for allocation in &configuration.allocations {
        if state.accounts.contains_key(&allocation.address) {
            return Err("Canonical genesis allocation adresi tekrar ediyor".into());
        }

        state.create_account(allocation.address.clone(), allocation.balance);
    }
    state.treasury_balance = configuration.initial_treasury_balance;
    state.burned_amount = configuration.initial_burned_amount;

    let genesis_fingerprint = genesis_configuration_fingerprint(&configuration)?;
    let genesis = genesis_block(&configuration, &genesis_fingerprint);

    let mut blockchain = Blockchain::new(genesis);
    blockchain.economy.max_supply = configuration.max_supply;
    blockchain.economy.block_reward = configuration.block_reward;
    blockchain.economy.minimum_fee = configuration.minimum_fee;
    blockchain.economy.fee_divisor = configuration.fee_divisor;
    blockchain.economy.validator_fee_percent = configuration.validator_fee_percent;
    blockchain.economy.liquidity_fee_percent = configuration.liquidity_fee_percent;
    blockchain.economy.treasury_fee_percent = configuration.treasury_fee_percent;
    blockchain.economy.burn_fee_percent = configuration.burn_fee_percent;
    blockchain.economy.liquidity_reserve = configuration.initial_liquidity_reserve;
    blockchain.economy.mint(configuration.genesis_supply)?;

    Ok(CanonicalBootstrap {
        blockchain,
        state,
        consensus,
        genesis_fingerprint,
    })
}

fn genesis_block(configuration: &GenesisConfiguration, fingerprint: &str) -> Block {
    Block::new(
        0,
        configuration.timestamp,
        GENESIS_PREVIOUS_HASH.to_string(),
        GENESIS_VALIDATOR.to_string(),
        fingerprint.to_string(),
        Vec::new(),
    )
}

fn append_string(buffer: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let length = u64::try_from(value.len()).map_err(|_| "Genesis string uzunluğu overflow")?;
    buffer.extend_from_slice(&length.to_be_bytes());
    buffer.extend_from_slice(value.as_bytes());
    Ok(())
}

fn append_count(buffer: &mut Vec<u8>, count: usize) -> Result<(), String> {
    let count = u64::try_from(count).map_err(|_| "Genesis liste uzunluğu overflow")?;
    buffer.extend_from_slice(&count.to_be_bytes());
    Ok(())
}

fn genesis_configuration_fingerprint(
    configuration: &GenesisConfiguration,
) -> Result<String, String> {
    let mut canonical = Vec::new();

    append_string(&mut canonical, GENESIS_FINGERPRINT_DOMAIN)?;
    append_string(&mut canonical, &configuration.network_id)?;
    canonical.extend_from_slice(&configuration.network_protocol_version.to_be_bytes());
    canonical.extend_from_slice(&configuration.timestamp.to_be_bytes());

    append_count(&mut canonical, configuration.validators.len())?;
    for validator in &configuration.validators {
        append_string(&mut canonical, &validator.address)?;
        canonical.extend_from_slice(&validator.stake.to_be_bytes());
    }

    let mut canonical_allocations = configuration.allocations.iter().collect::<Vec<_>>();
    canonical_allocations.sort_by(|left, right| left.address.cmp(&right.address));
    append_count(&mut canonical, canonical_allocations.len())?;
    for allocation in canonical_allocations {
        append_string(&mut canonical, &allocation.address)?;
        canonical.extend_from_slice(&allocation.balance.to_be_bytes());
    }

    for value in [
        configuration.genesis_supply,
        configuration.max_supply,
        configuration.block_reward,
        configuration.minimum_fee,
        configuration.fee_divisor,
        configuration.validator_fee_percent,
        configuration.liquidity_fee_percent,
        configuration.treasury_fee_percent,
        configuration.burn_fee_percent,
        configuration.initial_liquidity_reserve,
        configuration.initial_treasury_balance,
        configuration.initial_burned_amount,
    ] {
        canonical.extend_from_slice(&value.to_be_bytes());
    }

    let mut hasher = Sha256::new();
    hasher.update(canonical);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn genesis_identity_test_vectors()
-> Result<((String, String), Vec<(&'static str, String, String)>), String> {
    fn identity(configuration: &GenesisConfiguration) -> Result<(String, String), String> {
        let fingerprint = genesis_configuration_fingerprint(configuration)?;
        let hash = genesis_block(configuration, &fingerprint).hash;
        Ok((fingerprint, hash))
    }

    let baseline = canonical_genesis_configuration();
    let baseline_identity = identity(&baseline)?;
    let mut configurations = Vec::new();

    let mut changed = baseline.clone();
    changed.network_id.push_str("-changed");
    configurations.push(("network_id", changed));

    let mut changed = baseline.clone();
    changed.network_protocol_version += 1;
    configurations.push(("network_protocol_version", changed));

    let mut changed = baseline.clone();
    changed.timestamp += 1;
    configurations.push(("genesis_timestamp", changed));

    let mut changed = baseline.clone();
    changed.validators[0].address = "0".repeat(64);
    configurations.push(("validator_address", changed));

    let mut changed = baseline.clone();
    changed.validators[0].stake += 1;
    configurations.push(("validator_stake", changed));

    let mut changed = baseline.clone();
    changed.allocations[0].balance -= 1;
    changed.allocations[1].balance += 1;
    configurations.push(("genesis_allocation", changed));

    let mut changed = baseline.clone();
    changed.genesis_supply += 1;
    configurations.push(("genesis_supply", changed));

    let mut changed = baseline.clone();
    changed.max_supply += 1;
    configurations.push(("max_supply", changed));

    let mut changed = baseline.clone();
    changed.block_reward += 1;
    configurations.push(("block_reward", changed));

    let mut changed = baseline.clone();
    changed.minimum_fee += 1;
    configurations.push(("minimum_fee", changed));

    let mut changed = baseline.clone();
    changed.fee_divisor += 1;
    configurations.push(("fee_divisor", changed));

    let mut changed = baseline.clone();
    changed.validator_fee_percent += 1;
    configurations.push(("validator_percent", changed));

    let mut changed = baseline.clone();
    changed.liquidity_fee_percent += 1;
    configurations.push(("liquidity_reserve_percent", changed));

    let mut changed = baseline.clone();
    changed.treasury_fee_percent += 1;
    configurations.push(("treasury_percent", changed));

    let mut changed = baseline.clone();
    changed.burn_fee_percent += 1;
    configurations.push(("burn_percent", changed));

    let mut changed = baseline.clone();
    changed.initial_liquidity_reserve += 1;
    configurations.push(("initial_liquidity_reserve", changed));

    let mut changed = baseline.clone();
    changed.initial_treasury_balance += 1;
    configurations.push(("initial_treasury_balance", changed));

    let mut changed = baseline.clone();
    changed.initial_burned_amount += 1;
    configurations.push(("initial_burned_amount", changed));

    let variants = configurations
        .into_iter()
        .map(|(field, configuration)| {
            let (fingerprint, hash) = identity(&configuration)?;
            Ok((field, fingerprint, hash))
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok((baseline_identity, variants))
}
