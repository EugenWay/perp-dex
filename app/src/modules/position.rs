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

        let (config, balance, existing_pos_opt) = {
            let st = PerpetualDEXState::get();

            let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();
            let balance = st.balances.get(&account).copied().unwrap_or(0);
            let existing = st.positions.get(&key).cloned();

            (config, balance, existing)
        };

        let total_cost = collateral_delta_usd;
        if balance < total_cost {
            return Err(Error::InsufficientBalance);
        }

        let mut pos;
        let is_new_position;

        if let Some(mut existing) = existing_pos_opt {
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

        pos.size_usd = pos.size_usd.saturating_add(size_delta_usd);
        pos.collateral_usd = pos.collateral_usd.saturating_add(collateral_delta_usd);
        pos.increased_at_block = current_block;

        let mut st = PerpetualDEXState::get_mut();

        let pool = st
            .pool_amounts
            .entry(market.clone())
            .or_insert_with(PoolAmounts::default);

        let total_liquidity = pool.liquidity_usd;
        let max_allowed_oi_from_liquidity = total_liquidity.saturating_mul(config.reserve_factor_bps as u128) / 10_000;

        if is_long {
            let new_oi = pool.long_oi_usd.saturating_add(size_delta_usd);

            if new_oi > config.max_long_oi {
                return Err(Error::MaxOpenInterestExceeded);
            }

            if new_oi > max_allowed_oi_from_liquidity {
                return Err(Error::InsufficientLiquidity);
            }

            pool.long_oi_usd = new_oi;
        } else {
            let new_oi = pool.short_oi_usd.saturating_add(size_delta_usd);

            if new_oi > config.max_short_oi {
                return Err(Error::MaxOpenInterestExceeded);
            }

            if new_oi > max_allowed_oi_from_liquidity {
                return Err(Error::InsufficientLiquidity);
            }

            pool.short_oi_usd = new_oi;
        }

        {
            let bal_entry = st.balances.entry(account).or_insert(0);
            if *bal_entry < total_cost {
                return Err(Error::InsufficientBalance);
            }
            *bal_entry = bal_entry.saturating_sub(total_cost);
        }

        if pos.collateral_usd > 0 && pos.size_usd > 0 {
            pos.liquidation_price_usd = Self::calculate_liquidation_price(&pos, config.liquidation_threshold_bps);

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

        let (config, mut pos) = {
            let st = PerpetualDEXState::get();

            let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();
            let pos = st.positions.get(&key).cloned().ok_or(Error::PositionNotFound)?;

            (config, pos)
        };

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

        let mut st = PerpetualDEXState::get_mut();

        let pool = st
            .pool_amounts
            .entry(market.clone())
            .or_insert_with(PoolAmounts::default);

        if is_long {
            pool.long_oi_usd = pool.long_oi_usd.saturating_sub(size_delta_usd);
        } else {
            pool.short_oi_usd = pool.short_oi_usd.saturating_sub(size_delta_usd);
        }

        if pnl_partial > 0 {
            let pnl_usd = pnl_partial as u128;
            pool.liquidity_usd = pool.liquidity_usd.saturating_sub(pnl_usd);
        } else if pnl_partial < 0 {
            let loss_usd = pnl_partial.unsigned_abs();
            pool.liquidity_usd = pool.liquidity_usd.saturating_add(loss_usd);
        }

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

    /// Liquidate a position with liquidator reward
    /// Returns (position_key, liquidation_fee_paid_to_liquidator)
    pub fn liquidate_position(
        liquidator: ActorId,
        position_key: PositionKey,
        execution_price_usd: u128,
        liquidation_fee_bps: u16,
    ) -> Result<(PositionKey, u128), Error> {
        let now = exec::block_timestamp();

        let (mut pos, market, owner) = {
            let st = PerpetualDEXState::get();
            let pos = st.positions.get(&position_key).cloned().ok_or(Error::PositionNotFound)?;
            let market = pos.market.clone();
            let owner = pos.account;
            (pos, market, owner)
        };

        // Settle fees first
        RiskModule::settle_position_fees(&mut pos, &market, now)?;

        // Calculate PnL
        let total_pnl = Self::calculate_pnl(&pos, execution_price_usd);

        // Calculate liquidation fee (from remaining collateral)
        let liquidation_fee = pos.collateral_usd.saturating_mul(liquidation_fee_bps as u128) / 10_000;

        // Remaining collateral after liquidation fee
        let remaining_collateral = pos.collateral_usd.saturating_sub(liquidation_fee);

        // Calculate payout to position owner (collateral - fee + pnl)
        let mut payout_to_owner = remaining_collateral;
        if total_pnl >= 0 {
            payout_to_owner = payout_to_owner.saturating_add(total_pnl as u128);
        } else {
            let loss = total_pnl.unsigned_abs();
            payout_to_owner = payout_to_owner.saturating_sub(payout_to_owner.min(loss));
        }

        // Save position data before mutating state
        let size_usd = pos.size_usd;
        let is_long = pos.is_long;

        let mut st = PerpetualDEXState::get_mut();

        let pool = st
            .pool_amounts
            .entry(market.clone())
            .or_insert_with(PoolAmounts::default);

        // Update pool OI
        if is_long {
            pool.long_oi_usd = pool.long_oi_usd.saturating_sub(size_usd);
        } else {
            pool.short_oi_usd = pool.short_oi_usd.saturating_sub(size_usd);
        }

        // Update pool liquidity based on PnL
        if total_pnl > 0 {
            let pnl_usd = total_pnl as u128;
            pool.liquidity_usd = pool.liquidity_usd.saturating_sub(pnl_usd);
        } else if total_pnl < 0 {
            let loss_usd = total_pnl.unsigned_abs();
            pool.liquidity_usd = pool.liquidity_usd.saturating_add(loss_usd);
        }

        // Pay liquidation fee to liquidator
        {
            let liquidator_bal = st.balances.entry(liquidator).or_insert(0);
            *liquidator_bal = liquidator_bal.saturating_add(liquidation_fee);
        }

        // Pay remaining to position owner
        {
            let owner_bal = st.balances.entry(owner).or_insert(0);
            *owner_bal = owner_bal.saturating_add(payout_to_owner);
        }

        // Remove position
        st.positions.remove(&position_key);
        if let Some(vec) = st.account_positions.get_mut(&owner) {
            if let Some(i) = vec.iter().position(|k| *k == position_key) {
                vec.swap_remove(i);
            }
        }

        Ok((position_key, liquidation_fee))
    }
}
