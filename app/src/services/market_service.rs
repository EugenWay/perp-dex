use sails_rs::{prelude::*, gstd::msg};
use crate::{errors::Error, types::*, modules::market::MarketModule, PerpetualDEXState};

#[derive(Default)]
pub struct MarketService;
impl MarketService { pub fn new() -> Self { Self::default() } }

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
        let minted = MarketModule::add_liquidity(lp, market_id.clone(), long_token_amount, short_token_amount, min_mint)?;

        // credit LP market tokens
        let st = PerpetualDEXState::get_mut();
        let mt = st.market_tokens.get_mut(&market_id).ok_or(Error::MarketNotFound)?;
        // helper balance update
        let entry = mt.balances.iter_mut().find(|(a, _)| *a == lp);
        if let Some(e) = entry { e.1 = e.1.saturating_add(minted); } else { mt.balances.push((lp, minted)); }
        Ok(minted)
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
        // burn first (balance check)
        let st = PerpetualDEXState::get_mut();
        let mt = st.market_tokens.get_mut(&market_id).ok_or(Error::MarketNotFound)?;
        let bal = mt.balances.iter_mut().find(|(a, _)| *a == lp).ok_or(Error::InsufficientMarketTokens)?;
        if bal.1 < market_token_amount { return Err(Error::InsufficientMarketTokens); }
        bal.1 = bal.1.saturating_sub(market_token_amount);

        let (lo, sh) = MarketModule::remove_liquidity(lp, market_id, market_token_amount, min_long_out, min_short_out)?;
        Ok((lo, sh))
    }

    #[export]
    pub fn get_pool(&self, market_id: String) -> Result<PoolAmounts, Error> {
        MarketModule::get_pool(&market_id)
    }
}