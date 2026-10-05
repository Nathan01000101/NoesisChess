use rustc_hash::FxHashMap;
use std::any::Any;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ai::Player;
use crate::board::{
    castle_rook_squares, ep_victim_square, get_piece, has_non_pawn_piece, make_move, to_fen,
    undo_move,
};
use crate::eval::{eval_stm, material_value};
use crate::movegen::{find_legal_move, get_all_captures, get_all_moves, is_in_check};
use crate::types::{
    BLACK_LONG, BLACK_SHORT, Board, Move, MoveContext, MoveListBuf, Piece, PieceType, Side, Undo,
    WHITE_LONG, WHITE_SHORT, WHITE_TO_MOVE,
};
use crate::{app, types};

use macroquad::miniquad::date;
use macroquad::prelude::*;

const MOVE_REPETITION_PENALTY: i32 = 25;
const MAX_TABLE_SIZE_BYTES: usize = 1_073_741_824; // 1gb
const NODES_PER_TIME_CHECK: u64 = 0x7FF; // check clock every 2048 nodes

const INFINITY: i32 = 1_000_000;
const MATE: i32 = 500_000;
const MATE_THRESHOLD: i32 = MATE - 1_000; // |score| above this means "this is a mate score"

// Null move pruning knobs.
const NULL_MIN_DEPTH: i32 = 3;
const NULL_BASE_REDUCTION: i32 = 2;

// Mobility scoring

pub struct Engine {
    pub depth: usize,
    opening_book: FxHashMap<String, Vec<(u8, u8)>>,
    zobrist: ZobristTable,
    tt: Mutex<FxHashMap<u64, TTEntry>>,
    pub move_history: Mutex<Vec<u64>>,
}

impl Engine {
    pub fn new(depth: usize) -> Self {
        rand::srand(date::now() as u64);
        Self {
            depth,
            opening_book: build_book(),
            zobrist: ZobristTable::new(),
            tt: Mutex::new(FxHashMap::default()),
            move_history: Mutex::new(Vec::new()),
        }
    }
}

impl Player for Engine {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_move(&self, context: MoveContext) -> Move {
        let start = Instant::now();
        let mut b = context.board.clone();
        let side = if context.board.state & WHITE_TO_MOVE != 0 {
            Side::White
        } else {
            Side::Black
        };

        let time_budget = compute_time_budget(&context);

        // cap transposition table growth
        let mut tt_unlock = self.tt.lock().unwrap();
        if tt_unlock.len() * size_of::<TTEntry>() > MAX_TABLE_SIZE_BYTES {
            tt_unlock.clear();
        }
        println!(
            "info hashfull {}",
            ((tt_unlock.len() as f32 * size_of::<TTEntry>() as f32 / MAX_TABLE_SIZE_BYTES as f32)
                * 1000.0) as i32
        );
        drop(tt_unlock);

        // before calculating move manually, check if position exists in our opening book
        let full_fen: String = to_fen(&context.board);
        let parts: Vec<&str> = full_fen.split_whitespace().collect();
        let fen = parts[..4].join(" ");

        rand::srand(date::now() as u64);
        if let Some(mvs) = self.opening_book.get(&fen) {
            if mvs.len() > 0 {
                let mv = mvs.get(rand::gen_range(0, mvs.len())).unwrap();
                if let Some(valid) = find_legal_move(&mut b, mv.0, mv.1, None) {
                    return valid;
                }
            }
        }

        let mut tt_guard = self.tt.lock().unwrap();
        let tt: &mut FxHashMap<u64, TTEntry> = &mut *tt_guard;

        let mut mh_guard = self.move_history.lock().unwrap();

        let root_hash = self.zobrist.hash(&context.board);

        let mut moves = MoveListBuf::new();
        get_all_moves(&mut b, side, &mut moves);
        let stop_sort_at = moves.len;

        if stop_sort_at == 0 {
            // mated or stalemated at the root.. caller shouldn't have asked for move
            return Move::NULL;
        }

        // initial root ordering, MVV-LVA-TT, killers aren't populated so don't need to consider them
        let mut killers = [[Move::NULL; 2]; 64];
        let tt_move = tt.get(&root_hash).map(|entry| entry.best_move);
        let mut move_scores = [0; 218];
        score_moves_mvv_lva_tt(&b, &moves, &mut move_scores, tt_move);

        let mut best_move: Move = moves.data[0];
        let mut best_move_idx: Option<usize> = None;
        let mut best_hash: u64 = 0;

        let mut control = SearchControl::new(start, time_budget);
        let mut current_depth: i32 = 1;

        loop {
            let mut moves_sorted = false;
            if best_move_idx.is_some() {
                move_scores[best_move_idx.unwrap()] = -INFINITY * 2 - current_depth;
            }

            let mut depth_best_move = best_move;
            let mut depth_best_move_idx = best_move_idx;
            let mut depth_best_hash = best_hash;
            let mut depth_best_score = -INFINITY;

            // set when a move after the first one is proven better with a
            // completed full-window search. lets us keep it if we abort.
            let mut improved_on_first = false;

            let mut alpha = -INFINITY;
            let beta = INFINITY;
            let mut depth_completed = true;

            for i in 0..moves.len {
                if !moves_sorted {
                    if bubble_pass(&mut moves, &mut move_scores, i, stop_sort_at) {
                        moves_sorted = true;
                    }
                }

                let mv = moves.data[i];
                let undo = make_move(&mut b, mv);
                let child_hash = self.zobrist.update_hash(root_hash, &b, mv, &undo);

                // PVS: first move (previous best) gets the full window,
                // every other move is only tested against alpha.
                let mut score = if i == 0 {
                    -negamax(
                        &mut b,
                        child_hash,
                        current_depth - 1,
                        1,
                        -beta,
                        -alpha,
                        true,
                        &self.zobrist,
                        tt,
                        &mut killers,
                        &mut control,
                    )
                } else {
                    -negamax(
                        &mut b,
                        child_hash,
                        current_depth - 1,
                        1,
                        -alpha - 1,
                        -alpha,
                        true,
                        &self.zobrist,
                        tt,
                        &mut killers,
                        &mut control,
                    )
                };

                // move beat alpha in the null window -> get its real score.
                // decided on the raw score, before the repetition penalty.
                if i != 0 && !control.aborted && score > alpha && score < beta {
                    score = -negamax(
                        &mut b,
                        child_hash,
                        current_depth - 1,
                        1,
                        -beta,
                        -alpha,
                        true,
                        &self.zobrist,
                        tt,
                        &mut killers,
                        &mut control,
                    );
                }

                undo_move(&mut b, mv, undo);

                if control.aborted {
                    depth_completed = false;
                    break;
                }

                // check for repetition (applied after the final search)
                if mh_guard.contains(&child_hash) {
                    score -= MOVE_REPETITION_PENALTY;
                }

                if score > depth_best_score {
                    depth_best_score = score;
                    depth_best_move = mv;
                    depth_best_move_idx = Some(i);
                    depth_best_hash = child_hash;
                    if i != 0 {
                        improved_on_first = true;
                    }
                }
                if score > alpha {
                    alpha = score;
                }
            }

            if depth_completed {
                best_move = depth_best_move;
                best_move_idx = depth_best_move_idx;
                best_hash = depth_best_hash;

                // UCI info
                println!(
                    "info depth {} time {} score cp {} nodes {} nps {}",
                    current_depth,
                    start.elapsed().as_millis(),
                    depth_best_score,
                    control.nodes,
                    (control.nodes as f32 / start.elapsed().as_secs_f32()) as i32
                );
            } else {
                // a later move was fully searched and beat the previous best at
                // this depth, so it's trustworthy even though the iteration aborted
                if improved_on_first {
                    best_move = depth_best_move;
                    best_hash = depth_best_hash;
                    println!(
                        "info string depth {} aborted, using improved move from partial search",
                        current_depth
                    );
                } else {
                    println!(
                        "info string depth {} aborted, keeping depth {} result",
                        current_depth,
                        current_depth - 1
                    );
                }
                break;
            }

            current_depth += 1;
            if current_depth > self.depth as i32 {
                break; // dont exceed max depth
            }

            // do not start next iter if we dont have time.
            if start.elapsed().as_millis() * 3 >= time_budget {
                break;
            }
        }

        println!(
            "info string thinking took {}ms",
            start.elapsed().as_millis()
        );

        // add our move to our move history
        make_move(&mut b, best_move);
        mh_guard.push(best_hash);

        best_move
    }

    fn reset(&self) {
        self.move_history.lock().unwrap().clear();
    }
}

struct SearchControl {
    start: Instant,
    budget: Duration,
    nodes: u64,
    aborted: bool,
}

impl SearchControl {
    fn new(start: Instant, budget_ms: u128) -> Self {
        Self {
            start,
            budget: Duration::from_millis(budget_ms as u64),
            nodes: 0,
            aborted: false,
        }
    }

    // only actually checks the clock every NODES_PER_TIME_CHECK nodes
    #[inline]
    fn poll(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        self.nodes += 1;
        if self.nodes & NODES_PER_TIME_CHECK == 0 {
            if self.start.elapsed() >= self.budget {
                self.aborted = true;
            }
        }
        self.aborted
    }
}

#[inline]
fn score_to_tt(value: i32, ply: i32) -> i32 {
    if value >= MATE_THRESHOLD {
        value + ply
    } else if value <= -MATE_THRESHOLD {
        value - ply
    } else {
        value
    }
}

#[inline]
fn score_from_tt(value: i32, ply: i32) -> i32 {
    if value >= MATE_THRESHOLD {
        value - ply
    } else if value <= -MATE_THRESHOLD {
        value + ply
    } else {
        value
    }
}

// board   -> current node state
// hash    -> current hash state
// depth   -> plies left (may go negative via reductions; <= 0 drops to quiescence)
// ply     -> plies searched so far
// alpha   -> best score the side to move is already guaranteed
// beta    -> score at which the opponent stops considering this line
// null_ok -> may we try a null move here (false directly under a null move)
//
// Everything is from the point of view of the side to move.
fn negamax(
    board: &mut Board,
    hash: u64,
    depth: i32,
    ply: i32,
    mut alpha: i32,
    mut beta: i32,
    null_ok: bool,
    ztable: &ZobristTable,
    tt: &mut FxHashMap<u64, TTEntry>,
    killers: &mut [[Move; 2]; 64],
    control: &mut SearchControl,
) -> i32 {
    if control.poll() {
        return 0; // discarded by caller once it sees control.aborted
    }

    let side = if board.state & WHITE_TO_MOVE != 0 {
        Side::White
    } else {
        Side::Black
    };

    // only trust an entry searched at least as deep as we need.
    let mut tt_move: Option<Move> = None;
    if let Some(entry) = tt.get(&hash) {
        tt_move = Some(entry.best_move);
        if entry.depth >= depth {
            let value = score_from_tt(entry.value, ply);
            match entry.bound {
                Bound::Exact => return value,
                Bound::Lower => alpha = alpha.max(value),
                Bound::Upper => beta = beta.min(value),
            }
            if alpha >= beta {
                return value;
            }
        }
    }

    // window we actually search with, for classifying the bound on store
    let alpha_orig = alpha;
    let beta_orig = beta;

    // if we have reached max depth, return base value, but make sure we don't
    // fall for horizon effect
    if depth <= 0 {
        return quiescence(board, ply, alpha, beta, control);
    }

    let in_check = is_in_check(board, side);

    // Null Move Prunning
    // If we hand the opponent a free move and are STILL at or > beta, then
    // our real best move is > beta too so why waste resources
    if null_ok
        && beta - alpha == 1
        && !in_check
        && depth >= NULL_MIN_DEPTH
        && beta.abs() < MATE_THRESHOLD
        && has_non_pawn_piece(board, side)
    {
        if eval_stm(board, side) >= beta {
            // make null -> flip side to move, drop en passant right
            let prev_ep = board.en_passant_target;
            let mut null_hash = hash ^ ztable.black_to_move;
            if let Some(ep) = prev_ep {
                null_hash ^= ztable.en_passant_file[(ep % 8) as usize];
            }
            board.en_passant_target = None;
            board.state ^= WHITE_TO_MOVE;

            let r = NULL_BASE_REDUCTION + depth / 6;
            // only care whether it beats beta
            let score = -negamax(
                board,
                null_hash,
                depth - 1 - r,
                ply + 1,
                -beta,
                -beta + 1,
                false,
                ztable,
                tt,
                killers,
                control,
            );

            // undo null
            board.state ^= WHITE_TO_MOVE;
            board.en_passant_target = prev_ep;

            if control.aborted {
                return 0;
            }

            if score >= beta {
                return beta;
            }
        }
    }

    let mut moves = MoveListBuf::new();
    let mut move_scores = [0; 218];
    get_all_moves(board, side, &mut moves);

    // mate / stalemate check, before we touch moves.data
    if moves.len == 0 {
        return if in_check { -MATE + ply } else { 0 };
    }

    // move value ordering + TT ordering + killer ordering
    // victim value * 10 + (6 - attacker value)
    let p = ply as usize;
    let this_ply_killers = if p < killers.len() {
        killers[p]
    } else {
        [Move::NULL; 2]
    };
    let stop_sort_at = score_moves_full(
        board,
        &mut moves,
        &mut move_scores,
        tt_move,
        this_ply_killers,
    );

    let mut best_move: Move = moves.data[0];
    let mut best = -INFINITY;

    let mut moves_sorted = false;
    for i in 0..moves.len {
        if !moves_sorted && i < stop_sort_at {
            moves_sorted = bubble_pass(&mut moves, &mut move_scores, i, stop_sort_at);
        }
        let mv = moves.data[i];
        let undo = make_move(board, mv);
        let child_hash = ztable.update_hash(hash, board, mv, &undo);

        let mut score = if i == 0 {
            -negamax(
                board,
                child_hash,
                depth - 1,
                ply + 1,
                -beta,
                -alpha,
                true,
                ztable,
                tt,
                killers,
                control,
            )
        } else {
            -negamax(
                board,
                child_hash,
                depth - 1,
                ply + 1,
                -alpha - 1,
                -alpha,
                true,
                ztable,
                tt,
                killers,
                control,
            )
        };

        if i != 0 && score > alpha && score < beta {
            // full search needed
            if !control.aborted {
                score = -negamax(
                    board,
                    child_hash,
                    depth - 1,
                    ply + 1,
                    -beta,
                    -alpha,
                    true,
                    ztable,
                    tt,
                    killers,
                    control,
                )
            }
        }

        undo_move(board, mv, undo);
        if control.aborted {
            return 0;
        }

        if score > best {
            best = score;
            best_move = mv;
        }
        if score > alpha {
            alpha = score;
        }

        if alpha >= beta {
            //tests whether the move was a capture
            if !board.is_piece(mv.get_to()) {
                if p < killers.len() && killers[p][0] != mv {
                    killers[p][1] = killers[p][0];
                    killers[p][0] = mv;
                }
            }
            break;
        }
    }

    let bound = if best <= alpha_orig {
        Bound::Upper
    } else if best >= beta_orig {
        Bound::Lower
    } else {
        Bound::Exact
    };
    let should_insert = match tt.get(&hash) {
        Some(entry) => depth >= entry.depth,
        None => true,
    };
    if should_insert {
        tt.insert(
            hash,
            TTEntry {
                depth,
                value: score_to_tt(best, ply),
                bound,
                best_move,
            },
        );
    }

    best
}

fn quiescence(
    board: &mut Board,
    ply: i32,
    mut alpha: i32,
    beta: i32,
    control: &mut SearchControl,
) -> i32 {
    if control.poll() {
        return 0;
    }

    let side = if board.state & WHITE_TO_MOVE != 0 {
        Side::White
    } else {
        Side::Black
    };
    let in_check = is_in_check(board, side);
    let stand_pat = eval_stm(board, side);

    // stand-pat
    if !in_check {
        if stand_pat >= beta {
            return stand_pat;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }
    }

    // gen moves, reg. moves if in check and all captures otherwise
    let mut moves: MoveListBuf = MoveListBuf::new();
    let mut move_scores = [0; 218];
    if in_check {
        get_all_moves(board, side, &mut moves);
    } else {
        get_all_captures(board, side, &mut moves);
    }
    let stop_sort_at = moves.len;

    // found mate
    if stop_sort_at == 0 {
        if in_check {
            return -MATE + ply;
        }
        return stand_pat;
    }

    score_moves_mvv_lva(board, &mut moves, &mut move_scores);

    let mut best = if in_check { -INFINITY } else { stand_pat };

    let mut moves_sorted = false;
    for i in 0..moves.len {
        if !moves_sorted {
            moves_sorted = bubble_pass(&mut moves, &mut move_scores, i, stop_sort_at);
        }
        let mv = moves.data[i];
        let undo = make_move(board, mv);
        let score = -quiescence(board, ply + 1, -beta, -alpha, control);
        undo_move(board, mv, undo);
        if control.aborted {
            return 0;
        }

        if score > best {
            best = score;
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            break;
        }
    }

    best
}

// returns the number of elements that aren't in the else catch
fn score_moves_full(
    board: &Board,
    moves: &mut MoveListBuf,
    scores: &mut [i32; 218],
    tt_move: Option<Move>,
    killers: [Move; 2],
) -> usize {
    let mut k = 0;
    for i in 0..moves.len {
        let mv = moves.data[i];

        let score = if tt_move == Some(mv) {
            -INFINITY
        } else if board.is_piece(mv.get_to()) {
            let victim = material_value(get_piece(board, mv.get_to()).piece_type);
            let attacker = material_value(get_piece(board, mv.get_from()).piece_type);
            -(victim * 10 + (6 - attacker))
        } else if mv == killers[0] || mv == killers[1] {
            -1
        } else {
            0
        };

        if score != 0 {
            // data[k] is a quiet move already scored 0, so it goes to i with 0
            moves.data.swap(k, i);
            scores[i] = 0;
            scores[k] = score; // set after scores[i] so k == i still works
            k += 1;
        } else {
            scores[i] = 0;
        }
    }
    k
}

fn score_moves_mvv_lva_tt(
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
            scores[i] = -(material_value(get_piece(board, mv.get_to()).piece_type) * 10
                + (6 - material_value(get_piece(board, mv.get_from()).piece_type)));
        }
        // Quiet
        else {
            scores[i] = 0;
        }
    }
}

fn score_moves_mvv_lva(board: &Board, moves: &MoveListBuf, scores: &mut [i32; 218]) {
    for i in 0..moves.len {
        let mv = moves.data[i];
        //  Captures
        if board.is_piece(mv.get_to()) {
            scores[i] = -(material_value(get_piece(board, mv.get_to()).piece_type) * 10
                + (6 - material_value(get_piece(board, mv.get_from()).piece_type)));
        }
        // Quiet
        else {
            scores[i] = 0;
        }
    }
}

// does a pass of bubble sort (index n to index 0)
// moves: list of moves
// scores: the score of the corresponding index of move
// pass: how many passes have previously happened
// Returns:
//  if moves are sorted
fn bubble_pass(
    moves: &mut MoveListBuf,
    scores: &mut [i32; 218],
    pass: usize,
    stop_sort_at: usize,
) -> bool {
    let mut sorted = true;

    // last index is pass because we know the first 'pass' indexes are sorted
    for i in ((pass + 1)..stop_sort_at).rev() {
        let pivot = scores[i];
        let next = scores[i - 1];

        // swap condition
        if next > pivot {
            // swap moves
            let tmp = moves.data[i - 1];
            moves.data[i - 1] = moves.data[i];
            moves.data[i] = tmp;
            // swap scores
            scores[i - 1] = pivot;
            scores[i] = next;

            sorted = false;
        }
    }

    sorted
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

    let file = File::open("assets/book.txt").expect("engine's opening book is missing");
    let reader = BufReader::new(file);

    let mut last_fen = String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    for line in reader.lines() {
        let line = line.expect("failed to read line");
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('$') {
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
    let col = bytes[0] - b'a'; // 'a' -> 0, 'h' -> 7
    let row = (bytes[1] - b'0') - 1;
    (row, col)
}

#[derive(Clone, Copy)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
struct TTEntry {
    depth: i32,
    value: i32,
    bound: Bound,
    best_move: Move,
}

struct ZobristTable {
    pieces: [[[u64; 64]; 2]; 6],
    black_to_move: u64,
    en_passant_file: [u64; 8],
    castling: [u64; 4], // WK, WQ, BK, BQ
}

impl ZobristTable {
    fn new() -> Self {
        // "RNG" but will keep same for determinism tests
        let mut rng: u64 = 0x0EE5_150E_5150_ABCD;

        let mut pieces = [[[0u64; 64]; 2]; 6];
        for pt in 0..6 {
            for color in 0..2 {
                for sq in 0..64 {
                    pieces[pt][color][sq] = splitmix64(&mut rng);
                }
            }
        }

        let mut en_passant_file = [0u64; 8];
        for f in 0..8 {
            en_passant_file[f] = splitmix64(&mut rng);
        }

        let mut castling = [0u64; 4];
        for i in 0..4 {
            castling[i] = splitmix64(&mut rng);
        }

        ZobristTable {
            pieces,
            black_to_move: splitmix64(&mut rng),
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
        if board.state & WHITE_TO_MOVE == 0 {
            h ^= self.black_to_move;
        }

        if let Some(target) = board.en_passant_target {
            h ^= self.en_passant_file[(target % 8) as usize];
        }

        if board.state & WHITE_SHORT != 0 {
            h ^= self.castling[0];
        }
        if board.state & WHITE_LONG != 0 {
            h ^= self.castling[1];
        }
        if board.state & BLACK_SHORT != 0 {
            h ^= self.castling[2];
        }
        if board.state & BLACK_LONG != 0 {
            h ^= self.castling[3];
        }

        h
    }

    fn update_hash(&self, hash: u64, board: &Board, mv: Move, undo: &Undo) -> u64 {
        let mut h = hash;
        let from = mv.get_from();
        let to = mv.get_to();
        let move_type = mv.get_flags() & 0b11;

        // board is post-move: whatever sits on `to` is the moved piece
        // (already promoted if this was a promotion)
        let after = get_piece(board, to);
        let side = after.color;
        let before_type = if move_type == types::PROMOTION_MOVE {
            PieceType::Pawn
        } else {
            after.piece_type
        };

        // moving piece: off `from` (as it was), onto `to` (as it is now)
        h ^= self.pieces[before_type as usize][side as usize][from as usize];
        h ^= self.pieces[after.piece_type as usize][side as usize][to as usize];

        // captured piece (normal, promo-capture, or en passant)
        if let Some(captured) = undo.captured_piece {
            let cap_sq = if move_type == types::EN_PASSANT_MOVE {
                ep_victim_square(from, to)
            } else {
                to
            };
            h ^=
                self.pieces[captured.piece_type as usize][captured.color as usize][cap_sq as usize];
        }

        // castling also moves the rook
        if move_type == types::CASTLE_MOVE {
            let (rook_from, rook_to) = castle_rook_squares(from, to);
            h ^= self.pieces[PieceType::Rook as usize][side as usize][rook_from as usize];
            h ^= self.pieces[PieceType::Rook as usize][side as usize][rook_to as usize];
        }

        // side to move always flips
        h ^= self.black_to_move;

        // en passant file: remove old, add new
        if let Some(prev_ep) = undo.previous_en_passant_target {
            h ^= self.en_passant_file[(prev_ep % 8) as usize];
        }
        if let Some(new_ep) = board.en_passant_target {
            h ^= self.en_passant_file[(new_ep % 8) as usize];
        }

        // castling rights: toggle any that changed
        let (prev, cur) = (undo.previous_state, board.state);
        if (prev & WHITE_SHORT) != (cur & WHITE_SHORT) {
            h ^= self.castling[0];
        }
        if (prev & WHITE_LONG) != (cur & WHITE_LONG) {
            h ^= self.castling[1];
        }
        if (prev & BLACK_SHORT) != (cur & BLACK_SHORT) {
            h ^= self.castling[2];
        }
        if (prev & BLACK_LONG) != (cur & BLACK_LONG) {
            h ^= self.castling[3];
        }

        h
    }
}

// 64 bit hashing
// hashing is reproducable and deterministic
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
