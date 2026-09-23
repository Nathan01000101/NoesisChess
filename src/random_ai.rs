use std::any::Any;
use std::thread;
use std::time::Duration;
use macroquad::{ prelude::*};
use macroquad::miniquad::date;
use crate::types::{MoveContext, MoveListBuf, Side, WHITE_TO_MOVE};
use crate::movegen::get_all_moves;
use crate::ai::Player;

pub struct RandomAI;
impl Player for RandomAI {
    fn as_any(&self) -> &dyn Any { self }

    fn get_move(&self, context: MoveContext) -> (u8, u8) {
        let mut moves = MoveListBuf::new();
        let mut b = context.board.clone();
        get_all_moves(&mut b, if context.board.state & WHITE_TO_MOVE != 0 {Side::White} else {Side::Black}, &mut moves);
        thread::sleep(Duration::from_secs_f32(0.1));
        rand::srand(date::now() as u64);
        moves.data[rand::gen_range(0, moves.len)]
    }

    fn reset(&self) {
        
    }
}