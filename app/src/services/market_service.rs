use sails_rs::{prelude::*, gstd::msg};
use crate::{errors::Error, types::*, modules::market::MarketModule};

#[derive(Default)]
pub struct MarketService;
impl MarketService {
    pub fn new() -> Self {
        Self::default()
    }
}

#[service]
impl MarketService {
    #[export]
    pub fn add_liquidity(
        &mut self,
        market_id: String,
        long_token_amount: u128,
        short_token_amount: u128,
        min_mint: u128,
    ) -> Result<u128, Error> {
        let lp = msg::source();
        MarketModule::add_liquidity(
            lp,
            market_id,
            long_token_amount,
            short_token_amount,
            min_mint,
        )
    }

    #[export]
    pub fn remove_liquidity(
        &mut self,
        market_id: String,
        market_token_amount: u128,
        min_long_out: u128,
        min_short_out: u128,
    ) -> Result<(u128, u128), Error> {
        let lp = msg::source();
        MarketModule::remove_liquidity(
            lp,
            market_id,
            market_token_amount,
            min_long_out,
            min_short_out,
        )
    }

    #[export]
    pub fn get_pool(&self, market_id: String) -> Result<PoolAmounts, Error> {
        MarketModule::get_pool(&market_id)
    }
}