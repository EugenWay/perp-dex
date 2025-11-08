use sails_rs::{prelude::*, gstd::exec};
use crate::{
    types::*,
    errors::Error,
    PerpetualDEXState,
};

pub struct PositionModule;

impl PositionModule {
    // ========================================================================
    // INCREASE POSITION
    // ========================================================================
    
    pub fn increase_position(
        account: ActorId,
        market: String,
        collateral_token: String,
        is_long: bool,
        size_delta_usd: u128,
        collateral_delta_amount: u128,
        execution_price: u128,
    ) -> Result<PositionKey, Error> {
        let st = PerpetualDEXState::get_mut();
        
        // Get market config
        let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?.clone();
        
        // Get position key using State function
        let position_key = PerpetualDEXState::get_position_key(
            account,
            &market,
            &collateral_token,
            is_long,
        );
        
        // Get or create position
        let position = if let Some(pos) = st.positions.get_mut(&position_key) {
            pos
        } else {
            // Create new position
            let new_pos = Position {
                key: position_key,
                account,
                market: market.clone(),
                collateral_token: collateral_token.clone(),
                is_long,
                size_in_usd: 0,
                size_in_tokens: 0,
                collateral_amount: 0,
                entry_price: execution_price,
                liquidation_price: 0,
                borrowing_factor: 0,
                funding_fee_per_size: 0,
                increased_at_block: exec::block_height(),
                decreased_at_block: 0,
                last_fee_update: exec::block_timestamp(),
            };
            st.positions.insert(position_key, new_pos);
            
            // Add to account positions
            st.account_positions
                .entry(account)
                .or_insert_with(Vec::new)
                .push(position_key);
            
            st.positions.get_mut(&position_key).unwrap()
        };
        
        // Calculate new entry price (weighted average)
        if position.size_in_usd > 0 {
            let total_cost = position.size_in_usd * position.entry_price / 1_000000 
                           + size_delta_usd * execution_price / 1_000000;
            let total_size = position.size_in_usd + size_delta_usd;
            position.entry_price = total_cost * 1_000000 / total_size;
        } else {
            position.entry_price = execution_price;
        }
        
        // Calculate size in tokens
        let size_delta_tokens = Self::usd_to_tokens(size_delta_usd, execution_price);
        
        // Update position
        position.size_in_usd += size_delta_usd;
        position.size_in_tokens += size_delta_tokens;
        position.collateral_amount += collateral_delta_amount;
        position.increased_at_block = exec::block_height();
        
        // Update pool state (pool becomes counterparty)
        let pool = st.pool_amounts.entry(market.clone()).or_insert_with(PoolAmounts::default);
        
        if is_long {
            // Trader long → pool short
            pool.long_oi += size_delta_usd;
            pool.long_oi_in_tokens += size_delta_tokens;
            pool.long_token_amount += collateral_delta_amount;
            
            // Check max OI
            if pool.long_oi > config.max_long_oi {
                return Err(Error::MaxOpenInterestExceeded);
            }
        } else {
            // Trader short → pool long
            pool.short_oi += size_delta_usd;
            pool.short_oi_in_tokens += size_delta_tokens;
            pool.short_token_amount += collateral_delta_amount;
            
            if pool.short_oi > config.max_short_oi {
                return Err(Error::MaxOpenInterestExceeded);
            }
        }
        
        // Calculate liquidation price
        position.liquidation_price = Self::calculate_liquidation_price(
            position,
            config.liquidation_threshold_bps,
        );
        
        // Validate leverage
        let collateral_value = position.collateral_amount * execution_price / 1_000000;
        if collateral_value > 0 {
            let leverage = position.size_in_usd * 10000 / collateral_value;
            if leverage > config.max_leverage as u128 * 10000 {
                return Err(Error::MaxLeverageExceeded);
            }
        }
        
        Ok(position_key)
    }
    
    // ========================================================================
    // DECREASE POSITION
    // ========================================================================
    
    pub fn decrease_position(
        account: ActorId,
        market: String,
        collateral_token: String,
        is_long: bool,
        size_delta_usd: u128,
        collateral_delta_amount: u128,
        execution_price: u128,
    ) -> Result<PositionKey, Error> {
        let st = PerpetualDEXState::get_mut();
        
        // Get position
        let position_key = PerpetualDEXState::get_position_key(
            account,
            &market,
            &collateral_token,
            is_long,
        );
        
        let position = st.positions.get_mut(&position_key).ok_or(Error::PositionNotFound)?;
        
        // Check size
        if size_delta_usd > position.size_in_usd {
            return Err(Error::InsufficientPositionSize);
        }
        
        // Calculate PnL
        let pnl = Self::calculate_pnl(position, execution_price);
        
        // Calculate proportional decrease
        let size_delta_tokens = position.size_in_tokens * size_delta_usd / position.size_in_usd;
        let pnl_delta = pnl * size_delta_usd as i128 / position.size_in_usd as i128;
        
        // Update position
        position.size_in_usd -= size_delta_usd;
        position.size_in_tokens -= size_delta_tokens;
        position.decreased_at_block = exec::block_height();
        
        // Update collateral
        if collateral_delta_amount > 0 {
            if collateral_delta_amount > position.collateral_amount {
                return Err(Error::InsufficientCollateral);
            }
            position.collateral_amount -= collateral_delta_amount;
        }
        
        // Calculate payout
        let mut payout = collateral_delta_amount;
        if pnl_delta > 0 {
            payout += pnl_delta as u128;
        } else if pnl_delta < 0 {
            let loss = (-pnl_delta) as u128;
            if loss >= payout {
                payout = 0;
            } else {
                payout -= loss;
            }
        }
        
        // Update pool
        let pool = st.pool_amounts.entry(market.clone()).or_insert_with(PoolAmounts::default);
        
        if is_long {
            pool.long_oi -= size_delta_usd;
            pool.long_oi_in_tokens -= size_delta_tokens;
            
            if pnl_delta > 0 {
                // Trader profit → pool pays
                if payout > pool.long_token_amount {
                    return Err(Error::InsufficientPoolLiquidity);
                }
                pool.long_token_amount -= payout;
            } else {
                // Trader loss → pool gains
                pool.long_token_amount -= collateral_delta_amount;
            }
        } else {
            pool.short_oi -= size_delta_usd;
            pool.short_oi_in_tokens -= size_delta_tokens;
            
            if pnl_delta > 0 {
                if payout > pool.short_token_amount {
                    return Err(Error::InsufficientPoolLiquidity);
                }
                pool.short_token_amount -= payout;
            } else {
                pool.short_token_amount -= collateral_delta_amount;
            }
        }
        
        // If position fully closed, remove it
        if position.size_in_usd == 0 {
            st.positions.remove(&position_key);
        } else {
            // Update liquidation price
            let config = st.market_configs.get(&market).ok_or(Error::MarketNotFound)?;
            position.liquidation_price = Self::calculate_liquidation_price(
                position,
                config.liquidation_threshold_bps,
            );
        }
        
        Ok(position_key)
    }
    
    // ========================================================================
    // CALCULATIONS
    // ========================================================================
    
    fn calculate_pnl(position: &Position, current_price: u128) -> i128 {
        let price_delta = if position.is_long {
            current_price as i128 - position.entry_price as i128
        } else {
            position.entry_price as i128 - current_price as i128
        };
        
        position.size_in_tokens as i128 * price_delta / 1_000000
    }
    
    fn calculate_liquidation_price(
        position: &Position,
        liquidation_threshold_bps: u16,
    ) -> u128 {
        let threshold_value = position.collateral_amount * liquidation_threshold_bps as u128 / 10000;
        let size_in_tokens = position.size_in_tokens;
        
        if size_in_tokens == 0 {
            return 0;
        }
        
        if position.is_long {
            let loss_allowed = if position.collateral_amount > threshold_value {
                position.collateral_amount - threshold_value
            } else {
                0
            };
            
            let price_drop = loss_allowed * 1_000000 / size_in_tokens;
            
            if price_drop >= position.entry_price {
                0
            } else {
                position.entry_price - price_drop
            }
        } else {
            let loss_allowed = if position.collateral_amount > threshold_value {
                position.collateral_amount - threshold_value
            } else {
                0
            };
            
            let price_rise = loss_allowed * 1_000000 / size_in_tokens;
            position.entry_price + price_rise
        }
    }
    
    fn usd_to_tokens(usd_amount: u128, price: u128) -> u128 {
        if price == 0 {
            return 0;
        }
        usd_amount * 1_000000 / price
    }
    
    // ========================================================================
    // QUERIES
    // ========================================================================
    
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
        let position = Self::get_position(key)?;
        Ok(Self::calculate_pnl(&position, current_price))
    }
}