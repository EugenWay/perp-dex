use crate::{PerpetualDEXState, errors::Error, modules::oracle::OracleModule, types::*};
use sails_rs::prelude::*;

pub struct MarketModule;

impl MarketModule {
    /// Create a new market (admin only).
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

    /// Update market configuration (admin only).
    pub fn set_market_config(caller: ActorId, market_id: String, config: MarketConfig) -> Result<(), Error> {
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

    /// Add liquidity (LP deposits tokens → converted to USD, LP tokens minted).
    /// Funds from LPs go ONLY into `liquidity_usd`.
    pub fn add_liquidity(
        lp: ActorId,
        market_id: String,
        long_token_amount: u128,
        short_token_amount: u128,
        min_mint: u128,
    ) -> Result<u128, Error> {
        let (long_price, short_price, pool_liq_snapshot, total_supply_snapshot) = {
            let st = PerpetualDEXState::get();

            if !st.markets.contains_key(&market_id) {
                return Err(Error::MarketNotFound);
            }

            let market = st.markets.get(&market_id).unwrap();

            let long_price = OracleModule::mid(&market.long_token)?;
            let short_price = OracleModule::mid(&market.short_token)?;

            let pool = st.pool_amounts.get(&market_id).unwrap();
            let pl = pool.liquidity_usd;

            let mt = st.market_tokens.get(&market_id).unwrap();
            let ts = mt.total_supply;

            (long_price, short_price, pl, ts)
        };

        // Convert deposits to USD
        let long_usd = long_token_amount.saturating_mul(long_price) / USD_SCALE;
        let short_usd = short_token_amount.saturating_mul(short_price) / USD_SCALE;

        let added_value = long_usd.saturating_add(short_usd);

        let mint_amount = if total_supply_snapshot == 0 {
            // First deposit → LP supply = pool USD value
            added_value
        } else {
            // Pro-rata share based on current pool value
            let total_pool_value = pool_liq_snapshot;
            if total_pool_value == 0 {
                return Err(Error::InsufficientLiquidity);
            }
            total_supply_snapshot.saturating_mul(added_value) / total_pool_value
        };

        if mint_amount < min_mint {
            return Err(Error::SlippageExceeded);
        }

        let mut st = PerpetualDEXState::get_mut();

        let mut pool = st.pool_amounts.remove(&market_id).ok_or(Error::MarketNotFound)?;
        let mut mt = st.market_tokens.remove(&market_id).ok_or(Error::MarketNotFound)?;

        // LP funds go into shared liquidity
        pool.liquidity_usd = pool.liquidity_usd.saturating_add(long_usd).saturating_add(short_usd);

        // Mint LP tokens
        mt.total_supply = mt.total_supply.saturating_add(mint_amount);

        let entry = mt.balances.iter_mut().find(|(a, _)| *a == lp);
        if let Some(e) = entry {
            e.1 = e.1.saturating_add(mint_amount);
        } else {
            mt.balances.push((lp, mint_amount));
        }

        st.pool_amounts.insert(market_id.clone(), pool);
        st.market_tokens.insert(market_id, mt);

        Ok(mint_amount)
    }

    /// Remove liquidity (LP burns tokens → receives tokens back).
    /// Funds are taken ONLY from `liquidity_usd` (plus pro-rata share of fees).
    pub fn remove_liquidity(
        lp: ActorId,
        market_id: String,
        market_token_amount: u128,
        min_long_out: u128,
        min_short_out: u128,
    ) -> Result<(u128, u128), Error> {
        let (long_price, short_price, pool_liq, fee_long_total, fee_short_total, total_supply_snapshot) = {
            let st = PerpetualDEXState::get();

            if !st.markets.contains_key(&market_id) {
                return Err(Error::MarketNotFound);
            }

            let market = st.markets.get(&market_id).unwrap();

            let long_price = OracleModule::mid(&market.long_token)?;
            let short_price = OracleModule::mid(&market.short_token)?;

            let pool = st.pool_amounts.get(&market_id).unwrap();
            let pl = pool.liquidity_usd;
            let fl = pool.claimable_fee_usd_long;
            let fs = pool.claimable_fee_usd_short;

            let mt = st.market_tokens.get(&market_id).unwrap();
            if mt.total_supply == 0 {
                return Err(Error::InsufficientLiquidity);
            }

            (long_price, short_price, pl, fl, fs, mt.total_supply)
        };

        // Pro-rata share of pool liquidity
        let liq_usd = pool_liq.saturating_mul(market_token_amount) / total_supply_snapshot;

        // Split base liquidity between long/short tokens by current prices
        let price_sum = long_price.saturating_add(short_price);
        if price_sum == 0 {
            return Err(Error::InvalidPrice);
        }

        let long_usd_base = liq_usd.saturating_mul(long_price) / price_sum;
        let short_usd_base = liq_usd.saturating_sub(long_usd_base);

        // Pro-rata share of accumulated fees
        let fee_long_usd = fee_long_total.saturating_mul(market_token_amount) / total_supply_snapshot;
        let fee_short_usd = fee_short_total.saturating_mul(market_token_amount) / total_supply_snapshot;

        let total_long_usd = long_usd_base.saturating_add(fee_long_usd);
        let total_short_usd = short_usd_base.saturating_add(fee_short_usd);

        // Convert USD back to tokens
        let long_out_tokens = total_long_usd.saturating_mul(USD_SCALE) / long_price;
        let short_out_tokens = total_short_usd.saturating_mul(USD_SCALE) / short_price;

        if long_out_tokens < min_long_out || short_out_tokens < min_short_out {
            return Err(Error::SlippageExceeded);
        }

        let mut st = PerpetualDEXState::get_mut();

        let mut pool = st.pool_amounts.remove(&market_id).ok_or(Error::MarketNotFound)?;
        let mut mt = st.market_tokens.remove(&market_id).ok_or(Error::MarketNotFound)?;

        // Burn LP balance
        {
            let bal = mt
                .balances
                .iter_mut()
                .find(|(a, _)| *a == lp)
                .ok_or(Error::InsufficientMarketTokens)?;
            if bal.1 < market_token_amount {
                return Err(Error::InsufficientMarketTokens);
            }
            bal.1 = bal.1.saturating_sub(market_token_amount);
        }

        // Decrease shared liquidity and fee buckets
        pool.liquidity_usd = pool.liquidity_usd.saturating_sub(liq_usd);

        pool.claimable_fee_usd_long = pool.claimable_fee_usd_long.saturating_sub(fee_long_usd);
        pool.claimable_fee_usd_short = pool.claimable_fee_usd_short.saturating_sub(fee_short_usd);

        mt.total_supply = mt.total_supply.saturating_sub(market_token_amount);

        st.pool_amounts.insert(market_id.clone(), pool);
        st.market_tokens.insert(market_id, mt);

        Ok((long_out_tokens, short_out_tokens))
    }

    /// Get pool amounts (USD).
    pub fn get_pool(market_id: &str) -> Result<PoolAmounts, Error> {
        let st = PerpetualDEXState::get();
        st.pool_amounts.get(market_id).cloned().ok_or(Error::MarketNotFound)
    }
}
