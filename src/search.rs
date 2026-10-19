use std::time::{Duration, Instant};

use crate::{
    board::{has_non_pawn_piece, make_move, undo_move},
    engine::{
        pick_move, score_moves_full, score_moves_mvv_lva, score_moves_mvv_lva_tt, update_history,
    },
    eval::{MATE, format_eval},
    movegen::{get_all_captures, get_all_moves, is_in_check, see_ge},
    nnue::{AccumulatorPair, apply_move, evaluate, get_accumulator_pair},
    tt::{Bound, TranspositionTable, score_from_tt, score_to_tt},
    types::{
        Board, EN_PASSANT_MOVE, GameHistory, Move, MoveListBuf, PROMOTION_MOVE, Side, WHITE_TO_MOVE,
    },
    zobrist::ZobristTable,
};

pub const NODES_PER_POLL_CMP: u64 = 0x7FF; // check clock every 2048 nodes

pub const INFINITY: i32 = 10_000_000;
pub const MATE_THRESHOLD: i32 = MATE - 1_000; // |score| above this means "this is a mate score"

// size of our accumulator stack
// any eval after MAX_PLIES deep will have to be computed with its own accumulator
pub const MAX_PLIES: usize = 255;

// Late Move Reduction knobs
pub const LMR_MIN_DEPTH: i32 = 3;
pub const LMR_MIN_MOVES: usize = 3;

// Late Move Pruning knobs
const LMP_MAX_DEPTH: i32 = 8;

// Null Move Pruning knobs
const NULL_MIN_DEPTH: i32 = 3;
const NULL_BASE_REDUCTION: i32 = 2;

// Reverse Futility Pruning knobs
const RFP_MAX_DEPTH: i32 = 7;
const RFP_MARGIN: i32 = 60; // centipawns per ply

// Internal Iterative Reductions
const IIR_MIN_DEPTH: i32 = 5;

pub const MAX_HISTORY: i32 = 16000;

pub struct Limits {
    pub time_ms: u64,   // time constraint (0 -> no limit)
    pub max_nodes: u64, // node constraint (0 -> no limit)
    pub max_depth: i32,
    pub uci_info: bool, // on/off print setting for UCI
}

pub struct SearchResult {
    pub result: Move,
    pub score: i32,
    pub depth: i32,
}

pub struct SearchControl {
    start: Instant,
    budget: Duration,
    nodes: u64,
    max_nodes: u64,
    pub aborted: bool,
    lmr: [[i32; 64]; 64],
}

impl SearchControl {
    pub fn new(start: Instant, budget_ms: u128, max_nodes: u64) -> Self {
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
            max_nodes: max_nodes,
            aborted: false,
            lmr,
        }
    }

    // only actually checks the clock every NODES_PER_TIME_CHECK nodes
    #[inline]
    pub fn poll(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        self.nodes += 1;
        if self.nodes & NODES_PER_POLL_CMP == 0 {
            if self.start.elapsed() >= self.budget {
                self.aborted = true;
            }

            if self.max_nodes != 0 && self.nodes >= self.max_nodes {
                self.aborted = true;
            }
        }
        self.aborted
    }
}

pub struct Searcher {
    // match long life time
    tt: TranspositionTable,
    zobrist: ZobristTable,
    history: [[[i32; 64]; 64]; 2],

    // search long life time
    killers: [[Move; 2]; 64],
    accumulators: Box<[AccumulatorPair; MAX_PLIES + 1]>,
    repetition: GameHistory,
    control: SearchControl,
}

impl Searcher {
    pub fn new(tt_size_mb: usize) -> Self {
        let fresh_pair = AccumulatorPair::new();
        Searcher {
            tt: TranspositionTable::new(tt_size_mb),
            zobrist: ZobristTable::new(),
            history: [[[0i32; 64]; 64]; 2],

            killers: [[Move::NULL; 2]; 64],
            accumulators: vec![fresh_pair; MAX_PLIES + 1]
                .into_boxed_slice()
                .try_into()
                .ok()
                .expect("len = MAX_PLIES + !"),
            repetition: GameHistory {
                hashes: [0; MAX_PLIES + 100],
                root: 0,
            },
            control: SearchControl::new(Instant::now(), 0, 0),
        }
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.history = [[[0; 64]; 64]; 2];
    }

    pub fn load_repetition(&mut self, hashes: &[u64]) {
        let n = hashes.len();
        let start = n.saturating_sub(101);
        let window = &hashes[start..];
        self.repetition.hashes[..window.len()].copy_from_slice(window);
        self.repetition.root = window.len() - 1;
    }

    pub fn search(
        &mut self,
        board: &Board,
        game_hashes: &[u64],
        limits: &Limits,
    ) -> Option<SearchResult> {
        let mut board: Board = *board;
        let side = board.side_to_move();
        let root_hash = *game_hashes.last()?;

        let mut moves = MoveListBuf::new();
        let mut scores = [0; 218];
        get_all_moves(&mut board, side, &mut moves);
        let moves_len = moves.len; // sacrifice to the holy borrow checker
        if moves_len == 0 {
            // no moves available (error from caller)
            return None;
        }

        self.tt.new_search();
        let tt_move = self
            .tt
            .probe(root_hash)
            .map(|entry| entry.best_move)
            .filter(|m| *m != Move::NULL);
        score_moves_mvv_lva_tt(&board, &moves, &mut scores, tt_move);

        self.killers = [[Move::NULL; 2]; 64];
        self.accumulators[0] = get_accumulator_pair(&board);
        self.load_repetition(game_hashes);
        let time_budget = if limits.time_ms == 0 {
            u64::MAX / 2
        } else {
            limits.time_ms
        };
        self.control = SearchControl::new(Instant::now(), time_budget.into(), limits.max_nodes);

        let mut result = SearchResult {
            result: moves.data[0],
            score: 0,
            depth: 0,
        };

        let mut best_idx = None;
        let max_depth = if limits.max_depth == 0 {
            MAX_PLIES as i32 - 1
        } else {
            limits.max_depth
        };
        for depth in 1..=max_depth {
            if best_idx.is_some() {
                scores[best_idx.unwrap()] = -INFINITY * 2 - depth;
            }

            let mut alpha = -INFINITY;
            let beta = INFINITY;
            let mut best = (-INFINITY, moves.data[0]);
            let mut completed = true; // has the depth been completed

            for i in 0..moves.len {
                pick_move(&mut moves, &mut scores, i, moves_len);
                let mv = moves.data[i];
                let undo = make_move(&mut board, mv);
                let child_hash = self.zobrist.update_hash(root_hash, &board, mv, &undo);

                // set child accumulators
                self.accumulators[1] = self.accumulators[0];
                apply_move(&mut self.accumulators[1], &board, mv, &undo);

                let mut score: i32 = if i == 0 {
                    -self.negamax(&mut board, child_hash, depth - 1, 1, -beta, -alpha, true)
                } else {
                    -self.negamax(
                        &mut board,
                        child_hash,
                        depth - 1,
                        1,
                        -alpha - 1,
                        -alpha,
                        true,
                    )
                };

                // move beat alpha in (alpha - 1, alpha) window, get real score
                if i != 0 && !self.control.aborted && score > alpha && score < beta {
                    score =
                        -self.negamax(&mut board, child_hash, depth - 1, 1, -beta, -alpha, true);
                }

                undo_move(&mut board, mv, undo);
                if self.control.aborted {
                    completed = false;
                    break;
                }
                if score > best.0 {
                    best = (score, mv);
                    best_idx = Some(i);
                }

                alpha = alpha.max(score); // beta modified by next round because we flip alpha and beta
            }

            if completed {
                result = SearchResult {
                    result: best.1,
                    score: best.0,
                    depth,
                };
                if limits.uci_info {
                    println!(
                        "info depth {} time {} score {} nodes {} nps {} hashfull {}",
                        depth,
                        self.control.start.elapsed().as_millis(),
                        format_eval(best.0),
                        self.control.nodes,
                        (self.control.nodes as f32 / self.control.start.elapsed().as_secs_f32())
                            as i32,
                        self.tt.hashfull(),
                    );
                }
            } else {
                break;
            }

            if limits.time_ms != 0
                && self.control.start.elapsed().as_millis() as u64 * 3 >= limits.time_ms
            {
                break;
            }
        }

        Some(result)
    }

    fn negamax(
        &mut self,
        board: &mut Board,
        hash: u64,
        mut depth: i32,
        ply: i32,
        mut alpha: i32,
        beta: i32,
        null_ok: bool,
    ) -> i32 {
        if self.control.poll() {
            return 0;
        }

        let side = board.side_to_move();
        let opposing = if side == Side::White {
            Side::Black
        } else {
            Side::White
        };

        let p_idx = ply as usize;

        // most likely won't trigger, just a fail safe
        if p_idx >= MAX_PLIES {
            return evaluate(&self.accumulators[p_idx], side);
        }

        let this_ply_killers = if p_idx < self.killers.len() {
            self.killers[p_idx]
        } else {
            [Move::NULL; 2]
        };

        // repetition + 50-move rule detection
        let rep_idx = self.repetition.root + p_idx;
        self.repetition.hashes[rep_idx] = hash;
        if board.half_moves >= 100 || self.is_repetition(rep_idx, board.half_moves) {
            return 0;
        }

        if depth <= 0 {
            return self.quiescence(board, hash, ply, alpha, beta);
        }

        let is_pv = beta - alpha > 1;

        let mut tt_move = None;
        if let Some(entry) = self.tt.probe(hash) {
            if entry.best_move != Move::NULL {
                tt_move = Some(entry.best_move);
            }
            if entry.depth as i32 >= depth {
                let value = score_from_tt(entry.value, ply);
                match entry.bound() {
                    Bound::Exact => return value,
                    Bound::Lower => {
                        if value >= beta {
                            return value;
                        }
                    }
                    Bound::Upper => {
                        if value <= alpha {
                            return value;
                        }
                    }
                };
            }
        }

        let alpha_before = alpha;

        let in_check = is_in_check(board, side);
        let static_eval = if in_check {
            -INFINITY
        } else {
            evaluate(&self.accumulators[p_idx], side)
        };

        // Reverse Futiity Pruning
        if !is_pv
            && !in_check
            && depth <= RFP_MAX_DEPTH
            && beta.abs() < MATE_THRESHOLD
            && static_eval - RFP_MARGIN * depth >= beta
        {
            return static_eval;
        }

        // Null Move Pruning
        if null_ok
            && !is_pv
            && depth >= NULL_MIN_DEPTH
            && beta.abs() < MATE_THRESHOLD
            && has_non_pawn_piece(board, side)
            && !in_check
            && static_eval >= beta
        {
            let ep_target = board.en_passant_target;
            let half_moves = board.half_moves;
            let mut child_hash = hash ^ self.zobrist.black_to_move;
            if let Some(target) = ep_target {
                child_hash ^= self.zobrist.en_passant_file[(target % 8) as usize];
            }
            board.en_passant_target = None;
            board.half_moves = 0;
            board.state ^= WHITE_TO_MOVE;

            // update accumulators
            self.accumulators[p_idx + 1] = self.accumulators[p_idx];

            let reduction = NULL_BASE_REDUCTION + depth / 6;
            let score = -self.negamax(
                board,
                child_hash,
                depth - 1 - reduction,
                ply + 1,
                -beta,
                -beta + 1,
                false,
            );

            // revert board state
            board.state ^= WHITE_TO_MOVE;
            board.half_moves = half_moves;
            board.en_passant_target = ep_target;

            if self.control.aborted {
                return 0;
            }

            if score >= beta {
                // we say, even if lose a move we're still better
                return beta;
            }
        }

        let mut moves = MoveListBuf::new();
        let mut scores = [0; 218];
        get_all_moves(board, side, &mut moves);
        let moves_len = moves.len; // sacrifice to the holy borrow checker; thanks rust

        // mate / stalemate
        if moves.len == 0 {
            return if in_check { -MATE + ply } else { 0 };
        }

        // for history
        let mut quiets_tried = [Move::NULL; 64];
        let mut n_quiets = 0;

        score_moves_full(
            board,
            &mut moves,
            &mut scores,
            tt_move,
            this_ply_killers,
            &self.history,
        );

        let mut best = (-INFINITY, moves.data[0]);
        for i in 0..moves.len {
            pick_move(&mut moves, &mut scores, i, moves_len);

            let mv = moves.data[i];

            // move classification before moving
            let mv_type = mv.get_flags() & 0b11;
            let quiet = mv_type != PROMOTION_MOVE
                && mv_type != EN_PASSANT_MOVE
                && !board.is_piece(mv.get_to());

            // Late Move Pruning
            if quiet
                && !is_pv
                && !in_check
                && depth <= LMP_MAX_DEPTH
                && n_quiets >= (3 + depth * depth) as usize
                && best.0 > -MATE_THRESHOLD
            {
                continue;
            }

            let undo = make_move(board, mv);
            let child_hash = self.zobrist.update_hash(hash, board, mv, &undo);
            self.tt.prefetch(child_hash);

            let is_killer = mv == this_ply_killers[0] || mv == this_ply_killers[1];

            // update accumulators
            self.accumulators[p_idx + 1] = self.accumulators[p_idx];
            apply_move(&mut self.accumulators[p_idx + 1], board, mv, &undo);

            let mut score: i32;
            if i == 0 {
                score = -self.negamax(board, child_hash, depth - 1, ply + 1, -beta, -alpha, true);
            } else {
                let mut reduction = 0;
                if depth >= IIR_MIN_DEPTH && tt_move.is_none() {
                    depth -= 1;
                }
                if depth >= LMR_MIN_DEPTH && i >= LMR_MIN_MOVES && quiet && !in_check && !is_killer
                {
                    // only compute this if rest of LMR conditions are true, saves a bit of time
                    let gives_check = is_in_check(board, opposing);

                    if !gives_check {
                        reduction += self.control.lmr[depth.min(63) as usize][i.min(63)];
                        if is_pv {
                            reduction -= 1;
                        }
                        reduction = reduction.clamp(0, depth - 2); // reduced search never drops below depth 1
                    }
                }

                // null-window search
                score = -self.negamax(
                    board,
                    child_hash,
                    depth - 1 - reduction,
                    ply + 1,
                    -alpha - 1,
                    -alpha,
                    true,
                );

                // if alpha was beat during null-window search, search null-window w/o reduction
                if reduction > 0 && score > alpha && !self.control.aborted {
                    score = -self.negamax(
                        board,
                        child_hash,
                        depth - 1,
                        ply + 1,
                        -alpha - 1,
                        -alpha,
                        true,
                    );
                }

                // still inside the window (PV node): get exact score in full window
                if score > alpha && score < beta && !self.control.aborted {
                    score =
                        -self.negamax(board, child_hash, depth - 1, ply + 1, -beta, -alpha, true);
                }
            }

            undo_move(board, mv, undo);
            if self.control.aborted {
                return 0;
            }

            if score > best.0 {
                best.0 = score;
                best.1 = mv;
            }

            alpha = alpha.max(score);
            if alpha >= beta {
                if quiet {
                    let history_bonus = (depth * depth).min(1200);
                    update_history(
                        &mut self.history[side as usize][mv.get_from() as usize]
                            [mv.get_to() as usize],
                        history_bonus,
                    );
                    for q in &quiets_tried[..n_quiets] {
                        update_history(
                            &mut self.history[side as usize][q.get_from() as usize]
                                [q.get_to() as usize],
                            -history_bonus,
                        );
                    }
                    if p_idx < self.killers.len() && this_ply_killers[0] != mv {
                        self.killers[p_idx][1] = self.killers[p_idx][0]; // push last killer backwards
                        self.killers[p_idx][0] = mv;
                    }
                }

                break;
            }

            if quiet && n_quiets < 64 {
                quiets_tried[n_quiets] = mv;
                n_quiets += 1;
            }
        }

        let bound = if best.0 <= alpha_before {
            Bound::Upper
        } else if best.0 >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };

        self.tt
            .store(hash, depth, score_to_tt(best.0, ply), bound, best.1);

        best.0
    }

    fn quiescence(
        &mut self,
        board: &mut Board,
        hash: u64,
        ply: i32,
        mut alpha: i32,
        beta: i32,
    ) -> i32 {
        if self.control.poll() {
            return 0;
        }

        let side = board.side_to_move();
        let p_idx = ply as usize;

        if p_idx >= MAX_PLIES {
            return evaluate(&self.accumulators[p_idx], side);
        }

        let mut tt_move = None;
        if let Some(entry) = self.tt.probe(hash) {
            if entry.best_move != Move::NULL {
                tt_move = Some(entry.best_move);
            }
            if entry.depth as i32 >= 0 {
                let value = score_from_tt(entry.value, ply);
                match entry.bound() {
                    Bound::Exact => return value,
                    Bound::Lower => {
                        if value >= beta {
                            return value;
                        }
                    }
                    Bound::Upper => {
                        if value <= alpha {
                            return value;
                        }
                    }
                };
            }
        }

        let in_check = is_in_check(board, side);
        let alpha_before = alpha;
        let static_eval = if in_check {
            -INFINITY
        } else {
            evaluate(&self.accumulators[p_idx], side)
        };

        if static_eval >= beta {
            self.tt
                .store(hash, 0, static_eval, Bound::Lower, Move::NULL);
            return static_eval;
        }
        alpha = alpha.max(static_eval);
        let mut best = (static_eval, Move::NULL);

        let mut moves = MoveListBuf::new();
        let mut scores = [0; 218];
        if in_check {
            get_all_moves(board, side, &mut moves);
        } else {
            get_all_captures(board, side, &mut moves);
        }

        let moves_len = moves.len;
        if moves_len == 0 {
            if in_check {
                // mate!
                return -MATE + ply;
            }
            return best.0;
        }

        score_moves_mvv_lva_tt(board, &moves, &mut scores, tt_move);

        for i in 0..moves.len {
            pick_move(&mut moves, &mut scores, i, moves_len);

            let mv = moves.data[i];

            // skip captures that lose material. never prune evasions though
            if !in_check && !see_ge(board, mv, 0) {
                continue;
            }

            let undo = make_move(board, mv);
            let child_hash = self.zobrist.update_hash(hash, board, mv, &undo);
            self.tt.prefetch(child_hash);

            // update accumulators
            self.accumulators[p_idx + 1] = self.accumulators[p_idx];
            apply_move(&mut self.accumulators[p_idx + 1], board, mv, &undo);

            let score = -self.quiescence(board, child_hash, ply + 1, -beta, -alpha);

            undo_move(board, mv, undo);
            if self.control.aborted {
                return 0;
            }

            if score > best.0 {
                best = (score, mv);
            }
            alpha = alpha.max(score);

            if alpha >= beta {
                break;
            }
        }

        // store in TT
        let bound = if best.0 <= alpha_before {
            Bound::Upper
        } else if best.0 >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };

        self.tt.store(hash, 0, best.0, bound, best.1);

        best.0
    }

    fn is_repetition(&self, idx: usize, half_moves: u16) -> bool {
        let hashes = &self.repetition.hashes;
        // nothing before the last capture/pawn move can repeat
        let earliest = (idx as isize - half_moves as isize).max(0);
        // same side to move = even distance back; shortest cycle is 4 plies
        let mut i = idx as isize - 4;
        while i >= earliest {
            if hashes[i as usize] == hashes[idx] {
                return true;
            }
            i -= 2;
        }
        false
    }
}
