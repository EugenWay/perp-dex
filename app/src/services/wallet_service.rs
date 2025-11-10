use sails_rs::{prelude::*, gstd::msg};
use crate::{errors::Error, PerpetualDEXState, types::Usd};

/// Internal USD wallet (micro-USD). This is a temporary in-program balance.
/// In production this would be backed by real FT transfers.
#[derive(Default)]
pub struct WalletService;

impl WalletService {
    pub fn new() -> Self {
        Self::default()
    }
}

#[service]
impl WalletService {
    #[export]
    pub fn deposit(&mut self, amount: Usd) -> Result<Usd, Error> {
        if amount == 0 {
            return Err(Error::InvalidParameter);
        }
        let caller = msg::source();
        let mut st = PerpetualDEXState::get_mut();
        let bal = st.balances.entry(caller).or_insert(0);
        *bal = bal.saturating_add(amount);
        Ok(*bal)
    }

    #[export]
    pub fn withdraw(&mut self, amount: Usd) -> Result<Usd, Error> {
        if amount == 0 {
            return Err(Error::InvalidParameter);
        }
        let caller = msg::source();
        let mut st = PerpetualDEXState::get_mut();
        let bal = st.balances.get_mut(&caller).ok_or(Error::InsufficientBalance)?;
        if *bal < amount {
            return Err(Error::InsufficientBalance);
        }
        *bal = bal.saturating_sub(amount);
        Ok(*bal)
    }

    #[export]
    pub fn balance_of(&self, account: ActorId) -> Usd {
        let st = PerpetualDEXState::get();
        st.balances.get(&account).copied().unwrap_or(0)
    }

    #[export]
    pub fn my_balance(&self) -> Usd {
        let caller = msg::source();
        self.balance_of(caller)
    }
}