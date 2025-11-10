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
    pub order_counter: u64,
    pub oracle: OracleState,
    pub admin: ActorId,
    pub keepers: Vec<ActorId>,
    pub liquidators: Vec<ActorId>,
    pub next_request_id: u64,
    pub balances: HashMap<ActorId, Usd>,
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
            order_counter: 0,
            oracle: OracleState::new(),
            admin,
            keepers: Vec::new(),
            liquidators: Vec::new(),
            next_request_id: 1,
            balances: HashMap::new(),
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
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&self.next_request_id.to_le_bytes());
        let key = H256::from(bytes);
        self.next_request_id += 1;
        key
    }
    
    /// Get position key for account/market/collateral/side combination
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

use services::{TradingService, ExecutorService, AdminService, OracleService, ViewService, WalletService, MarketService};

pub struct VaraPerpDexProgram(());

#[program]
impl VaraPerpDexProgram {
    /// Create new program instance. Admin is msg::source() (contract deployer)
    pub fn new() -> Self {
        let admin = msg::source();
        PerpetualDEXState::init(admin);
        Self(())
    }

    // Public services exposed to external callers
    pub fn trading(&self) -> TradingService { Default::default() }
    pub fn executor(&self) -> ExecutorService { Default::default() }
    pub fn view(&self) -> ViewService { Default::default() }
    pub fn admin(&self) -> AdminService { Default::default() }
    pub fn oracle(&self) -> OracleService { Default::default() }
    pub fn wallet(&self) -> WalletService { Default::default() }
    pub fn market(&self) -> MarketService { Default::default() }
