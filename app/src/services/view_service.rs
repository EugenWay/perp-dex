use sails_rs::prelude::*;
#[derive(Default)]
pub struct ViewService;

#[service]
impl ViewService {
    #[export]
    pub fn placeholder(&self) -> bool {
        true
    }
    
    // TODO:
    // - get_position
    // - get_market
    // - get_pool_amounts
    // - get_order
}