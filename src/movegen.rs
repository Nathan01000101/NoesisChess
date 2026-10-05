use crate::attacks::{KING_MOVES, KNIGHT_MOVES, PAWN_MOVES, get_bishop_moves, get_rook_moves};
use crate::board::{get_piece, get_side_bitboard, make_move, undo_move};
use crate::types::{
    BLACK_LONG, BLACK_SHORT, Bitboard, Board, CASTLE_MOVE, EN_PASSANT_MOVE, Move, MoveBuf,
    MoveListBuf, NORMAL_MOVE, PROMOTION_BISHOP, PROMOTION_KNIGHT, PROMOTION_MOVE, PROMOTION_QUEEN,
    PROMOTION_ROOK, PieceType, Side, WHITE_LONG, WHITE_SHORT, WHITE_TO_MOVE,
};

const FILE_A: u64 = 0x0101_0101_0101_0101;
const FILE_H: u64 = 0x8080_8080_8080_8080;
const RANK_1: u64 = 0x0000_0000_0000_00FF;
const RANK_3: u64 = 0x0000_0000_00FF_0000;
const RANK_6: u64 = 0x0000_FF00_0000_0000;
const RANK_8: u64 = 0xFF00_0000_0000_0000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenMode {
    All,
    Captures,
    Quiets,
}

// returns the other side, e.g. if white -> ret black
#[inline]
fn other(side: Side) -> Side {
    if side == Side::White {
        Side::Black
    } else {
        Side::White
    }
}

// returns board's bitboard of the given piecetype
#[inline]
fn pieces(board: &Board, side: Side, pt: PieceType) -> u64 {
    board.bitboards[side as usize][pt as usize].0
}

// returns the side that is next to move
#[inline]
fn side_to_move(board: &Board) -> Side {
    if board.state & WHITE_TO_MOVE != 0 {
        Side::White
    } else {
        Side::Black
    }
}

// returns the bitboard of where possible pawn attacks could be coming from
#[inline]
fn pawn_attackers_of(target: u64, by: Side) -> u64 {
    if by == Side::White {
        ((target >> 7) & !FILE_A) | ((target >> 9) & !FILE_H)
    } else {
        ((target << 7) & !FILE_H) | ((target << 9) & !FILE_A)
    }
}

// shifts a bitboard n ranks "forward" from side's point of view
#[inline]
fn forward(bb: u64, side: Side, n: u32) -> u64 {
    if side == Side::White {
        bb << n
    } else {
        bb >> n
    }
}

// the last rank for side's pawns
#[inline]
fn promo_rank(side: Side) -> u64 {
    if side == Side::White { RANK_8 } else { RANK_1 }
}

// Which promotions a gen mode wants, as (queen, underpromotions).
// Queen promos count as "tactical", so qsearch (Captures) sees them even when
// they don't capture. Underpromos only count as captures if they capture.
// Captures + Quiets together always equal All.
#[inline]
fn promo_wanted(mode: GenMode, is_capture: bool) -> (bool, bool) {
    match (mode, is_capture) {
        (GenMode::All, _) => (true, true),
        (GenMode::Captures, true) => (true, true),
        (GenMode::Captures, false) => (true, false),
        (GenMode::Quiets, true) => (false, false),
        (GenMode::Quiets, false) => (false, true),
    }
}

#[inline]
fn is_underpromotion(mv: Move) -> bool {
    let flags = mv.get_flags();
    flags & 0b11 == PROMOTION_MOVE && flags & (0b11 << 2) != PROMOTION_QUEEN
}

// ---------------------------------------------------------------------------
// push helpers
// ---------------------------------------------------------------------------

// every set bit in targets is a normal move from `from`
#[inline]
fn push_targets(out: &mut MoveListBuf, from: u8, mut targets: u64) {
    while targets != 0 {
        out.push(Move::new(from, targets.trailing_zeros() as u8, NORMAL_MOVE));
        targets &= targets - 1;
    }
}

// every set bit in targets is a pawn move from (to - delta)
#[inline]
fn push_pawn_moves(out: &mut MoveListBuf, mut targets: u64, delta: i8) {
    while targets != 0 {
        let to = targets.trailing_zeros() as u8;
        let from = (to as i8 - delta) as u8;
        out.push(Move::new(from, to, NORMAL_MOVE));
        targets &= targets - 1;
    }
}

// every set bit in targets is a promotion from (to - delta)
#[inline]
fn push_promotions(out: &mut MoveListBuf, mut targets: u64, delta: i8, queen: bool, under: bool) {
    while targets != 0 {
        let to = targets.trailing_zeros() as u8;
        let from = (to as i8 - delta) as u8;
        push_promo_set(out, from, to, queen, under);
        targets &= targets - 1;
    }
}

#[inline]
fn push_promo_set(out: &mut MoveListBuf, from: u8, to: u8, queen: bool, under: bool) {
    if queen {
        out.push(Move::new(from, to, PROMOTION_MOVE | PROMOTION_QUEEN));
    }
    if under {
        out.push(Move::new(from, to, PROMOTION_MOVE | PROMOTION_ROOK));
        out.push(Move::new(from, to, PROMOTION_MOVE | PROMOTION_BISHOP));
        out.push(Move::new(from, to, PROMOTION_MOVE | PROMOTION_KNIGHT));
    }
}

// make/undo legality check, only used for the rare cases (en passant, pinned pieces)
#[inline]
fn push_if_legal(board: &mut Board, side: Side, out: &mut MoveListBuf, mv: Move) {
    let undo = make_move(board, mv);
    let ok = !is_in_check(board, side);
    undo_move(board, mv, undo);
    if ok {
        out.push(mv);
    }
}

// ---------------------------------------------------------------------------
// attack queries
// ---------------------------------------------------------------------------

pub fn attackers_to(board: &Board, square: u8, by: Side, occupied: u64) -> u64 {
    let t = 1u64 << square;
    let queens = pieces(board, by, PieceType::Queen);
    (pawn_attackers_of(t, by) & pieces(board, by, PieceType::Pawn))
        | (KNIGHT_MOVES[square as usize].0 & pieces(board, by, PieceType::Knight))
        | (KING_MOVES[square as usize].0 & pieces(board, by, PieceType::King))
        | (get_rook_moves(occupied, square) & (pieces(board, by, PieceType::Rook) | queens))
        | (get_bishop_moves(occupied, square) & (pieces(board, by, PieceType::Bishop) | queens))
}

// determines if the given square is attacked by the given side
#[inline]
fn is_attacked(board: &Board, square: u8, by: Side, occupied: u64) -> bool {
    let t = 1u64 << square;
    if pawn_attackers_of(t, by) & pieces(board, by, PieceType::Pawn) != 0 {
        return true;
    }
    if KNIGHT_MOVES[square as usize].0 & pieces(board, by, PieceType::Knight) != 0 {
        return true;
    }
    if KING_MOVES[square as usize].0 & pieces(board, by, PieceType::King) != 0 {
        return true;
    }
    let queens = pieces(board, by, PieceType::Queen);
    if get_rook_moves(occupied, square) & (pieces(board, by, PieceType::Rook) | queens) != 0 {
        return true;
    }
    if get_bishop_moves(occupied, square) & (pieces(board, by, PieceType::Bishop) | queens) != 0 {
        return true;
    }
    false
}

// determines if a given square is attacked by a given side
pub fn is_square_attacked(board: &Board, square: u8, side: Side) -> bool {
    is_attacked(board, square, side, board.occupied)
}

// determines if a side is in check with a given board state
pub fn is_in_check(board: &Board, side: Side) -> bool {
    let king = pieces(board, side, PieceType::King).trailing_zeros() as u8;
    is_attacked(board, king, other(side), board.occupied)
}

// returns a bitboard of all the pieces pinned to the given king_sq
pub fn pinned_pieces(board: &Board, side: Side, king_sq: u8) -> u64 {
    let occ = board.occupied;
    let friendly = get_side_bitboard(board, side).0;
    let enemy = other(side);
    let enemy_queens = pieces(board, enemy, PieceType::Queen);
    let mut pinned = 0u64;

    let rook_rays = get_rook_moves(occ, king_sq);
    let blockers = rook_rays & friendly;
    if blockers != 0 {
        let occ_x = occ ^ blockers;
        let mut snipers = get_rook_moves(occ_x, king_sq)
            & (pieces(board, enemy, PieceType::Rook) | enemy_queens)
            & !rook_rays;
        while snipers != 0 {
            let s = snipers.trailing_zeros() as u8;
            pinned |= blockers & get_rook_moves(occ_x, s);
            snipers &= snipers - 1;
        }
    }

    let bishop_rays = get_bishop_moves(occ, king_sq);
    let blockers = bishop_rays & friendly;
    if blockers != 0 {
        let occ_x = occ ^ blockers;
        let mut snipers = get_bishop_moves(occ_x, king_sq)
            & (pieces(board, enemy, PieceType::Bishop) | enemy_queens)
            & !bishop_rays;
        while snipers != 0 {
            let s = snipers.trailing_zeros() as u8;
            pinned |= blockers & get_bishop_moves(occ_x, s);
            snipers &= snipers - 1;
        }
    }

    pinned
}

// when in check from exactly one piece, non king moves must either capture
// the attacker or block ray between attacker and king
fn check_mask(board: &Board, king_sq: u8, checkers: u64) -> u64 {
    let occ = board.occupied;
    let c_sq = checkers.trailing_zeros() as u8;
    let checker = get_piece(board, c_sq);
    let between = match checker.piece_type {
        PieceType::Rook => get_rook_moves(occ, king_sq) & get_rook_moves(occ, c_sq),
        PieceType::Bishop => get_bishop_moves(occ, king_sq) & get_bishop_moves(occ, c_sq),
        PieceType::Queen => {
            if get_rook_moves(occ, king_sq) & checkers != 0 {
                get_rook_moves(occ, king_sq) & get_rook_moves(occ, c_sq)
            } else {
                get_bishop_moves(occ, king_sq) & get_bishop_moves(occ, c_sq)
            }
        }
        _ => 0, // knights and pawns can't be interposed against
    };
    checkers | between
}

// --------------------------
// the move generator ^.^
// --------------------------

fn generate(board: &mut Board, side: Side, mode: GenMode, out: &mut MoveListBuf) {
    let enemy = other(side);
    let occ = board.occupied;
    let friendly = get_side_bitboard(board, side).0;
    let enemy_bb = get_side_bitboard(board, enemy).0;
    let king_bb = pieces(board, side, PieceType::King);
    let king_sq = king_bb.trailing_zeros() as u8;

    let checkers = attackers_to(board, king_sq, enemy, occ);
    let in_check = checkers != 0;
    let double_check = checkers & checkers.wrapping_sub(1) != 0;

    // generation mode
    let mode_mask = match mode {
        GenMode::All => !friendly,
        GenMode::Captures => enemy_bb,
        GenMode::Quiets => !occ,
    };

    // king moves -> never restricted by the check mask, but the king must
    //      not be in its own occupancy when we test the destination.
    let occ_without_king = occ ^ king_bb;
    let mut king_targets = KING_MOVES[king_sq as usize].0 & mode_mask;
    while king_targets != 0 {
        let to = king_targets.trailing_zeros() as u8;
        if !is_attacked(board, to, enemy, occ_without_king) {
            out.push(Move::new(king_sq, to, NORMAL_MOVE));
        }
        king_targets &= king_targets - 1;
    }

    // castling  -> quiet only when not in check
    if !in_check && mode != GenMode::Captures {
        let (can_long, can_short) = if side == Side::White {
            (
                board.state & WHITE_LONG != 0,
                board.state & WHITE_SHORT != 0,
            )
        } else {
            (
                board.state & BLACK_LONG != 0,
                board.state & BLACK_SHORT != 0,
            )
        };
        let base = (king_sq / 8) * 8;
        if can_long
            && !board.is_piece(base + 1)
            && !board.is_piece(base + 2)
            && !board.is_piece(base + 3)
            && !is_attacked(board, base + 2, enemy, occ)
            && !is_attacked(board, base + 3, enemy, occ)
        {
            out.push(Move::new(king_sq, base + 2, CASTLE_MOVE));
        }
        if can_short
            && !board.is_piece(base + 5)
            && !board.is_piece(base + 6)
            && !is_attacked(board, base + 5, enemy, occ)
            && !is_attacked(board, base + 6, enemy, occ)
        {
            out.push(Move::new(king_sq, base + 6, CASTLE_MOVE));
        }
    }

    // when in double check only the king can move
    if double_check {
        return;
    }

    // squares that resolve a single check (everything if not in check)
    let evasion = if in_check {
        check_mask(board, king_sq, checkers)
    } else {
        !0
    };
    let target = mode_mask & evasion;

    let pinned = pinned_pieces(board, side, king_sq);

    // friendly, non king pieces that arent pinned
    let free = friendly & !pinned & !king_bb;

    // knights
    let mut knights = pieces(board, side, PieceType::Knight) & free;
    while knights != 0 {
        let from = knights.trailing_zeros() as u8;
        push_targets(out, from, KNIGHT_MOVES[from as usize].0 & target);
        knights &= knights - 1;
    }

    // queens, bishops and rooks
    let queens = pieces(board, side, PieceType::Queen);

    let mut diag = (pieces(board, side, PieceType::Bishop) | queens) & free;
    while diag != 0 {
        let from = diag.trailing_zeros() as u8;
        push_targets(out, from, get_bishop_moves(occ, from) & target);
        diag &= diag - 1;
    }

    let mut orth = (pieces(board, side, PieceType::Rook) | queens) & free;
    while orth != 0 {
        let from = orth.trailing_zeros() as u8;
        push_targets(out, from, get_rook_moves(occ, from) & target);
        orth &= orth - 1;
    }

    // pawns
    let pawns = pieces(board, side, PieceType::Pawn) & free;
    let last_rank = promo_rank(side);
    let (push_delta, double_rank): (i8, u64) = if side == Side::White {
        (8, RANK_3)
    } else {
        (-8, RANK_6)
    };

    // pushes (the double push is computed from the unmasked single push so a
    // single push that doesn't block a check can still lead to one that does)
    let single = forward(pawns, side, 8) & !occ;
    let double = forward(single & double_rank, side, 8) & !occ & evasion;
    let single = single & evasion;

    if mode != GenMode::Captures {
        push_pawn_moves(out, single & !last_rank, push_delta);
        push_pawn_moves(out, double, push_delta * 2);
    }
    let (queen, under) = promo_wanted(mode, false);
    push_promotions(out, single & last_rank, push_delta, queen, under);

    // captures
    if mode != GenMode::Quiets {
        let caps = if side == Side::White {
            [((pawns << 7) & !FILE_H, 7i8), ((pawns << 9) & !FILE_A, 9i8)]
        } else {
            [
                ((pawns >> 7) & !FILE_A, -7i8),
                ((pawns >> 9) & !FILE_H, -9i8),
            ]
        };
        let (queen, under) = promo_wanted(mode, true);
        for (bb, delta) in caps {
            let bb = bb & enemy_bb & evasion;
            push_pawn_moves(out, bb & !last_rank, delta);
            push_promotions(out, bb & last_rank, delta, queen, under);
        }
    }

    // en passant -> always make/undo checked, it's the one move that can
    // uncover a check along a rank by removing two pieces at once
    if mode != GenMode::Quiets && side == side_to_move(board) {
        if let Some(ep) = board.en_passant_target {
            let mut cands =
                pawn_attackers_of(1u64 << ep, side) & pieces(board, side, PieceType::Pawn);
            while cands != 0 {
                let from = cands.trailing_zeros() as u8;
                push_if_legal(board, side, out, Move::new(from, ep, EN_PASSANT_MOVE));
                cands &= cands - 1;
            }
        }
    }

    // pinned pieces while not in check
    if !in_check {
        let ep = board.en_passant_target;
        let mut p = pinned;
        while p != 0 {
            let from = p.trailing_zeros() as u8;
            p &= p - 1;

            let piece = get_piece(board, from);
            if piece.piece_type == PieceType::Knight {
                continue;
            }
            let is_pawn = piece.piece_type == PieceType::Pawn;

            let mut pseudo = MoveBuf::new();
            get_pseudo_legal_moves(board, from, &mut pseudo);
            for i in 0..pseudo.len {
                let to = pseudo.data[i];
                if is_pawn && Some(to) == ep {
                    continue;
                } // done above
                let is_capture = board.is_piece(to);

                // a pinned pawn can promote by capturing its pinner
                if is_pawn && (1u64 << to) & last_rank != 0 {
                    let (queen, under) = promo_wanted(mode, is_capture);
                    let mut promos = MoveListBuf::new();
                    push_promo_set(&mut promos, from, to, queen, under);
                    for j in 0..promos.len {
                        push_if_legal(board, side, out, promos.data[j]);
                    }
                    continue;
                }

                match mode {
                    GenMode::Captures if !is_capture => continue,
                    GenMode::Quiets if is_capture => continue,
                    _ => {}
                }
                push_if_legal(board, side, out, Move::new(from, to, NORMAL_MOVE));
            }
        }
    }
}

// gets all legal moves for a side
pub fn get_all_moves(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    generate(board, side, GenMode::All, buffer);
}

// gets all captures a side can make, including en passant and queen promotions
pub fn get_all_captures(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    generate(board, side, GenMode::Captures, buffer);
}

// gets all moves for a side that aren't captures (or queen promotions)
pub fn get_all_quiets(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    generate(board, side, GenMode::Quiets, buffer);
}

// gets num of moves that a side can make
pub fn get_move_count(board: &mut Board, side: Side) -> u8 {
    let mut buf = MoveListBuf::new();
    generate(board, side, GenMode::All, &mut buf);
    buf.len as u8
}

// Builds the real Move (with flags) for a from/to pair coming from outside the
// engine: GUI clicks, UCI strings, the opening book. promo = None means queen.
// Returns None if the move isn't legal for the side to move.
pub fn find_legal_move(
    board: &mut Board,
    from: u8,
    to: u8,
    promo: Option<PieceType>,
) -> Option<Move> {
    let want_promo = match promo.unwrap_or(PieceType::Queen) {
        PieceType::Queen => PROMOTION_QUEEN,
        PieceType::Rook => PROMOTION_ROOK,
        PieceType::Bishop => PROMOTION_BISHOP,
        PieceType::Knight => PROMOTION_KNIGHT,
        _ => return None,
    };

    let side = side_to_move(board);
    let mut all = MoveListBuf::new();
    generate(board, side, GenMode::All, &mut all);

    for i in 0..all.len {
        let mv = all.data[i];
        if mv.get_from() != from || mv.get_to() != to {
            continue;
        }
        let flags = mv.get_flags();
        if flags & 0b11 == PROMOTION_MOVE && flags & (0b11 << 2) != want_promo {
            continue;
        }
        return Some(mv);
    }
    None
}

// ---------------------------------------------------------------------------
// per-piece API, kept for the GUI / standalone callers. Not  to be used by the search.
// ---------------------------------------------------------------------------

// is the piece on square pinned?
pub fn is_pinned(board: &Board, square: u8, friendly_side: Side) -> bool {
    let king_square = pieces(board, friendly_side, PieceType::King).trailing_zeros() as u8;
    pinned_pieces(board, friendly_side, king_square) & (1u64 << square) != 0
}

// propagates list with possible moves for piece on square
pub fn get_pseudo_legal_moves(board: &Board, square: u8, list: &mut MoveBuf) {
    if board.is_piece(square) {
        let p = get_piece(board, square);
        let friendly = get_side_bitboard(board, p.color);
        match p.piece_type {
            PieceType::Bishop => {
                let mut moves = get_bishop_moves(board.occupied, square) & !friendly.0;

                while moves != 0 {
                    let bit = moves.trailing_zeros();
                    list.push(bit as u8);
                    moves ^= 1 << bit;
                }
            }
            PieceType::Knight => {
                let mut possible: Bitboard = KNIGHT_MOVES[square as usize];

                while possible.0 != 0 {
                    let bit = possible.0.trailing_zeros();
                    if !friendly.get(bit as u8) {
                        list.push(bit as u8);
                    }
                    possible.0 ^= 1 << bit;
                }
            }
            PieceType::King => {
                let mut possible: Bitboard = KING_MOVES[square as usize];
                while possible.0 != 0 {
                    let bit = possible.0.trailing_zeros();
                    if !friendly.get(bit as u8) {
                        list.push(bit as u8);
                    }
                    possible.0 ^= 1 << bit;
                }
            }
            PieceType::Pawn => {
                // capture moves
                let enemy = get_side_bitboard(
                    board,
                    if p.color == Side::White {
                        Side::Black
                    } else {
                        Side::White
                    },
                )
                .0;
                let possible = PAWN_MOVES[p.color as usize][square as usize];
                let stm = side_to_move(board);
                let ep_bit = if board.en_passant_target.is_some() && p.color == stm {
                    1 << board.en_passant_target.unwrap()
                } else {
                    0
                };

                let mut valid = (enemy | ep_bit) & possible.0;

                while valid != 0 {
                    list.push(valid.trailing_zeros() as u8);
                    valid ^= 1 << valid.trailing_zeros();
                }

                // move forward
                let forward: i32 = if p.color == Side::White { 8 } else { -8 };

                if (enemy | friendly.0) & (1 << (square as i32 + forward) as u8) == 0 {
                    list.push((square as i32 + forward) as u8);
                    let starting_rank = if p.color == Side::White { 1 } else { 6 };
                    if square / 8 == starting_rank {
                        if (enemy | friendly.0) & (1 << (square as i32 + forward * 2) as u8) == 0 {
                            list.push((square as i32 + forward * 2) as u8);
                        }
                    }
                }
            }
            PieceType::Queen => {
                let mut s_moves = get_rook_moves(board.occupied, square) & !friendly.0;

                while s_moves != 0 {
                    let bit = s_moves.trailing_zeros();
                    list.push(bit as u8);
                    s_moves ^= 1 << bit;
                }
                let mut d_moves = get_bishop_moves(board.occupied, square) & !friendly.0;

                while d_moves != 0 {
                    let bit = d_moves.trailing_zeros();
                    list.push(bit as u8);
                    d_moves ^= 1 << bit;
                }
            }
            PieceType::Rook => {
                let mut moves = get_rook_moves(board.occupied, square) & !friendly.0;

                while moves != 0 {
                    let bit = moves.trailing_zeros();
                    list.push(bit as u8);
                    moves ^= 1 << bit;
                }
            }
        }
    }
}

// returns the legal destination squares for the piece on `square` (for the GUI).
// Now just filters the real generator, so the GUI and the engine can never
// disagree about what's legal. One entry per destination: underpromotions are
// skipped since the GUI auto-queens (use find_legal_move to build the Move).
pub fn get_valid_moves(
    board: &mut Board,
    square: u8,
    _currently_in_check: bool,
    move_buf: &mut MoveBuf,
) {
    if !board.is_piece(square) {
        return;
    }
    let side = get_piece(board, square).color;

    let mut all = MoveListBuf::new();
    generate(board, side, GenMode::All, &mut all);

    for i in 0..all.len {
        let mv = all.data[i];
        if mv.get_from() != square || is_underpromotion(mv) {
            continue;
        }
        move_buf.push(mv.get_to());
    }
}

pub fn get_valid_moves_standalone(board: &mut Board, square: u8) -> Vec<u8> {
    let piece = if board.is_piece(square) {
        get_piece(board, square)
    } else {
        return Vec::new();
    };
    let in_check = is_in_check(board, piece.color);
    let mut buf = MoveBuf::new();
    get_valid_moves(board, square, in_check, &mut buf);

    let mut list = Vec::new();
    for i in 0..buf.len {
        list.push(buf.data[i]);
    }
    list
}
