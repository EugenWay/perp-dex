use sails_rs::prelude::{ActorId, H256, Vec};
use sails_rs::gstd::exec;

/// Cancellation windows (in blocks)
pub const DEPOSIT_CXL_DELAY: u32 = 10;
pub const ORDER_CXL_DELAY: u32 = 5;

/// Current block info
#[inline]
pub fn now() -> (u32, u64) {
    (exec::block_height(), exec::block_timestamp())
}

/// Canonical position key (keccak)
pub fn position_key(
    account: ActorId,
    market: &str,
    collateral_token: &str,
    is_long: bool,
) -> H256 {
    use sp_core::hashing::keccak_256;
    let mut data = Vec::new();
    data.extend_from_slice(account.as_ref());
    data.extend_from_slice(market.as_bytes());
    data.extend_from_slice(collateral_token.as_bytes());
    data.push(if is_long { 1 } else { 0 });
    H256::from(keccak_256(&data))
}