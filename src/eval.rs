use crate::types::{PieceType, Side, Board};
use crate::attacks::{PAWN_DOUBLED_MASKS, PAWN_ISOLATED_MASKS, PAWN_PASSED_MASKS};
use crate::movegen::get_move_count;

// Pawn structure scoring
const DOUBLED_PAWN_PENALTY: i32 = -20;
const ISOLATED_PAWN_PENALTY: i32 = -15;
const PASSED_PAWN_REWARD: [i32; 6] = [150, 110, 70, 40, 20, 10]; // passed pawns must be pushed! 

static PIECE_ORDER: [PieceType; 6] = [PieceType::King, PieceType::Queen, PieceType::Rook, PieceType::Bishop, PieceType::Knight, PieceType::Pawn];

// white evaluation, made public and unchanged so anything outside the
// search that calls it still gets what it expects.
pub fn evaluate(board: &Board) -> i32 {
    let mut eval = 0;
    // white
    for &piece_type in PIECE_ORDER.iter() {
        let mut bb = board.bitboards[0][piece_type as usize];
        while bb.0 != 0 {
            let sq = bb.0.trailing_zeros() as u8;
            let val = piece_value(piece_type, Side::White, sq, board.moves);
            eval += val;
            bb.0 &= bb.0 - 1;
        }
    }

    // black
    for &piece_type in PIECE_ORDER.iter() {
        let mut bb = board.bitboards[1][piece_type as usize];
        while bb.0 != 0 {
            let sq = bb.0.trailing_zeros() as u8;
            let val = piece_value(piece_type ,Side::Black, sq, board.moves);
            eval -= val;
            bb.0 &= bb.0 - 1;
        }
    }
    
    eval + pawn_structure_score(board)
}

fn pawn_structure_score(board: &Board) -> i32{
    let mut eval = 0;

    // iterate over each side
    for color in 0..2{
        let pawns = board.bitboards[color][PieceType::Pawn as usize].0;
        let mut pawns_itr = board.bitboards[color][PieceType::Pawn as usize].0;
        let enemy_pawns = board.bitboards[if color == 0 {1} else {0}][PieceType::Pawn as usize].0;

        while pawns_itr != 0{
            let pawn_bit = pawns_itr.trailing_zeros() as usize;
            //passed pawn check
            if enemy_pawns & PAWN_PASSED_MASKS[color][pawn_bit] == 0{
                let rank = pawn_bit / 8;
                let ranks_to_promo = if color == 0 { 7 - rank } else { rank };
                let reward = PASSED_PAWN_REWARD[ranks_to_promo.min(5)];
                if color == 0 {eval += reward} else {eval -= reward};
            }

            //doubled check
            if pawns & PAWN_DOUBLED_MASKS[color][pawn_bit] != 0{
                if color == 0 {eval += DOUBLED_PAWN_PENALTY} else {eval -= DOUBLED_PAWN_PENALTY};
            }

            // isolated check
            if pawns & PAWN_ISOLATED_MASKS[pawn_bit] == 0{
                if color == 0 {eval += ISOLATED_PAWN_PENALTY} else {eval -= ISOLATED_PAWN_PENALTY};
            }
            pawns_itr &= pawns_itr - 1;
        }
    }

    eval
}

fn mobility_score(board: &mut Board) -> i32{
    let mut eval = 0;

    for color in 0..2{
        let num_of_moves = get_move_count(board, if color == 0 {Side::White} else {Side::Black});
    }

    eval
}

// negamax needs the score from the point of view of the side to move.
#[inline]
pub fn eval_stm(board: &Board, side: Side) -> i32 {
    let white_relative = evaluate(board);
    if side == Side::White { white_relative } else { -white_relative }
}

fn piece_value(piece_type: PieceType, color: Side, coord: u8, moves: u8) -> i32{
    let idx = if color == Side::White {63 - coord as usize} else {coord as usize};

    match piece_type {
        PieceType::Pawn   => if moves < 60 {return 100 + PAWN_TABLE[idx]}          else {return 105 + PAWN_TABLE_LATE[idx]},
        PieceType::Knight => if moves < 55 {return 305 + KNIGHT_TABLE[idx]}        else {return 275 + KNIGHT_TABLE[idx]},
        PieceType::Bishop => if moves < 50 { return 333 + BISHOP_TABLE[idx] }      else {return 350 + BISHOP_TABLE_LATE[idx]},
        PieceType::Rook   => if moves < 60 {return 563 + ROOK_TABLE[idx]}          else {return 570 + ROOK_TABLE_LATE[idx]},
        PieceType::Queen  => if moves < 18 {return 950 + QUEEN_TABLE_EARLY[idx]}   else {return 950 + QUEEN_TABLE_LATE[idx]},
        PieceType::King   => if moves < 40 {return 100000 + KING_TABLE_EARLY[idx]} else {return 100000 + KING_TABLE_LATE[idx] },
    };
}

pub fn material_value(p_type: PieceType) -> i32{
    match p_type {
            PieceType::Pawn   => 100,
            PieceType::Knight => 300,
            PieceType::Bishop => 300,
            PieceType::Rook   => 500,
            PieceType::Queen  => 900,
            PieceType::King   => 0
    }
}



// ALL OF THESE ARE FROM BLACKS PERSPECTIVE
const PAWN_TABLE: [i32; 64] = [
    0,    0,    0,    0,    0,    0,    0,    0   ,  // promotion
    40,   40,   40,   40,   40,   40,   40,   40  ,
    25,   25,   25,   25,   25,   25,   25,   25  ,
    10,   10,   25,   50,   50,   25,   10,   10  ,
    5,    5,    25,   45,   45,   25,   5,    5   ,
    5,    5,    10,   5,    5,   10,    5,    5   ,
    5,    5,    5,   -10,  -10,   5,    5,    5   ,  // slight penalty for blocking center
    0,    0,    0,    0,    0,    0,    0,    0   ,  // starting rank
];

const PAWN_TABLE_LATE: [i32; 64] = [
    0,    0,    0,    0,    0,    0,    0,    0  ,  // PROMOTE PROMOTE PROMOTE
    45,  45,  45,  45,  45,  45,  45,  45  ,  //
    30,   30,   30,   30,   30,   30,   30,   30  ,  //
    25,   25,   25,   25,   25,   25,   25,   25  ,  //
    20,   20,   20,   20,   20,   20,   20,   20  ,  //
    10,   10,   10,   10,   10,   10,   10,   10  ,  //
    5,    5,    5,    5,    5,    5,    5,    5  ,  //
    0,    0,    0,    0,    0,    0,    0,    0  ,  // starting rank
];

const KNIGHT_TABLE: [i32; 64] = [
     -50,    -30,  -30,   -30,  -30,   -30,   -30,  -50  ,  // avoid edges
     -50,     0,    0,     5,    5,     0,     0,   -50  ,
     -50,     5,    10,    15,   15,    10,    5,   -50  ,
     -50,     5,    10,    25,   25,    10,    5,   -50  ,
     -50,     5,    10,    25,   25,    10,    5,   -50  ,
     -50,     0,    10,    15,   15,    10,    5,   -50  ,
     -50,    -5,   -5,     5,    5,    -5,    -5,   -50  ,
     -50,    -10,  -30,   -30,  -30,   -30,   -10,  -50  ,  // starting rank
];

const BISHOP_TABLE: [i32; 64] = [
     -20,   -10,   -10,   -10,   -10,   -10,   -10,  -20 , // avoid edges
     -10,    0,     0,     0,     0,     0,     0,   -10 ,
     -10,    0,     5,     10,    10,    5,     0,   -10 ,
     -10,    5,     5,     10,    10,    5,     5,   -10 ,
     -10,    0,     10,    10,    10,    10,    0,   -10 ,
     -10,    10,    10,    10,    10,    10,    10,  -10 ,
     -10,    15,     0,     0,     0,     0,    15,  -10 ,
     -20,   -10,   -10,   -10,   -10,   -10,   -10,  -20 , //starting rank
];

const BISHOP_TABLE_LATE: [i32; 64] = [
     -10,  -5,  -5,  -5,  -5,  -5,  -5, -10 ,  // less harsh edge penalty
      -5,   5,   5,   5,   5,   5,   5,  -5 ,
      -5,   5,  10,  10,  10,  10,   5,  -5 ,
      -5,   5,  10,  15,  15,  10,   5,  -5 ,
      -5,   5,  10,  15,  15,  10,   5,  -5 ,
      -5,   5,  10,  10,  10,  10,   5,  -5 ,
      -5,   5,   5,   5,   5,   5,   5,  -5 ,
     -10,  -5, -15,  -5,  -5, -15,  -5, -10 ,
];

const ROOK_TABLE: [i32; 64] = [
      0,   0,   0,   0,   0,   0,   0,   0 ,
     10,  15,  15,  15,  15,  15,  15,  10 ,  // 7th rank bonus
     -5,   0,   0,   0,   0,   0,   0,  -5 ,
     -5,   0,   0,   0,   0,   0,   0,  -5 ,
     -5,   0,   0,   0,   0,   0,   0,  -5 ,
     -5,   0,   0,   0,   0,   0,   0,  -5 ,
     -5,   0,   0,   0,   0,   0,   0,  -5 ,
      0,   0,   5,  10,  10,   5,   0,   0 ,  // starting rank
];

const ROOK_TABLE_LATE: [i32; 64] = [
      5,   5,   5,   5,   5,   5,   5,   5 ,
     15,  20,  20,  20,  20,  20,  20,  15 ,
      0,   5,   5,   5,   5,   5,   5,   0 ,
      0,   5,  10,  10,  10,  10,   5,   0 ,
      0,   5,  10,  10,  10,  10,   5,   0 ,
      0,   5,   5,  10,  10,   5,   5,   0 ,
      0,   5,   5,   5,   5,   5,   5,   0 ,
      0,   5,   5,   5,   5,   5,   5,   0 ,  // starting rank
];


const QUEEN_TABLE_EARLY: [i32; 64] = [
    -30, -20, -20, -20, -20, -20, -20, -30, // Avoid early queen moves
    -20, -15, -15, -15, -15, -15, -15, -20,
    -20, -20, -20, -20, -20, -20, -20, -20,
    -20, -20, -20, -25, -25, -20, -20, -20,
    -20, -20, -20, -25, -25, -20, -20, -20,
    -10, -15, -10, -15, -15, -10, -15, -10,
     -5,   5,   5,   5,   5,   5,   5,  -5,
    -10,   0,   0,  15,   0,   0,   0, -10, // Keep king near king to start
];

const QUEEN_TABLE_LATE: [i32; 64] = [
     -20, -10, -10,  -5,  -5, -10, -10, -20 , // avoid edges
     -10,   0,   0,   0,   0,   0,   0, -10 ,
     -10,   0,   5,   5,   5,   5,   0, -10 ,
      -5,   0,   5,   5,   5,   5,   0,  -5 ,
      -5,   0,   5,   5,   5,   5,   0,  -5 ,
     -10,   5,   5,   5,   5,   5,   0, -10 ,
     -10,   0,   5,   0,   0,   0,   0, -10 ,
     -20, -10, -10, -15,  -5, -10, -10, -20 , // staring rank
];

const KING_TABLE_EARLY: [i32; 64] = [
     -30, -40, -40, -50, -50, -40, -40, -30 , // RUN AWAY!!
     -30, -40, -40, -50, -50, -40, -40, -30 ,
     -30, -40, -40, -50, -50, -40, -40, -30 ,
     -30, -40, -40, -50, -50, -40, -40, -30 ,
     -20, -30, -30, -40, -40, -30, -30, -20 ,
     -10, -20, -20, -20, -20, -20, -20, -10 ,
      20,  20,   0,   0,   0,   0,  20,  20 ,
      20,  30,  30,  10,  10,  10,  30,  20 ,  // castled positions rewarded
];

const KING_TABLE_LATE: [i32; 64] = [
     -30, -30, -30, -30, -30, -30, -30, -30 , // GET IN THE MIX!!
     -30, -10, -10, -10, -10, -10, -10, -30 ,
     -30, -10,   5,   5,   5,   5, -10, -30 ,
     -30, -10,   5,   5,   5,   5, -10, -30 ,
     -30, -10,   5,   5,   5,   5, -10, -30 ,
     -30, -10,   5,   5,   5,   5, -10, -30 ,
     -30, -10, -10, -10, -10, -10, -10, -30 ,
     -30, -30, -30, -30, -30, -30, -30, -30 ,  // middle positions rewarded
];