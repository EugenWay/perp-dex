use sails_rs::prelude::*;
use crate::{
    modules::oracle::{OracleModule, SignedPrice},
    errors::Error,
    types::*,
};

/// Public service for oracle price updates
/// Anyone can submit signed prices (keepers, bots, etc)
#[derive(Default)]
pub struct OracleService;  // ← Unit struct, no fields

#[service]
impl OracleService {
    #[export]
    pub fn set_prices(&mut self, batch: Vec<SignedPrice>) -> Result<(), Error> {
        OracleModule::set_prices(batch)
    }

    /// Get current price for a token
    #[export]
    pub fn get_price(&self, token: String) -> Result<Price, Error> {
        OracleModule::get_price(&token)
    }

    /// Get mid price (average of min/max)
    #[export]
    pub fn get_mid_price(&self, token: String) -> Result<u128, Error> {
        OracleModule::mid(&token)
    }

    /// Get price spread (difference between max and min)
    #[export]
    pub fn get_spread(&self, token: String) -> Result<u128, Error> {
        OracleModule::spread(&token)
    }

    /// Get last update timestamp
    #[export]
    pub fn last_update(&self, token: String) -> Option<u64> {
        OracleModule::last_update(&token)
    }

    /// Get last signer who updated the price
    #[export]
    pub fn last_signer(&self, token: String) -> Option<ActorId> {
        OracleModule::last_signer(&token)
    }
}