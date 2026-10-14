use rustc_hash::FxHashMap;
use std::any::Any;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ai::Player;
use crate::board::{get_piece, to_fen};
use crate::datagen;
use crate::eval::material_value;
use crate::movegen::{find_legal_move, see_ge};
use crate::search::{INFINITY, Limits, MAX_HISTORY, Searcher};
use crate::types::{Board, Move, MoveContext, MoveListBuf, WHITE_TO_MOVE};

use macroquad::prelude::*;

const TT_SIZE_MB: usize = 128; // rounded down to a power of two worth of buckets

pub struct Engine {
    pub max_depth: usize,
    opening_book: FxHashMap<String, Vec<(u8, u8)>>,
    searcher: Mutex<Searcher>,
}

impl Engine {
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            opening_book: build_book(),
            searcher: Mutex::new(Searcher::new(TT_SIZE_MB)),
        }
    }
}

impl Player for Engine {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_move(&self, context: MoveContext) -> Move {
        let mut b = context.board.clone();
        // book lookup
        let full_fen = to_fen(&context.board);
        let fen = full_fen
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(moves) = self.opening_book.get(&fen) {
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos() as u64;
            let mv = moves
                .get(datagen::Rng(time).below(moves.len()))
                .expect("invalid book index");
            if let Some(valid) = find_legal_move(&mut b, mv.0, mv.1, None) {
                return valid;
            } else {
                println!("book provided invalid move");
            }
        }

        let time_budget = compute_time_budget(&context);
        let limits = Limits {
            time_ms: if context.wtime.as_millis() == 0 && context.btime.as_millis() == 0 {
                0
            } else {
                time_budget as u64
            },
            max_nodes: context.max_nodes,
            max_depth: self.max_depth.min(context.depth as usize) as i32,
            uci_info: true,
        };

        let mut searcher_guard = self.searcher.lock().unwrap();
        match searcher_guard.search(&b, &context.game_hashes, &limits) {
            Some(result) => result.result,
            None => Move::NULL,
        }
    }

    fn reset(&self) {
        self.searcher.lock().unwrap().clear();
    }
}

pub fn update_history(entry: &mut i32, bonus: i32) {
    *entry += bonus - *entry * bonus.abs() / MAX_HISTORY;
}

// returns the number of elements that aren't in the else catch
pub fn score_moves_full(
    board: &Board,
    moves: &mut MoveListBuf,
    scores: &mut [i32; 218],
    tt_move: Option<Move>,
    killers: [Move; 2],
    history: &[[[i32; 64]; 64]; 2],
) -> usize {
    let side = board.side_to_move();
    for i in 0..moves.len {
        let mv = moves.data[i];

        scores[i] = if tt_move == Some(mv) {
            -INFINITY
        } else if board.is_piece(mv.get_to()) {
            let victim = material_value(get_piece(board, mv.get_to()).piece_type);
            let attacker = material_value(get_piece(board, mv.get_from()).piece_type);
            let mvv_lva = victim * 10 + attacker;
            if victim >= attacker {
                -(1_000_000 + mvv_lva)
            } else {
                if see_ge(board, mv, 0) {
                    -(1_000_000 + mvv_lva)
                } else {
                    500_000 - mvv_lva
                }
            }
        } else if mv == killers[0] || mv == killers[1] {
            -900_000
        } else {
            -history[side as usize][mv.get_from() as usize][mv.get_to() as usize]
        };
    }
    moves.len
}

pub fn score_moves_mvv_lva_tt(
    board: &Board,
    moves: &MoveListBuf,
    scores: &mut [i32; 218],
    tt_move: Option<Move>,
) {
    for i in 0..moves.len {
        let mv = moves.data[i];
        //  TT Move
        if tt_move == Some(mv) {
            scores[i] = -INFINITY;
        }
        // Captures
        else if board.is_piece(mv.get_to()) {
            let victim = material_value(get_piece(board, mv.get_to()).piece_type);
            let attacker = material_value(get_piece(board, mv.get_from()).piece_type);
            let mvv_lva = victim * 10 + attacker;
            if victim >= attacker {
                scores[i] = -(1_000_000 + mvv_lva);
            } else {
                if see_ge(board, mv, 0) {
                    scores[i] = -(1_000_000 + mvv_lva);
                } else {
                    scores[i] = 500_000 - mvv_lva;
                }
            }
        }
        // Quiet
        else {
            scores[i] = 0;
        }
    }
}

pub fn score_moves_mvv_lva(board: &Board, moves: &MoveListBuf, scores: &mut [i32; 218]) {
    for i in 0..moves.len {
        let mv = moves.data[i];
        //  Captures
        if board.is_piece(mv.get_to()) {
            let victim = material_value(get_piece(board, mv.get_to()).piece_type);
            let attacker = material_value(get_piece(board, mv.get_from()).piece_type);
            let mvv_lva = victim * 10 + attacker;
            if victim >= attacker {
                scores[i] = -(1_000_000 + mvv_lva);
            } else {
                if see_ge(board, mv, 0) {
                    scores[i] = -(1_000_000 + mvv_lva);
                } else {
                    scores[i] = 500_000 - mvv_lva;
                }
            }
        }
        // Quiet
        else {
            scores[i] = 0;
        }
    }
}

// does a pass of selection sort
// leaving the next lowest score at index 'start'
// INPUTS:
//    moves: list of moves
//    scores: the score of the corresponding index of move
//    start: how many passes have previously happened
//    end: when does the sort end (typically moves.len)
pub fn pick_move(moves: &mut MoveListBuf, scores: &mut [i32; 218], start: usize, end: usize) {
    let mut lowest = start;
    for i in start + 1..end {
        if scores[i] < scores[lowest] {
            lowest = i;
        }
    }

    if lowest != start {
        scores.swap(lowest, start);
        moves.data.swap(lowest, start);
    }
}

fn compute_time_budget(context: &MoveContext) -> u128 {
    const SAFETY_MARGIN_MS: u128 = 50; // never spend all time
    const MIN_BUDGET_MS: u128 = 20; // min time

    let moves_left: u128 = if context.moves_to_go > 0 {
        context.moves_to_go as u128
    } else {
        if context.board.moves < 40 { 30 } else { 15 }
    };
    let is_white = context.board.state & WHITE_TO_MOVE != 0;

    let time_left_ms = if is_white {
        context.wtime.as_millis()
    } else {
        context.btime.as_millis()
    };
    let increment_ms = if is_white {
        context.winc.as_millis()
    } else {
        context.binc.as_millis()
    };
    let base = time_left_ms / moves_left;
    let budget = base + increment_ms;

    let budget = budget.min(time_left_ms.saturating_sub(SAFETY_MARGIN_MS));

    budget.max(MIN_BUDGET_MS)
}

fn build_book() -> FxHashMap<String, Vec<(u8, u8)>> {
    let mut book: FxHashMap<String, Vec<(u8, u8)>> = FxHashMap::default();

    let book_str = include_str!("../assets/book.txt");

    let mut last_fen = String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    for line in book_str.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('$') {
            let raw = line.trim_start_matches('$');
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
                .or_default()
                .push((from.0 * 8 + from.1, to.0 * 8 + to.1));
        }
    }
    book
}

fn square_to_coord(sq: &str) -> (u8, u8) {
    let bytes = sq.as_bytes();
    let col = bytes[0] - b'a'; // 'a' -> 0, 'h' -> 7
    let row = (bytes[1] - b'0') - 1;
    (row, col)
}
