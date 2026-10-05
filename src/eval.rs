use crate::types::{PieceType, Side, Board};
use crate::attacks::{PAWN_DOUBLED_MASKS, PAWN_ISOLATED_MASKS, PAWN_PASSED_MASKS};
use crate::movegen::get_move_count;

// Pawn structure scoring
const DOUBLED_PAWN_PENALTY: i32 = -20;
const ISOLATED_PAWN_PENALTY: i32 = -15;
const PASSED_PAWN_REWARD: [i32; 6] = [150, 110, 70, 40, 20, 10]; // passed pawns must be pushed! 

pub const PST_MOVES: usize = 71;
pub static PST: [[[[i32; 64]; 6]; 2]; PST_MOVES] = build_pst(); // indexed PST[moves][color][piecetype][square]
const TAPER_SCALE: i32 = 256;
 

const fn taper_range(pt: PieceType) -> (usize, usize) {
    match pt {
        PieceType::Pawn   => (50, 70), 
        PieceType::Knight => (45, 65), 
        PieceType::Bishop => (40, 60), 
        PieceType::Rook   => (50, 70), 
        PieceType::Queen  => (12, 24), 
        PieceType::King   => (30, 50), 
    }
}

// king value is 0 here because kings will always cancel out anyways
const fn early_value(pt: PieceType, idx: usize) -> i32 {
    match pt {
        PieceType::Pawn   => 100 + PAWN_TABLE[idx],
        PieceType::Knight => 305 + KNIGHT_TABLE[idx],
        PieceType::Bishop => 333 + BISHOP_TABLE[idx],
        PieceType::Rook   => 563 + ROOK_TABLE[idx],
        PieceType::Queen  => 950 + QUEEN_TABLE_EARLY[idx],
        PieceType::King   =>   0 + KING_TABLE_EARLY[idx],
    }
}
 
const fn late_value(pt: PieceType, idx: usize) -> i32 {
    match pt {
        PieceType::Pawn   => 105 + PAWN_TABLE_LATE[idx],
        PieceType::Knight => 275 + KNIGHT_TABLE[idx],
        PieceType::Bishop => 350 + BISHOP_TABLE_LATE[idx],
        PieceType::Rook   => 570 + ROOK_TABLE_LATE[idx],
        PieceType::Queen  => 950 + QUEEN_TABLE_LATE[idx],
        PieceType::King   =>   0 + KING_TABLE_LATE[idx],
    }
}
 
const fn table_index(color: usize, sq: usize) -> usize {
    if color == Side::White as usize { sq ^ 56 } else { sq }
}
 
const fn build_pst() -> [[[[i32; 64]; 6]; 2]; PST_MOVES] {
    const PIECES: [PieceType; 6] = [
        PieceType::Pawn, PieceType::Knight, PieceType::Bishop,
        PieceType::Rook, PieceType::Queen,  PieceType::King,
    ];
 
    let mut table = [[[[0i32; 64]; 6]; 2]; PST_MOVES];
 
    let mut p = 0;
    while p < 6 {
        let pt = PIECES[p];
        let (start, end) = taper_range(pt);
        assert!(end < PST_MOVES, "PST_MOVES must be greater than every taper end");
 
        let mut m = 0;
        while m < PST_MOVES {
            // weight of the late values at this move number
            let w = if m <= start {
                0
            } else if m >= end {
                TAPER_SCALE
            } else {
                ((m - start) as i32 * TAPER_SCALE) / (end - start) as i32
            };
 
            let mut color = 0;
            while color < 2 {
                let mut sq = 0;
                while sq < 64 {
                    let idx = table_index(color, sq);
                    let v = (early_value(pt, idx) * (TAPER_SCALE - w)
                           + late_value(pt, idx) * w) / TAPER_SCALE;
 
                    table[m][color][pt as usize][sq] =
                        if color == Side::White as usize { v } else { -v };
                    sq += 1;
                }
                color += 1;
            }
            m += 1;
        }
        p += 1;
    }
 
    table
}


// white evaluation, made public and unchanged so anything outside the
// search that calls it still gets what it expects.
pub fn evaluate(board: &Board) -> i32 {
    let mut eval = 0;
    let pst = &PST[(board.moves as usize).min(PST_MOVES - 1)]; 

    for color in 0..2{
        for pt in 0..6 {
            let mut bb = board.bitboards[color][pt];
            while bb.0 != 0 {
                let sq = bb.0.trailing_zeros() as usize;
                eval += pst[color][pt][sq];
                bb.0 &= bb.0 - 1;
            }
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