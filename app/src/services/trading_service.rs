use sails_rs::{prelude::*, gstd::msg};
use crate::{types::*, errors::Error, modules::trading::TradingModule};

#[derive(Default)]
pub struct TradingService;

impl TradingService {
    pub fn new() -> Self {
        Self::default()
    }
}

#[service]
impl TradingService {
    #[export]
    pub fn create_order(&mut self, params: CreateOrderParams) -> Result<ExecutionResult, Error> {
        let caller = msg::source();
        TradingModule::create_order(caller, params)
    }

    #[export]
    pub fn market_open(
        &mut self,
        market: String,
        collateral_token: String,
        side: OrderSide,
        size_delta_usd: u128,
        collateral_amount: u128,
        acceptable_price: u128,
        execution_fee: u128,
    ) -> Result<ExecutionResult, Error> {
        let params = CreateOrderParams {
            market,
            collateral_token,
            order_type: OrderType::MarketIncrease,
            side,
            size_delta_usd,
            collateral_delta_amount: collateral_amount,
            trigger_price: acceptable_price,
            acceptable_price,
            execution_fee,
        };
        self.create_order(params)
    }

    #[export]
    pub fn market_close(
        &mut self,
        market: String,
        collateral_token: String,
        side: OrderSide,
        size_delta_usd: u128,
        collateral_amount: u128,
        acceptable_price: u128,
        execution_fee: u128,
    ) -> Result<ExecutionResult, Error> {
        let params = CreateOrderParams {
            market,
            collateral_token,
            order_type: OrderType::MarketDecrease,
            side,
            size_delta_usd,
            collateral_delta_amount: collateral_amount,
            trigger_price: acceptable_price,
            acceptable_price,
            execution_fee,
        };
        self.create_order(params)
    }

    #[export]
    pub fn set_stop_loss(
        &mut self,
        market: String,
        collateral_token: String,
        side: OrderSide,
        size_delta_usd: u128,
        trigger_price: u128,
        acceptable_price: u128,
        execution_fee: u128,
    ) -> Result<ExecutionResult, Error> {
        let params = CreateOrderParams {
            market,
            collateral_token,
            order_type: OrderType::StopLossDecrease,
            side,
            size_delta_usd,
            collateral_delta_amount: 0,
            trigger_price,
            acceptable_price,
            execution_fee,
        };
        self.create_order(params)
    }

    #[export]
    pub fn update_order(
        &mut self,
        key: RequestKey,
        params: UpdateOrderParams,
    ) -> Result<(), Error> {
        let caller = msg::source();
        TradingModule::update_order(caller, key, params)
    }

    #[export]
    pub fn cancel_order(&mut self, key: RequestKey) -> Result<(), Error> {
        let caller = msg::source();
        TradingModule::cancel_order(caller, key)
    }

    #[export]
    pub fn execute_saved_order(&mut self, key: RequestKey) -> Result<ExecutionResult, Error> {
        let executor = msg::source();
        TradingModule::execute_saved_order(executor, key)
    }

    #[export]
    pub fn get_order(&self, key: RequestKey) -> Result<Order, Error> {
        TradingModule::get_order(&key)
    }

    #[export]
    pub fn get_my_orders(&self) -> Vec<(RequestKey, Order)> {
        let caller = msg::source();
        TradingModule::get_account_orders(caller)
    }

    #[export]
    pub fn get_account_orders(&self, account: ActorId) -> Vec<(RequestKey, Order)> {
        TradingModule::get_account_orders(account)
    }

    #[export]
    pub fn get_pending_orders(&self) -> Vec<(RequestKey, Order)> {
        TradingModule::get_pending_orders()
    }
}