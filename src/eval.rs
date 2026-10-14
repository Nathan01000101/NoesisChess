use crate::{search::MATE_THRESHOLD, types::PieceType};

pub const MATE: i32 = 500_000;

pub fn material_value(p_type: PieceType) -> i32 {
    match p_type {
        PieceType::Pawn => 100,
        PieceType::Knight => 300,
        PieceType::Bishop => 300,
        PieceType::Rook => 500,
        PieceType::Queen => 900,
        PieceType::King => 0,
    }
}

// returns eval in form:
// if mate score: "mate n"
// else: "cp n"
pub fn format_eval(eval: i32) -> String {
    if eval.abs() > MATE_THRESHOLD {
        let mut mate_in: i32 = (MATE - eval.abs()) / 2;
        if eval < 0 {
            String::from("mate -") + &mate_in.to_string()
        } else {
            mate_in += 1;
            String::from("mate ") + &mate_in.to_string()
        }
    } else {
        String::from("cp ") + &eval.to_string()
    }
}
