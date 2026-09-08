use std::any::Any;
use crate::Board;
use crate::Side;
use crate::ai::Player;

pub struct HumanPlayer;
impl Player for HumanPlayer {
    fn as_any(&self) -> &dyn Any { self }

    fn get_move(&self, board: &Board, side: Side) -> (u8, u8) {
        todo!();
    }

    fn reset(&self) {
        
    }
}