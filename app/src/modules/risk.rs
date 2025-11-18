use crate::{PerpetualDEXState, errors::Error, types::*};

#[derive(Clone, Debug, Default)]
pub struct SettledFees {
    pub funding_fee: i128,   // signed USD
    pub borrowing_fee: u128, // USD
    pub total_fee_usd: i128, // net
}

pub struct RiskModule;

impl RiskModule {
    /// Updates pool-level funding accumulators only
    ///
    /// Borrowing fees are calculated and collected per-position in settle_position_fees
    pub fn accrue_pool(market: &str, current_time: u64) -> Result<(), Error> {
        let mut st = PerpetualDEXState::get_mut();
        let cfg = st.market_configs.get(market).ok_or(Error::MarketNotFound)?.clone();
        let pool = st.pool_amounts.get_mut(market).ok_or(Error::MarketNotFound)?;

        let dt = current_time.saturating_sub(pool.last_funding_update);
        if dt == 0 {
            return Ok(());
        }

        // Calculate funding rate in microUSD/USD
        let funding_rate_micro = Self::funding_rate_micro(pool, &cfg, dt)?;

        pool.accumulated_funding_long_per_usd =
            pool.accumulated_funding_long_per_usd.saturating_add(funding_rate_micro);
        pool.accumulated_funding_short_per_usd = pool
            .accumulated_funding_short_per_usd
            .saturating_sub(funding_rate_micro);

        pool.last_funding_update = current_time;
        Ok(())
    }

    /// Settles fees for a position and updates pool balances
    ///
    /// Architecture (single source of truth):
    /// - Funding fees: zero-sum between long/short via claimable_fee_*
    ///   - Calculated from accumulated_funding_*_per_usd indices (updated in accrue_pool)
    ///   - Longs pay → claimable_fee_usd_short++ (shorts can claim)
    ///   - Shorts pay → claimable_fee_usd_long++ (longs can claim)
    ///
    /// - Borrowing fees: trader → LP claimable_fee_*
    ///   - Calculated per-position based on utilization
    ///   - Added to claimable_fee_* HERE (not in accrue_pool)
    ///   - This ensures sum(position_fees) = LP_claimable (no double counting)
    pub fn settle_position_fees(pos: &mut Position, market: &str, current_time: u64) -> Result<SettledFees, Error> {
        let mut st = PerpetualDEXState::get_mut();
        let cfg = st.market_configs.get(market).ok_or(Error::MarketNotFound)?.clone();
        let pool = st.pool_amounts.get_mut(market).ok_or(Error::MarketNotFound)?;

        let mut fees = SettledFees::default();

        // 1. FUNDING FEE (zero-sum between long/short)
        let current_funding = if pos.is_long {
            pool.accumulated_funding_long_per_usd
        } else {
            pool.accumulated_funding_short_per_usd
        };

        // funding_delta is in microUSD/USD, multiply by size and divide by USD_SCALE
        let funding_delta_micro = current_funding - pos.funding_fee_per_usd;
        fees.funding_fee = (pos.size_usd as i128).saturating_mul(funding_delta_micro) / (USD_SCALE as i128);

        pos.funding_fee_per_usd = current_funding;

        // Update claimable for opposite side (maintains zero-sum)
        if fees.funding_fee > 0 {
            // Position PAYS funding → opposite side can claim
            let payment = fees.funding_fee as u128;
            if pos.is_long {
                pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_add(payment);
            } else {
                pool.claimable_fee_usd_long = pool.claimable_fee_usd_long.saturating_add(payment);
            }
        } else if fees.funding_fee < 0 {
            // Position RECEIVES funding → deduct from our side's claimable
            let credit = (-fees.funding_fee) as u128;
            if pos.is_long {
                if pool.claimable_fee_usd_long < credit {
                    // Insufficient funding pool - should not happen in normal operation
                    // In bootstrap/extreme scenarios, we simply limit credit to available
                    let available = pool.claimable_fee_usd_long;
                    pool.claimable_fee_usd_long = 0;
                    pos.collateral_usd = pos.collateral_usd.saturating_add(available);

                    // Update fees to reflect what was actually paid
                    fees.funding_fee = -(available as i128);
                    fees.total_fee_usd = fees.funding_fee.saturating_add(fees.borrowing_fee as i128);

                    // Note: remaining funding credit is lost (acceptable in edge cases)
                    return Ok(fees);
                }
                pool.claimable_fee_usd_long = pool.claimable_fee_usd_long.saturating_sub(credit);
            } else {
                if pool.claimable_fee_usd_short < credit {
                    let available = pool.claimable_fee_usd_short;
                    pool.claimable_fee_usd_short = 0;
                    pos.collateral_usd = pos.collateral_usd.saturating_add(available);

                    fees.funding_fee = -(available as i128);
                    fees.total_fee_usd = fees.funding_fee.saturating_add(fees.borrowing_fee as i128);

                    return Ok(fees);
                }
                pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_sub(credit);
            }
        }

        // 2. BORROWING FEE (trader pays → goes to LP claimable)
        let dt = current_time.saturating_sub(pos.last_fee_update);
        if dt > 0 && pos.size_usd > 0 {
            fees.borrowing_fee = Self::position_borrowing_fee(pos, pool, &cfg, dt)?;

            // Add borrowing fee to LP claimable for this side
            // This is the ONLY place where borrowing fees are calculated and added
            if pos.is_long {
                pool.claimable_fee_usd_long = pool.claimable_fee_usd_long.saturating_add(fees.borrowing_fee);
            } else {
                pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_add(fees.borrowing_fee);
            }

            // Track total for statistics
            pool.total_borrowing_fees_usd = pool.total_borrowing_fees_usd.saturating_add(fees.borrowing_fee);
        }
        pos.last_fee_update = current_time;

        fees.total_fee_usd = fees.funding_fee.saturating_add(fees.borrowing_fee as i128);

        // 3. APPLY NET FEE TO POSITION COLLATERAL
        if fees.total_fee_usd > 0 {
            let fee = fees.total_fee_usd as u128;
            if fee > pos.collateral_usd {
                pos.collateral_usd = 0;
                return Err(Error::InsufficientCollateral);
            }
            pos.collateral_usd = pos.collateral_usd.saturating_sub(fee);
        } else if fees.total_fee_usd < 0 {
            let credit = (-fees.total_fee_usd) as u128;
            pos.collateral_usd = pos.collateral_usd.saturating_add(credit);
        }

        Ok(fees)
    }

    /// Calculates funding rate in microUSD per USD of position size
    ///
    /// Unit: microUSD/USD (as specified in PoolAmounts comment)
    /// Example: 500 microUSD/USD = 0.05% = 5 bps per period
    fn funding_rate_micro(pool: &PoolAmounts, cfg: &MarketConfig, dt: u64) -> Result<i128, Error> {
        let total_oi = pool.long_oi_usd.saturating_add(pool.short_oi_usd);
        if total_oi == 0 {
            return Ok(0);
        }

        // Calculate imbalance in basis points
        let imbalance = (pool.long_oi_usd as i128) - (pool.short_oi_usd as i128);
        let ratio_bps = (imbalance.saturating_mul(10_000)) / (total_oi as i128);

        // Apply non-linear exponent
        let mut base = ratio_bps.unsigned_abs() as i128;
        let exp = cfg.funding_exponent.max(1);
        for _ in 1..exp {
            base = base.saturating_mul(ratio_bps.unsigned_abs() as i128) / 10_000;
        }

        // Apply factor (in bps)
        let rate_bps = (base.saturating_mul(cfg.funding_factor as i128)) / 10_000;

        // Set sign: positive = longs pay, negative = shorts pay
        let rate_bps = if imbalance > 0 {
            rate_bps
        } else if imbalance < 0 {
            -rate_bps
        } else {
            0
        };

        // Annualize and apply time delta
        let seconds_per_year = 365 * 24 * 60 * 60u128;
        let rate_annual_bps = rate_bps.saturating_mul(dt as i128) / (seconds_per_year as i128);

        // Cap at ±10 bps per hour (proportional for any dt)
        let max_per_hour = 10i128;
        let cap_bps = max_per_hour.saturating_mul(dt as i128) / 3600;
        let rate_capped_bps = rate_annual_bps.max(-cap_bps).min(cap_bps);

        // Convert bps to microUSD/USD: 1 bps = 100 microUSD/USD
        // Example: 5 bps = 500 microUSD/USD = 0.05%
        let rate_micro = rate_capped_bps.saturating_mul(100);

        Ok(rate_micro)
    }

    fn position_borrowing_fee(pos: &Position, pool: &PoolAmounts, cfg: &MarketConfig, dt: u64) -> Result<u128, Error> {
        let liquidity = if pos.is_long {
            pool.long_liquidity_usd
        } else {
            pool.short_liquidity_usd
        };
        if liquidity == 0 {
            return Ok(0);
        }

        // Calculate utilization in bps
        let util_bps = pos.size_usd.saturating_mul(10_000) / liquidity;

        // Apply non-linear exponent to utilization
        let exponent = cfg.borrowing_exponent.max(1);
        let mut util_exp = util_bps;
        for _ in 1..exponent {
            util_exp = util_exp.saturating_mul(util_bps) / 10_000;
        }

        // Calculate APR rate in bps (capped at 100%)
        let rate_bps = cfg
            .borrowing_factor
            .saturating_mul(util_exp)
            .saturating_div(10_000)
            .min(10_000);

        // Apply time factor: fee = size * rate * dt / year
        let seconds_per_year = 365 * 24 * 60 * 60u128;
        Ok(rate_bps
            .saturating_mul(pos.size_usd)
            .saturating_mul(dt as u128)
            .saturating_div(seconds_per_year * 10_000))
    }

    /// NOTE: This check does NOT include unsettled funding/borrowing fees.
    /// Liquidators MUST apply virtual settlement before calling this.
    /// Recommended: use effective_collateral_after_virtual_settle().
    pub fn is_liquidatable(pos: &Position, current_price_usd: u128, liq_bps: u16) -> bool {
        if pos.size_usd == 0 || pos.entry_price_usd == 0 {
            return false;
        }

        let tokens_usdx = pos.size_usd.saturating_mul(USD_SCALE) / pos.entry_price_usd;
        if tokens_usdx == 0 {
            return false;
        }

        let price_delta = if pos.is_long {
            current_price_usd as i128 - pos.entry_price_usd as i128
        } else {
            pos.entry_price_usd as i128 - current_price_usd as i128
        };
        let pnl = (price_delta.saturating_mul(tokens_usdx as i128)) / (USD_SCALE as i128);

        let current_value = (pos.collateral_usd as i128).saturating_add(pnl);
        let threshold = (pos.collateral_usd as i128).saturating_mul(liq_bps as i128) / 10_000;

        current_value <= threshold
    }
}
