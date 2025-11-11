use sails_rs::prelude::*;
use crate::{
    types::*,
    utils,
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
        let mut st = PerpetualDEXState::get_mut();
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
        let mut st = PerpetualDEXState::get_mut();
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
        let mut st = PerpetualDEXState::get_mut();
        if !st.markets.contains_key(&market_id) { return Err(Error::MarketNotFound); }

        let market = st.markets.get(&market_id).ok_or(Error::MarketNotFound)?;
        let long_price  = OracleModule::mid(&market.long_token)?;
        let short_price = OracleModule::mid(&market.short_token)?;

        // USD = tokens * token_price / USD_SCALE
        let long_usd  = long_token_amount.saturating_mul(long_price)  / USD_SCALE;
        let short_usd = short_token_amount.saturating_mul(short_price) / USD_SCALE;

        // Immutable snapshots for calculations and checks
        let (pool_long_liq, pool_short_liq) = {
            let pool = st.pool_amounts.get(&market_id).ok_or(Error::MarketNotFound)?;
            (pool.long_liquidity_usd, pool.short_liquidity_usd)
        };
        let total_supply_snapshot = {
            let mt = st.market_tokens.get(&market_id).ok_or(Error::MarketNotFound)?;
            mt.total_supply
        };

        // Compute mint amount on snapshots
        let mint_amount = if total_supply_snapshot == 0 {
            long_usd.saturating_add(short_usd)
        } else {
            let total_pool_value = pool_long_liq.saturating_add(pool_short_liq);
            if total_pool_value == 0 { return Err(Error::InsufficientLiquidity); }
            let added_value = long_usd.saturating_add(short_usd);
            total_supply_snapshot.saturating_mul(added_value) / total_pool_value
        };

        if mint_amount < min_mint { return Err(Error::SlippageExceeded); }

        // Mutations: remove → work → insert (to avoid overlapping &mut borrows)
        let mut pool = st.pool_amounts.remove(&market_id).ok_or(Error::MarketNotFound)?;
        let mut mt   = st.market_tokens.remove(&market_id).ok_or(Error::MarketNotFound)?;

        // Update pool in USD
        pool.long_liquidity_usd  = pool.long_liquidity_usd.saturating_add(long_usd);
        pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_add(short_usd);

        // Update supply
        mt.total_supply = mt.total_supply.saturating_add(mint_amount);

        // Insert back
        st.pool_amounts.insert(market_id.clone(), pool);
        st.market_tokens.insert(market_id, mt);

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
        let mut st = PerpetualDEXState::get_mut();
        if !st.markets.contains_key(&market_id) { return Err(Error::MarketNotFound); }

        let market = st.markets.get(&market_id).ok_or(Error::MarketNotFound)?;
        let long_price  = OracleModule::mid(&market.long_token)?;
        let short_price = OracleModule::mid(&market.short_token)?;

        // Immutable snapshots for calculations and checks
        let (pool_long_liq, pool_short_liq, fee_long_total, fee_short_total) = {
            let pool = st.pool_amounts.get(&market_id).ok_or(Error::MarketNotFound)?;
            (
                pool.long_liquidity_usd,
                pool.short_liquidity_usd,
                pool.claimable_fee_usd_long,
                pool.claimable_fee_usd_short,
            )
        };
        let total_supply_snapshot = {
            let mt = st.market_tokens.get(&market_id).ok_or(Error::MarketNotFound)?;
            if mt.total_supply == 0 { return Err(Error::InsufficientLiquidity); }
            mt.total_supply
        };

        // Proportional share in USD (on snapshots)
        let long_usd  = pool_long_liq .saturating_mul(market_token_amount) / total_supply_snapshot;
        let short_usd = pool_short_liq.saturating_mul(market_token_amount) / total_supply_snapshot;

        // Include claimable fees proportionally (USD)
        let fee_long_usd  = fee_long_total .saturating_mul(market_token_amount) / total_supply_snapshot;
        let fee_short_usd = fee_short_total.saturating_mul(market_token_amount) / total_supply_snapshot;

        let total_long_usd  = long_usd .saturating_add(fee_long_usd);
        let total_short_usd = short_usd.saturating_add(fee_short_usd);

        // Convert USD back to token amounts via current token prices
        let long_out_tokens  = total_long_usd .saturating_mul(USD_SCALE) / long_price;
        let short_out_tokens = total_short_usd.saturating_mul(USD_SCALE) / short_price;

        if long_out_tokens < min_long_out || short_out_tokens < min_short_out {
            return Err(Error::SlippageExceeded);
        }

        // Mutations: remove → work → insert (to avoid overlapping &mut borrows)
        let mut pool = st.pool_amounts.remove(&market_id).ok_or(Error::MarketNotFound)?;
        let mut mt   = st.market_tokens.remove(&market_id).ok_or(Error::MarketNotFound)?;

        // Deduct from pool USD (only principal parts; fees also proportional)
        pool.long_liquidity_usd  = pool.long_liquidity_usd .saturating_sub(long_usd);
        pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_sub(short_usd);
        pool.claimable_fee_usd_long  = pool.claimable_fee_usd_long .saturating_sub(fee_long_usd);
        pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_sub(fee_short_usd);

        // Burn
        mt.total_supply = mt.total_supply.saturating_sub(market_token_amount);

        // Insert back
        st.pool_amounts.insert(market_id.clone(), pool);
        st.market_tokens.insert(market_id, mt);

        Ok((long_out_tokens, short_out_tokens))
    }

    /// Get pool amounts (USD)
    pub fn get_pool(market_id: &str) -> Result<PoolAmounts, Error> {
        let st = PerpetualDEXState::get();
        st.pool_amounts.get(market_id).cloned().ok_or(Error::MarketNotFound)
    }
}