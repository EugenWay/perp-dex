use crate::{PerpetualDEXState, errors::Error, modules::risk::RiskModule, types::*};
use sails_rs::gstd::exec;
use sails_rs::prelude::*;

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
        let key = PerpetualDEXState::get_position_key(account, &market, &collateral_token, is_long);
        let now = exec::block_timestamp();
        let current_block = exec::block_height();

        // --- Read-only snapshot phase ---
        let (config, balance, existing_pos_opt) = {
            let st = PerpetualDEXState::get();

            let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();

            let balance = st.balances.get(&account).copied().unwrap_or(0);

            // clone existing position (do not remove from state yet)
            let existing = st.positions.get(&key).cloned();

            (config, balance, existing)
        };

        let total_cost = collateral_delta_usd;
        if balance < total_cost {
            return Err(Error::InsufficientBalance);
        }

        // Build/adjust position off-state, including fee settlement
        let mut pos;
        let is_new_position;

        if let Some(mut existing) = existing_pos_opt {
            // settle funding/borrowing fees using RiskModule (it will touch state internally)
            RiskModule::settle_position_fees(&mut existing, &market, now)?;
            pos = existing;
            is_new_position = false;
        } else {
            pos = Position {
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
                increased_at_block: current_block,
                decreased_at_block: 0,
                last_fee_update: now,
            };
            is_new_position = true;
        }

        if pos.size_usd > 0 {
            let old_notional = pos.size_usd;
            let new_notional = size_delta_usd;
            let total_size = old_notional.saturating_add(new_notional);
            if total_size == 0 {
                return Err(Error::MathOverflow);
            }

            pos.entry_price_usd = old_notional
                .saturating_mul(pos.entry_price_usd)
                .saturating_add(new_notional.saturating_mul(execution_price_usd))
                / total_size;
        } else {
            pos.entry_price_usd = execution_price_usd;
        }

        // Apply position deltas
        pos.size_usd = pos.size_usd.saturating_add(size_delta_usd);
        pos.collateral_usd = pos.collateral_usd.saturating_add(collateral_delta_usd);
        pos.increased_at_block = current_block;

        // Liquidation price and leverage check are done after pool/balance checks
        // but use the same config snapshot.

        // --- Mutation phase (single mutable borrow) ---
        let mut st = PerpetualDEXState::get_mut();

        // Re-check market exists (paranoia) and fetch pool
        let pool = st
            .pool_amounts
            .entry(market.clone())
            .or_insert_with(PoolAmounts::default);

        // Open interest limit checks + updates
        if is_long {
            let new_oi = pool.long_oi_usd.saturating_add(size_delta_usd);
            if new_oi > config.max_long_oi {
                return Err(Error::MaxOpenInterestExceeded);
            }
            pool.long_oi_usd = new_oi;
            pool.long_liquidity_usd = pool.long_liquidity_usd.saturating_add(collateral_delta_usd);
        } else {
            let new_oi = pool.short_oi_usd.saturating_add(size_delta_usd);
            if new_oi > config.max_short_oi {
                return Err(Error::MaxOpenInterestExceeded);
            }
            pool.short_oi_usd = new_oi;
            pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_add(collateral_delta_usd);
        }

        // Charge collateral from internal USD wallet
        {
            let bal_entry = st.balances.entry(account).or_insert(0);
            if *bal_entry < total_cost {
                return Err(Error::InsufficientBalance);
            }
            *bal_entry = bal_entry.saturating_sub(total_cost);
        }

        // Recompute liquidation price with latest position state
        if pos.collateral_usd > 0 && pos.size_usd > 0 {
            pos.liquidation_price_usd = Self::calculate_liquidation_price(&pos, config.liquidation_threshold_bps);

            // Leverage check: size / collateral ≤ max_leverage
            let leverage_bps = pos.size_usd.saturating_mul(10_000) / pos.collateral_usd;
            if leverage_bps > (config.max_leverage as u128).saturating_mul(10_000) {
                return Err(Error::MaxLeverageExceeded);
            }
        }

        if is_new_position {
            st.account_positions.entry(account).or_insert_with(Vec::new).push(key);
        }

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
        let key = PerpetualDEXState::get_position_key(account, &market, &collateral_token, is_long);
        let now = exec::block_timestamp();
        let current_block = exec::block_height();

        // --- Read-only snapshot phase ---
        let (config, mut pos) = {
            let st = PerpetualDEXState::get();

            let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();

            let pos = st.positions.get(&key).cloned().ok_or(Error::PositionNotFound)?;

            (config, pos)
        };

        // Settle fees on local copy
        RiskModule::settle_position_fees(&mut pos, &market, now)?;

        if size_delta_usd > pos.size_usd {
            return Err(Error::InsufficientPositionSize);
        }
        if collateral_delta_usd > pos.collateral_usd {
            return Err(Error::InsufficientCollateral);
        }

        let total_pnl = Self::calculate_pnl(&pos, execution_price_usd);
        let pnl_partial = if pos.size_usd == 0 {
            0
        } else {
            (total_pnl.saturating_mul(size_delta_usd as i128)) / (pos.size_usd as i128)
        };

        pos.size_usd = pos.size_usd.saturating_sub(size_delta_usd);
        pos.collateral_usd = pos.collateral_usd.saturating_sub(collateral_delta_usd);
        pos.decreased_at_block = current_block;

        let mut payout_usd = collateral_delta_usd;
        if pnl_partial >= 0 {
            payout_usd = payout_usd.saturating_add(pnl_partial as u128);
        } else {
            let loss = pnl_partial.unsigned_abs();
            payout_usd = payout_usd.saturating_sub(payout_usd.min(loss));
        }

        // --- Mutation phase ---
        let mut st = PerpetualDEXState::get_mut();

        // Update pool OI and liquidity
        let pool = st
            .pool_amounts
            .entry(market.clone())
            .or_insert_with(PoolAmounts::default);

        if is_long {
            pool.long_oi_usd = pool.long_oi_usd.saturating_sub(size_delta_usd);
            pool.long_liquidity_usd = pool.long_liquidity_usd.saturating_sub(collateral_delta_usd);
        } else {
            pool.short_oi_usd = pool.short_oi_usd.saturating_sub(size_delta_usd);
            pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_sub(collateral_delta_usd);
        }

        // Credit payout to user's internal USD balance
        {
            let bal = st.balances.entry(account).or_insert(0);
            *bal = bal.saturating_add(payout_usd);
        }

        if pos.size_usd > 0 {
            pos.liquidation_price_usd = Self::calculate_liquidation_price(&pos, config.liquidation_threshold_bps);
            st.positions.insert(key, pos);
        } else {
            st.positions.remove(&key);
            if let Some(vec) = st.account_positions.get_mut(&account) {
                if let Some(i) = vec.iter().position(|k| *k == key) {
                    vec.swap_remove(i);
                }
            }
        }

        Ok(key)
    }

    fn calculate_pnl(pos: &Position, current_price_usd: u128) -> i128 {
        if pos.size_usd == 0 || pos.entry_price_usd == 0 {
            return 0;
        }
        if pos.is_long {
            let price_diff = (current_price_usd as i128) - (pos.entry_price_usd as i128);
            (pos.size_usd as i128).saturating_mul(price_diff) / (pos.entry_price_usd as i128)
        } else {
            let price_diff = (pos.entry_price_usd as i128) - (current_price_usd as i128);
            (pos.size_usd as i128).saturating_mul(price_diff) / (pos.entry_price_usd as i128)
        }
    }

    fn calculate_liquidation_price(pos: &Position, liq_bps: u16) -> u128 {
        if pos.size_usd == 0 || pos.entry_price_usd == 0 {
            return 0;
        }
        let threshold_usd = (pos.collateral_usd.saturating_mul(liq_bps as u128)) / 10_000;
        let loss_allowed = pos.collateral_usd.saturating_sub(threshold_usd);
        let loss_ratio = loss_allowed.saturating_mul(USD_SCALE) / pos.size_usd;
        if pos.is_long {
            let ratio = USD_SCALE.saturating_sub(loss_ratio);
            pos.entry_price_usd.saturating_mul(ratio) / USD_SCALE
        } else {
            let ratio = USD_SCALE.saturating_add(loss_ratio);
            pos.entry_price_usd.saturating_mul(ratio) / USD_SCALE
        }
    }

    pub fn get_position(key: &PositionKey) -> Result<Position, Error> {
        let st = PerpetualDEXState::get();
        st.positions.get(key).cloned().ok_or(Error::PositionNotFound)
    }

    pub fn get_account_positions(account: ActorId) -> Vec<Position> {
        let st = PerpetualDEXState::get();
        st.positions
            .values()
            .filter(|p| p.account == account)
            .cloned()
            .collect()
    }

    pub fn get_position_pnl(key: &PositionKey, current_price: u128) -> Result<i128, Error> {
        let pos = Self::get_position(key)?;
        Ok(Self::calculate_pnl(&pos, current_price))
    }
}
