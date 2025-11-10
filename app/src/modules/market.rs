use sails_rs::prelude::*;
use crate::{
    types::*,
    errors::Error,
    PerpetualDEXState,
    modules::oracle::OracleModule,
};

pub struct MarketModule;

impl MarketModule {
    /// Create a new market (admin only)
    pub fn create_market(
        caller: ActorId,
        market_id: String,
        index_token: String,
        long_token: String,
        short_token: String,
        market_token: ActorId,
        config: MarketConfig,
    ) -> Result<(), Error> {
        let st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) { return Err(Error::Unauthorized); }
        if st.markets.contains_key(&market_id) { return Err(Error::MarketAlreadyExists); }

        let market = Market { market_token, index_token, long_token, short_token };
        st.markets.insert(market_id.clone(), market);
        st.market_configs.insert(market_id.clone(), config);
        st.pool_amounts.insert(market_id.clone(), PoolAmounts::default());
        st.market_tokens.insert(market_id, MarketTokenInfo::default());
        Ok(())
    }

    /// Update market configuration (admin only)
    pub fn set_market_config(
        caller: ActorId,
        market_id: String,
        config: MarketConfig,
    ) -> Result<(), Error> {
        let st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) { return Err(Error::Unauthorized); }
        if !st.markets.contains_key(&market_id) { return Err(Error::MarketNotFound); }
        st.market_configs.insert(market_id, config);
        Ok(())
    }

    /// Add liquidity (inputs are token amounts; internally we convert to USD)
    pub fn add_liquidity(
        _lp: ActorId,
        market_id: String,
        long_token_amount: u128,
        short_token_amount: u128,
        min_mint: u128,
    ) -> Result<u128, Error> {
        let st = PerpetualDEXState::get_mut();
        if !st.markets.contains_key(&market_id) { return Err(Error::MarketNotFound); }

        // Convert token amounts to USD via oracle mid
        let mid = OracleModule::mid(&market_id)?; // market_id is also index_token key
        // USD = tokens * price  (price already in USD per 1 unit)
        let long_usd  = long_token_amount.saturating_mul(mid) / USD_SCALE;
        let short_usd = short_token_amount.saturating_mul(mid) / USD_SCALE;

        let pool = st.pool_amounts.get_mut(&market_id).ok_or(Error::MarketNotFound)?;
        let mt = st.market_tokens.get_mut(&market_id).ok_or(Error::MarketNotFound)?;

        // If first LP — simple sum
        let mint_amount = if mt.total_supply == 0 {
            long_usd.saturating_add(short_usd)
        } else {
            let total_pool_value = pool.long_liquidity_usd.saturating_add(pool.short_liquidity_usd);
            if total_pool_value == 0 { return Err(Error::InsufficientLiquidity); }
            let added_value = long_usd.saturating_add(short_usd);
            mt.total_supply.saturating_mul(added_value) / total_pool_value
        };

        if mint_amount < min_mint { return Err(Error::SlippageExceeded); }

        // Update pool in USD
        pool.long_liquidity_usd = pool.long_liquidity_usd.saturating_add(long_usd);
        pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_add(short_usd);

        // Mint market tokens to LP (balances updated by service layer; here just supply)
        mt.total_supply = mt.total_supply.saturating_add(mint_amount);

        Ok(mint_amount)
    }

    /// Remove liquidity (returns token amounts by converting USD share back via oracle)
    pub fn remove_liquidity(
        _lp: ActorId,
        market_id: String,
        market_token_amount: u128,
        min_long_out: u128,
        min_short_out: u128,
    ) -> Result<(u128, u128), Error> {
        let st = PerpetualDEXState::get_mut();
        if !st.markets.contains_key(&market_id) { return Err(Error::MarketNotFound); }

        let pool = st.pool_amounts.get_mut(&market_id).ok_or(Error::MarketNotFound)?;
        let mt = st.market_tokens.get_mut(&market_id).ok_or(Error::MarketNotFound)?;
        if mt.total_supply == 0 { return Err(Error::InsufficientLiquidity); }

        // Proportional share in USD
        let long_usd  = pool.long_liquidity_usd .saturating_mul(market_token_amount) / mt.total_supply;
        let short_usd = pool.short_liquidity_usd.saturating_mul(market_token_amount) / mt.total_supply;

        // Include claimable fees proportionally (USD)
        let fee_long_usd  = pool.claimable_fee_usd_long .saturating_mul(market_token_amount) / mt.total_supply;
        let fee_short_usd = pool.claimable_fee_usd_short.saturating_mul(market_token_amount) / mt.total_supply;

        let total_long_usd  = long_usd .saturating_add(fee_long_usd);
        let total_short_usd = short_usd.saturating_add(fee_short_usd);

        // Convert USD back to token amounts via current mid
        let mid = OracleModule::mid(&market_id)?;
        // tokens = USD * USD_SCALE / price
        let long_out_tokens  = total_long_usd .saturating_mul(USD_SCALE) / mid;
        let short_out_tokens = total_short_usd.saturating_mul(USD_SCALE) / mid;

        if long_out_tokens < min_long_out || short_out_tokens < min_short_out {
            return Err(Error::SlippageExceeded);
        }

        // Deduct from pool USD
        pool.long_liquidity_usd  = pool.long_liquidity_usd .saturating_sub(long_usd);
        pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_sub(short_usd);
        pool.claimable_fee_usd_long  = pool.claimable_fee_usd_long .saturating_sub(fee_long_usd);
        pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_sub(fee_short_usd);

        // Burn
        mt.total_supply = mt.total_supply.saturating_sub(market_token_amount);

        Ok((long_out_tokens, short_out_tokens))
    }

    /// Get pool amounts (USD)
    pub fn get_pool(market_id: &str) -> Result<PoolAmounts, Error> {
        let st = PerpetualDEXState::get();
        st.pool_amounts.get(market_id).cloned().ok_or(Error::MarketNotFound)
    }
}