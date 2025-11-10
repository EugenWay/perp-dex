use sails_rs::prelude::{ActorId, H256, Vec, String};
use sails_rs::gstd::exec;
use crate::types::Price;

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

pub fn verify_signature(
    _token: &str,
    _price: &Price,
    _timestamp: u64,
    _signer: &ActorId,
    _signature: &[u8],
) -> bool {
    // TODO: Implement real signature verification
    // WARNING: This stub returns true for all signatures - NOT SAFE for production!
    true
}

/// Resolve market ID or token name to the correct oracle price key.
/// If given a known market ID, returns its `index_token`
pub fn price_key(id_or_token: &str) -> String {
    let st = crate::PerpetualDEXState::get();
    if let Some(m) = st.markets.get(id_or_token) {
        m.index_token.clone()
    } else {
        String::from(id_or_token)
    }
}