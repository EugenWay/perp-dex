use crate::{
    PerpetualDEXState,
    errors::Error,
    modules::{oracle::OracleModule, position::PositionModule, pricing::PricingModule, risk::RiskModule},
    types::*,
    utils,
};
use sails_rs::{gstd::exec, prelude::*};

pub struct TradingModule;

impl TradingModule {
    pub fn create_order(caller: ActorId, params: CreateOrderParams) -> Result<ExecutionResult, Error> {
        let st = PerpetualDEXState::get();
        if !st.markets.contains_key(&params.market) {
            return Err(Error::MarketNotFound);
        }
        if !st.market_configs.contains_key(&params.market) {
            return Err(Error::MarketNotFound);
        }

        Self::validate_order_params(&params)?;

        let price_key = utils::price_key(&params.market);
        OracleModule::ensure_fresh(&price_key)?;

        match params.order_type {
            OrderType::MarketIncrease | OrderType::MarketDecrease => Self::execute_market_order(caller, params),
            OrderType::LimitIncrease | OrderType::LimitDecrease | OrderType::StopLossDecrease => {
                let mid = OracleModule::mid(&price_key)?;
                if Self::can_execute_limit_order(&params, mid) {
                    Self::execute_limit_order(caller, params)
                } else {
                    Self::save_order(caller, params)
                }
            }
            _ => Err(Error::UnsupportedOrderType),
        }
    }

    fn execute_market_order(caller: ActorId, params: CreateOrderParams) -> Result<ExecutionResult, Error> {
        let quote = match params.order_type {
            OrderType::MarketIncrease => {
                PricingModule::quote_increase(&params.market, &params.side, params.size_delta_usd)?
            }
            OrderType::MarketDecrease => {
                PricingModule::quote_decrease(&params.market, &params.side, params.size_delta_usd)?
            }
            _ => return Err(Error::UnsupportedOrderType),
        };

        Self::validate_execution_price(&params, quote.execution_price)?;
        let key = Self::execute_position_change(caller, &params, quote.execution_price)?;
        Ok(ExecutionResult::Executed {
            position_key: key,
            execution_price: quote.execution_price,
        })
    }

    fn execute_limit_order(caller: ActorId, params: CreateOrderParams) -> Result<ExecutionResult, Error> {
        let quote = match params.order_type {
            OrderType::LimitIncrease => {
                PricingModule::quote_increase(&params.market, &params.side, params.size_delta_usd)?
            }
            OrderType::LimitDecrease | OrderType::StopLossDecrease => {
                PricingModule::quote_decrease(&params.market, &params.side, params.size_delta_usd)?
            }
            _ => return Err(Error::UnsupportedOrderType),
        };

        Self::validate_execution_price(&params, quote.execution_price)?;
        let key = Self::execute_position_change(caller, &params, quote.execution_price)?;
        Ok(ExecutionResult::Executed {
            position_key: key,
            execution_price: quote.execution_price,
        })
    }

    fn save_order(caller: ActorId, params: CreateOrderParams) -> Result<ExecutionResult, Error> {
        let now_block = exec::block_height();
        let now_time = exec::block_timestamp();

        let mut st = PerpetualDEXState::get_mut();
        let key = st.generate_request_key();

        let order = Order {
            key,
            account: caller,
            receiver: caller,
            callback_contract: None,
            market: params.market,
            collateral_token: params.collateral_token,
            order_type: params.order_type,
            size_delta_usd: params.size_delta_usd,
            collateral_delta_amount: params.collateral_delta_amount,
            trigger_price: params.trigger_price,
            acceptable_price: params.acceptable_price,
            min_output_amount: 0,
            is_long: matches!(params.side, OrderSide::Long),
            is_frozen: false,
            status: OrderStatus::Created,
            execution_fee: params.execution_fee,
            callback_gas_limit: 0,
            created_at_block: now_block,
            created_at_time: now_time,
            updated_at_block: now_block,
            updated_at_time: now_time,
        };

        st.orders.insert(key, order);
        st.account_orders.entry(caller).or_insert_with(Vec::new).push(key);

        Ok(ExecutionResult::Saved { order_key: key })
    }

    pub fn execute_saved_order(executor: ActorId, key: RequestKey) -> Result<ExecutionResult, Error> {
        // --- Snapshot phase (immutable state) ---
        let (order, params, execution_price) = {
            let st = PerpetualDEXState::get();

            let order = st.orders.get(&key).cloned().ok_or(Error::OrderNotFound)?;

            if order.status != OrderStatus::Created {
                return Err(Error::OrderAlreadyProcessed);
            }

            let price_key = utils::price_key(&order.market);
            OracleModule::ensure_fresh(&price_key)?;
            let mid = OracleModule::mid(&price_key)?;

            let params = Self::order_to_params(&order);
            if !Self::can_execute_limit_order(&params, mid) {
                return Err(Error::OrderCannotBeExecutedYet);
            }

            let quote = match order.order_type {
                OrderType::LimitIncrease => {
                    PricingModule::quote_increase(&order.market, &params.side, params.size_delta_usd)?
                }
                OrderType::LimitDecrease | OrderType::StopLossDecrease => {
                    PricingModule::quote_decrease(&order.market, &params.side, params.size_delta_usd)?
                }
                _ => return Err(Error::UnsupportedOrderType),
            };

            Self::validate_execution_price(&params, quote.execution_price)?;

            (order, params, quote.execution_price)
        };

        // --- Position / pool mutation (handled inside modules) ---
        let position_key = Self::execute_position_change(order.account, &params, execution_price)?;

        // --- Final mutation: execution fee + order status ---
        {
            let now_block = exec::block_height();
            let now_time = exec::block_timestamp();
            let mut st = PerpetualDEXState::get_mut();

            if executor != order.account && order.execution_fee > 0 {
                if let Some(b) = st.balances.get_mut(&order.account) {
                    if *b >= order.execution_fee {
                        *b = b.saturating_sub(order.execution_fee);
                        let exb = st.balances.entry(executor).or_insert(0);
                        *exb = exb.saturating_add(order.execution_fee);
                    }
                }
            }

            if let Some(om) = st.orders.get_mut(&key) {
                // Extra safety: ensure still Created
                if om.status != OrderStatus::Created {
                    return Err(Error::OrderAlreadyProcessed);
                }
                om.status = OrderStatus::Executed;
                om.updated_at_block = now_block;
                om.updated_at_time = now_time;
            } else {
                return Err(Error::OrderNotFound);
            }
        }

        Ok(ExecutionResult::Executed {
            position_key,
            execution_price,
        })
    }

    pub fn update_order(caller: ActorId, key: RequestKey, params: UpdateOrderParams) -> Result<(), Error> {
        let now_block = exec::block_height();
        let now_time = exec::block_timestamp();

        let mut st = PerpetualDEXState::get_mut();
        let o = st.orders.get_mut(&key).ok_or(Error::OrderNotFound)?;
        if o.account != caller {
            return Err(Error::Unauthorized);
        }
        if o.status != OrderStatus::Created {
            return Err(Error::OrderAlreadyProcessed);
        }

        if let Some(v) = params.size_delta_usd {
            o.size_delta_usd = v;
        }
        if let Some(v) = params.trigger_price {
            o.trigger_price = v;
        }
        if let Some(v) = params.acceptable_price {
            o.acceptable_price = v;
        }

        o.updated_at_block = now_block;
        o.updated_at_time = now_time;
        Ok(())
    }

    pub fn cancel_order(caller: ActorId, key: RequestKey) -> Result<(), Error> {
        let now_block = exec::block_height();
        let now_time = exec::block_timestamp();

        let mut st = PerpetualDEXState::get_mut();
        let o = st.orders.get_mut(&key).ok_or(Error::OrderNotFound)?;
        if o.account != caller {
            return Err(Error::Unauthorized);
        }
        if o.status != OrderStatus::Created {
            return Err(Error::OrderAlreadyProcessed);
        }
        o.status = OrderStatus::Cancelled;
        o.updated_at_block = now_block;
        o.updated_at_time = now_time;
        Ok(())
    }

    fn validate_order_params(p: &CreateOrderParams) -> Result<(), Error> {
        if p.size_delta_usd == 0 {
            return Err(Error::InvalidOrderSize);
        }
        if p.acceptable_price == 0 {
            return Err(Error::InvalidPrice);
        }
        if matches!(
            p.order_type,
            OrderType::LimitIncrease | OrderType::LimitDecrease | OrderType::StopLossDecrease
        ) && p.trigger_price == 0
        {
            return Err(Error::InvalidTriggerPrice);
        }
        if matches!(p.order_type, OrderType::MarketIncrease | OrderType::LimitIncrease)
            && p.collateral_delta_amount == 0
        {
            return Err(Error::InvalidCollateralAmount);
        }
        Ok(())
    }

    fn can_execute_limit_order(p: &CreateOrderParams, current_price: u128) -> bool {
        let is_long = matches!(p.side, OrderSide::Long);
        match p.order_type {
            OrderType::LimitIncrease => {
                if is_long {
                    current_price <= p.trigger_price
                } else {
                    current_price >= p.trigger_price
                }
            }
            OrderType::LimitDecrease => {
                if is_long {
                    current_price >= p.trigger_price
                } else {
                    current_price <= p.trigger_price
                }
            }
            OrderType::StopLossDecrease => {
                if is_long {
                    current_price <= p.trigger_price
                } else {
                    current_price >= p.trigger_price
                }
            }
            _ => false,
        }
    }

    fn validate_execution_price(p: &CreateOrderParams, execution_price: u128) -> Result<(), Error> {
        let is_long = matches!(p.side, OrderSide::Long);
        let is_increase = matches!(p.order_type, OrderType::MarketIncrease | OrderType::LimitIncrease);
        let ok = match (is_long, is_increase) {
            (true, true) => execution_price <= p.acceptable_price,
            (true, false) => execution_price >= p.acceptable_price,
            (false, true) => execution_price >= p.acceptable_price,
            (false, false) => execution_price <= p.acceptable_price,
        };
        if !ok {
            return Err(Error::PriceNotAcceptable);
        }
        Ok(())
    }

    fn order_to_params(o: &Order) -> CreateOrderParams {
        CreateOrderParams {
            market: o.market.clone(),
            collateral_token: o.collateral_token.clone(),
            order_type: o.order_type.clone(),
            side: if o.is_long { OrderSide::Long } else { OrderSide::Short },
            size_delta_usd: o.size_delta_usd,
            collateral_delta_amount: o.collateral_delta_amount,
            trigger_price: o.trigger_price,
            acceptable_price: o.acceptable_price,
            execution_fee: o.execution_fee,
        }
    }

    fn execute_position_change(caller: ActorId, p: &CreateOrderParams, price: u128) -> Result<PositionKey, Error> {
        let now = exec::block_timestamp();
        RiskModule::accrue_pool(&p.market, now)?;

        let is_long = matches!(p.side, OrderSide::Long);
        match p.order_type {
            OrderType::MarketIncrease | OrderType::LimitIncrease => PositionModule::increase_position(
                caller,
                p.market.clone(),
                p.collateral_token.clone(),
                is_long,
                p.size_delta_usd,
                p.collateral_delta_amount,
                price,
            ),
            OrderType::MarketDecrease | OrderType::LimitDecrease | OrderType::StopLossDecrease => {
                PositionModule::decrease_position(
                    caller,
                    p.market.clone(),
                    p.collateral_token.clone(),
                    is_long,
                    p.size_delta_usd,
                    p.collateral_delta_amount,
                    price,
                )
            }
            _ => Err(Error::UnsupportedOrderType),
        }
    }

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
                    .filter_map(|k| st.orders.get(k).map(|o| (*k, o.clone())))
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
}
