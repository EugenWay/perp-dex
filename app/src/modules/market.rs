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

        if !st.is_admin(caller) {
            return Err(Error::Unauthorized);
        }
        if st.markets.contains_key(&market_id) {
            return Err(Error::MarketAlreadyExists);
        }

        let market = Market {
            market_token,
            index_token,
            long_token,
            short_token,
        };

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

        if !st.is_admin(caller) {
            return Err(Error::Unauthorized);
        }
        if !st.markets.contains_key(&market_id) {
            return Err(Error::MarketNotFound);
        }

        st.market_configs.insert(market_id, config);
        Ok(())
    }

    /// Add liquidity (token amounts → converted to USD via oracle)
    pub fn add_liquidity(
        _lp: ActorId,
        market_id: String,
        long_token_amount: u128,
        short_token_amount: u128,
        min_mint: u128,
    ) -> Result<u128, Error> {

        // --- Immutable snapshot phase ---
        let (
            long_price,
            short_price,
            pool_long_liq,
            pool_short_liq,
            total_supply_snapshot,
        ) = {
            let st = PerpetualDEXState::get();

            if !st.markets.contains_key(&market_id) {
                return Err(Error::MarketNotFound);
            }

            let market = st.markets.get(&market_id).unwrap();

            let long_price = OracleModule::mid(&market.long_token)?;
            let short_price = OracleModule::mid(&market.short_token)?;

            let pool = st.pool_amounts.get(&market_id).unwrap();
            let pl = pool.long_liquidity_usd;
            let ps = pool.short_liquidity_usd;

            let mt = st.market_tokens.get(&market_id).unwrap();
            let ts = mt.total_supply;

            (long_price, short_price, pl, ps, ts)
        };

        // Convert to USD
        let long_usd =
            long_token_amount.saturating_mul(long_price) / USD_SCALE;
        let short_usd =
            short_token_amount.saturating_mul(short_price) / USD_SCALE;

        // TODO: enforce near-neutral LP deposits (optional future feature)
        // Example: ensure USD difference < X bps

        // Mint amount calculation
        let mint_amount = if total_supply_snapshot == 0 {
            long_usd.saturating_add(short_usd)
        } else {
            let total_pool_value = pool_long_liq.saturating_add(pool_short_liq);
            if total_pool_value == 0 {
                return Err(Error::InsufficientLiquidity);
            }
            let added_value = long_usd.saturating_add(short_usd);
            total_supply_snapshot
                .saturating_mul(added_value)
                / total_pool_value
        };

        if mint_amount < min_mint {
            return Err(Error::SlippageExceeded);
        }

        // --- Mutation phase ---
        let mut st = PerpetualDEXState::get_mut();

        let mut pool =
            st.pool_amounts.remove(&market_id).ok_or(Error::MarketNotFound)?;
        let mut mt =
            st.market_tokens.remove(&market_id).ok_or(Error::MarketNotFound)?;

        pool.long_liquidity_usd =
            pool.long_liquidity_usd.saturating_add(long_usd);
        pool.short_liquidity_usd =
            pool.short_liquidity_usd.saturating_add(short_usd);

        mt.total_supply = mt.total_supply.saturating_add(mint_amount);

        st.pool_amounts.insert(market_id.clone(), pool);
        st.market_tokens.insert(market_id, mt);

        Ok(mint_amount)
    }

    /// Remove liquidity (return token amounts, based on USD shares)
    pub fn remove_liquidity(
        _lp: ActorId,
        market_id: String,
        market_token_amount: u128,
        min_long_out: u128,
        min_short_out: u128,
    ) -> Result<(u128, u128), Error> {

        // --- Immutable snapshot phase ---
        let (
            long_price,
            short_price,
            pool_long_liq,
            pool_short_liq,
            fee_long_total,
            fee_short_total,
            total_supply_snapshot,
        ) = {
            let st = PerpetualDEXState::get();

            if !st.markets.contains_key(&market_id) {
                return Err(Error::MarketNotFound);
            }

            let market = st.markets.get(&market_id).unwrap();

            let long_price = OracleModule::mid(&market.long_token)?;
            let short_price = OracleModule::mid(&market.short_token)?;

            let pool = st.pool_amounts.get(&market_id).unwrap();
            let pl = pool.long_liquidity_usd;
            let ps = pool.short_liquidity_usd;
            let fl = pool.claimable_fee_usd_long;
            let fs = pool.claimable_fee_usd_short;

            let mt = st.market_tokens.get(&market_id).unwrap();
            if mt.total_supply == 0 {
                return Err(Error::InsufficientLiquidity);
            }

            (long_price, short_price, pl, ps, fl, fs, mt.total_supply)
        };

        let long_usd = pool_long_liq.saturating_mul(market_token_amount) / total_supply_snapshot;
        let short_usd = pool_short_liq.saturating_mul(market_token_amount) / total_supply_snapshot;

        let fee_long_usd = fee_long_total.saturating_mul(market_token_amount) / total_supply_snapshot;
        let fee_short_usd = fee_short_total.saturating_mul(market_token_amount)/ total_supply_snapshot;

        let total_long_usd = long_usd.saturating_add(fee_long_usd);
        let total_short_usd = short_usd.saturating_add(fee_short_usd);

        let long_out_tokens = total_long_usd.saturating_mul(USD_SCALE) / long_price;
        let short_out_tokens = total_short_usd.saturating_mul(USD_SCALE) / short_price;

        if long_out_tokens < min_long_out || short_out_tokens < min_short_out {
            return Err(Error::SlippageExceeded);
        }

        let mut st = PerpetualDEXState::get_mut();

        let mut pool = st.pool_amounts.remove(&market_id).ok_or(Error::MarketNotFound)?;
        let mut mt = st.market_tokens.remove(&market_id).ok_or(Error::MarketNotFound)?;

        pool.long_liquidity_usd = pool.long_liquidity_usd.saturating_sub(long_usd);
        pool.short_liquidity_usd = pool.short_liquidity_usd.saturating_sub(short_usd);

        pool.claimable_fee_usd_long = pool.claimable_fee_usd_long.saturating_sub(fee_long_usd);
        pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_sub(fee_short_usd);

        mt.total_supply = mt.total_supply.saturating_sub(market_token_amount);

        st.pool_amounts.insert(market_id.clone(), pool);
        st.market_tokens.insert(market_id, mt);

        Ok((long_out_tokens, short_out_tokens))
    }

    /// Get pool amounts (USD)
    pub fn get_pool(market_id: &str) -> Result<PoolAmounts, Error> {
        let st = PerpetualDEXState::get();
        st.pool_amounts
            .get(market_id)
            .cloned()
            .ok_or(Error::MarketNotFound)
    }
}