use sails_rs::{
    prelude::*,
    ActorId,
    String,
};
use sails_rs::collections::BTreeMap;

pub type RequestKey = H256;
pub type PositionKey = H256;

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct Market {
    pub market_token: ActorId,
    pub index_token: String,
    pub long_token: String,
    pub short_token: String,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct MarketConfig {
    pub market_id: String,
    pub pi_factor_positive: u128,
    pub pi_factor_negative: u128,
    pub pi_exponent: u128,
    pub funding_factor: u128,
    pub funding_exponent: u128,
    pub funding_factor_above_kink: u128,
    pub optimal_imbalance_ratio: u128,
    pub borrowing_factor: u128,
    pub borrowing_exponent: u128,
    pub skip_borrowing_for_smaller_side: bool,
    pub trading_fee_bps: u16,
    pub max_leverage: u8,
    pub min_collateral_usd: u128,
    pub liquidation_threshold_bps: u16,
    pub reserve_factor_bps: u16,
    pub max_long_oi: u128,
    pub max_short_oi: u128,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, Default)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct PoolAmounts {
    pub long_token_amount: u128,
    pub short_token_amount: u128,
    pub long_oi: u128,
    pub short_oi: u128,
    pub long_oi_in_tokens: u128,
    pub short_oi_in_tokens: u128,
    pub position_impact_pool_amount: u128,
    pub swap_impact_pool_amount: u128,
    pub claimable_fee_amount_long: u128,
    pub claimable_fee_amount_short: u128,
    pub total_borrowing_fees: u128,
    pub last_funding_update: u64,
    pub accumulated_funding_long: i128,
    pub accumulated_funding_short: i128,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct Position {
    pub key: PositionKey,
    pub account: ActorId,
    pub market: String,
    pub collateral_token: String,
    pub is_long: bool,
    pub size_in_usd: u128,
    pub size_in_tokens: u128,
    pub collateral_amount: u128,
    pub entry_price: u128,
    pub liquidation_price: u128,
    pub borrowing_factor: u128,
    pub funding_fee_per_size: i128,
    pub increased_at_block: u32,
    pub decreased_at_block: u32,
    pub last_fee_update: u64,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct DepositRequest {
    pub key: RequestKey,
    pub account: ActorId,
    pub receiver: ActorId,
    pub callback_contract: Option<ActorId>,
    pub market: String,
    pub long_token_amount: u128,
    pub short_token_amount: u128,
    pub min_market_tokens: u128,
    pub execution_fee: u128,
    pub callback_gas_limit: u64,
    pub created_at_block: u32,
    pub created_at_time: u64,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct WithdrawalRequest {
    pub key: RequestKey,
    pub account: ActorId,
    pub receiver: ActorId,
    pub callback_contract: Option<ActorId>,
    pub market: String,
    pub market_token_amount: u128,
    pub min_long_token_amount: u128,
    pub min_short_token_amount: u128,
    pub execution_fee: u128,
    pub callback_gas_limit: u64,
    pub created_at_block: u32,
    pub created_at_time: u64,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum OrderType {
    MarketIncrease,
    LimitIncrease,
    MarketDecrease,
    LimitDecrease,
    StopLossDecrease,
    MarketSwap,
    LimitSwap,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct Order {
    pub key: RequestKey,
    pub account: ActorId,
    pub receiver: ActorId,
    pub callback_contract: Option<ActorId>,
    pub market: String,
    pub collateral_token: String,
    pub order_type: OrderType,
    pub size_delta_usd: u128,
    pub collateral_delta_amount: u128,
    pub trigger_price: u128,
    pub acceptable_price: u128,
    pub min_output_amount: u128,
    pub is_long: bool,
    pub is_frozen: bool,
    pub execution_fee: u128,
    pub callback_gas_limit: u64,
    pub created_at_block: u32,
    pub created_at_time: u64,
    pub updated_at_block: u32,
    pub updated_at_time: u64,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct Price {
    pub min: u128,
    pub max: u128,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OracleConfig {
    pub max_age_seconds: u64,
}

// ✅ FIXED: Using BTreeMap for all maps (HashMap doesn't work with codec)
#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OracleState {
    pub prices: BTreeMap<String, Price>,
    pub timestamps: BTreeMap<String, u64>,      // ✅ Changed from HashMap
    pub last_signer: BTreeMap<String, ActorId>, // ✅ Changed from HashMap
    pub config: OracleConfig,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, Default)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct MarketTokenInfo {
    pub total_supply: u128,
    pub balances: Vec<(ActorId, u128)>,
}