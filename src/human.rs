use std::any::Any;
use crate::types::{Board, Side};
use crate::ai::Player;

pub struct HumanPlayer;
impl Player for HumanPlayer {
    fn as_any(&self) -> &dyn Any { self }

    fn get_move(&self, _board: &Board, _side: Side, time_remaining: std::time::Duration, increment: std::time::Duration) -> (u8, u8) {
        todo!();
    }

    fn reset(&self) {
        
    }
}