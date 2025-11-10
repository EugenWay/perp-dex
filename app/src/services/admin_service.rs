use sails_rs::{prelude::*, gstd::msg};
use crate::{
    errors::Error,
    types::*,
    modules::{market::MarketModule, oracle::OracleModule},
    PerpetualDEXState,
};

#[derive(Default)]
pub struct AdminService;

impl AdminService {
    pub fn new() -> Self { Self::default() }
}

#[service]
impl AdminService {
    /// Create a new market (admin only).
    #[export]
    pub fn create_market(
        &mut self,
        market_id: String,
        index_token: String,
        long_token: String,
        short_token: String,
        market_token: ActorId,
        config: MarketConfig,
    ) -> Result<(), Error> {
        let caller = msg::source();
        MarketModule::create_market(
            caller, market_id, index_token, long_token, short_token, market_token, config,
        )
    }

    /// Update market config (admin only).
    #[export]
    pub fn set_market_config(&mut self, market_id: String, config: MarketConfig) -> Result<(), Error> {
        let caller = msg::source();
        MarketModule::set_market_config(caller, market_id, config)
    }

    /// Update oracle config (admin only).
    #[export]
    pub fn set_oracle_config(&mut self, cfg: OracleConfig) -> Result<(), Error> {
        let caller = msg::source();
        OracleModule::set_config(caller, cfg)
    }

    /// Add keeper (admin only).
    #[export]
    pub fn add_keeper(&mut self, keeper: ActorId) -> Result<(), Error> {
        let caller = msg::source();
        let st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) { return Err(Error::Unauthorized); }
        if !st.keepers.contains(&keeper) {
            st.keepers.push(keeper);
        }
        Ok(())
    }

    /// Remove keeper (admin only).
    #[export]
    pub fn remove_keeper(&mut self, keeper: ActorId) -> Result<(), Error> {
        let caller = msg::source();
        let st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) { return Err(Error::Unauthorized); }
        if let Some(i) = st.keepers.iter().position(|k| *k == keeper) {
            st.keepers.swap_remove(i);
        }
        Ok(())
    }

    /// (Optional) Liquidator management — mirror keepers if you use separate role.
    #[export]
    pub fn add_liquidator(&mut self, liquidator: ActorId) -> Result<(), Error> {
        let caller = msg::source();
        let st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) { return Err(Error::Unauthorized); }
        if !st.liquidators.contains(&liquidator) {
            st.liquidators.push(liquidator);
        }
        Ok(())
    }

    #[export]
    pub fn remove_liquidator(&mut self, liquidator: ActorId) -> Result<(), Error> {
        let caller = msg::source();
        let st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) { return Err(Error::Unauthorized); }
        if let Some(i) = st.liquidators.iter().position(|k| *k == liquidator) {
            st.liquidators.swap_remove(i);
        }
        Ok(())
    }
}