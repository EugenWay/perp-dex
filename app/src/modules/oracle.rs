use sails_rs::{prelude::*, gstd::exec};
use sails_rs::collections::BTreeMap;
use crate::{types::*, errors::Error, PerpetualDEXState, utils};

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct SignedPrice {
    pub token: String,
    pub price: Price,
    pub timestamp: u64,
    pub signer: ActorId,
    pub signature: Vec<u8>,
}

impl OracleState {
    pub fn new() -> Self {
        Self {
            prices: BTreeMap::new(),
            timestamps: BTreeMap::new(),
            last_signer: BTreeMap::new(),
            config: OracleConfig { max_age_seconds: 60 },
        }
    }

    pub fn with_config(config: OracleConfig) -> Self {
        Self {
            prices: BTreeMap::new(),
            timestamps: BTreeMap::new(),
            last_signer: BTreeMap::new(),
            config,
        }
    }
}

pub struct OracleModule;

impl OracleModule {
    pub fn set_prices(batch: Vec<SignedPrice>) -> Result<(), Error> {
        let mut st = PerpetualDEXState::get_mut();
        let now = exec::block_timestamp();

        for sp in batch {
            if now.saturating_sub(sp.timestamp) > st.oracle.config.max_age_seconds {
                return Err(Error::PriceStale);
            }
            if !utils::verify_signature(&sp.token, &sp.price, sp.timestamp, &sp.signer, &sp.signature) {
                return Err(Error::InvalidOracleSignature);
            }
            st.oracle.prices.insert(sp.token.clone(), sp.price);
            st.oracle.timestamps.insert(sp.token.clone(), sp.timestamp);
            st.oracle.last_signer.insert(sp.token, sp.signer);
        }
        Ok(())
    }

    pub fn get_price(token: &str) -> Result<Price, Error> {
        let st = PerpetualDEXState::get();
        st.oracle.prices.get(token).cloned().ok_or(Error::PriceNotAvailable)
    }

    pub fn mid(token: &str) -> Result<u128, Error> {
        let p = Self::get_price(token)?;
        Ok((p.min + p.max) / 2)
    }

    pub fn spread(token: &str) -> Result<u128, Error> {
        let p = Self::get_price(token)?;
        Ok(p.max.saturating_sub(p.min))
    }

    pub fn ensure_fresh(token: &str) -> Result<(), Error> {
        let st = PerpetualDEXState::get();
        let ts = st.oracle.timestamps.get(token).ok_or(Error::PriceNotAvailable)?;
        let now = exec::block_timestamp();
        if now.saturating_sub(*ts) > st.oracle.config.max_age_seconds {
            return Err(Error::PriceStale);
        }
        Ok(())
    }

    pub fn last_update(token: &str) -> Option<u64> {
        let st = PerpetualDEXState::get();
        st.oracle.timestamps.get(token).cloned()
    }

    pub fn last_signer(token: &str) -> Option<ActorId> {
        let st = PerpetualDEXState::get();
        st.oracle.last_signer.get(token).cloned()
    }

    pub fn set_config(caller: ActorId, cfg: OracleConfig) -> Result<(), Error> {
        let mut st = PerpetualDEXState::get_mut();
        if !st.is_admin(caller) {
            return Err(Error::Unauthorized);
        }
        st.oracle.config = cfg;
        Ok(())
    }
}