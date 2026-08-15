use kybernetes::economy::Economy;
use proptest::prelude::*;

proptest! {
    #[test]
    fn calculated_fee_matches_configured_rule(amount in any::<u64>()) {
        let economy = Economy::new();
        let expected = (amount / economy.fee_divisor.max(1)).max(economy.minimum_fee);

        prop_assert_eq!(economy.calculate_fee(amount), expected);
        prop_assert!(economy.validate_fee(amount, expected));
    }

    #[test]
    fn calculated_fee_is_monotonic(first in any::<u64>(), second in any::<u64>()) {
        let economy = Economy::new();
        let lower = first.min(second);
        let higher = first.max(second);

        prop_assert!(economy.calculate_fee(lower) <= economy.calculate_fee(higher));
    }

    #[test]
    fn fee_distribution_preserves_every_unit(fee in any::<u64>()) {
        let economy = Economy::new();
        let (validator, liquidity, treasury, burn) = economy.distribute_fee(fee);

        let distributed = u128::from(validator)
            + u128::from(liquidity)
            + u128::from(treasury)
            + u128::from(burn);

        prop_assert_eq!(distributed, u128::from(fee));
        prop_assert_eq!(burn, 0);
    }

    #[test]
    fn fee_distribution_matches_configured_shares(fee in any::<u64>()) {
        let economy = Economy::new();
        let (validator, liquidity, treasury, burn) = economy.distribute_fee(fee);

        let expected_liquidity =
            u128::from(fee) * u128::from(economy.liquidity_fee_percent) / 100;
        let expected_treasury =
            u128::from(fee) * u128::from(economy.treasury_fee_percent) / 100;

        prop_assert_eq!(u128::from(liquidity), expected_liquidity);
        prop_assert_eq!(u128::from(treasury), expected_treasury);
        prop_assert_eq!(u128::from(validator) + expected_liquidity + expected_treasury, u128::from(fee));
        prop_assert_eq!(burn, 0);
    }
}
