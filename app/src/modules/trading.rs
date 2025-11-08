use sails_rs::{prelude::*, gstd::exec};
use crate::{
    types::*,
    errors::Error,
    PerpetualDEXState,
    modules::oracle::OracleModule,
};
pub struct TradingModule;

impl TradingModule {
    /// Market orders execute immediately using current oracle price (MID).
    /// Limit/Stop orders check trigger price against MID - execute or save.
    pub fn create_order(
        caller: ActorId,
        params: CreateOrderParams,
    ) -> Result<ExecutionResult, Error> {
        let st = PerpetualDEXState::get_mut();

        // Validate market exists (config проверим позже в Commit 6)
        if !st.markets.contains_key(&params.market) {
            return Err(Error::MarketNotFound);
        }

        // Validate order parameters
        Self::validate_order_params(&params)?;

        // Oracle MID/SPREAD/AGE checks
        let mid = OracleModule::mid(&params.market)?;
        let _spread = OracleModule::spread(&params.market)?; // пригодится позже
        let last = OracleModule::last_update(&params.market).ok_or(Error::PriceStale)?;

        let now = exec::block_timestamp();
        let max_age = st.oracle.config.max_age_seconds;
        if now.saturating_sub(last) > max_age {
            return Err(Error::PriceStale);
        }

        // Route based on order type
        match params.order_type {
            OrderType::MarketIncrease | OrderType::MarketDecrease => {
                Self::execute_market_order(caller, params, mid)
            }
            OrderType::LimitIncrease | OrderType::LimitDecrease | OrderType::StopLossDecrease => {
                if Self::can_execute_limit_order(&params, mid) {
                    Self::execute_limit_order(caller, params, mid)
                } else {
                    Self::save_order(caller, params)
                }
            }
            _ => Err(Error::UnsupportedOrderType),
        }
    }

    fn execute_market_order(
        caller: ActorId,
        params: CreateOrderParams,
        execution_price: u128,
    ) -> Result<ExecutionResult, Error> {
        // Check acceptable price (slippage protection)
        Self::validate_execution_price(&params, execution_price)?;

        // Execute position change
        let position_key = Self::execute_position_change(
            caller,
            &params,
            execution_price,
        )?;

        Ok(ExecutionResult::Executed {
            position_key,
            execution_price,
        })
    }

    fn execute_limit_order(
        caller: ActorId,
        params: CreateOrderParams,
        execution_price: u128,
    ) -> Result<ExecutionResult, Error> {
        // Check acceptable price
        Self::validate_execution_price(&params, execution_price)?;

        // Execute position change
        let position_key = Self::execute_position_change(
            caller,
            &params,
            execution_price,
        )?;

        Ok(ExecutionResult::Executed {
            position_key,
            execution_price,
        })
    }

    fn save_order(
        caller: ActorId,
        params: CreateOrderParams,
    ) -> Result<ExecutionResult, Error> {
        let st = PerpetualDEXState::get_mut();

        let key = {
            st.generate_request_key()
        };

        // Create order
        let order = Order {
            key,
            account: caller,
            receiver: caller, // Default to sender
            callback_contract: None,
            market: params.market,
            collateral_token: params.collateral_token,
            order_type: params.order_type,
            size_delta_usd: params.size_delta_usd,
            collateral_delta_amount: params.collateral_delta_amount,
            trigger_price: params.trigger_price,
            acceptable_price: params.acceptable_price,
            min_output_amount: 0, // Not used for perp orders
            is_long: Self::order_side_to_bool(&params.side),
            is_frozen: false,
            status: OrderStatus::Created,
            execution_fee: params.execution_fee,
            callback_gas_limit: 0,
            created_at_block: exec::block_height(),
            created_at_time: exec::block_timestamp(),
            updated_at_block: exec::block_height(),
            updated_at_time: exec::block_timestamp(),
        };

        // Store order
        st.orders.insert(key, order);

        // Add to account orders
        st.account_orders
            .entry(caller)
            .or_insert_with(Vec::new)
            .push(key);

        Ok(ExecutionResult::Saved { order_key: key })
    }

    /// Execute saved order - can be called by anyone
    pub fn execute_saved_order(
        executor: ActorId,
        key: RequestKey,
    ) -> Result<ExecutionResult, Error> {
        let st = PerpetualDEXState::get_mut();

        // Get order
        let order = st.orders.get(&key).ok_or(Error::OrderNotFound)?.clone();

        // Check order status
        if order.status != OrderStatus::Created {
            return Err(Error::OrderAlreadyProcessed);
        }

        // Oracle MID + AGE
        let mid = OracleModule::mid(&order.market)?;
        let last = OracleModule::last_update(&order.market).ok_or(Error::PriceStale)?;
        let now = exec::block_timestamp();
        let max_age = st.oracle.config.max_age_seconds;
        if now.saturating_sub(last) > max_age {
            return Err(Error::PriceStale);
        }

        // Check if can execute
        let params = Self::order_to_params(&order);
        if !Self::can_execute_limit_order(&params, mid) {
            return Err(Error::OrderCannotBeExecutedYet);
        }

        // Validate execution price
        Self::validate_execution_price(&params, mid)?;

        // Execute position change
        let position_key = Self::execute_position_change(
            order.account,
            &params,
            mid,
        )?;

        // Update order status
        let order_mut = st.orders.get_mut(&key).unwrap();
        order_mut.status = OrderStatus::Executed;
        order_mut.updated_at_block = exec::block_height();
        order_mut.updated_at_time = exec::block_timestamp();

        // Pay execution fee to executor (TODO в отдельной итерации)

        Ok(ExecutionResult::Executed {
            position_key,
            execution_price: mid,
        })
    }

    pub fn update_order(
        caller: ActorId,
        key: RequestKey,
        params: UpdateOrderParams,
    ) -> Result<(), Error> {
        let st = PerpetualDEXState::get_mut();

        let order = st.orders.get_mut(&key).ok_or(Error::OrderNotFound)?;

        // Check ownership
        if order.account != caller {
            return Err(Error::Unauthorized);
        }

        // Check status
        if order.status != OrderStatus::Created {
            return Err(Error::OrderAlreadyProcessed);
        }

        // Update fields
        if let Some(size) = params.size_delta_usd {
            order.size_delta_usd = size;
        }
        if let Some(trigger) = params.trigger_price {
            order.trigger_price = trigger;
        }
        if let Some(acceptable) = params.acceptable_price {
            order.acceptable_price = acceptable;
        }

        order.updated_at_block = exec::block_height();
        order.updated_at_time = exec::block_timestamp();

        Ok(())
    }

    pub fn cancel_order(caller: ActorId, key: RequestKey) -> Result<(), Error> {
        let st = PerpetualDEXState::get_mut();

        let order = st.orders.get_mut(&key).ok_or(Error::OrderNotFound)?;

        // Check ownership
        if order.account != caller {
            return Err(Error::Unauthorized);
        }

        // Check status
        if order.status != OrderStatus::Created {
            return Err(Error::OrderAlreadyProcessed);
        }

        // Update status
        order.status = OrderStatus::Cancelled;
        order.updated_at_block = exec::block_height();
        order.updated_at_time = exec::block_timestamp();

        Ok(())
    }

    fn validate_order_params(params: &CreateOrderParams) -> Result<(), Error> {
        if params.size_delta_usd == 0 {
            return Err(Error::InvalidOrderSize);
        }

        if params.trigger_price == 0 || params.acceptable_price == 0 {
            return Err(Error::InvalidPrice);
        }

        // Increase orders need collateral
        if matches!(
            params.order_type,
            OrderType::MarketIncrease | OrderType::LimitIncrease
        ) {
            if params.collateral_delta_amount == 0 {
                return Err(Error::InvalidCollateralAmount);
            }
        }

        Ok(())
    }

    fn can_execute_limit_order(params: &CreateOrderParams, current_price: u128) -> bool {
        let is_long = Self::order_side_to_bool(&params.side);

        match params.order_type {
            OrderType::LimitIncrease => {
                // Long: buy when price <= trigger
                // Short: sell when price >= trigger
                if is_long {
                    current_price <= params.trigger_price
                } else {
                    current_price >= params.trigger_price
                }
            }
            OrderType::LimitDecrease => {
                // Take profit
                // Long: sell when price >= trigger
                // Short: buy when price <= trigger
                if is_long {
                    current_price >= params.trigger_price
                } else {
                    current_price <= params.trigger_price
                }
            }
            OrderType::StopLossDecrease => {
                // Stop loss
                // Long: sell when price <= trigger
                // Short: buy when price >= trigger
                if is_long {
                    current_price <= params.trigger_price
                } else {
                    current_price >= params.trigger_price
                }
            }
            _ => false,
        }
    }

    fn validate_execution_price(
        params: &CreateOrderParams,
        execution_price: u128,
    ) -> Result<(), Error> {
        let is_long = Self::order_side_to_bool(&params.side);
        let is_increase = matches!(
            params.order_type,
            OrderType::MarketIncrease | OrderType::LimitIncrease
        );

        // Long/Increase: MID <= acceptable
        // Long/Decrease: MID >= acceptable
        // Short/Increase: MID >= acceptable
        // Short/Decrease: MID <= acceptable
        let ok = match (is_long, is_increase) {
            (true,  true)  => execution_price <= params.acceptable_price,
            (true,  false) => execution_price >= params.acceptable_price,
            (false, true)  => execution_price >= params.acceptable_price,
            (false, false) => execution_price <= params.acceptable_price,
        };
        if !ok { return Err(Error::PriceNotAcceptable); }
        Ok(())
    }

    fn execute_position_change(
        caller: ActorId,
        params: &CreateOrderParams,
        price: u128,
    ) -> Result<PositionKey, Error> {
        use crate::modules::position::PositionModule;

        let is_long = Self::order_side_to_bool(&params.side);

        match params.order_type {
            OrderType::MarketIncrease | OrderType::LimitIncrease => {
                PositionModule::increase_position(
                    caller,
                    params.market.clone(),
                    params.collateral_token.clone(),
                    is_long,
                    params.size_delta_usd,
                    params.collateral_delta_amount,
                    price,
                )
            }
            OrderType::MarketDecrease | OrderType::LimitDecrease | OrderType::StopLossDecrease => {
                PositionModule::decrease_position(
                    caller,
                    params.market.clone(),
                    params.collateral_token.clone(),
                    is_long,
                    params.size_delta_usd,
                    params.collateral_delta_amount,
                    price,
                )
            }
            _ => Err(Error::UnsupportedOrderType),
        }
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_order(key: &RequestKey) -> Result<Order, Error> {
        let st = PerpetualDEXState::get();
        st.orders.get(key).cloned().ok_or(Error::OrderNotFound)
    }

    pub fn get_account_orders(account: ActorId) -> Vec<(RequestKey, Order)> {
        let st = PerpetualDEXState::get();

        st.account_orders
            .get(&account)
            .map(|keys| {
                keys.iter()
                    .filter_map(|key| {
                        st.orders.get(key).map(|o| (*key, o.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_pending_orders() -> Vec<(RequestKey, Order)> {
        let st = PerpetualDEXState::get();

        st.orders
            .iter()
            .filter(|(_, o)| o.status == OrderStatus::Created)
            .map(|(k, o)| (*k, o.clone()))
            .collect()
    }

    // ========================================================================
    // HELPER FUNCTIONS
    // ========================================================================

    fn order_side_to_bool(side: &OrderSide) -> bool {
        matches!(side, OrderSide::Long)
    }

    fn bool_to_order_side(is_long: bool) -> OrderSide {
        if is_long {
            OrderSide::Long
        } else {
            OrderSide::Short
        }
    }

    fn order_to_params(order: &Order) -> CreateOrderParams {
        CreateOrderParams {
            market: order.market.clone(),
            collateral_token: order.collateral_token.clone(),
            order_type: order.order_type.clone(),
            side: Self::bool_to_order_side(order.is_long),
            size_delta_usd: order.size_delta_usd,
            collateral_delta_amount: order.collateral_delta_amount,
            trigger_price: order.trigger_price,
            acceptable_price: order.acceptable_price,
            execution_fee: order.execution_fee,
        }
    }
}