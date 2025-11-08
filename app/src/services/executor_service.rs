use sails_rs::prelude::*;
#[derive(Default)]
pub struct ExecutorService;

#[service]
impl ExecutorService {
    #[export]
    pub fn placeholder(&self) -> bool {
        true
    }
    
    // TODO:
    // - execute_deposit
    // - execute_withdrawal
    // - execute_order
    // - liquidate_position
}