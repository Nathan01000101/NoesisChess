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
use crate::eval::{MATE, eval_stm, format_eval, material_value};
use crate::movegen::{find_legal_move, get_all_captures, get_all_moves, is_in_check};
use crate::types;
use crate::types::{
    BLACK_LONG, BLACK_SHORT, Board, EN_PASSANT_MOVE, Move, MoveContext, MoveListBuf,
    PROMOTION_MOVE, PieceType, Side, Undo, WHITE_LONG, WHITE_SHORT, WHITE_TO_MOVE,
};

use macroquad::miniquad::date;
use macroquad::prelude::*;

const MOVE_REPETITION_PENALTY: i32 = 25;
const TT_SIZE_MB: usize = 128; // rounded down to a power of two worth of buckets
const NODES_PER_TIME_CHECK: u64 = 0x7FF; // check clock every 2048 nodes

const INFINITY: i32 = 10_000_000;
const MATE_THRESHOLD: i32 = MATE - 1_000; // |score| above this means "this is a mate score"

// Null move pruning knobs.
const NULL_MIN_DEPTH: i32 = 3;
const NULL_BASE_REDUCTION: i32 = 2;

const MAX_HISTORY: i32 = 16000;

const LMR_MIN_DEPTH: i32 = 3;
const LMR_MIN_MOVES: usize = 3;

pub struct Engine {
    pub depth: usize,
    opening_book: FxHashMap<String, Vec<(u8, u8)>>,
    zobrist: ZobristTable,
    tt: Mutex<TranspositionTable>,
    pub move_history: Mutex<Vec<u64>>,
}

impl Engine {
    pub fn new(depth: usize) -> Self {
        rand::srand(date::now() as u64);
        Self {
            depth,
            opening_book: build_book(),
            zobrist: ZobristTable::new(),
            tt: Mutex::new(TranspositionTable::new(TT_SIZE_MB)),
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
        let tt: &mut TranspositionTable = &mut *tt_guard;

        // new search -> entries from older searches start aging out
        tt.new_search();

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
        let mut history = [[[0i32; 64]; 64]; 2];
        let tt_move = tt
            .probe(root_hash)
            .map(|entry| entry.best_move)
            .filter(|m| *m != Move::NULL);
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
                        &mut history,
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
                        &mut history,
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
                        &mut history,
                        &mut control,
                    );
                }

                undo_move(&mut b, mv, undo);

                if control.aborted {
                    depth_completed = false;
                    break;
                }

                // check for repetition
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

                println!(
                    "info depth {} time {} score {} nodes {} nps {} hashfull {}",
                    current_depth,
                    start.elapsed().as_millis(),
                    format_eval(depth_best_score),
                    control.nodes,
                    (control.nodes as f32 / start.elapsed().as_secs_f32()) as i32,
                    tt.hashfull()
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

            // if we've found #1, don't waste time searching further
            if depth_best_score == MATE - 1 {
                return best_move;
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
        mh_guard.push(best_hash);

        best_move
    }

    fn reset(&self) {
        self.move_history.lock().unwrap().clear();
        self.tt.lock().unwrap().clear();
    }
}

struct SearchControl {
    start: Instant,
    budget: Duration,
    nodes: u64,
    aborted: bool,
    lmr: [[i32; 64]; 64],
}

impl SearchControl {
    fn new(start: Instant, budget_ms: u128) -> Self {
        let mut lmr = [[0; 64]; 64];
        for d in 1..64 {
            for m in 1..64 {
                lmr[d][m] = (0.75 + (d as f64).ln() * (m as f64).ln() / 2.25) as i32;
            }
        }
        Self {
            start,
            budget: Duration::from_millis(budget_ms as u64),
            nodes: 0,
            aborted: false,
            lmr,
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

fn update_history(entry: &mut i32, bonus: i32) {
    *entry += bonus - *entry * bonus.abs() / MAX_HISTORY;
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

fn negamax(
    board: &mut Board,
    hash: u64,
    depth: i32,
    ply: i32,
    mut alpha: i32,
    mut beta: i32,
    null_ok: bool,
    ztable: &ZobristTable,
    tt: &mut TranspositionTable,
    killers: &mut [[Move; 2]; 64],
    history: &mut [[[i32; 64]; 64]; 2],
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
    let opponent = if side == Side::White {
        Side::Black
    } else {
        Side::White
    };

    // only trust an entry searched at least as deep as we need.
    let mut tt_move: Option<Move> = None;
    if let Some(entry) = tt.probe(hash) {
        if entry.best_move != Move::NULL {
            tt_move = Some(entry.best_move);
        }
        if entry.depth as i32 >= depth {
            let value = score_from_tt(entry.value, ply);
            match entry.bound() {
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
    let pv_node = beta - alpha > 1;

    if depth <= 0 {
        return quiescence(board, ply, alpha, beta, control);
    }

    let in_check = is_in_check(board, side);

    // Null move pruning: non-PV nodes only
    if null_ok
        && !pv_node
        && !in_check
        && depth >= NULL_MIN_DEPTH
        && beta.abs() < MATE_THRESHOLD
        && has_non_pawn_piece(board, side)
        && eval_stm(board, side) >= beta
    {
        // make null -> flip side to move, drop en passant right
        let prev_ep = board.en_passant_target;
        let mut null_hash = hash ^ ztable.black_to_move;
        if let Some(ep) = prev_ep {
            null_hash ^= ztable.en_passant_file[(ep % 8) as usize];
        }
        board.en_passant_target = None;
        board.state ^= WHITE_TO_MOVE;

        let r = NULL_BASE_REDUCTION + depth / 6;
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
            history,
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

    let mut moves = MoveListBuf::new();
    let mut move_scores = [0; 218];
    get_all_moves(board, side, &mut moves);

    // mate / stalemate
    if moves.len == 0 {
        return if in_check { -MATE + ply } else { 0 };
    }

    // for history
    let mut quiets_tried = [Move::NULL; 64];
    let mut n_quiets = 0;

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
        history,
    );

    let mut best_move: Move = moves.data[0];
    let mut best = -INFINITY;

    let mut moves_sorted = false;
    for i in 0..moves.len {
        if !moves_sorted && i < stop_sort_at {
            moves_sorted = bubble_pass(&mut moves, &mut move_scores, i, stop_sort_at);
        }
        let mv = moves.data[i];

        // classify on the pre-move board
        let move_type = mv.get_flags() & 0b11;
        let quiet = !board.is_piece(mv.get_to())
            && move_type != PROMOTION_MOVE
            && move_type != EN_PASSANT_MOVE;
        let is_killer = mv == this_ply_killers[0] || mv == this_ply_killers[1];

        let undo = make_move(board, mv);
        let child_hash = ztable.update_hash(hash, board, mv, &undo);
        let new_depth = depth - 1;

        let mut score;
        if i == 0 {
            // first move: full window, full depth
            score = -negamax(
                board,
                child_hash,
                new_depth,
                ply + 1,
                -beta,
                -alpha,
                true,
                ztable,
                tt,
                killers,
                history,
                control,
            );
        } else {
            // late move reductions
            let mut r = 0;
            if depth >= LMR_MIN_DEPTH && i >= LMR_MIN_MOVES && quiet && !in_check && !is_killer {
                // only compute this if rest of LMR conditions are true, saves a few computations
                let gives_check = is_in_check(board, opponent);

                if !gives_check {
                    r = control.lmr[depth.min(63) as usize][i.min(63)];
                    if pv_node {
                        r -= 1;
                    }
                    r = r.clamp(0, depth - 2); // reduced search never drops below depth 1
                }
            }

            // 1) (possibly reduced) null-window search
            score = -negamax(
                board,
                child_hash,
                new_depth - r,
                ply + 1,
                -alpha - 1,
                -alpha,
                true,
                ztable,
                tt,
                killers,
                history,
                control,
            );

            // 2) reduced move beat alpha: verify at full depth, still null window
            if r > 0 && score > alpha && !control.aborted {
                score = -negamax(
                    board,
                    child_hash,
                    new_depth,
                    ply + 1,
                    -alpha - 1,
                    -alpha,
                    true,
                    ztable,
                    tt,
                    killers,
                    history,
                    control,
                );
            }

            // 3) still inside the window (PV node): get the exact score
            if score > alpha && score < beta && !control.aborted {
                score = -negamax(
                    board,
                    child_hash,
                    new_depth,
                    ply + 1,
                    -beta,
                    -alpha,
                    true,
                    ztable,
                    tt,
                    killers,
                    history,
                    control,
                );
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
            if quiet {
                let history_bonus = (depth * depth).min(1200);
                update_history(
                    &mut history[side as usize][mv.get_from() as usize][mv.get_to() as usize],
                    history_bonus,
                );
                for q in &quiets_tried[..n_quiets] {
                    update_history(
                        &mut history[side as usize][q.get_from() as usize][q.get_to() as usize],
                        -history_bonus,
                    );
                }
                if p < killers.len() && killers[p][0] != mv {
                    killers[p][1] = killers[p][0];
                    killers[p][0] = mv;
                }
            }
            break;
        }

        if quiet && n_quiets < 64 {
            quiets_tried[n_quiets] = mv;
            n_quiets += 1;
        }
    }

    let bound = if best <= alpha_orig {
        Bound::Upper
    } else if best >= beta_orig {
        Bound::Lower
    } else {
        Bound::Exact
    };

    tt.store(hash, depth, score_to_tt(best, ply), bound, best_move);

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
    history: &[[[i32; 64]; 64]; 2],
) -> usize {
    let side = if board.state & WHITE_TO_MOVE != 0 {
        Side::White
    } else {
        Side::Black
    };
    for i in 0..moves.len {
        let mv = moves.data[i];

        scores[i] = if tt_move == Some(mv) {
            -INFINITY
        } else if board.is_piece(mv.get_to()) {
            let victim = material_value(get_piece(board, mv.get_to()).piece_type);
            let attacker = material_value(get_piece(board, mv.get_from()).piece_type);
            -(1_000_000 + (victim * 10 + (6 - attacker)))
        } else if mv == killers[0] || mv == killers[1] {
            -900_000
        } else {
            -history[side as usize][mv.get_from() as usize][mv.get_to() as usize]
        };
    }
    moves.len
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

// Fixed size, Each bucket is a 64-byte cache line
// each holding 4 entries
// ---------------------------------------------------------------------------

const BUCKET_SIZE: usize = 4;
const BOUND_MASK: u8 = 0b11; // flags bits 0-1: bound (0 means empty slot)
const GEN_SHIFT: u8 = 2; // flags bits 2-7: search generation
const GEN_MASK: u8 = 0x3F;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

impl Bound {
    #[inline]
    fn to_bits(self) -> u8 {
        match self {
            Bound::Exact => 1,
            Bound::Lower => 2,
            Bound::Upper => 3,
        }
    }

    #[inline]
    fn from_bits(bits: u8) -> Bound {
        match bits {
            1 => Bound::Exact,
            2 => Bound::Lower,
            _ => Bound::Upper,
        }
    }
}

// 16 bytes: 8 + 4 + 2 + 1 + 1
#[derive(Clone, Copy)]
#[repr(C)]
struct TTEntry {
    key: u64, // full zobrist hash, used to verify the slot really is this position
    value: i32,
    best_move: Move,
    depth: i8,
    flags: u8, // bound | generation << 2
}

impl TTEntry {
    const EMPTY: TTEntry = TTEntry {
        key: 0,
        value: 0,
        best_move: Move::NULL,
        depth: 0,
        flags: 0,
    };

    #[inline]
    fn is_empty(&self) -> bool {
        self.flags & BOUND_MASK == 0
    }

    #[inline]
    fn bound(&self) -> Bound {
        Bound::from_bits(self.flags & BOUND_MASK)
    }

    #[inline]
    fn generation(&self) -> u8 {
        self.flags >> GEN_SHIFT
    }
}

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Bucket {
    entries: [TTEntry; BUCKET_SIZE],
}

impl Bucket {
    const EMPTY: Bucket = Bucket {
        entries: [TTEntry::EMPTY; BUCKET_SIZE],
    };
}

struct TranspositionTable {
    buckets: Vec<Bucket>,
    mask: usize,
    generation: u8,
}

impl TranspositionTable {
    fn new(size_mb: usize) -> Self {
        let bytes = size_mb * 1024 * 1024;
        let wanted = (bytes / std::mem::size_of::<Bucket>()).max(1);
        // power of two bucket count so the index is just `hash & mask`
        let count = if wanted.is_power_of_two() {
            wanted
        } else {
            wanted.next_power_of_two() / 2
        };

        Self {
            buckets: vec![Bucket::EMPTY; count],
            mask: count - 1,
            generation: 0,
        }
    }

    fn clear(&mut self) {
        self.buckets.fill(Bucket::EMPTY);
        self.generation = 0;
    }

    // call once at the start of every search so old entries can be told apart
    fn new_search(&mut self) {
        self.generation = (self.generation + 1) & GEN_MASK;
    }

    #[inline]
    fn index(&self, hash: u64) -> usize {
        (hash as usize) & self.mask
    }

    #[inline]
    fn probe(&self, hash: u64) -> Option<TTEntry> {
        self.buckets[self.index(hash)]
            .entries
            .iter()
            .find(|e| !e.is_empty() && e.key == hash)
            .copied()
    }

    fn store(&mut self, hash: u64, depth: i32, value: i32, bound: Bound, best_move: Move) {
        let cur_gen = self.generation;
        let idx = self.index(hash);
        let bucket = &mut self.buckets[idx];
        let depth8 = depth.clamp(i8::MIN as i32, i8::MAX as i32) as i8;

        // pick a slot: same position > empty > least valuable
        // (value = depth, minus a penalty for every search it has sat unused)
        let mut victim = 0;
        let mut victim_worth = i32::MAX;
        for (i, e) in bucket.entries.iter().enumerate() {
            if !e.is_empty() && e.key == hash {
                // same position: keep the old entry if it's deeper and from this search
                if depth8 < e.depth && e.generation() == cur_gen {
                    return;
                }
                victim = i;
                break;
            }
            if e.is_empty() {
                victim = i;
                victim_worth = i32::MIN;
                continue;
            }
            let age = (cur_gen.wrapping_sub(e.generation()) & GEN_MASK) as i32;
            let worth = e.depth as i32 - 8 * age;
            if worth < victim_worth {
                victim_worth = worth;
                victim = i;
            }
        }

        bucket.entries[victim] = TTEntry {
            key: hash,
            value,
            best_move,
            depth: depth8,
            flags: (cur_gen << GEN_SHIFT) | bound.to_bits(),
        };
    }

    // UCI hashfull print
    fn hashfull(&self) -> usize {
        let sample = self.buckets.len().min(250);
        let mut used = 0;
        for bucket in &self.buckets[..sample] {
            for e in &bucket.entries {
                if !e.is_empty() && e.generation() == self.generation {
                    used += 1;
                }
            }
        }
        used * 1000 / (sample * BUCKET_SIZE)
    }
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
