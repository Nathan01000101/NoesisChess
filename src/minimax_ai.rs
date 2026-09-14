use std::any::Any;
use rustc_hash::FxHashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;
use std::time::{Instant};
use crate::types::{Board, PieceType, Piece, Side, Undo, BLACK_SHORT, BLACK_LONG, WHITE_SHORT, WHITE_LONG, WHITE_TO_MOVE, MoveListBuf};
use crate::board::{get_piece, make_move, undo_move, to_fen};
use crate::movegen::{get_all_moves, get_all_captures, is_in_check};

use crate::ai::Player;

use macroquad::{ prelude::*};
use macroquad::miniquad::date;

const MOVE_REPETITION_PENALTY: i32 = 25;
const MAX_TABLE_SIZE: usize = 10_000_000;

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

        println!("tt table is %{:.2} full", (self.tt.lock().unwrap().len() as f32/MAX_TABLE_SIZE as f32) * 100.0);
        // cap transposition table growth
        if self.tt.lock().unwrap().len() > MAX_TABLE_SIZE {   
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
        let root_hash = self.zobrist.hash(board);
        let mut best_move: (u8, u8) = (64, 64);
        let mut best_hash: u64 = 0;

        let mut best_eval: i32 = if side == Side::White {i32::MIN} else {i32::MAX};

        let mut alpha = i32::MIN;
        let mut beta = i32::MAX;

        rand::srand(date::now() as u64);
        let mut moves = MoveListBuf::new() ;
        get_all_moves(&mut b, side, &mut moves);
        println!("root move count: {}", moves.len);

        // move ordering
        // victim_value * 10 - attacker_value
        moves.data[..moves.len].sort_by_key(|mv| {
            match if board.is_piece(mv.1) {Some(get_piece(board, mv.1))} else {None} {
                Some(victim) => {
                    let attacker_val = match if board.is_piece(mv.0) {Some(get_piece(board, mv.0))} else {None} {
                        Some(a) => material_value(a.piece_type),
                        None => 0,
                    };
                    -(material_value(victim.piece_type) * 10 - attacker_val)
                }
                None => 0,
            }
        });

        for i in 0..moves.len{
            let undo = make_move(&mut b, moves.data[i].0,moves.data[i].1);
            let child_hash = self.zobrist.update_hash(root_hash, &b, &undo);
            let bonus = calculate_depth_bonus(i as u8, moves.len as u8);
            let mut eval = minimax(&mut b, child_hash, self.depth.saturating_sub((1 - bonus) as usize), 1,  alpha, beta, &self.zobrist, tt);

            // see if the move already exists in our move history
            if mh_guard.contains(&child_hash){
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
                best_move = moves.data[i];
                best_hash = child_hash;
            }
             println!("{:?} side={:?} eval(post-penalty)={}", moves.data[i], side, eval); 
        }
        println!("CHOSE {:?} best_eval={}", best_move, best_eval);
        println!("single-threaded minimax took {}ms to think", start.elapsed().as_millis());

        // add our move to our move history 
        make_move(&mut b, best_move.0, best_move.1);
        mh_guard.push(best_hash);

        best_move
    }

    fn reset(&self) {
        self.move_history.lock().unwrap().clear();
    }
}

fn minimax(board: &mut Board, hash: u64, depth: usize, ply: i32, mut alpha: i32, mut beta: i32,
        ztable: &ZobristTable,
        tt: &mut FxHashMap<u64, TTEntry>) -> i32 {
    let side = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
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

    let mut moves = MoveListBuf::new();
    get_all_moves(board, side, &mut moves);
    

    // move ordering
    // victim value * 10 - attacker value
    moves.data[..moves.len].sort_by_key(|mv| {
        match if board.is_piece(mv.1) {Some(get_piece(board, mv.1))} else {None} {
            Some(victim) => {
                let attacker_val = match if board.is_piece(mv.0) {Some(get_piece(board, mv.0))} else {None} {
                    Some(a) => material_value(a.piece_type),
                    None => 0,
                };
                -(material_value(victim.piece_type) * 10 - attacker_val)
            }
            None => 0,
        }
    });

    // mate check
    if moves.len == 0 {
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
        for i in 0..moves.len {
            let undo = make_move(board, moves.data[i].0, moves.data[i].1);
            let child_hash = ztable.update_hash(hash, board, &undo);
            eval = minimax(board, child_hash, depth -1, ply + 1, alpha, beta, ztable, tt).max(eval);
            undo_move(board, undo);

            alpha = alpha.max(eval);
            if alpha >= beta { break; }
        }
        eval
    } else {
        let mut eval = i32::MAX;
        for i in 0..moves.len {
            let undo = make_move(board, moves.data[i].0, moves.data[i].1);
            let child_hash = ztable.update_hash(hash, board, &undo);
            eval = minimax(board, child_hash, depth - 1, ply + 1, alpha, beta, ztable, tt).min(eval);
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
    let mut moves: MoveListBuf = MoveListBuf::new();
    
    if in_check {
        get_all_moves(board, side, &mut moves);
        all_legal = true;
    } else {
        get_all_captures(board, side, &mut moves);  
    };

    // Mate / stalemate detection when in check with no legal moves
    if moves.len == 0 {
        if in_check {
            return if side == Side::White { -500000 + ply } else { 500000 - ply };
        }
        else if all_legal{
            return 0;
        }
        return stand_pat; // quiet position, no captures to consider
    }

    // MVV-style ordering: capture biggest victim first
    moves.data[..moves.len].sort_by_key(|mv| match if board.is_piece(mv.1) {Some(get_piece(board, mv.1))} else {None} {
        Some(p) => -piece_value(p, mv.1, board.moves),
        None => 0,
    });

    if side == Side::White {
        for i in 0..moves.len {
            let undo = make_move(board, moves.data[i].0, moves.data[i].1);
            let score = quiescence(board, ply + 1, alpha, beta, ztable, tt);
            undo_move(board, undo);
            if score >= beta { return beta; }
            if score > alpha { alpha = score; }
        }
        alpha
    } else {
        for i in 0..moves.len {
            let undo = make_move(board, moves.data[i].0, moves.data[i].1);
            let score = quiescence(board, ply + 1, alpha, beta, ztable, tt);
            undo_move(board, undo);
            if score <= alpha { return alpha; }
            if score < beta { beta = score; }
        }
        beta
    }
}

pub fn evaluate(board: &Board) -> i32 {
    let mut score = 0;
    for color in 0..2 {
        let side = if color == 0 { Side::White } else { Side::Black };
        for pt in 0..6 {
            let piece_type = [PieceType::King, PieceType::Queen, PieceType::Rook,
                               PieceType::Bishop, PieceType::Knight, PieceType::Pawn][pt];
            let mut bb = board.bitboards[side as usize][piece_type as usize];
            while bb.0 != 0 {
                let sq = bb.0.trailing_zeros() as u8;
                let val = piece_value(Piece { piece_type, color: side }, sq, board.moves);
                score += if side == Side::White { val } else { -val };
                bb.0 &= bb.0 - 1;
            }
        }
    }
    score
}

fn calculate_depth_bonus(move_index: u8, root_moves: u8) -> i32{
    let mut bonus = 0;

    if move_index as f32 / root_moves as f32 > 0.7{
        bonus -= 1;
    }

    if root_moves < 10{
        bonus += 1;
    }
    bonus
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
            if board.is_piece(sq) {
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

    fn update_hash(&self, hash: u64, board: &Board, undo: &Undo) -> u64 {
        let mut h = hash;
        let from = undo.last_move.from;
        let to = undo.last_move.to;
        let moved = undo.moving_piece_before;
        let side = moved.color;

        h ^= self.pieces[moved.piece_type as usize][side as usize][from as usize];

        if let Some(captured) = undo.captured_piece {
            let is_en_passant = moved.piece_type == PieceType::Pawn
                && undo.previous_en_passant_target == Some(to)
                && to % 8 != from % 8;

            let captured_sq = if is_en_passant {
                let delta_col = (to % 8) as i16 - (from % 8) as i16;
                (from as i16 + delta_col) as u8
            } else {
                to
            };
            h ^= self.pieces[captured.piece_type as usize][captured.color as usize][captured_sq as usize];
        }

        let final_type = if moved.piece_type == PieceType::Pawn && (to < 8 || to > 55) {
            PieceType::Queen
        } else {
            moved.piece_type
        };
        h ^= self.pieces[final_type as usize][side as usize][to as usize];

        if moved.piece_type == PieceType::King {
            let delta = to as i32 - from as i32;
            let row = to / 8;
            if delta == 2 {
                h ^= self.pieces[PieceType::Rook as usize][side as usize][(row * 8 + 7) as usize];
                h ^= self.pieces[PieceType::Rook as usize][side as usize][(row * 8 + 5) as usize];
            } else if delta == -2 {
                h ^= self.pieces[PieceType::Rook as usize][side as usize][(row * 8) as usize];
                h ^= self.pieces[PieceType::Rook as usize][side as usize][(row * 8 + 3) as usize];
            }
        }

        h ^= self.black_to_move;

        if let Some(prev_ep) = undo.previous_en_passant_target {
            h ^= self.en_passant_file[(prev_ep % 8) as usize];
        }
        if let Some(new_ep) = board.en_passant_target {
            h ^= self.en_passant_file[(new_ep % 8) as usize];
        }

        let (prev, cur) = (undo.previous_state, board.state);
        if (prev & WHITE_SHORT) != (cur & WHITE_SHORT) { h ^= self.castling[0]; }
        if (prev & WHITE_LONG)  != (cur & WHITE_LONG)  { h ^= self.castling[1]; }
        if (prev & BLACK_SHORT) != (cur & BLACK_SHORT) { h ^= self.castling[2]; }
        if (prev & BLACK_LONG)  != (cur & BLACK_LONG)  { h ^= self.castling[3]; }

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