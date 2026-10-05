use crate::ai::Player;
use crate::types::{Move, MoveContext};
use std::any::Any;

pub struct HumanPlayer;
impl Player for HumanPlayer {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_move(&self, _context: MoveContext) -> Move {
        todo!();
    }

    fn reset(&self) {}
}
