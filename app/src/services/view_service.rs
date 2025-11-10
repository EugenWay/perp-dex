use sails_rs::{prelude::*, gstd::msg};
use crate::{
    types::*,
    errors::Error,
    modules::{position::PositionModule, market::MarketModule, oracle::OracleModule},
    PerpetualDEXState,
};

#[derive(Default)]
pub struct ViewService;
impl ViewService { pub fn new() -> Self { Self::default() } }

#[service]
impl ViewService {
    // Market views
    #[export]
    pub fn get_market(&self, market_id: String) -> Result<Market, Error> {
        let st = PerpetualDEXState::get();
        st.markets.get(&market_id).cloned().ok_or(Error::MarketNotFound)
    }

    #[export]
    pub fn get_market_config(&self, market_id: String) -> Result<MarketConfig, Error> {
        let st = PerpetualDEXState::get();
        st.market_configs.get(&market_id).cloned().ok_or(Error::MarketNotFound)
    }

    #[export]
    pub fn get_pool(&self, market_id: String) -> Result<PoolAmounts, Error> {
        MarketModule::get_pool(&market_id)
    }

    #[export]
    pub fn get_all_markets(&self) -> Vec<(String, Market)> {
        let st = PerpetualDEXState::get();
        st.markets.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    #[export]
    pub fn get_market_token_info(&self, market_id: String) -> Result<MarketTokenInfo, Error> {
        let st = PerpetualDEXState::get();
        st.market_tokens.get(&market_id).cloned().ok_or(Error::MarketNotFound)
    }

    // Position views
    #[export]
    pub fn get_position(&self, key: PositionKey) -> Result<Position, Error> {
        PositionModule::get_position(&key)
    }

    #[export]
    pub fn get_account_positions(&self, account: ActorId) -> Vec<Position> {
        PositionModule::get_account_positions(account)
    }

    #[export]
    pub fn get_my_positions(&self) -> Vec<Position> {
        let caller = msg::source();
        PositionModule::get_account_positions(caller)
    }

    #[export]
    pub fn get_position_pnl(&self, key: PositionKey) -> Result<i128, Error> {
        let pos = PositionModule::get_position(&key)?;
        let current_price = OracleModule::mid(&pos.market)?;
        PositionModule::get_position_pnl(&key, current_price)
    }

    #[export]
    pub fn get_market_positions(&self, market_id: String) -> Vec<Position> {
        let st = PerpetualDEXState::get();
        st.positions.values().filter(|p| p.market == market_id).cloned().collect()
    }

    // Order views
    #[export]
    pub fn get_order(&self, key: RequestKey) -> Result<Order, Error> {
        let st = PerpetualDEXState::get();
        st.orders.get(&key).cloned().ok_or(Error::OrderNotFound)
    }

    #[export]
    pub fn get_account_orders(&self, account: ActorId) -> Vec<(RequestKey, Order)> {
        let st = PerpetualDEXState::get();
        st.account_orders.get(&account)
            .map(|keys| keys.iter().filter_map(|k| st.orders.get(k).map(|o| (*k, o.clone()))).collect())
            .unwrap_or_default()
    }

    #[export]
    pub fn get_my_orders(&self) -> Vec<(RequestKey, Order)> {
        let caller = msg::source();
        self.get_account_orders(caller)
    }

    #[export]
    pub fn get_pending_orders(&self) -> Vec<(RequestKey, Order)> {
        let st = PerpetualDEXState::get();
        st.orders.iter().filter(|(_, o)| o.status == OrderStatus::Created).map(|(k, o)| (*k, o.clone())).collect()
    }

    // Oracle views
    #[export]
    pub fn get_oracle_price(&self, token: String) -> Result<Price, Error> {
        OracleModule::get_price(&token)
    }
    #[export]
    pub fn get_oracle_mid(&self, token: String) -> Result<u128, Error> {
        OracleModule::mid(&token)
    }
    #[export]
    pub fn get_oracle_spread(&self, token: String) -> Result<u128, Error> {
        OracleModule::spread(&token)
    }
    #[export]
    pub fn get_oracle_last_update(&self, token: String) -> Option<u64> {
        OracleModule::last_update(&token)
    }

    // Balances
    #[export]
    pub fn get_balance(&self, account: ActorId) -> u128 {
        let st = PerpetualDEXState::get();
        st.balances.get(&account).copied().unwrap_or(0)
    }
    #[export]
    pub fn my_balance(&self) -> u128 {
        let caller = msg::source();
        self.get_balance(caller)
    }

    // Admin views
    #[export]
    pub fn get_admin(&self) -> ActorId { PerpetualDEXState::get().admin }
    #[export]
    pub fn get_keepers(&self) -> Vec<ActorId> { PerpetualDEXState::get().keepers.clone() }
    #[export]
    pub fn get_liquidators(&self) -> Vec<ActorId> { PerpetualDEXState::get().liquidators.clone() }

    // Stats
    #[export]
    pub fn get_total_positions(&self) -> u64 { PerpetualDEXState::get().positions.len() as u64 }
    #[export]
    pub fn get_total_orders(&self) -> u64 { PerpetualDEXState::get().orders.len() as u64 }
    #[export]
    pub fn get_total_markets(&self) -> u64 { PerpetualDEXState::get().markets.len() as u64 }
}