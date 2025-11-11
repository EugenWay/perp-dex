use sails_rs::gstd::exec;
use sails_rs::prelude::*;
use crate::{
    types::*,
    errors::Error,
    PerpetualDEXState,
    modules::risk::RiskModule,
};

pub struct PositionModule;

impl PositionModule {
    pub fn increase_position(
        account: ActorId,
        market: String,
        collateral_token: String,
        is_long: bool,
        size_delta_usd: u128,
        collateral_delta_usd: u128,
        execution_price_usd: u128,
    ) -> Result<PositionKey, Error> {
        let mut st = PerpetualDEXState::get_mut();
        let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();

        // Check user has sufficient USD balance
        let bal = st.balances.get(&account).copied().unwrap_or(0);
        let total_cost = collateral_delta_usd; // trading fee берётся в TradingModule
        if bal < total_cost { return Err(Error::InsufficientBalance); }

        let key = PerpetualDEXState::get_position_key(account, &market, &collateral_token, is_long);
        let now = exec::block_timestamp();

        // Take position by ownership (or create)
        let mut pos = match st.positions.remove(&key) {
            Some(mut p) => {
                // settle fees on existing position
                RiskModule::settle_position_fees(&mut p, &market, now)?;
                p
            }
            None => {
                // register new key in account_positions
                st.account_positions.entry(account).or_insert_with(Vec::new).push(key);
                Position {
                    key,
                    account,
                    market: market.clone(),
                    collateral_token: collateral_token.clone(),
                    is_long,
                    size_usd: 0,
                    collateral_usd: 0,
                    entry_price_usd: execution_price_usd,
                    liquidation_price_usd: 0,
                    funding_fee_per_usd: 0,
                    borrowing_factor: 0,
                    increased_at_block: exec::block_height(),
                    decreased_at_block: 0,
                    last_fee_update: now,
                }
            }
        };

        // Weighted average entry
        if pos.size_usd > 0 {
            let total_cost_usd = pos.size_usd
                .saturating_mul(pos.entry_price_usd) / USD_SCALE
                + size_delta_usd.saturating_mul(execution_price_usd) / USD_SCALE;
            let total_size_usd = pos.size_usd.saturating_add(size_delta_usd);
            if total_size_usd == 0 { return Err(Error::MathOverflow); }
            pos.entry_price_usd = total_cost_usd.saturating_mul(USD_SCALE) / total_size_usd;
        } else {
            pos.entry_price_usd = execution_price_usd;
        }

        // OI limits (USD)
        let pool = st.pool_amounts.entry(market.clone()).or_insert_with(PoolAmounts::default);
        if is_long {
            let new_oi = pool.long_oi_usd.saturating_add(size_delta_usd);
            if new_oi > config.max_long_oi { return Err(Error::MaxOpenInterestExceeded); }
        } else {
            let new_oi = pool.short_oi_usd.saturating_add(size_delta_usd);
            if new_oi > config.max_short_oi { return Err(Error::MaxOpenInterestExceeded); }
        }

        // Deduct user balance (collateral part)
        let bal_entry = st.balances.entry(account).or_insert(0);
        *bal_entry = bal_entry.saturating_sub(collateral_delta_usd);

        // Update position amounts
        pos.size_usd = pos.size_usd.saturating_add(size_delta_usd);
        pos.collateral_usd = pos.collateral_usd.saturating_add(collateral_delta_usd);
        pos.increased_at_block = exec::block_height();

        // Update pool OI and liquidity backing (USD)
        let pool = st.pool_amounts.entry(market.clone()).or_insert_with(PoolAmounts::default);
        if is_long {
            pool.long_oi_usd = pool.long_oi_usd.saturating_add(size_delta_usd);
            pool.long_liquidity_usd = pool.long_liquidity_usd.saturating_add(collateral_delta_usd);
        } else {
            pool.short_oi_usd = pool.short_oi_usd.saturating_add(size_delta_usd);
            pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_add(collateral_delta_usd);
        }

        // Liquidation price cache
        pos.liquidation_price_usd = Self::calculate_liquidation_price(&pos, config.liquidation_threshold_bps);

        // Leverage check
        if pos.collateral_usd > 0 {
            let leverage_bps = pos.size_usd.saturating_mul(10_000) / pos.collateral_usd;
            if leverage_bps > (config.max_leverage as u128).saturating_mul(10_000) {
                return Err(Error::MaxLeverageExceeded);
            }
        }

        // Put position back
        st.positions.insert(key, pos);

        Ok(key)
    }

    pub fn decrease_position(
        account: ActorId,
        market: String,
        collateral_token: String,
        is_long: bool,
        size_delta_usd: u128,
        collateral_delta_usd: u128,
        execution_price_usd: u128,
    ) -> Result<PositionKey, Error> {
        let mut st = PerpetualDEXState::get_mut();
        let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();

        let key = PerpetualDEXState::get_position_key(account, &market, &collateral_token, is_long);

        // Take position by ownership
        let mut pos = st.positions.remove(&key).ok_or(Error::PositionNotFound)?;

        // Settle fees before decrease
        let now = exec::block_timestamp();
        RiskModule::settle_position_fees(&mut pos, &market, now)?;

        if size_delta_usd > pos.size_usd { return Err(Error::InsufficientPositionSize); }
        if collateral_delta_usd > pos.collateral_usd { return Err(Error::InsufficientCollateral); }

        // PnL in USD (signed)
        let pnl = Self::calculate_pnl(&pos, execution_price_usd);

        // Reduce size/collateral
        pos.size_usd = pos.size_usd.saturating_sub(size_delta_usd);
        pos.collateral_usd = pos.collateral_usd.saturating_sub(collateral_delta_usd);
        pos.decreased_at_block = exec::block_height();

        // Payout to user: withdrawn collateral + PnL
        let mut payout_usd = collateral_delta_usd;
        if pnl >= 0 {
            payout_usd = payout_usd.saturating_add(pnl as u128);
        } else {
            let loss = pnl.unsigned_abs();
            payout_usd = payout_usd.saturating_sub(payout_usd.min(loss));
        }

        // Update pool OI and liquidity (USD)
        let pool = st.pool_amounts.entry(market.clone()).or_insert_with(PoolAmounts::default);
        if is_long {
            pool.long_oi_usd = pool.long_oi_usd.saturating_sub(size_delta_usd);
            pool.long_liquidity_usd = pool.long_liquidity_usd.saturating_sub(collateral_delta_usd);
        } else {
            pool.short_oi_usd = pool.short_oi_usd.saturating_sub(size_delta_usd);
            pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_sub(collateral_delta_usd);
        }

        // Return payout into user internal USD balance
        let bal = st.balances.entry(account).or_insert(0);
        *bal = bal.saturating_add(payout_usd);

        // If position not closed — refresh liquidation price and insert back; else cleanup
        if pos.size_usd > 0 {
            pos.liquidation_price_usd = Self::calculate_liquidation_price(&pos, config.liquidation_threshold_bps);
            st.positions.insert(key, pos);
        } else {
            if let Some(vec) = st.account_positions.get_mut(&account) {
                if let Some(i) = vec.iter().position(|k| *k == key) {
                    vec.swap_remove(i);
                }
            }
        }

        Ok(key)
    }

    fn calculate_pnl(pos: &Position, current_price_usd: u128) -> i128 {
        // tokens = size_usd / entry_price
        // PnL = (current - entry) * tokens
        if pos.entry_price_usd == 0 { return 0; }
        let tokens_usdx = pos.size_usd.saturating_mul(USD_SCALE) / pos.entry_price_usd; // USD * 1e6 / USD = 1e6 "token units"
        let price_delta = if pos.is_long {
            current_price_usd as i128 - pos.entry_price_usd as i128
        } else {
            pos.entry_price_usd as i128 - current_price_usd as i128
        };
        // (price_delta * tokens) / USD_SCALE  -> USD
        (price_delta.saturating_mul(tokens_usdx as i128)) / (USD_SCALE as i128)
    }

    fn calculate_liquidation_price(pos: &Position, liq_bps: u16) -> u128 {
        if pos.size_usd == 0 || pos.entry_price_usd == 0 { return 0; }
        let threshold_usd = (pos.collateral_usd.saturating_mul(liq_bps as u128)) / 10_000;

        // tokens in 1e6 units
        let tokens_usdx = pos.size_usd.saturating_mul(USD_SCALE) / pos.entry_price_usd;
        if tokens_usdx == 0 { return 0; }

        let loss_allowed = pos.collateral_usd.saturating_sub(threshold_usd);
        // |delta_price| = loss * USD_SCALE / tokens
        let delta = loss_allowed.saturating_mul(USD_SCALE) / tokens_usdx;

        if pos.is_long {
            pos.entry_price_usd.saturating_sub(delta)
        } else {
            pos.entry_price_usd.saturating_add(delta)
        }
    }

    pub fn get_position(key: &PositionKey) -> Result<Position, Error> {
        let st = PerpetualDEXState::get();
        st.positions.get(key).cloned().ok_or(Error::PositionNotFound)
    }

    pub fn get_account_positions(account: ActorId) -> Vec<Position> {
        let st = PerpetualDEXState::get();
        st.positions.values().filter(|p| p.account == account).cloned().collect()
    }

    pub fn get_position_pnl(key: &PositionKey, current_price: u128) -> Result<i128, Error> {
        let pos = Self::get_position(key)?;
        Ok(Self::calculate_pnl(&pos, current_price))
    }
}