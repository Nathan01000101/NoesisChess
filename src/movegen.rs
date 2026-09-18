use crate::types::{Bitboard, Side, PieceType, Board, MoveBuf, MoveListBuf, WHITE_TO_MOVE, WHITE_LONG, WHITE_SHORT, BLACK_LONG, BLACK_SHORT};
use crate::board::{get_piece, make_move, undo_move, get_side_bitboard};
use crate::attacks::{KING_MOVES, KNIGHT_MOVES, PAWN_MOVES, get_bishop_moves, get_rook_moves};

const FILE_A: u64 = 0x0101_0101_0101_0101;
const FILE_H: u64 = 0x8080_8080_8080_8080;
const RANK_3: u64 = 0x0000_0000_00FF_0000;
const RANK_6: u64 = 0x0000_FF00_0000_0000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenMode { All, Captures, Quiets }

// returns the other side, e.g. if white -> ret black
#[inline]
fn other(side: Side) -> Side {
    if side == Side::White { Side::Black } else { Side::White }
}

// returns board's bitboard of the given piecetype
#[inline]
fn pieces(board: &Board, side: Side, pt: PieceType) -> u64 {
    board.bitboards[side as usize][pt as usize].0
}

// returns the side that is next to move
#[inline]
fn side_to_move(board: &Board) -> Side {
    if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black }
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
    if pawn_attackers_of(t, by) & pieces(board, by, PieceType::Pawn) != 0 { return true; }
    if KNIGHT_MOVES[square as usize].0 & pieces(board, by, PieceType::Knight) != 0 { return true; }
    if KING_MOVES[square as usize].0 & pieces(board, by, PieceType::King) != 0 { return true; }
    let queens = pieces(board, by, PieceType::Queen);
    if get_rook_moves(occupied, square) & (pieces(board, by, PieceType::Rook) | queens) != 0 { return true; }
    if get_bishop_moves(occupied, square) & (pieces(board, by, PieceType::Bishop) | queens) != 0 { return true; }
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
            out.push(king_sq, to);
        }
        king_targets &= king_targets - 1;
    }

    // castling  -> quiet only when not in check
    if !in_check && mode != GenMode::Captures {
        let (can_long, can_short) = if side == Side::White {
            (board.state & WHITE_LONG != 0, board.state & WHITE_SHORT != 0)
        } else {
            (board.state & BLACK_LONG != 0, board.state & BLACK_SHORT != 0)
        };
        let base = (king_sq / 8) * 8;
        if can_long
            && !board.is_piece(base + 1) && !board.is_piece(base + 2) && !board.is_piece(base + 3)
            && !is_attacked(board, base + 2, enemy, occ)
            && !is_attacked(board, base + 3, enemy, occ)
        {
            out.push(king_sq, base + 2);
        }
        if can_short
            && !board.is_piece(base + 5) && !board.is_piece(base + 6)
            && !is_attacked(board, base + 5, enemy, occ)
            && !is_attacked(board, base + 6, enemy, occ)
        {
            out.push(king_sq, base + 6);
        }
    }

    // when in double check only the king can move
    if double_check { return; }

    let target = if in_check { mode_mask & check_mask(board, king_sq, checkers) } else { mode_mask };

    let pinned = pinned_pieces(board, side, king_sq);

    // friendly, non king pieces that arent pinned
    let free = friendly & !pinned & !king_bb;

    // knights
    let mut knights = pieces(board, side, PieceType::Knight) & free;
    while knights != 0 {
        let from = knights.trailing_zeros() as u8;
        let mut t = KNIGHT_MOVES[from as usize].0 & target;
        while t != 0 {
            out.push(from, t.trailing_zeros() as u8);
            t &= t - 1;
        }
        knights &= knights - 1;
    }

    // queens, bishops and rooks
    let queens = pieces(board, side, PieceType::Queen);

    let mut diag = (pieces(board, side, PieceType::Bishop) | queens) & free;
    while diag != 0 {
        let from = diag.trailing_zeros() as u8;
        let mut t = get_bishop_moves(occ, from) & target;
        while t != 0 {
            out.push(from, t.trailing_zeros() as u8);
            t &= t - 1;
        }
        diag &= diag - 1;
    }

    let mut orth = (pieces(board, side, PieceType::Rook) | queens) & free;
    while orth != 0 {
        let from = orth.trailing_zeros() as u8;
        let mut t = get_rook_moves(occ, from) & target;
        while t != 0 {
            out.push(from, t.trailing_zeros() as u8);
            t &= t - 1;
        }
        orth &= orth - 1;
    }

    // pawns TODO: MAKE PROMOTING TO OTHER PIECES AVAILABLE
    // FOR RIGHT NOW, AUTO PROMOTE TO QUEEN
    let pawns = pieces(board, side, PieceType::Pawn) & free;

    if mode != GenMode::Captures {
        if side == Side::White {
            let single = (pawns << 8) & !occ;
            let double = ((single & RANK_3) << 8) & !occ;
            let mut s = single & target;
            while s != 0 {
                let to = s.trailing_zeros() as u8;
                out.push(to - 8, to);
                s &= s - 1;
            }
            let mut d = double & target;
            while d != 0 {
                let to = d.trailing_zeros() as u8;
                out.push(to - 16, to);
                d &= d - 1;
            }
        } else {
            let single = (pawns >> 8) & !occ;
            let double = ((single & RANK_6) >> 8) & !occ;
            let mut s = single & target;
            while s != 0 {
                let to = s.trailing_zeros() as u8;
                out.push(to + 8, to);
                s &= s - 1;
            }
            let mut d = double & target;
            while d != 0 {
                let to = d.trailing_zeros() as u8;
                out.push(to + 16, to);
                d &= d - 1;
            }
        }
    }

    if mode != GenMode::Quiets {
        if side == Side::White {
            let mut left = (pawns << 7) & !FILE_H & enemy_bb & target;
            while left != 0 {
                let to = left.trailing_zeros() as u8;
                out.push(to - 7, to);
                left &= left - 1;
            }
            let mut right = (pawns << 9) & !FILE_A & enemy_bb & target;
            while right != 0 {
                let to = right.trailing_zeros() as u8;
                out.push(to - 9, to);
                right &= right - 1;
            }
        } else {
            let mut left = (pawns >> 7) & !FILE_A & enemy_bb & target;
            while left != 0 {
                let to = left.trailing_zeros() as u8;
                out.push(to + 7, to);
                left &= left - 1;
            }
            let mut right = (pawns >> 9) & !FILE_H & enemy_bb & target;
            while right != 0 {
                let to = right.trailing_zeros() as u8;
                out.push(to + 9, to);
                right &= right - 1;
            }
        }
    }

    if mode != GenMode::Quiets && side == side_to_move(board) {
        if let Some(ep) = board.en_passant_target {
            let mut cands = pawn_attackers_of(1u64 << ep, side) & pieces(board, side, PieceType::Pawn);
            while cands != 0 {
                let from = cands.trailing_zeros() as u8;
                let undo = make_move(board, from, ep);
                let ok = !is_in_check(board, side);
                undo_move(board, undo);
                if ok { out.push(from, ep); }
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
            if piece.piece_type == PieceType::Knight { continue; }

            let mut pseudo = MoveBuf::new();
            get_pseudo_legal_moves(board, from, &mut pseudo);
            for i in 0..pseudo.len {
                let to = pseudo.data[i];
                if piece.piece_type == PieceType::Pawn && Some(to) == ep { continue; } // done above
                let is_capture = board.is_piece(to);
                match mode {
                    GenMode::Captures if !is_capture => continue,
                    GenMode::Quiets if is_capture => continue,
                    _ => {}
                }
                let undo = make_move(board, from, to);
                let ok = !is_in_check(board, side);
                undo_move(board, undo);
                if ok { out.push(from, to); }
            }
        }
    }
}

// gets all moves that a side can make ((from), (to))
pub fn get_all_moves(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    generate(board, side, GenMode::All, buffer);
}

// gets all captures a side can make ((from), (to)), including en passant
pub fn get_all_captures(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    generate(board, side, GenMode::Captures, buffer);
}

// gets all moves for a side that aren't captures
pub fn get_all_quiets(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    generate(board, side, GenMode::Quiets, buffer);
}

// gets num of moves that a side can make
pub fn get_move_count(board: &mut Board, side: Side) -> u8 {
    let mut buf = MoveListBuf::new();
    generate(board, side, GenMode::All, &mut buf);
    buf.len as u8
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
pub fn get_pseudo_legal_moves(board: &Board, square: u8, list: &mut MoveBuf){
    if board.is_piece(square) {
        let p = get_piece(board, square);
        let friendly = get_side_bitboard(board, p.color);
        match p.piece_type{
            PieceType::Bishop => {
                let mut moves = get_bishop_moves(board.occupied, square) & !friendly.0 ;

                while moves != 0{
                    let bit = moves.trailing_zeros();
                    list.push(bit as u8);
                    moves ^= 1 << bit;
                }
            },
            PieceType::Knight => {
                let mut possible: Bitboard = KNIGHT_MOVES[square as usize];

                while possible.0 != 0{
                    let bit = possible.0.trailing_zeros();
                    if !friendly.get(bit as u8){
                        list.push(bit as u8);
                    }
                    possible.0 ^= 1 << bit;
                }
            },
            PieceType::King => {
                let mut possible: Bitboard = KING_MOVES[square as usize];
                while possible.0 != 0{
                    let bit = possible.0.trailing_zeros();
                    if !friendly.get(bit as u8){
                        list.push(bit as u8);
                    }
                    possible.0 ^= 1 << bit;
                }
            },
            PieceType::Pawn => {

                // capture moves
                let enemy = get_side_bitboard(board, if p.color == Side::White {Side::Black} else {Side::White}).0;
                let possible = PAWN_MOVES[p.color as usize][square as usize];
                let stm = side_to_move(board);
                let ep_bit = if board.en_passant_target.is_some() && p.color == stm {
                    1 << board.en_passant_target.unwrap()
                } else { 0 };

                let mut valid = (enemy | ep_bit) & possible.0;

                while valid != 0{
                    list.push(valid.trailing_zeros() as u8);
                    valid ^= 1 << valid.trailing_zeros();
                }

                // move forward
                let forward:i32 = if p.color == Side::White { 8 } else { -8 };

                if (enemy | friendly.0) & (1 << (square as i32 + forward) as u8) == 0{
                    list.push((square as i32 + forward) as u8);
                    let starting_rank = if p.color == Side::White { 1 } else { 6 };
                    if square / 8 == starting_rank{
                        if (enemy | friendly.0) & (1 << ((square as i32 + forward*2)) as u8) == 0{
                            list.push(((square as i32 + forward*2)) as u8);
                        }
                    }

                }

            },
            PieceType::Queen => {
                let mut s_moves = get_rook_moves(board.occupied, square) & !friendly.0 ;

                while s_moves != 0{
                    let bit = s_moves.trailing_zeros();
                    list.push(bit as u8);
                    s_moves ^= 1 << bit;
                }
                let mut d_moves = get_bishop_moves(board.occupied, square) & !friendly.0 ;

                while d_moves != 0{
                    let bit = d_moves.trailing_zeros();
                    list.push(bit as u8);
                    d_moves ^= 1 << bit;
                }
            }
            PieceType::Rook => {
                let mut moves = get_rook_moves(board.occupied, square) & !friendly.0 ;

                while moves != 0{
                    let bit = moves.trailing_zeros();
                    list.push(bit as u8);
                    moves ^= 1 << bit;
                }
            }
        }
    }
}

// returns all valid moves for a piece
pub fn get_valid_moves(board: &mut Board, square: u8, currently_in_check: bool, move_buf: &mut MoveBuf){
    if !board.is_piece(square) {return}
    let piece = get_piece(board, square);
    let mut not_checked = MoveBuf::new();
    get_pseudo_legal_moves(&board, square, &mut not_checked);

    // add castling move if neccesary
    if piece.piece_type == PieceType::King{
        let opposite_side = other(piece.color);
        let can_long = if piece.color == Side::White {board.state & WHITE_LONG != 0} else {board.state & BLACK_LONG != 0};
        let can_short = if piece.color == Side::White {board.state & WHITE_SHORT != 0} else {board.state & BLACK_SHORT != 0};

        let row = square / 8;
        if can_long{
            if !board.is_piece(row * 8 + 1) && !board.is_piece(row * 8 + 2) && !board.is_piece(row * 8 + 3) {
                if
                    !is_square_attacked(&board, row * 8 + 2 , opposite_side)
                    && !is_square_attacked(&board, row * 8 + 3, opposite_side) {
                    if !currently_in_check {
                        not_checked.push(row * 8 + 2);
                    }
                }
            }
        }

        if can_short {
            if !board.is_piece(row * 8 + 6) && !board.is_piece(row * 8 + 5) {
                if !is_square_attacked(&board, row * 8 + 6, opposite_side)
                    && !is_square_attacked(&board, row * 8 + 5, opposite_side) {
                    if !currently_in_check {
                        not_checked.push(row * 8 + 6);
                    }
                }
            }
        }
    }

    // --- legality filtering ---
    let moving_color = piece.color;
    let piece_is_king = piece.piece_type == PieceType::King;
    let pinned = is_pinned(board, square, moving_color);
    let ep_target = board.en_passant_target;

    let friendly = get_side_bitboard(board, moving_color);
    for i in 0..not_checked.len {
        if friendly.get(not_checked.data[i]) {
            continue;
        }

        let is_en_passant = piece.piece_type == PieceType::Pawn
            && ep_target.is_some()
            && not_checked.data[i] == ep_target.unwrap();

        let needs_full_check = piece_is_king
            || currently_in_check
            || pinned
            || is_en_passant;

        if !needs_full_check {
            move_buf.push(not_checked.data[i]);
            continue;
        }

        let undo = make_move(board, square, not_checked.data[i]);
        let in_check = is_in_check(board, moving_color);
        undo_move(board, undo);
        if !in_check {
            move_buf.push(not_checked.data[i]);
        }
    }
}

pub fn get_valid_moves_standalone(board: &mut Board, square: u8) -> Vec<u8> {
    let piece = if board.is_piece(square) { get_piece(board, square)} else {return Vec::new()};
    let in_check = is_in_check(board, piece.color);
    let mut buf = MoveBuf::new();
    get_valid_moves(board, square, in_check, &mut buf);

    let mut list = Vec::new();
    for i in 0..buf.len{
        list.push(buf.data[i]);
    }
    list
}