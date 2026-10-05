use crate::types::{Move, MoveContext};
use std::any::Any;

pub trait Player {
    fn as_any(&self) -> &dyn Any;
    fn get_move(&self, context: MoveContext) -> Move;
    fn reset(&self);
}
