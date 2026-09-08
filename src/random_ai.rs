use std::any::Any;
use std::thread;
use std::time::Duration;
use macroquad::{ prelude::*};
use macroquad::miniquad::date;
use crate::{Board, get_side_bitboard, get_valid_moves, is_in_check};
use crate::Side;
use crate::ai::Player;

pub struct RandomAI;
impl Player for RandomAI {
    fn as_any(&self) -> &dyn Any { self }

    fn get_move(&self, board: &Board, side: Side) -> (u8, u8) {
        todo!();
        let mut moves = Vec::new();
        let mut b = board.clone();
        thread::sleep(Duration::from_secs_f32(0.1));
        let mut friendly_bb = get_side_bitboard(board, side);
        let is_in_check = is_in_check(board, side);
        while friendly_bb.0 != 0{
            let bit = friendly_bb.0.trailing_zeros();
            // get_valid_moves(&mut b, bit as u8, is_in_check);

            friendly_bb.0 ^= 1 << bit;
        }

        rand::srand(date::now() as u64);
        moves[rand::gen_range(0, moves.len())]
    }

    fn reset(&self) {
        
    }
}