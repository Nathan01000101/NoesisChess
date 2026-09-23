use std::any::Any;
use crate::types::{MoveContext};
use crate::ai::Player;

pub struct HumanPlayer;
impl Player for HumanPlayer {
    fn as_any(&self) -> &dyn Any { self }

    fn get_move(&self, _context: MoveContext) -> (u8, u8) {
        todo!();
    }

    fn reset(&self) {
        
    }
}