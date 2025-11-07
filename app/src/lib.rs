#![no_std]
#![warn(clippy::new_without_default)]
#![allow(static_mut_refs)]

pub mod utils;
pub mod types;
pub mod events;
pub mod errors;
mod services;
mod modules;

use sails_rs::prelude::*;
use sails_rs::collections::HashMap;
use sails_rs::gstd::msg;

use types::*;

static mut STATE: Option<PerpetualDEXState> = None;

#[derive(Debug, Clone)]
pub struct PerpetualDEXState {
    pub markets: HashMap<String, Market>,
    pub market_configs: HashMap<String, MarketConfig>,
    pub pool_amounts: HashMap<String, PoolAmounts>,
    pub market_tokens: HashMap<String, MarketTokenInfo>,
    pub positions: HashMap<PositionKey, Position>,
    pub account_positions: HashMap<ActorId, Vec<PositionKey>>,
    pub deposit_requests: HashMap<RequestKey, DepositRequest>,
    pub withdrawal_requests: HashMap<RequestKey, WithdrawalRequest>,
    pub orders: HashMap<RequestKey, Order>,
    pub account_orders: HashMap<ActorId, Vec<RequestKey>>,
    pub oracle_prices: HashMap<String, Price>,
    pub admin: ActorId,
    pub keepers: Vec<ActorId>,
    pub liquidators: Vec<ActorId>,
    pub next_request_id: u64,
}

impl PerpetualDEXState {
    fn new(admin: ActorId) -> Self {
        Self {
            markets: HashMap::new(),
            market_configs: HashMap::new(),
            pool_amounts: HashMap::new(),
            market_tokens: HashMap::new(),
            positions: HashMap::new(),
            account_positions: HashMap::new(),
            deposit_requests: HashMap::new(),
            withdrawal_requests: HashMap::new(),
            orders: HashMap::new(),
            account_orders: HashMap::new(),
            oracle_prices: HashMap::new(),
            admin,
            keepers: Vec::new(),
            liquidators: Vec::new(),
            next_request_id: 1,
        }
    }

    pub fn get() -> &'static Self {
        unsafe { STATE.as_ref().expect("State not initialized") }
    }

    pub fn get_mut() -> &'static mut Self {
        unsafe { STATE.as_mut().expect("State not initialized") }
    }

    pub fn init(admin: ActorId) {
        unsafe { STATE = Some(Self::new(admin)); }
    }

    pub fn generate_request_key(&mut self) -> RequestKey {
        let key = H256::from_low_u64_be(self.next_request_id);
        self.next_request_id += 1;
        key
    }

    /// Delegate to utils::position_key for compatibility
    pub fn get_position_key(
        account: ActorId,
        market: &str,
        collateral_token: &str,
        is_long: bool,
    ) -> PositionKey {
        crate::utils::position_key(account, market, collateral_token, is_long)
    }

    pub fn is_keeper(&self, actor: ActorId) -> bool {
        self.keepers.contains(&actor)
    }
    pub fn is_liquidator(&self, actor: ActorId) -> bool {
        self.liquidators.contains(&actor)
    }
    pub fn is_admin(&self, actor: ActorId) -> bool {
        self.admin == actor
    }
}

use services::{ ExchangeService, ExecutorService, ViewService, AdminService };

pub struct PerpetualDEXProgram(());

#[program]
impl PerpetualDEXProgram {
    /// Admin is taken from msg::source()
    pub fn new() -> Self {
        let creator = msg::source();
        PerpetualDEXState::init(creator);
        Self(())
    }

    pub fn exchange(&self) -> ExchangeService { ExchangeService::new() }
    pub fn executor(&self) -> ExecutorService { ExecutorService::new() }
    pub fn view(&self) -> ViewService { ViewService::new() }
    pub fn admin(&self) -> AdminService { AdminService::new() }
}