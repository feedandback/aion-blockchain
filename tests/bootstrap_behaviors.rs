use kybernetes::bootstrap::{CanonicalBootstrap, canonical_bootstrap};
use kybernetes::economy::Economy;
use kybernetes::protocol::{
    GENESIS_PREVIOUS_HASH, GENESIS_SUPPLY_MICRO_KBN, GENESIS_TIMESTAMP, GENESIS_VALIDATOR,
    GENESIS_VALIDATOR_A_ADDRESS, GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN,
    GENESIS_VALIDATOR_A_STAKE, GENESIS_VALIDATOR_B_ADDRESS,
    GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN, GENESIS_VALIDATOR_B_STAKE,
};
use kybernetes::wallet::Wallet;

const PRIVATE_KEY_ONE: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const PRIVATE_KEY_TWO: &str = "0202020202020202020202020202020202020202020202020202020202020202";

fn bootstrap() -> CanonicalBootstrap {
    canonical_bootstrap().expect("canonical bootstrap must be valid")
}

fn account_snapshot(bootstrap: &CanonicalBootstrap) -> Vec<(String, u64, u64)> {
    let mut accounts = bootstrap
        .state
        .accounts
        .iter()
        .map(|(address, account)| (address.clone(), account.balance, account.nonce))
        .collect::<Vec<_>>();
    accounts.sort();
    accounts
}

fn validator_snapshot(bootstrap: &CanonicalBootstrap) -> Vec<(String, u64)> {
    bootstrap
        .consensus
        .validators
        .iter()
        .map(|validator| (validator.address.clone(), validator.stake))
        .collect()
}

fn economy_snapshot(
    bootstrap: &CanonicalBootstrap,
) -> (u64, u64, u64, u64, u64, u64, u64, u64, u64) {
    let economy = &bootstrap.blockchain.economy;
    (
        economy.total_supply,
        economy.max_supply,
        economy.block_reward,
        economy.minimum_fee,
        economy.liquidity_reserve,
        economy.validator_fee_percent,
        economy.liquidity_fee_percent,
        economy.treasury_fee_percent,
        economy.burn_fee_percent,
    )
}

#[test]
fn canonical_bootstraps_have_the_same_genesis_hash() {
    let first = bootstrap();
    let second = bootstrap();
    let first_genesis = first
        .blockchain
        .chain
        .first()
        .expect("canonical chain must contain genesis");
    let second_genesis = second
        .blockchain
        .chain
        .first()
        .expect("canonical chain must contain genesis");

    assert_eq!(first.blockchain.chain.len(), 1);
    assert_eq!(second.blockchain.chain.len(), 1);
    assert_eq!(first_genesis.hash, first_genesis.calculate_hash());
    assert_eq!(second_genesis.hash, second_genesis.calculate_hash());
    assert_eq!(first_genesis.hash, second_genesis.hash);
    assert_eq!(first_genesis.index, 0);
    assert_eq!(first_genesis.timestamp, GENESIS_TIMESTAMP);
    assert_eq!(first_genesis.previous_hash, GENESIS_PREVIOUS_HASH);
    assert_eq!(first_genesis.validator, GENESIS_VALIDATOR);
    assert!(first_genesis.validator_public_key.is_empty());
    assert!(first_genesis.validator_signature.is_none());
    assert!(first_genesis.transactions.is_empty());
    assert!(first.blockchain.is_valid());
    assert!(second.blockchain.is_valid());
}

#[test]
fn canonical_bootstraps_have_the_same_initial_balances() {
    let first = bootstrap();
    let second = bootstrap();

    assert_eq!(account_snapshot(&first), account_snapshot(&second));
    assert_eq!(first.state.accounts.len(), 2);
    assert_eq!(
        first.state.balance_of(GENESIS_VALIDATOR_A_ADDRESS),
        GENESIS_VALIDATOR_A_ALLOCATION_MICRO_KBN
    );
    assert_eq!(
        first.state.balance_of(GENESIS_VALIDATOR_B_ADDRESS),
        GENESIS_VALIDATOR_B_ALLOCATION_MICRO_KBN
    );
    assert_eq!(first.state.nonce_of(GENESIS_VALIDATOR_A_ADDRESS), 0);
    assert_eq!(first.state.nonce_of(GENESIS_VALIDATOR_B_ADDRESS), 0);
    assert_eq!(first.state.treasury(), 0);
    assert_eq!(first.state.burned(), 0);
}

#[test]
fn canonical_bootstraps_have_the_same_validator_addresses() {
    let first = bootstrap();
    let second = bootstrap();
    let first_addresses = first
        .consensus
        .validators
        .iter()
        .map(|validator| validator.address.as_str())
        .collect::<Vec<_>>();

    assert_eq!(validator_snapshot(&first), validator_snapshot(&second));
    assert_eq!(
        first_addresses,
        vec![GENESIS_VALIDATOR_A_ADDRESS, GENESIS_VALIDATOR_B_ADDRESS]
    );
}

#[test]
fn canonical_bootstraps_have_the_same_validator_stakes() {
    let first = bootstrap();
    let second = bootstrap();
    let first_stakes = first
        .consensus
        .validators
        .iter()
        .map(|validator| validator.stake)
        .collect::<Vec<_>>();

    assert_eq!(validator_snapshot(&first), validator_snapshot(&second));
    assert_eq!(
        first_stakes,
        vec![GENESIS_VALIDATOR_A_STAKE, GENESIS_VALIDATOR_B_STAKE]
    );
    assert_eq!(
        first.consensus.total_stake(),
        GENESIS_VALIDATOR_A_STAKE + GENESIS_VALIDATOR_B_STAKE
    );
}

#[test]
fn canonical_bootstraps_have_the_same_initial_economy() {
    let first = bootstrap();
    let second = bootstrap();
    let baseline = Economy::new();
    let allocated_supply = first
        .state
        .accounts
        .values()
        .map(|account| account.balance)
        .sum::<u64>();

    assert_eq!(economy_snapshot(&first), economy_snapshot(&second));
    assert_eq!(
        first.blockchain.economy.total_supply,
        GENESIS_SUPPLY_MICRO_KBN
    );
    assert_eq!(first.blockchain.economy.liquidity_reserve, 0);
    assert_eq!(first.blockchain.economy.max_supply, baseline.max_supply);
    assert_eq!(first.blockchain.economy.block_reward, baseline.block_reward);
    assert_eq!(first.blockchain.economy.minimum_fee, baseline.minimum_fee);
    assert_eq!(
        first.blockchain.economy.validator_fee_percent,
        baseline.validator_fee_percent
    );
    assert_eq!(
        first.blockchain.economy.liquidity_fee_percent,
        baseline.liquidity_fee_percent
    );
    assert_eq!(
        first.blockchain.economy.treasury_fee_percent,
        baseline.treasury_fee_percent
    );
    assert_eq!(
        first.blockchain.economy.burn_fee_percent,
        baseline.burn_fee_percent
    );
    assert_eq!(allocated_supply, GENESIS_SUPPLY_MICRO_KBN);
}

#[test]
fn different_local_wallets_do_not_change_canonical_consensus() {
    let first_wallet =
        Wallet::from_private_key_hex(PRIVATE_KEY_ONE).expect("first private key must be valid");
    let second_wallet =
        Wallet::from_private_key_hex(PRIVATE_KEY_TWO).expect("second private key must be valid");
    let first = bootstrap();
    let second = bootstrap();

    assert_ne!(first_wallet.address(), second_wallet.address());
    for local_address in [first_wallet.address(), second_wallet.address()] {
        assert!(!first.consensus.is_validator_allowed(local_address));
        assert!(!first.state.accounts.contains_key(local_address));
    }

    assert_eq!(
        first.blockchain.chain[0].hash,
        second.blockchain.chain[0].hash
    );
    assert_eq!(account_snapshot(&first), account_snapshot(&second));
    assert_eq!(validator_snapshot(&first), validator_snapshot(&second));
    assert_eq!(economy_snapshot(&first), economy_snapshot(&second));
}
