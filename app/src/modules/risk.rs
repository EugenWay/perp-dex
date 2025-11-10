use crate::{
    types::*,
    errors::Error,
    PerpetualDEXState,
};

#[derive(Clone, Debug, Default)]
pub struct SettledFees {
    pub funding_fee: i128,          // signed USD
    pub borrowing_fee: u128,        // USD
    pub total_fee_usd: i128,        // net
}

pub struct RiskModule;

impl RiskModule {
    pub fn accrue_pool(market: &str, current_time: u64) -> Result<(), Error> {
        let mut st = PerpetualDEXState::get_mut();
        let cfg = st.market_configs.get(market).ok_or(Error::MarketNotFound)?.clone();
        let pool = st.pool_amounts.get_mut(market).ok_or(Error::MarketNotFound)?;

        let dt = current_time.saturating_sub(pool.last_funding_update);
        if dt == 0 { return Ok(()); }

        // funding
        let funding_rate = Self::funding_rate(pool, &cfg, dt)?;
        pool.accumulated_funding_long_per_usd = pool.accumulated_funding_long_per_usd.saturating_add(funding_rate);
        pool.accumulated_funding_short_per_usd = pool.accumulated_funding_short_per_usd.saturating_sub(funding_rate);

        // borrowing
        let borrowing_fees = Self::pool_borrowing_fees(pool, &cfg, dt)?;
        pool.total_borrowing_fees_usd = pool.total_borrowing_fees_usd.saturating_add(borrowing_fees);

        pool.last_funding_update = current_time;
        Ok(())
    }

    pub fn settle_position_fees(
        pos: &mut Position,
        market: &str,
        current_time: u64,
    ) -> Result<SettledFees, Error> {
        let st = PerpetualDEXState::get();
        let pool = st.pool_amounts.get(market).ok_or(Error::MarketNotFound)?;
        let cfg = st.market_configs.get(market).ok_or(Error::MarketNotFound)?;

        let mut fees = SettledFees::default();

        // funding: diff of checkpoints * size_usd / USD_SCALE -> USD
        let current_funding = if pos.is_long { pool.accumulated_funding_long_per_usd } else { pool.accumulated_funding_short_per_usd };
        let funding_delta = current_funding - pos.funding_fee_per_usd;
        fees.funding_fee = (pos.size_usd as i128).saturating_mul(funding_delta) / (USD_SCALE as i128);
        pos.funding_fee_per_usd = current_funding;

        // borrowing: APR-based applied to size_usd
        let dt = current_time.saturating_sub(pos.last_fee_update);
        if dt > 0 && pos.size_usd > 0 {
            fees.borrowing_fee = Self::position_borrowing_fee(pos, pool, cfg, dt)?;
        }
        pos.last_fee_update = current_time;

        fees.total_fee_usd = fees.funding_fee.saturating_add(fees.borrowing_fee as i128);

        // apply to collateral
        if fees.total_fee_usd > 0 {
            let fee = fees.total_fee_usd as u128;
            if fee > pos.collateral_usd { pos.collateral_usd = 0; return Err(Error::InsufficientCollateral); }
            pos.collateral_usd = pos.collateral_usd.saturating_sub(fee);
        } else if fees.total_fee_usd < 0 {
            let credit = (-fees.total_fee_usd) as u128;
            pos.collateral_usd = pos.collateral_usd.saturating_add(credit);
        }

        Ok(fees)
    }

    fn funding_rate(pool: &PoolAmounts, cfg: &MarketConfig, dt: u64) -> Result<i128, Error> {
        let total_oi = pool.long_oi_usd.saturating_add(pool.short_oi_usd);
        if total_oi == 0 { return Ok(0); }

        let imbalance = (pool.long_oi_usd as i128) - (pool.short_oi_usd as i128);
        let ratio_bps = (imbalance.saturating_mul(10_000)) / (total_oi as i128);
        let base = (ratio_bps.saturating_mul(cfg.funding_factor as i128)) / 10_000;
        let exponent = cfg.funding_exponent.max(1);
        let rate = base.saturating_mul(exponent as i128) / 10_000;

        let seconds_per_year = 365 * 24 * 60 * 60u128;
        let time_adj = rate.saturating_mul(dt as i128) / (seconds_per_year as i128);

        // cap ±10 bps per hour
        let max_per_hour = 10i128;
        let hours = (dt / 3600) as i128;
        let cap = max_per_hour.saturating_mul(hours);
        Ok(time_adj.max(-cap).min(cap))
    }

    fn pool_borrowing_fees(pool: &PoolAmounts, cfg: &MarketConfig, dt: u64) -> Result<u128, Error> {
        // util = OI / liquidity (bps)
        let long_util = if pool.long_liquidity_usd > 0 {
            pool.long_oi_usd.saturating_mul(10_000) / pool.long_liquidity_usd
        } else { 0 };
        let short_util = if pool.short_liquidity_usd > 0 {
            pool.short_oi_usd.saturating_mul(10_000) / pool.short_liquidity_usd
        } else { 0 };

        let long_rate = Self::borrowing_rate(long_util, cfg)?;
        let short_rate = Self::borrowing_rate(short_util, cfg)?;

        let (long_fee, short_fee) = if cfg.skip_borrowing_for_smaller_side {
            if pool.long_oi_usd > pool.short_oi_usd {
                (long_rate.saturating_mul(pool.long_oi_usd) / 10_000, 0)
            } else {
                (0, short_rate.saturating_mul(pool.short_oi_usd) / 10_000)
            }
        } else {
            (
                long_rate.saturating_mul(pool.long_oi_usd) / 10_000,
                short_rate.saturating_mul(pool.short_oi_usd) / 10_000,
            )
        };

        let total = long_fee.saturating_add(short_fee);
        let seconds_per_year = 365 * 24 * 60 * 60u128;
        Ok(total.saturating_mul(dt as u128) / seconds_per_year)
    }

    fn borrowing_rate(util_bps: u128, cfg: &MarketConfig) -> Result<u128, Error> {
        let exponent = cfg.borrowing_exponent.max(1);
        let util_exp = util_bps.saturating_mul(exponent) / 10_000;
        let rate = cfg.borrowing_factor.saturating_mul(util_exp) / 10_000;
        Ok(rate.min(10_000)) // cap 100% APR
    }

    fn position_borrowing_fee(
        pos: &Position,
        pool: &PoolAmounts,
        cfg: &MarketConfig,
        dt: u64,
    ) -> Result<u128, Error> {
        let liquidity = if pos.is_long { pool.long_liquidity_usd } else { pool.short_liquidity_usd };
        if liquidity == 0 { return Ok(0); }
        let util_bps = pos.size_usd.saturating_mul(10_000) / liquidity;
        let rate = Self::borrowing_rate(util_bps, cfg)?;
        Ok(rate.saturating_mul(pos.size_usd).saturating_mul(dt as u128) / (365 * 24 * 60 * 60 * 10_000))
    }

    pub fn is_liquidatable(pos: &Position, current_price_usd: u128, liq_bps: u16) -> bool {
        if pos.size_usd == 0 || pos.entry_price_usd == 0 { return false; }

        // tokens_usdx = size_usd * 1e6 / entry
        let tokens_usdx = pos.size_usd.saturating_mul(USD_SCALE) / pos.entry_price_usd;
        if tokens_usdx == 0 { return false; }

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