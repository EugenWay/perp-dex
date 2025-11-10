use crate::{
    types::*,
    utils,
    errors::Error,
    PerpetualDEXState,
    modules::oracle::OracleModule,
};
use crate::types::OrderSide;

#[derive(Clone, Debug)]
pub struct QuoteResult {
    pub execution_price: u128,
    pub price_impact: i128,      // bps signed
    pub price_impact_usd: i128,  // signed USD
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
        let spread = OracleModule::spread(market)?;
        let ask = mid.saturating_add(spread / 2);
        let bid = mid.saturating_sub(spread / 2);

        let impact_bps = Self::calculate_price_impact(pool, cfg, side, size_usd, is_increase)?;
        let (base, sign_worse_for_trader) = match side {
            OrderSide::Long  => (if is_increase { ask } else { bid }, is_increase),
            OrderSide::Short => (if is_increase { bid } else { ask }, is_increase),
        };

        let impact_amount = base.saturating_mul(impact_bps.unsigned_abs() as u128) / 10_000;
        let exec = if (impact_bps >= 0) == sign_worse_for_trader {
            // worse for trader
            if matches!(side, OrderSide::Long) == is_increase { base.saturating_add(impact_amount) } else { base.saturating_sub(impact_amount) }
        } else {
            // better for trader
            if matches!(side, OrderSide::Long) == is_increase { base.saturating_sub(impact_amount) } else { base.saturating_add(impact_amount) }
        };

        // clamp ±10%
        let max_dev = mid / 10;
        let execution_price = exec.max(mid.saturating_sub(max_dev)).min(mid.saturating_add(max_dev));

        // approximate impact in USD relative to size
        let price_impact_usd = ((execution_price as i128 - base as i128) * size_usd as i128) / (mid as i128);

        Ok(QuoteResult {
            execution_price,
            price_impact: impact_bps,
            price_impact_usd,
        })
    }

    fn calculate_price_impact(
        pool: &PoolAmounts,
        cfg: &MarketConfig,
        side: &OrderSide,
        _size_usd: u128,
        is_increase: bool,
    ) -> Result<i128, Error> {
        let imbalance = (pool.long_oi_usd as i128) - (pool.short_oi_usd as i128);
        let depth = pool.long_liquidity_usd.saturating_add(pool.short_liquidity_usd);
        if depth == 0 { return Ok(0); }

        let imbalance_abs = imbalance.unsigned_abs() as u128;
        let ratio_bps = (imbalance_abs.saturating_mul(10_000)) / depth; // bps

        let exponent = cfg.pi_exponent.max(1);
        let impact_base = ratio_bps.saturating_mul(exponent) / 10_000;

        let is_long = matches!(side, OrderSide::Long);
        let pi_factor = if is_increase {
            if (is_long && imbalance > 0) || (!is_long && imbalance < 0) {
                cfg.pi_factor_positive
            } else {
                cfg.pi_factor_negative
            }
        } else {
            if (is_long && imbalance > 0) || (!is_long && imbalance < 0) {
                cfg.pi_factor_negative
            } else {
                cfg.pi_factor_positive
            }
        };

        let impact_bps = (impact_base.saturating_mul(pi_factor) / 10_000) as i128;

        // sign: positive if making imbalance worse
        let signed = if is_increase {
            if (is_long && imbalance >= 0) || (!is_long && imbalance < 0) { impact_bps } else { -impact_bps }
        } else {
            if (is_long && imbalance > 0) || (!is_long && imbalance <= 0) { -impact_bps } else { impact_bps }
        };

        Ok(signed.max(-500).min(500)) // cap ±5%
    }
}