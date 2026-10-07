use std::fmt::Write;

use crate::types;
use crate::types::{
    BLACK_LONG, BLACK_SHORT, Bitboard, Board, CASTLE_MOVE, EN_PASSANT_MOVE, Move, NORMAL_MOVE,
    PROMOTION_MOVE, Piece, PieceType, Side, Undo, WHITE_LONG, WHITE_SHORT, WHITE_TO_MOVE,
};

const CASTLE_MASK: [u8; 64] = {
    let mut m = [!0u8; 64];
    m[0] = !WHITE_LONG;
    m[4] = !(WHITE_SHORT | WHITE_LONG);
    m[7] = !WHITE_SHORT;
    m[56] = !BLACK_LONG;
    m[60] = !(BLACK_SHORT | BLACK_LONG);
    m[63] = !BLACK_SHORT;
    m
};

#[inline]
pub fn castle_rook_squares(from: u8, to: u8) -> (u8, u8) {
    let row_start = to & !7;
    if to > from {
        (row_start + 7, row_start + 5)
    } else {
        (row_start, row_start + 3)
    }
}

#[inline]
pub fn ep_victim_square(from: u8, to: u8) -> u8 {
    (from & !7) | (to & 7)
}

// board logic
pub fn new_board() -> Board {
    from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
}

pub fn get_piece(board: &Board, square: u8) -> Piece {
    board.mailbox[square as usize].expect("tried to fetch empty piece")
}

pub fn get_side_bitboard(board: &Board, side: Side) -> Bitboard {
    let mut bitboard = Bitboard::EMPTY;
    bitboard.0 |= board.bitboards[side as usize][PieceType::King as usize].0
        | board.bitboards[side as usize][PieceType::Queen as usize].0
        | board.bitboards[side as usize][PieceType::Rook as usize].0
        | board.bitboards[side as usize][PieceType::Bishop as usize].0
        | board.bitboards[side as usize][PieceType::Knight as usize].0
        | board.bitboards[side as usize][PieceType::Pawn as usize].0;
    bitboard
}

pub fn from_fen(fen: &str) -> Board {
    let mut board = Board {
        bitboards: [[Bitboard::EMPTY; 6]; 2],
        mailbox: [None; 64],
        occupied: 0,
        moves: 0,
        en_passant_target: None,
        state: 0b10000,
        half_moves: 1,
    };

    let parts: Vec<&str> = fen.split(' ').collect();
    let placements = parts[0];

    let mut rank: i32 = 7; // FEN's first rank (8) is LERF row index 7
    let mut file: u8 = 0;

    for c in placements.chars() {
        if c == '/' {
            rank -= 1;
            file = 0;
            continue;
        }
        if c.is_ascii_digit() {
            file += c.to_digit(10).unwrap() as u8;
            continue;
        }

        let side = if c.is_uppercase() {
            Side::White
        } else {
            Side::Black
        };
        let p_type = char_to_piece_type(&c);
        let square = (rank as u8) * 8 + file;
        board.bitboards[side as usize][p_type as usize].set(square);
        board.mailbox[square as usize] = Some(Piece {
            piece_type: p_type,
            color: side,
        });
        board.occupied |= 1 << (rank as u8 * 8 + file) as usize;
        file += 1;
    }

    let turn = parts[1];
    if turn.chars().next() == Some('b') {
        board.state ^= WHITE_TO_MOVE
    };

    for c in parts[2].chars() {
        match c {
            'K' => board.state ^= WHITE_SHORT,
            'Q' => board.state ^= WHITE_LONG,
            'k' => board.state ^= BLACK_SHORT,
            'q' => board.state ^= BLACK_LONG,
            _ => {}
        }
    }

    if parts[3] != "-" {
        let mut chars = parts[3].chars();
        let ep_file = chars.next().unwrap() as u8 - b'a';
        let ep_rank = chars.next().unwrap().to_digit(10).unwrap() as u8 - 1; // no flip needed under LERF
        board.en_passant_target = Some(ep_rank * 8 + ep_file);
    }

    let _halfmove = parts[4].parse::<u16>().unwrap();

    let fullmove = parts[5].parse::<u8>().unwrap();
    let ply_offset = if turn.starts_with('b') { 1 } else { 0 };
    board.moves = fullmove.saturating_sub(1) * 2 + ply_offset;

    board
}

pub fn to_fen(board: &Board) -> String {
    let mut s = String::with_capacity(80);

    for rank in (0..8).rev() {
        let mut empty = 0u32;
        for file in 0..8u8 {
            let square = rank * 8 + file;
            if board.is_piece(square) {
                if empty > 0 {
                    s.push(char::from_digit(empty, 10).unwrap());
                    empty = 0;
                }
                s.push(piece_to_char(get_piece(board, square)));
            } else {
                empty += 1;
            }
        }
        if empty > 0 {
            s.push(char::from_digit(empty, 10).unwrap());
        }
        if rank != 0 {
            s.push('/');
        }
    }

    s.push(' ');
    s.push(if board.state & WHITE_TO_MOVE != 0 {
        'w'
    } else {
        'b'
    });

    s.push(' ');
    let castle_start = s.len();
    if board.state & WHITE_SHORT != 0 {
        s.push('K');
    }
    if board.state & WHITE_LONG != 0 {
        s.push('Q');
    }
    if board.state & BLACK_SHORT != 0 {
        s.push('k');
    }
    if board.state & BLACK_LONG != 0 {
        s.push('q');
    }
    if s.len() == castle_start {
        s.push('-');
    }

    s.push(' ');
    match board.en_passant_target {
        None => s.push('-'),
        Some(q) => {
            s.push((b'a' + (q % 8)) as char);
            s.push(char::from_digit((q / 8 + 1) as u32, 10).unwrap()); // no flip needed under LERF
        }
    }

    write!(s, " {} {}", board.half_moves, board.moves).unwrap();
    s
}

fn char_to_piece_type(c: &char) -> PieceType {
    match c.to_ascii_lowercase() {
        'k' => return PieceType::King,
        'q' => return PieceType::Queen,
        'r' => return PieceType::Rook,
        'b' => return PieceType::Bishop,
        'n' => return PieceType::Knight,
        'p' => return PieceType::Pawn,
        _ => panic!("unknown piece character: {}", c),
    }
}

fn piece_to_char(piece: Piece) -> char {
    let upper_case = piece.color == Side::White;
    match piece.piece_type {
        PieceType::King => {
            if upper_case {
                return 'K';
            } else {
                return 'k';
            }
        }
        PieceType::Queen => {
            if upper_case {
                return 'Q';
            } else {
                return 'q';
            }
        }
        PieceType::Bishop => {
            if upper_case {
                return 'B';
            } else {
                return 'b';
            }
        }
        PieceType::Knight => {
            if upper_case {
                return 'N';
            } else {
                return 'n';
            }
        }
        PieceType::Rook => {
            if upper_case {
                return 'R';
            } else {
                return 'r';
            }
        }
        PieceType::Pawn => {
            if upper_case {
                return 'P';
            } else {
                return 'p';
            }
        }
    }
}

pub fn make_move(board: &mut Board, mv: Move) -> Undo {
    let from = mv.get_from();
    let to = mv.get_to();
    let flags = mv.get_flags();
    let move_type = flags & 0b11;

    let mut moving_piece = get_piece(board, from);
    let piece_type_before = moving_piece.piece_type;
    let us = moving_piece.color;

    // where the victim actually sits: same as `to`, except en passant
    // (from's rank, to's file)
    let captured_sq = if move_type == EN_PASSANT_MOVE {
        (from & !7) | (to & 7)
    } else {
        to
    };

    let undo = Undo {
        captured_piece: board.mailbox[captured_sq as usize],
        previous_en_passant_target: board.en_passant_target,
        previous_state: board.state,
    };

    board.en_passant_target = None;
    if board.state & WHITE_TO_MOVE == 0 {
        board.moves += 1;
    }
    board.state ^= WHITE_TO_MOVE;

    // all castling-rights bookkeeping in one line
    board.state &= CASTLE_MASK[from as usize] & CASTLE_MASK[to as usize];

    match move_type {
        NORMAL_MOVE => {
            if piece_type_before == PieceType::Pawn && from.abs_diff(to) == 16 {
                board.en_passant_target = Some((from + to) / 2);
            }
        }
        CASTLE_MOVE => {
            let row_start = to & !7;
            let (rook_from, rook_to) = if to > from {
                (row_start + 7, row_start + 5) // kingside
            } else {
                (row_start, row_start + 3) // queenside
            };
            let rooks = &mut board.bitboards[us as usize][PieceType::Rook as usize].0;
            *rooks &= !(1 << rook_from);
            *rooks |= 1 << rook_to;
            board.occupied &= !(1 << rook_from);
            board.occupied |= 1 << rook_to;
            board.mailbox[rook_from as usize] = None;
            board.mailbox[rook_to as usize] = Some(Piece {
                piece_type: PieceType::Rook,
                color: us,
            });
        }
        PROMOTION_MOVE => {
            moving_piece.piece_type = match flags & (0b11 << 2) {
                types::PROMOTION_QUEEN => PieceType::Queen,
                types::PROMOTION_ROOK => PieceType::Rook,
                types::PROMOTION_BISHOP => PieceType::Bishop,
                types::PROMOTION_KNIGHT => PieceType::Knight,
                _ => unreachable!(),
            };
        }
        _ => {}
    }

    // capture removal for all move types
    if let Some(victim) = undo.captured_piece {
        board.bitboards[victim.color as usize][victim.piece_type as usize].0 &= !(1 << captured_sq);
        board.occupied &= !(1 << captured_sq);
        board.mailbox[captured_sq as usize] = None;
    }

    // move the piece (a promotion places the new type, removes the pawn)
    board.bitboards[us as usize][piece_type_before as usize].0 &= !(1 << from);
    board.bitboards[us as usize][moving_piece.piece_type as usize].0 |= 1 << to;
    board.occupied &= !(1 << from);
    board.occupied |= 1 << to;
    board.mailbox[from as usize] = None;
    board.mailbox[to as usize] = Some(moving_piece);

    undo
}

pub fn undo_move(board: &mut Board, mv: Move, undo: Undo) {
    let from = mv.get_from();
    let to = mv.get_to();
    let move_type = mv.get_flags() & 0b11;

    // restore side to move + castling rights
    board.state = undo.previous_state;
    board.en_passant_target = undo.previous_en_passant_target;
    if board.state & WHITE_TO_MOVE == 0 {
        board.moves -= 1;
    }

    // the piece as it stands now (maybe a promoted queen)
    let moved = get_piece(board, to);
    let us = moved.color;

    // demote if it was promoted
    let original_type = if move_type == PROMOTION_MOVE {
        PieceType::Pawn
    } else {
        moved.piece_type
    };

    // move piece back
    board.bitboards[us as usize][moved.piece_type as usize].0 &= !(1 << to);
    board.bitboards[us as usize][original_type as usize].0 |= 1 << from;
    board.occupied &= !(1 << to);
    board.occupied |= 1 << from;
    board.mailbox[to as usize] = None;
    board.mailbox[from as usize] = Some(Piece {
        piece_type: original_type,
        color: us,
    });

    if move_type == CASTLE_MOVE {
        let (rook_from, rook_to) = castle_rook_squares(from, to);
        let rooks = &mut board.bitboards[us as usize][PieceType::Rook as usize].0;
        *rooks &= !(1 << rook_to);
        *rooks |= 1 << rook_from;
        board.occupied &= !(1 << rook_to);
        board.occupied |= 1 << rook_from;
        board.mailbox[rook_to as usize] = None;
        board.mailbox[rook_from as usize] = Some(Piece {
            piece_type: PieceType::Rook,
            color: us,
        });
    }

    // put victim back (normal/promo captures, en passant)
    if let Some(victim) = undo.captured_piece {
        let captured_sq = if move_type == EN_PASSANT_MOVE {
            ep_victim_square(from, to)
        } else {
            to
        };
        board.bitboards[victim.color as usize][victim.piece_type as usize].0 |= 1 << captured_sq;
        board.occupied |= 1 << captured_sq;
        board.mailbox[captured_sq as usize] = Some(victim);
    }
}

// check if a side has a piece that is not king or pawn
pub fn has_non_pawn_piece(board: &Board, side: Side) -> bool {
    if board.bitboards[side as usize][PieceType::Bishop as usize].0 != 0 {
        return true;
    }
    if board.bitboards[side as usize][PieceType::Rook as usize].0 != 0 {
        return true;
    }
    if board.bitboards[side as usize][PieceType::Knight as usize].0 != 0 {
        return true;
    }
    if board.bitboards[side as usize][PieceType::Queen as usize].0 != 0 {
        return true;
    }
    false
}
