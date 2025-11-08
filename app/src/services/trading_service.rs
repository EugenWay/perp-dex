use sails_rs::prelude::*;
#[derive(Default)] 
pub struct TradingService;

#[service]
impl TradingService {
    #[export]
    pub fn placeholder(&self) -> bool {
        true
    }
    
    // TODO: Implement in Phase 2:
    // - create_order
    // - create_deposit
    // - create_withdrawal
    // - cancel_order
} 