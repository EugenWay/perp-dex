use sails_rs::{collections::BTreeMap, prelude::*};

pub type RequestKey = H256;
pub type PositionKey = H256;

/// Fixed-point USD type (micro-USD, 1e6)
pub type Usd = u128;
/// 1 USD = 1_000_000 micro-USD
pub const USD_SCALE: u128 = 1_000_000;

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct Market {
    pub market_token: ActorId,
    pub index_token: String,
    pub long_token: String,
    pub short_token: String,
}

/// Market configuration (risk, fees, limits)
#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct MarketConfig {
    pub market_id: String,

    // Price impact
    pub pi_factor_positive: u128, // bps
    pub pi_factor_negative: u128, // bps
    pub pi_exponent: u128,        // dimensionless

    // Funding
    pub funding_factor: u128,            // bps
    pub funding_exponent: u128,          // dimensionless
    pub funding_factor_above_kink: u128, // bps
    pub optimal_imbalance_ratio: u128,   // bps

    // Borrowing
    pub borrowing_factor: u128,   // bps
    pub borrowing_exponent: u128, // dimensionless
    pub skip_borrowing_for_smaller_side: bool,

    // Trading & risk
    pub trading_fee_bps: u16,
    pub max_leverage: u8,        // x
    pub min_collateral_usd: Usd, // fixed-point
    pub liquidation_threshold_bps: u16,
    pub reserve_factor_bps: u16,

    // OI caps (in USD)
    pub max_long_oi: Usd,
    pub max_short_oi: Usd,
}

/// Pool accounting in USD only
#[derive(Encode, Decode, TypeInfo, Clone, Debug, Default)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct PoolAmounts {
    pub liquidity_usd: Usd,
    pub claimable_fee_usd_long: Usd,
    pub claimable_fee_usd_short: Usd,
    pub long_oi_usd: Usd,
    pub short_oi_usd: Usd,
    pub position_impact_pool_usd: Usd,
    pub swap_impact_pool_usd: Usd,
    pub total_borrowing_fees_usd: Usd,
    pub last_funding_update: u64,
    pub accumulated_funding_long_per_usd: i128,
    pub accumulated_funding_short_per_usd: i128,
}

/// Position accounting in USD only (no token-sized fields)
#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct Position {
    /// Canonical keccak(account, market, collateral_token, is_long)
    pub key: PositionKey,
    /// Owner of the position
    pub account: ActorId,
    /// Market id (e.g. "BTC-USD")
    pub market: String,
    /// Collateral token symbol (I/O). Internally we account in USD.
    pub collateral_token: String,
    /// Side: long = true, short = false
    pub is_long: bool,

    /// Notional size in USD (fixed-point)
    pub size_usd: Usd,
    /// Collateral in USD (fixed-point)
    pub collateral_usd: Usd,

    /// Entry price in USD per 1 index unit (fixed-point, same scale as oracle mid)
    pub entry_price_usd: Usd,
    /// Cached liquidation price in USD per 1 index unit
    pub liquidation_price_usd: Usd,

    /// Funding checkpoint (accumulated funding per USD at last settle)
    pub funding_fee_per_usd: i128,
    /// Borrowing factor snapshot if needed (bps or fixed as per model)
    pub borrowing_factor: Usd,

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

#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum OrderStatus {
    Created,
    Executed,
    Cancelled,
    Frozen,
}

/// Order side - Long or Short position
#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum OrderSide {
    Long,
    Short,
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
    pub status: OrderStatus,
    pub execution_fee: u128,
    pub callback_gas_limit: u64,
    pub created_at_block: u32,
    pub created_at_time: u64,
    pub updated_at_block: u32,
    pub updated_at_time: u64,
}

/// Simplified parameters for creating orders
#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct CreateOrderParams {
    pub market: String,
    pub collateral_token: String,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub size_delta_usd: u128,
    pub collateral_delta_amount: u128,
    pub trigger_price: u128,
    pub acceptable_price: u128,
    pub execution_fee: u128,
}

/// Parameters for updating orders
#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct UpdateOrderParams {
    pub size_delta_usd: Option<u128>,
    pub trigger_price: Option<u128>,
    pub acceptable_price: Option<u128>,
}

/// Result of order creation
#[derive(Encode, Decode, TypeInfo, Clone, Debug, PartialEq, Eq)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum ExecutionResult {
    Executed {
        position_key: PositionKey,
        execution_price: u128,
    },
    Saved {
        order_key: RequestKey,
    },
}

/// USD price, scaled by USD_SCALE (micro-USD per 1 index unit)
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

#[derive(Encode, Decode, TypeInfo, Clone, Debug)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct OracleState {
    pub prices: BTreeMap<String, Price>,
    pub timestamps: BTreeMap<String, u64>,
    pub last_signer: BTreeMap<String, ActorId>,
    pub config: OracleConfig,
}

#[derive(Encode, Decode, TypeInfo, Clone, Debug, Default)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub struct MarketTokenInfo {
    pub total_supply: u128,
    pub balances: Vec<(ActorId, u128)>,
}
