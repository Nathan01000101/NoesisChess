use std::any::Any;
use rustc_hash::FxHashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;
use std::time::{Instant};
use crate::{BLACK_SHORT, get_piece, is_piece};
use crate::BLACK_LONG;
use crate::Board;
use crate::PieceType;
use crate::Piece;
use crate::Side;
use crate::WHITE_SHORT;
use crate::WHITE_TO_MOVE;
use crate::WHITE_LONG;
use crate::ai::Player;
use crate::undo_move;
use macroquad::{ prelude::*};
use macroquad::miniquad::date;
use crate::get_all_moves;
use crate::get_all_captures;
use crate::make_move;
use crate::is_in_check;
use crate::to_fen;

const MOVE_REPETITION_PENALTY: i32 = 25;

pub struct MinimaxAI {
    pub depth: usize, 
    opening_book: FxHashMap<String, Vec<(u8, u8)>>,
    zobrist: ZobristTable,
    tt: Mutex<FxHashMap<u64, TTEntry>>,
    pub move_history: Mutex<Vec<u64>>    
    }

impl MinimaxAI {
    pub fn new(depth: usize) -> Self{
        rand::srand(date::now() as u64);
        println!("hello?");
        Self {
            depth,
            opening_book: build_book(),
            zobrist: ZobristTable::new(),
            tt: Mutex::new(FxHashMap::default()),
            move_history: Mutex::new(Vec::new())
            
        }
    }
}
impl Player for MinimaxAI {
    fn as_any(&self) -> &dyn Any { self }
    fn get_move(&self, board: &Board, side: Side) -> (u8,u8) {
        let start = Instant::now();

        // cap transposition table growth
        if self.tt.lock().unwrap().len() > 800_000_000 {   
            self.tt.lock().unwrap().clear();
        }

        // before calculating move manually, check if position exists in our opening book
        let full_fen: String = to_fen(board);
        let parts: Vec<&str> = full_fen.split_whitespace().collect();
        let fen = parts[..4].join(" ");
        if self.opening_book.contains_key(&fen){
            let possible = self.opening_book.get(&fen);
            if let Some(mvs) = possible{
                if mvs.len() > 0{
                    let mv = mvs.get(rand::gen_range(0, mvs.len())).unwrap();
                    println!("minimax AI found book move");
                    return (mv.0, mv.1);
                }
            }
        }

        let mut tt_guard = self.tt.lock().unwrap();
        let tt: &mut FxHashMap<u64, TTEntry> = &mut *tt_guard;

        let mut mh_guard = self.move_history.lock().unwrap();

        let mut b = board.clone();
        let mut best_move: (u8, u8) = (64, 64);

        let mut best_eval: i32 = if side == Side::White {i32::MIN} else {i32::MAX};

        let mut alpha = i32::MIN;
        let mut beta = i32::MAX;

        rand::srand(date::now() as u64);
        let mut moves = get_all_moves(&mut b, side);
        println!("root move count: {}", moves.len());

        // move ordering
        // victim_value * 10 - attacker_value
        moves.sort_by_key(|mv| {
            match if is_piece(board, mv.1) {Some(get_piece(board, mv.1))} else {None} {
                Some(victim) => {
                    let attacker_val = match if is_piece(board, mv.0) {Some(get_piece(board, mv.0))} else {None} {
                        Some(a) => material_value(a.piece_type),
                        None => 0,
                    };
                    -(material_value(victim.piece_type) * 10 - attacker_val)
                }
                None => 0,
            }
        });

        for i in 0..moves.len(){
            let undo = make_move(&mut b, moves[i].0,moves[i].1);
            let depth_deduction = if i as f32 / moves.len() as f32 > 0.75 { 1 } else {0}; 
            let mut eval = minimax(&mut b, self.depth.saturating_sub(1 - depth_deduction), 1,  alpha, beta, &self.zobrist, tt);

            // see if the move already exists in our move history
            if mh_guard.contains(&self.zobrist.hash(&b)){
                if side == Side::White { eval -= MOVE_REPETITION_PENALTY} else { eval += MOVE_REPETITION_PENALTY}
                print!("found a move that has already been made!");
            }

            // alpha - beta pruning
            if side == Side::White {
                alpha = alpha.max(eval);
            } else {
                beta = beta.min(eval);
            }
        
            undo_move(&mut b, undo);
            if side == Side::White && eval > best_eval || side == Side::Black && eval < best_eval{
                best_eval = eval;
                best_move = moves[i];
            }
             println!("{:?} side={:?} eval(post-penalty)={}", moves[i], side, eval); 
        }
        println!("CHOSE {:?} best_eval={}", best_move, best_eval);
        println!("single-threaded minimax took {}ms to think", start.elapsed().as_millis());

        // add our move to our move history 
        make_move(&mut b, best_move.0, best_move.1);
        mh_guard.push(self.zobrist.hash(&b));

        best_move
    }

    fn reset(&self) {
        self.move_history.lock().unwrap().clear();
    }
}

fn minimax(board: &mut Board, depth: usize, ply: i32, mut alpha: i32, mut beta: i32,
        ztable: &ZobristTable,
        tt: &mut FxHashMap<u64, TTEntry>) -> i32 {
    let side = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
    let hash = ztable.hash(board);
    let alpha_orig = alpha;
    let beta_orig = beta;

    // Only trust an entry searched at least as deep as we need.
    if let Some(entry) = tt.get(&hash) {
        if entry.depth >= depth {
            match entry.bound {
                Bound::Exact => return entry.value,
                Bound::Lower => alpha = alpha.max(entry.value),
                Bound::Upper => beta  = beta.min(entry.value),
            }
            if alpha >= beta { return entry.value; }
        }
    } 

    if depth == 0 {
        return quiescence(board, ply, alpha, beta, ztable, tt);
    }

    let mut moves = get_all_moves(board, side);
    

    // move ordering
    // victim value * 10 - attacker value
    moves.sort_by_key(|mv| {
        match if is_piece(board, mv.1) {Some(get_piece(board, mv.1))} else {None} {
            Some(victim) => {
                let attacker_val = match if is_piece(board, mv.0) {Some(get_piece(board, mv.0))} else {None} {
                    Some(a) => material_value(a.piece_type),
                    None => 0,
                };
                -(material_value(victim.piece_type) * 10 - attacker_val)
            }
            None => 0,
        }
    });

    // mate check
    if moves.is_empty() {
        if is_in_check(board, side) {
            // side to move is mated; score from White's perspective
            return if board.state & WHITE_TO_MOVE != 0 { -500000 + ply  }
                else                 {  500000 - ply  };
        } else {
            return 0; // stalemate
        }
    }

    let value = if board.state & WHITE_TO_MOVE != 0 {
        let mut eval = i32::MIN;
        for mv in &moves {
            let undo = make_move(board, mv.0, mv.1);
            eval = minimax(board, depth -1, ply + 1, alpha, beta, ztable, tt).max(eval);
            undo_move(board, undo);

            alpha = alpha.max(eval);
            if alpha >= beta { break; }
        }
        eval
    } else {
        let mut eval = i32::MAX;
        for mv in moves {
            let undo = make_move(board, mv.0, mv.1);
            eval = minimax(board, depth - 1, ply + 1, alpha, beta, ztable, tt).min(eval);
            undo_move(board, undo);

            beta = beta.min(eval);
            if beta <= alpha { break; }
        }
        eval
    };

    let bound = if value <= alpha_orig { Bound::Upper }
            else if value >= beta_orig { Bound::Lower }
            else { Bound::Exact };
    tt.insert(hash, TTEntry { depth, value, bound });
    value
}

fn quiescence(
    board: &mut Board,
    ply: i32,
    mut alpha: i32,
    mut beta: i32,
    ztable: &ZobristTable,
    tt: &mut FxHashMap<u64, TTEntry>,
) -> i32 {
    let side = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
    let in_check = is_in_check(board, side);

    // Stand-pat — but only if we're not in check (can't "pass" out of check)
    let stand_pat = evaluate(board);
    if !in_check {
        if side == Side::White {
            if stand_pat >= beta { return beta; }
            if stand_pat > alpha { alpha = stand_pat; }
        } else {
            if stand_pat <= alpha { return alpha; }
            if stand_pat < beta { beta = stand_pat; }
        }
    }

    // Generate moves. If in check, search ALL moves. Otherwise only captures.
    let mut all_legal: bool = false;
    let mut moves: Vec<(u8, u8)> = Vec::new();
    
    if in_check {
        moves = get_all_moves(board, side);
        all_legal = true;
    } else {
        moves = get_all_captures(board, side)  
    };

    // Mate / stalemate detection when in check with no legal moves
    if moves.is_empty() {
        if in_check {
            return if side == Side::White { -500000 + ply } else { 500000 - ply };
        }
        else if all_legal{
            return 0;
        }
        return stand_pat; // quiet position, no captures to consider
    }

    // MVV-style ordering: capture biggest victim first
    moves.sort_by_key(|mv| match if is_piece(board, mv.1) {Some(get_piece(board, mv.1))} else {None} {
        Some(p) => -piece_value(p, mv.1, board.moves),
        None => 0,
    });

    if side == Side::White {
        for mv in moves {
            let undo = make_move(board, mv.0, mv.1);
            let score = quiescence(board, ply + 1, alpha, beta, ztable, tt);
            undo_move(board, undo);
            if score >= beta { return beta; }
            if score > alpha { alpha = score; }
        }
        alpha
    } else {
        for mv in moves {
            let undo = make_move(board, mv.0, mv.1);
            let score = quiescence(board, ply + 1, alpha, beta, ztable, tt);
            undo_move(board, undo);
            if score <= alpha { return alpha; }
            if score < beta { beta = score; }
        }
        beta
    }
}

pub fn evaluate(board: &Board) -> i32{
    let mut score: i32 = 0;
    for i in 0..64 {
        if is_piece(board, i) {
            let p = get_piece(board, i);
            if p.color == Side::White {
                score += piece_value(p, i, board.moves);
            } else {
                score -= piece_value(p, i, board.moves);
            }
        }
    }

    score
}

fn piece_value(piece: Piece, coord: u8, moves: u8) -> i32{
    if piece.color == Side::White{
        match piece.piece_type {
            PieceType::Pawn   => if moves < 60 {return 100 + PAWN_TABLE[63 - coord as usize]}          else {return 100 + PAWN_TABLE_LATE[63 - coord as usize]},
            PieceType::Knight => if moves < 55 {return 305 + KNIGHT_TABLE[63 - coord as usize]}        else {return 275 + KNIGHT_TABLE[63 - coord as usize]},
            PieceType::Bishop => if moves < 50 { return 333 + BISHOP_TABLE[63 - coord as usize] }      else {return 350 + BISHOP_TABLE_LATE[63 - coord as usize]},
            PieceType::Rook   => if moves < 60 {return 563 + ROOK_TABLE[63 - coord as usize]}          else {return 570 + ROOK_TABLE_LATE[63 - coord as usize]},
            PieceType::Queen  => if moves < 16 {return 950 + QUEEN_TABLE_EARLY[63 - coord as usize]}   else {return 950 + QUEEN_TABLE_LATE[ 63 - coord as usize]},
            PieceType::King   => if moves < 40 {return 100000 + KING_TABLE_EARLY[63 - coord as usize]} else {return 100000 + KING_TABLE_LATE[63 - coord as usize] },
        };
    }else{
        match piece.piece_type {
            PieceType::Pawn   => if moves < 60 {return 100 + PAWN_TABLE[coord as usize]}          else {return 100 + PAWN_TABLE_LATE[coord as usize]},
            PieceType::Knight => if moves < 55 {return 305 + KNIGHT_TABLE[coord as usize]}        else {return 250 + KNIGHT_TABLE[coord as usize]},
            PieceType::Bishop => if moves < 50 { return 333 + BISHOP_TABLE[coord as usize] }      else {return 350 + BISHOP_TABLE_LATE[coord as usize]},
            PieceType::Rook   => if moves < 60 {return 563 + ROOK_TABLE[coord as usize]}          else {return 570 + ROOK_TABLE_LATE[coord as usize]},
            PieceType::Queen  => if moves < 16 {return 950 + QUEEN_TABLE_EARLY[coord as usize]}   else {return 950 + QUEEN_TABLE_LATE[coord as usize]},
            PieceType::King   => if moves < 40 {return 100000 + KING_TABLE_EARLY[coord as usize]} else {return 100000 + KING_TABLE_LATE[coord as usize] },
        };
    }

}

fn material_value(p_type: PieceType) -> i32{
    match p_type {
            PieceType::Pawn   => 100,
            PieceType::Knight => 300,
            PieceType::Bishop => 300,
            PieceType::Rook   => 500,
            PieceType::Queen  => 900,
            PieceType::King   => 0
    }
}


fn build_book() -> FxHashMap<String, Vec<(u8, u8)>>{
    let mut book: FxHashMap<String, Vec<(u8, u8)>> = FxHashMap::default();

    let file = File::open("assets/book.txt").expect("engine's opening book is missing");
    let reader = BufReader::new(file);

    let mut last_fen = String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    for line in reader.lines() {
        let line = line.expect("failed to read line"); 
        if line.is_empty() || line.starts_with('#'){
            continue;
        }
        
        if line.starts_with('$'){
            let raw = line.replace("$", "");
            let parts: Vec<&str> = raw.split_whitespace().collect();
            if parts.len() < 4 {
                continue; // skip malformed
            }
            last_fen = parts[..4].join(" ");
            continue;
        }

        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            let from = square_to_coord(parts[0]);
            let to = square_to_coord(parts[1]);

            book.entry(last_fen.clone())
                .or_insert_with(Vec::new)
                .push((from.0 * 8 + from.1, to.0 * 8 + to.1));
        }
    }
    book
}

fn square_to_coord(sq: &str) -> (u8, u8) {
    let bytes = sq.as_bytes();
    let col = bytes[0] - b'a';        // 'a' -> 0, 'h' -> 7
    let row = (bytes[1] - b'0') - 1;  
    (row, col)
}

#[derive(Clone, Copy)]
enum Bound { Exact, Lower, Upper }

#[derive(Clone, Copy)]
struct TTEntry { depth: usize, value: i32, bound: Bound }

struct ZobristTable {
    pieces: [[[u64; 64]; 2]; 6],
    black_to_move: u64,
    en_passant_file: [u64; 8],
    castling: [u64; 4],  // WK, WQ, BK, BQ
}

impl ZobristTable {
    fn new() -> Self {
        let mut pieces = [[[0u64; 64]; 2]; 6];
        for pt in 0..6 {
            for color in 0..2 {
                for sq in 0..64 {
                    pieces[pt][color][sq] = rand::gen_range(0, u64::MAX);
                }
            }
        }

        let mut en_passant_file = [0u64; 8];
        for f in 0..8 {
            en_passant_file[f] = rand::gen_range(0, u64::MAX);
        }

        let mut castling = [0u64; 4];
        for i in 0..4 {
            castling[i] = rand::gen_range(0, u64::MAX);
        }

        ZobristTable {
            pieces,
            black_to_move: rand::gen_range(0, u64::MAX),
            en_passant_file,
            castling,
        }
    }

    fn hash(&self, board: &Board) -> u64 {
        let mut h: u64 = 0;
        for sq in 0..64 {
            if is_piece(&board, sq) {
                let p = get_piece(&board, sq);
                h ^= self.pieces[p.piece_type as usize][p.color as usize][sq as usize];
            }
        }
        if board.state & WHITE_TO_MOVE == 0 { h ^= self.black_to_move; }

        if let Some(target) = board.en_passant_target {
            h ^= self.en_passant_file[(target % 8) as usize];
        }

        if board.state & WHITE_SHORT != 0  { h ^= self.castling[0]; }
        if board.state & WHITE_LONG != 0 { h ^= self.castling[1]; }
        if board.state & BLACK_SHORT != 0 { h ^= self.castling[2]; }
        if board.state & BLACK_LONG != 0 { h ^= self.castling[3]; }

        h
    }

}

// ALL OF THESE ARE FROM WHITES PERSPECTIVE, USE 7 - ROW WHEN INDEXING FOR BLACK

const PAWN_TABLE: [i32; 64] = [
    0,    0,    0,    0,    0,    0,    0,    0   ,  // promotion 
    90,   90,   90,   90,   90,   90,   90,   90  ,
    25,   25,   50,   55,   55,   50,   25,   25  ,
    10,   10,   25,   50,   50,   25,   10,   10  ,
    5,    5,    25,   45,   45,   25,   5,    5   ,
    5,    5,    10,   5,    5,   10,    5,    5   ,
    5,    5,    5,   -10,  -10,   5,    5,    5   ,  // slight penalty for blocking center
    0,    0,    0,    0,    0,    0,    0,    0   ,  // starting rank
];

const PAWN_TABLE_LATE: [i32; 64] = [
    0,    0,    0,    0,    0,    0,    0,    0  ,  // PROMOTE PROMOTE PROMOTE
    120,  120,  120,  120,  120,  120,  120,  120  ,  // 
    70,   70,   70,   70,   70,   70,   70,   70  ,  // 
    40,   40,   40,   40,   40,   40,   40,   40  ,  // 
    25,   25,   25,   25,   25,   25,   25,   25  ,  // 
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