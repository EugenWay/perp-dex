use sails_rs::prelude::*;

#[derive(Debug, Clone, Encode, Decode, TypeInfo)]
#[codec(crate = sails_rs::scale_codec)]
#[scale_info(crate = sails_rs::scale_info)]
pub enum Error {
    // Access
    Unauthorized,
    NotKeeper,
    NotLiquidator,
    NotAdmin,

    // Market
    MarketNotFound,
    MarketAlreadyExists,

    // Requests
    RequestNotFound,
    RequestAlreadyExecuted,
    CancellationDelayNotPassed,

    // Position
    PositionNotFound,
    PositionNotLiquidatable,
    PositionTooSmall,

    // Risk
    InsufficientCollateral,
    LeverageTooHigh,
    OICapReached,
    InsufficientLiquidity,

    // Execution
    SlippageExceeded,
    PriceStale,
    InvalidTriggerPrice,
    OrderFrozen,

    // Balance
    InsufficientBalance,
    InsufficientMarketTokens,

    // Oracle
    PriceNotAvailable,
    InvalidOracleSignature,

    // Other
    InvalidParameter,
    MathOverflow,
}