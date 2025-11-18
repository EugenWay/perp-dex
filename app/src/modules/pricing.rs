use crate::{PerpetualDEXState, errors::Error, modules::oracle::OracleModule, types::*, utils};

#[derive(Clone, Debug)]
pub struct QuoteResult {
    pub execution_price: u128,
    pub price_impact_usd: i128, // Positive = better for trader, negative = worse
}

pub struct PricingModule;

impl PricingModule {
    pub fn quote_increase(market: &str, side: &OrderSide, size_usd: u128) -> Result<QuoteResult, Error> {
        Self::quote(market, side, size_usd, true)
    }

    pub fn quote_decrease(market: &str, side: &OrderSide, size_usd: u128) -> Result<QuoteResult, Error> {
        Self::quote(market, side, size_usd, false)
    }

    fn quote(market: &str, side: &OrderSide, size_usd: u128, is_increase: bool) -> Result<QuoteResult, Error> {
        let st = PerpetualDEXState::get();
        let cfg = st.market_configs.get(market).ok_or(Error::MarketNotFound)?;
        let pool = st.pool_amounts.get(market).ok_or(Error::MarketNotFound)?;

        let price_key = utils::price_key(market);
        let mid = OracleModule::mid(&price_key)?;
        let spread = OracleModule::spread(&price_key)?;
        let ask = mid.saturating_add(spread / 2);
        let bid = mid.saturating_sub(spread / 2);

        let price_impact_usd = Self::calculate_price_impact_usd(pool, cfg, side, size_usd, is_increase)?;

        // Convert impact to basis points for price adjustment
        let price_impact_bps = if size_usd > 0 {
            (price_impact_usd * 10_000) / (size_usd as i128)
        } else {
            0
        };

        // Base price: trader always pays worst price (taker)
        let base_price = match (side, is_increase) {
            (OrderSide::Long, true) => ask,
            (OrderSide::Long, false) => bid,
            (OrderSide::Short, true) => bid,
            (OrderSide::Short, false) => ask,
        };

        // Apply impact: positive improves price, negative worsens it
        let impact_abs = base_price.saturating_mul(price_impact_bps.unsigned_abs() as u128) / 10_000;

        let execution_price_unclamped = if price_impact_bps >= 0 {
            match (side, is_increase) {
                (OrderSide::Long, true) => base_price.saturating_sub(impact_abs),
                (OrderSide::Long, false) => base_price.saturating_add(impact_abs),
                (OrderSide::Short, true) => base_price.saturating_add(impact_abs),
                (OrderSide::Short, false) => base_price.saturating_sub(impact_abs),
            }
        } else {
            match (side, is_increase) {
                (OrderSide::Long, true) => base_price.saturating_add(impact_abs),
                (OrderSide::Long, false) => base_price.saturating_sub(impact_abs),
                (OrderSide::Short, true) => base_price.saturating_sub(impact_abs),
                (OrderSide::Short, false) => base_price.saturating_add(impact_abs),
            }
        };

        // Clamp to ±10% from mid
        let max_deviation = mid / 10;
        let execution_price = execution_price_unclamped
            .max(mid.saturating_sub(max_deviation))
            .min(mid.saturating_add(max_deviation));

        Ok(QuoteResult {
            execution_price,
            price_impact_usd,
        })
    }

    /// Calculates price impact in USD based on how the trade affects market balance.
    ///
    /// Formula: impact = (d_after^exp - d_before^exp) × factor × size / 10000
    /// where d is normalized imbalance in basis points: d = |longOI - shortOI| / totalOI × 10000
    ///
    /// Normalization makes impact scale-invariant: 10% imbalance always = 1000 bps,
    /// regardless of whether market is $100k or $100M.
    fn calculate_price_impact_usd(
        pool: &PoolAmounts,
        cfg: &MarketConfig,
        side: &OrderSide,
        size_usd: u128,
        is_increase: bool,
    ) -> Result<i128, Error> {
        let long_oi = pool.long_oi_usd as i128;
        let short_oi = pool.short_oi_usd as i128;

        // First trade on empty market has zero impact
        if long_oi == 0 && short_oi == 0 {
            return Ok(0);
        }

        // Calculate normalized imbalance before trade (in bps)
        let total_oi_before = long_oi + short_oi;
        if total_oi_before <= 0 {
            return Ok(0);
        }

        let d_before_abs = (long_oi - short_oi).abs() as u128;
        let d_before_bps = (d_before_abs * 10_000) / (total_oi_before as u128);

        // Simulate OI change
        let delta = size_usd as i128;

        let (new_long_oi, new_short_oi) = match (side, is_increase) {
            (OrderSide::Long, true) => (long_oi + delta, short_oi),
            (OrderSide::Long, false) => {
                if delta > long_oi {
                    return Err(Error::InsufficientOpenInterest);
                }
                (long_oi - delta, short_oi)
            }
            (OrderSide::Short, true) => (long_oi, short_oi + delta),
            (OrderSide::Short, false) => {
                if delta > short_oi {
                    return Err(Error::InsufficientOpenInterest);
                }
                (long_oi, short_oi - delta)
            }
        };

        let total_oi_after = new_long_oi + new_short_oi;
        if total_oi_after <= 0 {
            return Ok(0);
        }

        // Calculate normalized imbalance after trade (in bps)
        let d_after_abs = (new_long_oi - new_short_oi).abs() as u128;
        let d_after_bps = (d_after_abs * 10_000) / (total_oi_after as u128);

        // Choose factor: reward if improving balance, penalty if worsening
        let helps_balance = d_after_bps < d_before_bps;
        let impact_factor = if helps_balance {
            cfg.pi_factor_positive
        } else {
            cfg.pi_factor_negative
        };

        // Apply non-linear formula
        let exp = cfg.pi_exponent.max(1).min(8);
        let d_before_powered = Self::safe_power(d_before_bps, exp as u64)?;
        let d_after_powered = Self::safe_power(d_after_bps, exp as u64)?;

        // Overflow protection for u128 → i128
        if d_before_powered > i128::MAX as u128 || d_after_powered > i128::MAX as u128 {
            return Err(Error::MathOverflow);
        }

        let diff = d_after_powered as i128 - d_before_powered as i128;

        // Calculate relative impact with overflow protection
        let impact_relative = diff
            .checked_mul(impact_factor as i128)
            .ok_or(Error::MathOverflow)?
            .checked_div(10_000)
            .ok_or(Error::MathOverflow)?;

        // Convert to USD and invert sign for trader-centric semantics
        let price_impact_usd_raw = (impact_relative * size_usd as i128) / 10_000;
        let price_impact_usd = -price_impact_usd_raw;

        // Cap at ±10% of trade size
        let max_impact = (size_usd as i128) / 10;
        Ok(price_impact_usd.max(-max_impact).min(max_impact))
    }

    fn safe_power(base: u128, exp: u64) -> Result<u128, Error> {
        if exp == 0 {
            return Ok(1);
        }
        if exp == 1 {
            return Ok(base);
        }

        let mut result = base;
        for _ in 1..exp {
            result = result.checked_mul(base).ok_or(Error::MathOverflow)?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_trade_zero_impact() {
        let pool = PoolAmounts {
            long_oi_usd: 0,
            short_oi_usd: 0,
            ..Default::default()
        };

        let cfg = MarketConfig {
            pi_factor_positive: 100,
            pi_factor_negative: 200,
            pi_exponent: 2,
            ..Default::default()
        };

        let impact = PricingModule::calculate_price_impact_usd(&pool, &cfg, &OrderSide::Long, 10_000, true).unwrap();

        assert_eq!(impact, 0);
    }

    #[test]
    fn test_scale_invariance() {
        let cfg = MarketConfig {
            pi_factor_positive: 100,
            pi_factor_negative: 200,
            pi_exponent: 2,
            ..Default::default()
        };

        // Small market: 100k total, 20% imbalance
        let pool_small = PoolAmounts {
            long_oi_usd: 60_000,
            short_oi_usd: 40_000,
            ..Default::default()
        };

        // Large market: 100M total, 20% imbalance
        let pool_large = PoolAmounts {
            long_oi_usd: 60_000_000,
            short_oi_usd: 40_000_000,
            ..Default::default()
        };

        let impact_small =
            PricingModule::calculate_price_impact_usd(&pool_small, &cfg, &OrderSide::Long, 5_000, true).unwrap();

        let impact_large =
            PricingModule::calculate_price_impact_usd(&pool_large, &cfg, &OrderSide::Long, 5_000_000, true).unwrap();

        // Should scale proportionally
        let ratio = (impact_large as f64) / (impact_small as f64);
        let expected_ratio = 1000.0;

        assert!((ratio - expected_ratio).abs() / expected_ratio < 0.1);
    }

    #[test]
    fn test_helps_balance_positive_impact() {
        let pool = PoolAmounts {
            long_oi_usd: 100_000,
            short_oi_usd: 50_000,
            ..Default::default()
        };

        let cfg = MarketConfig {
            pi_factor_positive: 100,
            pi_factor_negative: 200,
            pi_exponent: 2,
            ..Default::default()
        };

        let impact = PricingModule::calculate_price_impact_usd(&pool, &cfg, &OrderSide::Short, 10_000, true).unwrap();

        assert!(impact > 0);
    }

    #[test]
    fn test_hurts_balance_negative_impact() {
        let pool = PoolAmounts {
            long_oi_usd: 100_000,
            short_oi_usd: 50_000,
            ..Default::default()
        };

        let cfg = MarketConfig {
            pi_factor_positive: 100,
            pi_factor_negative: 200,
            pi_exponent: 2,
            ..Default::default()
        };

        let impact = PricingModule::calculate_price_impact_usd(&pool, &cfg, &OrderSide::Long, 10_000, true).unwrap();

        assert!(impact < 0);
    }

    #[test]
    fn test_no_overflow_large_market() {
        let pool = PoolAmounts {
            long_oi_usd: 500_000_000_000,
            short_oi_usd: 300_000_000_000,
            ..Default::default()
        };

        let cfg = MarketConfig {
            pi_factor_positive: 100,
            pi_factor_negative: 200,
            pi_exponent: 5,
            ..Default::default()
        };

        let result = PricingModule::calculate_price_impact_usd(&pool, &cfg, &OrderSide::Long, 1_000_000_000, true);

        assert!(result.is_ok());
    }

    #[test]
    fn test_impact_capped() {
        let pool = PoolAmounts {
            long_oi_usd: 1_000_000,
            short_oi_usd: 10_000,
            ..Default::default()
        };

        let cfg = MarketConfig {
            pi_factor_positive: 10_000,
            pi_factor_negative: 10_000,
            pi_exponent: 3,
            ..Default::default()
        };

        let size = 50_000u128;
        let impact = PricingModule::calculate_price_impact_usd(&pool, &cfg, &OrderSide::Long, size, true).unwrap();

        assert_eq!(impact, -(size as i128) / 10);
    }

    #[test]
    fn test_insufficient_oi() {
        let pool = PoolAmounts {
            long_oi_usd: 50_000,
            short_oi_usd: 30_000,
            ..Default::default()
        };

        let result = PricingModule::calculate_price_impact_usd(
            &pool,
            &MarketConfig::default(),
            &OrderSide::Long,
            100_000,
            false,
        );

        assert!(matches!(result, Err(Error::InsufficientOpenInterest)));
    }
}
