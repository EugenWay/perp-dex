use sails_rs::prelude::*;
use crate::types::*;

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum ExchangeEvent {
    DepositCreated { key: RequestKey, account: ActorId, market: String, long_token_amount: u128, short_token_amount: u128 },
    WithdrawalCreated { key: RequestKey, account: ActorId, market: String, market_token_amount: u128 },
    OrderCreated { key: RequestKey, account: ActorId, order_type: OrderType, market: String, size_delta_usd: u128 },  // ✅ FIXED: accoun t -> account
    OrderUpdated { key: RequestKey, account: ActorId },
    OrderCancelled { key: RequestKey, account: ActorId, reason: String },
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum ExecutorEvent {
    DepositExecuted { key: RequestKey, account: ActorId, market_tokens_minted: u128 },
    DepositCancelled { key: RequestKey, reason: String },
    WithdrawalExecuted { key: RequestKey, account: ActorId, long_token_amount: u128, short_token_amount: u128 },
    WithdrawalCancelled { key: RequestKey, reason: String },
    OrderExecuted { key: RequestKey, account: ActorId, execution_price: u128 },
    OrderFrozen { key: RequestKey, reason: String },
    PositionIncreased { position_key: PositionKey, account: ActorId, market: String, size_delta: u128, collateral_delta: u128, execution_price: u128, price_impact: i128 },
    PositionDecreased { position_key: PositionKey, account: ActorId, market: String, size_delta: u128, collateral_delta: u128, execution_price: u128, price_impact: i128, pnl: i128 },
    PositionLiquidated { position_key: PositionKey, account: ActorId, market: String, liquidator: ActorId, liquidation_fee: u128 },
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum AdminEvent {
    MarketCreated { market_id: String, index_token: String, long_token: String, short_token: String },
    MarketConfigUpdated { market_id: String },
    KeeperAdded { keeper: ActorId },
    KeeperRemoved { keeper: ActorId },
}