use sails_rs::{
    prelude::*,
    gstd::msg,
};
use crate::{
    types::*,
    errors::Error,
    modules::trading::TradingModule,
};

/// Trading Service - Public API for order management
#[derive(Default)]
pub struct TradingService;

impl TradingService {
    pub fn new() -> Self {
        Self::default()
    }
}

#[service]
impl TradingService {
    // ========================================================================
    // ORDER CREATION
    // ========================================================================
    
    /// Create order with instant execution when possible
    /// 
    /// Market orders execute immediately at current oracle price.
    /// Limit orders check trigger price and execute or save.
    /// 
    /// # Events
    /// - OrderExecuted: order executed immediately
    /// - OrderCreated: order saved for later
    pub fn create_order(&mut self, params: CreateOrderParams) -> Result<ExecutionResult, Error> {
        let caller = msg::source();
        let result = TradingModule::create_order(caller, params.clone())?;
        
        // Emit appropriate event
        match &result {
            ExecutionResult::Executed { position_key, execution_price } => {
                self.notify_on(OrderExecuted {
                    account: caller,
                    market: params.market,
                    side: params.side,
                    size_delta: params.size_delta_usd,
                    execution_price: *execution_price,
                    position_key: *position_key,
                })
                .expect("Notification failed");
            }
            ExecutionResult::Saved { order_key } => {
                self.notify_on(OrderCreated {
                    account: caller,
                    order_key: *order_key,
                    market: params.market,
                    order_type: params.order_type,
                    side: params.side,
                    size_delta: params.size_delta_usd,
                    trigger_price: params.trigger_price,
                })
                .expect("Notification failed");
            }
        }
        
        Ok(result)
    }
    
    // ========================================================================
    // CONVENIENCE METHODS
    // ========================================================================
    
    /// Quick market open - open or add to position
    pub fn market_open(
        &mut self,
        market: String,
        collateral_token: String,
        side: OrderSide,
        size_delta_usd: u128,
        collateral_amount: u128,
        max_price: u128,
        execution_fee: u128,
    ) -> Result<ExecutionResult, Error> {
        let params = CreateOrderParams::market_increase(
            market,
            collateral_token,
            side,
            size_delta_usd,
            collateral_amount,
            max_price,
            execution_fee,
        );
        
        self.create_order(params)
    }
    
    /// Quick market close - close or reduce position
    pub fn market_close(
        &mut self,
        market: String,
        collateral_token: String,
        side: OrderSide,
        size_delta_usd: u128,
        collateral_amount: u128,
        min_price: u128,
        execution_fee: u128,
    ) -> Result<ExecutionResult, Error> {
        let params = CreateOrderParams::market_decrease(
            market,
            collateral_token,
            side,
            size_delta_usd,
            collateral_amount,
            min_price,
            execution_fee,
        );
        
        self.create_order(params)
    }
    
    /// Set stop-loss for position
    pub fn set_stop_loss(
        &mut self,
        market: String,
        collateral_token: String,
        side: OrderSide,
        size_delta_usd: u128,
        trigger_price: u128,
        min_price: u128,
        execution_fee: u128,
    ) -> Result<ExecutionResult, Error> {
        let params = CreateOrderParams::stop_loss(
            market,
            collateral_token,
            side,
            size_delta_usd,
            trigger_price,
            min_price,
            execution_fee,
        );
        
        self.create_order(params)
    }
    
    // ========================================================================
    // ORDER MANAGEMENT
    // ========================================================================
    
    /// Update saved order parameters
    pub fn update_order(
        &mut self,
        key: RequestKey,
        params: UpdateOrderParams,
    ) -> Result<(), Error> {
        let caller = msg::source();
        TradingModule::update_order(caller, key, params)?;
        
        self.notify_on(OrderUpdated {
            account: caller,
            order_key: key,
        })
        .expect("Notification failed");
        
        Ok(())
    }
    
    /// Cancel saved order
    pub fn cancel_order(&mut self, key: RequestKey) -> Result<(), Error> {
        let caller = msg::source();
        TradingModule::cancel_order(caller, key)?;
        
        self.notify_on(OrderCancelled {
            account: caller,
            order_key: key,
        })
        .expect("Notification failed");
        
        Ok(())
    }
    
    /// Execute saved order - can be called by anyone
    /// 
    /// Executor receives execution fee if not self-executing.
    pub fn execute_saved_order(&mut self, key: RequestKey) -> Result<ExecutionResult, Error> {
        let executor = msg::source();
        let result = TradingModule::execute_saved_order(executor, key)?;
        
        if let ExecutionResult::Executed { position_key, execution_price } = &result {
            let order = TradingModule::get_order(&key)?;
            
            self.notify_on(OrderExecuted {
                account: order.account,
                market: order.market,
                side: if order.is_long { OrderSide::Long } else { OrderSide::Short },
                size_delta: order.size_delta_usd,
                execution_price: *execution_price,
                position_key: *position_key,
            })
            .expect("Notification failed");
        }
        
        Ok(result)
    }
    
    // ========================================================================
    // QUERIES
    // ========================================================================
    
    /// Get order by key
    pub fn get_order(&self, key: RequestKey) -> Result<Order, Error> {
        TradingModule::get_order(&key)
    }
    
    /// Get all my orders
    pub fn get_my_orders(&self) -> Vec<(RequestKey, Order)> {
        let caller = msg::source();
        TradingModule::get_account_orders(caller)
    }
    
    /// Get orders for specific account
    pub fn get_account_orders(&self, account: ActorId) -> Vec<(RequestKey, Order)> {
        TradingModule::get_account_orders(account)
    }
    
    /// Get all pending orders
    pub fn get_pending_orders(&self) -> Vec<(RequestKey, Order)> {
        TradingModule::get_pending_orders()
    }
}

// ============================================================================
// EVENTS
// ============================================================================

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum TradingEvent {
    OrderCreated(OrderCreated),
    OrderExecuted(OrderExecuted),
    OrderUpdated(OrderUpdated),
    OrderCancelled(OrderCancelled),
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OrderCreated {
    pub account: ActorId,
    pub order_key: RequestKey,
    pub market: String,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub size_delta: u128,
    pub trigger_price: u128,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OrderExecuted {
    pub account: ActorId,
    pub market: String,
    pub side: OrderSide,
    pub size_delta: u128,
    pub execution_price: u128,
    pub position_key: PositionKey,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OrderUpdated {
    pub account: ActorId,
    pub order_key: RequestKey,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OrderCancelled {
    pub account: ActorId,
    pub order_key: RequestKey,
}