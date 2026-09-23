use std::any::Any;
use crate::types::MoveContext;



pub trait Player {
    fn as_any(&self) -> &dyn Any;
    fn get_move(&self, context: MoveContext) -> (u8, u8);
    fn reset(&self);
}