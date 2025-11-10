use sails_rs::{prelude::*, gstd::msg};
use crate::{
    errors::Error,
    types::*,
    modules::{trading::TradingModule, position::PositionModule, risk::RiskModule, oracle::OracleModule},
    PerpetualDEXState,
};

pub struct ExecutorService;

impl ExecutorService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExecutorService {
    fn default() -> Self {
        Self::new()
    }
}

#[service]
impl ExecutorService {
    /// Execute a saved limit/stop order (callable by keepers)
    #[export]
    pub fn execute_order(&mut self, order_key: RequestKey) -> Result<ExecutionResult, Error> {
        let executor = msg::source();
        TradingModule::execute_saved_order(executor, order_key)
    }

    /// Liquidate an underwater position (callable by keepers/liquidators)
    #[export]
    pub fn liquidate_position(
        &mut self,
        position_key: PositionKey,
    ) -> Result<(), Error> {
        let liquidator = msg::source();
        let st = PerpetualDEXState::get();
        
        // Check liquidator is authorized
        if !st.is_keeper(liquidator) && !st.is_liquidator(liquidator) {
            return Err(Error::NotLiquidator);
        }

        let position = PositionModule::get_position(&position_key)?;
        
        // Get current price
        let current_price = OracleModule::mid(&position.market)?;
        
        // Get liquidation threshold from config
        let config = st.market_configs.get(&position.market).ok_or(Error::MarketNotFound)?;
        
        // Check if liquidatable
        if !RiskModule::is_liquidatable(&position, current_price, config.liquidation_threshold_bps) {
            return Err(Error::PositionNotLiquidatable);
        }

        // Close the position
        PositionModule::decrease_position(
            position.account,
            position.market.clone(),
            position.collateral_token.clone(),
            position.is_long,
            position.size_in_usd,
            position.collateral_amount,
            current_price,
        )?;

        // In production, would pay liquidation reward to liquidator
        // For now, just emit event (events system TODO)
        
        Ok(())
    }

    /// Check if a position can be liquidated
    #[export]
    pub fn can_liquidate(&self, position_key: PositionKey) -> Result<bool, Error> {
        let position = PositionModule::get_position(&position_key)?;
        let current_price = OracleModule::mid(&position.market)?;
        
        let st = PerpetualDEXState::get();
        let config = st.market_configs.get(&position.market).ok_or(Error::MarketNotFound)?;
        
        Ok(RiskModule::is_liquidatable(&position, current_price, config.liquidation_threshold_bps))
    }

    /// Get all positions that can be liquidated
    #[export]
    pub fn get_liquidatable_positions(&self) -> Vec<PositionKey> {
        let st = PerpetualDEXState::get();
        let mut liquidatable = Vec::new();

        for (key, position) in st.positions.iter() {
            if let Ok(current_price) = OracleModule::mid(&position.market) {
                if let Some(config) = st.market_configs.get(&position.market) {
                    if RiskModule::is_liquidatable(position, current_price, config.liquidation_threshold_bps) {
                        liquidatable.push(*key);
                    }
                }
            }
        }

        liquidatable
    }

    /// Get all orders that can be executed
    #[export]
    pub fn get_executable_orders(&self) -> Vec<RequestKey> {
        let orders = TradingModule::get_pending_orders();
        let mut executable = Vec::new();

        for (key, order) in orders {
            if let Ok(mid) = OracleModule::mid(&order.market) {
                // Check if order trigger conditions are met
                let can_execute = match order.order_type {
                    OrderType::LimitIncrease => {
                        if order.is_long { mid <= order.trigger_price } else { mid >= order.trigger_price }
                    }
                    OrderType::LimitDecrease => {
                        if order.is_long { mid >= order.trigger_price } else { mid <= order.trigger_price }
                    }
                    OrderType::StopLossDecrease => {
                        if order.is_long { mid <= order.trigger_price } else { mid >= order.trigger_price }
                    }
                    _ => false,
                };

                if can_execute {
                    executable.push(key);
                }
            }
        }

        executable
    }
}