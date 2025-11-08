use sails_rs::prelude::*;

#[derive(Default)]
pub struct AdminService;

#[service]
impl AdminService {
    #[export]
    pub fn placeholder(&self) -> bool {
        true
    }
    
    // TODO:
    // - create_market
    // - update_market_config
    // - add_keeper
    // - remove_keeper
}