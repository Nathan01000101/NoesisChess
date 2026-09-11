use macroquad::{ prelude::*};
use macroquad::audio::{load_sound, play_sound_once};
use crate::ai::Player;
use crate::human::HumanPlayer;
use crate::random_ai::RandomAI;
use crate::minimax_ai::{MinimaxAI};
use std::fmt::Write;
use std::time::Duration;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self};
use std::env;

mod ai;
mod human;
mod random_ai;
mod minimax_ai;
mod tests;

const WINDOW_SIZE: f32 = 600.0;
pub const DEPTH: usize = 5;

// for accessing board struct
const WHITE_SHORT:  u8 = 0b00001;
const WHITE_LONG:   u8 = 0b00010;
const BLACK_SHORT:  u8 = 0b00100;
const BLACK_LONG:   u8 = 0b01000;
const WHITE_TO_MOVE: u8 = 0b10000;

const DIAGONAL_DIRS: [(i16, i16); 4] = [(-1, 1), (1, 1), (1, -1), (-1, -1)];
const DIAGONAL_MOVES: [[Bitboard; 4]; 64] = build_diagonal_attacks();

const STRAIGHT_DIRS: [(i16, i16); 4] = [(1, 0), (-1, 0), (0, -1), (0, 1)];
const STRAIGHT_MOVES: [[Bitboard; 4]; 64] = build_straight_attacks();

const KNIGHT_OFFSETS: [(i16, i16); 8] = [
    (-2, -1), (-2, 1), (-1, -2), (-1, 2),
    (1, -2), (1, 2), (2, -1), (2, 1),
];
const KNIGHT_MOVES: [Bitboard; 64] = build_knight_attacks();

const PAWN_MOVES: [[Bitboard; 64]; 2] = build_pawn_attacks();

const KING_OFFSETS: [(i16, i16); 8] = [
    (-1, -1), (-1, 0), (-1, 1), (0, -1),
    (0, 1), (1, -1), (1, 0), (1, 1),
];
const KING_MOVES: [Bitboard; 64] = build_king_attacks();

#[derive(Clone, Copy, PartialEq, Debug)]
enum PieceType {
    Pawn = 5, Knight = 4, Bishop = 3, Rook = 2, Queen = 1, King = 0
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    White = 0, Black = 1
}

enum DiagonalDirections{
    SouthEast = 0, NorthEast = 1, NorthWest = 2, SouthWest = 3
}

enum Directions{
    North = 0, South = 1, West = 2, East = 3
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Self = Self(0);

    pub const fn set(&mut self, square: u8){
        self.0 |= 1 << square;
    }

    pub fn get(&self, square:u8) -> bool{
        (self.0 & (1 << square)) != 0
    }
}

struct MoveBuf {
    data: [u8; 32],
    len: usize,
}
impl MoveBuf {
    fn new() -> Self { Self { data: [0; 32], len: 0 } }
    fn push(&mut self, sq: u8) { self.data[self.len] = sq; self.len += 1; }
}

struct MoveListBuf {
    data: [(u8, u8); 218],
    len: usize,
}
impl MoveListBuf {
    fn new() -> Self { Self { data: [(0, 0); 218], len: 0 } }
    fn push(&mut self, from: u8, to: u8) {
        self.data[self.len] = (from, to);
        self.len += 1;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Piece {
    piece_type: PieceType,
    color: Side
}


#[derive(Clone, Copy, PartialEq, Debug)]
struct Board{
    // bitboards[color][piecetype]
    // 0 -> white   1 -> black
    // 0 -> king    1 -> queen  2 -> rook   3 -> bishop 4 -> knight 5 -> pawn
    bitboards: [[Bitboard; 6]; 2],
    occupied: u64,
    moves: u8,
    en_passant_target: Option<u8>,
    state: u8 
    // 5th bit -> white to move,  4th bit -> black long castle right, 3rd -> black short castle right
    // 2nd bit -> white long castle right, 1st bit -> white short castle right
}


#[derive(Clone, Copy, PartialEq)]
struct Move{
    from: u8,
    to: u8,

}

#[derive(Clone, Copy, PartialEq)]
struct Undo{
    last_move: Move, // (from, to)
    moving_piece_before: Piece,
    captured_piece: Option<Piece>, // what piece occupied the square before moving
    previous_en_passant_target: Option<u8>, // stores where the en_passant target was before moving
    previous_state: u8
}

// board logic
fn new_board() -> Board {
    from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
}

fn is_piece(board: &Board, coord: u8) -> bool {
    board.occupied & 1 << coord != 0
}

fn get_piece(board: &Board, coord: u8) -> Piece {
    for i in 0..2 {
        for j in 0..6 {
            if board.bitboards[i][j].get(coord) {
                let color = if i == 0 { Side::White } else { Side::Black };
                let piece_type = match j {
                    0 => PieceType::King,
                    1 => PieceType::Queen,
                    2 => PieceType::Rook,
                    3 => PieceType::Bishop,
                    4 => PieceType::Knight,
                    5 => PieceType::Pawn,
                    _ => unreachable!(),
                };
                return Piece { piece_type, color };
            }
        }
    }
    panic!("tried to fetch empty piece");
}

fn get_side_bitboard(board: &Board, side: Side) -> Bitboard{
    let mut bitboard = Bitboard::EMPTY;
    bitboard.0 |= board.bitboards[side as usize][PieceType::King as usize].0 | board.bitboards[side as usize][PieceType::Queen as usize].0 | board.bitboards[side as usize][PieceType::Rook as usize].0 | board.bitboards[side as usize][PieceType::Bishop as usize].0 | board.bitboards[side as usize][PieceType::Knight as usize].0 | board.bitboards[side as usize][PieceType::Pawn as usize].0;
    bitboard
}

fn from_fen(fen: &str) -> Board {
    let mut board = Board { bitboards: [[Bitboard::EMPTY; 6]; 2], occupied: 0, moves: 0, en_passant_target: None, state: 0b10000 };

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

        let side = if c.is_uppercase() { Side::White } else { Side::Black };
        let p_type = char_to_piece_type(&c);
        let square = (rank as u8) * 8 + file;
        board.bitboards[side as usize][p_type as usize].set(square);
        board.occupied |= 1 << (rank as u8 * 8 + file) as usize;
        file += 1;
    }

    let turn = parts[1];
    if turn.chars().next() == Some('b') { board.state ^= WHITE_TO_MOVE };

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

    let fullmove = parts[5].parse::<u8>().unwrap();
    let ply_offset = if turn.starts_with('b') { 1 } else { 0 };
    board.moves = fullmove.saturating_sub(1) * 2 + ply_offset;

    board
}

fn to_fen(board: &Board) -> String {
    let mut s = String::with_capacity(80);

    for rank in (0..8).rev() {
        let mut empty = 0u32;
        for file in 0..8u8 {
            let square = rank * 8 + file;
            if is_piece(board, square) {
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
        if rank != 0 { s.push('/'); }
    }

    s.push(' ');
    s.push(if board.state & WHITE_TO_MOVE != 0 { 'w' } else { 'b' });

    s.push(' ');
    let castle_start = s.len();
    if board.state & WHITE_SHORT != 0 { s.push('K'); }
    if board.state & WHITE_LONG  != 0 { s.push('Q'); }
    if board.state & BLACK_SHORT != 0 { s.push('k'); }
    if board.state & BLACK_LONG  != 0 { s.push('q'); }
    if s.len() == castle_start { s.push('-'); }

    s.push(' ');
    match board.en_passant_target {
        None => s.push('-'),
        Some(q) => {
            s.push((b'a' + (q % 8)) as char);
            s.push(char::from_digit((q / 8 + 1) as u32, 10).unwrap()); // no flip needed under LERF
        }
    }

    write!(s, " 0 {}", board.moves / 2 + 1).unwrap();
    s
}

fn char_to_piece_type(c: &char) -> PieceType{
    match c.to_ascii_lowercase(){
        'k' => return PieceType::King,
        'q' => return PieceType::Queen,
        'r' => return PieceType::Rook,
        'b' => return PieceType::Bishop,
        'n' => return PieceType::Knight,
        'p' => return PieceType::Pawn,
        _   => panic!("unknown piece character: {}", c)
    }
}

fn piece_to_char(piece: Piece) -> char{
    let upper_case = piece.color == Side::White;
    match piece.piece_type{
        PieceType::King     => if upper_case { return 'K'} else {return 'k'},
        PieceType::Queen    => if upper_case { return 'Q'} else {return 'q'},
        PieceType::Bishop   => if upper_case { return 'B'} else {return 'b'},
        PieceType::Knight   => if upper_case { return 'N'} else {return 'n'},
        PieceType::Rook     => if upper_case { return 'R'} else {return 'r'},
        PieceType::Pawn     => if upper_case { return 'P'} else {return 'p'}
    }
}

const fn in_bounds_row_col(row: i16, col:i16) -> bool{
    row >= 0 && row < 8 && col >= 0 && col < 8
}

const fn build_knight_attacks() -> [Bitboard; 64]{
    let mut bitboards = [Bitboard::EMPTY; 64];    
    let mut i = 0;
    while i < 64{
        let r = i / 8;
        let c = i - r*8;

        let mut offset_counter = 0;
        while offset_counter < KNIGHT_OFFSETS.len(){
            let new_r = r + KNIGHT_OFFSETS[offset_counter].0;
            let new_c = c + KNIGHT_OFFSETS[offset_counter].1;

            if in_bounds_row_col(new_r, new_c){
                bitboards[i as usize].set((new_r*8 + new_c) as u8);
            }
            offset_counter += 1;
        }
        i += 1;
    }
    bitboards
}

const fn build_pawn_attacks() -> [[Bitboard; 64]; 2]{
    let mut bitboards = [[Bitboard::EMPTY; 64]; 2];    
    let mut i = 8;
    
    //white pawns
    while i < 56{

        if i % 8 != 0{
            bitboards[0][i].0 |= 1 << (i + 7);
        }
        if i % 8 != 7{
            bitboards[0][i].0 |= 1 << (i + 9);
        }

        i += 1;
    }
    
    i = 8;

    //black pawns
    while i < 56{

        if i % 8 != 0{
            bitboards[1][i].0 |= 1 << (i - 9);
        }
        if i % 8 != 7{
            bitboards[1][i].0 |= 1 << (i - 7);
        }
        i += 1;
    }

    bitboards
}

const fn build_king_attacks() -> [Bitboard; 64]{
    let mut bitboards = [Bitboard::EMPTY; 64];    
    let mut i = 0;
    while i < 64{
        let r = i / 8;
        let c = i - r*8;

        let mut offset_counter = 0;
        while offset_counter < KING_OFFSETS.len(){
            let new_r = r + KING_OFFSETS[offset_counter].0;
            let new_c = c + KING_OFFSETS[offset_counter].1;

            if in_bounds_row_col(new_r, new_c){
                bitboards[i as usize].set((new_r*8 + new_c) as u8);
            }
            offset_counter += 1;
        }
        i += 1;
    }
    bitboards
}

const fn build_diagonal_attacks() -> [[Bitboard; 4]; 64]{
    let mut bitboards = [[Bitboard::EMPTY; 4]; 64];
    let mut i = 0;
    while i < 64{
        let mut j = 0;
        let row = i / 8;
        let col = i - row * 8;
        while j < 4{
            let mut stopped = false;
            let mut step = 1;

            while !stopped{
                let new_r = row + DIAGONAL_DIRS[j].0 * step;
                let new_c = col + DIAGONAL_DIRS[j].1 * step;

                if in_bounds_row_col(new_r, new_c){
                    bitboards[i as usize][j as usize].set((new_r * 8 + new_c) as u8);
                }else{
                    stopped = true;
                }

                step += 1;
            }

            j += 1;
        }

        i += 1;
    }

    bitboards
}

const fn build_straight_attacks() -> [[Bitboard; 4]; 64]{
    let mut bitboards = [[Bitboard::EMPTY; 4]; 64];
    let mut i = 0;
    while i < 64{
        let mut j = 0;
        let row = i / 8;
        let col = i - row * 8;
        while j < 4{
            let mut stopped = false;
            let mut step = 1;

            while !stopped{
                let new_r = row + STRAIGHT_DIRS[j].0 * step;
                let new_c = col + STRAIGHT_DIRS[j].1 * step;

                if in_bounds_row_col(new_r, new_c){
                    bitboards[i as usize][j as usize].set((new_r * 8 + new_c) as u8);
                }else{
                    stopped = true;
                }

                step += 1;
            }

            j += 1;
        }

        i += 1;
    }

    bitboards
}

// Returns true if `sq` lies on the same rank, file, or diagonal as `king`.
fn is_on_ray_from(king: u8, sq: u8) -> bool {
    if king == sq { return false; }
    let dr = (sq / 8 )as i16 - (king / 8) as i16;
    let dc = (sq - (sq / 8) * 8 ) as i16 - (king - (king / 8) * 8 ) as i16;
    // Same rank, same file, or same diagonal (|dr| == |dc|).
    dr == 0 || dc == 0 || dr.abs() == dc.abs()
}

fn ray_march(origin: u8, direction: (i16, i16), enemy_bitboard: Bitboard, friendly_bitboard: Bitboard, buffer: &mut MoveBuf){
    let mut bb = match direction {
        (-1,  1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::SouthEast as usize],
        ( 1,  1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::NorthEast as usize],
        ( 1, -1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::NorthWest as usize],
        (-1, -1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::SouthWest as usize],
        ( 1,  0)    => STRAIGHT_MOVES[origin as usize][Directions::North as usize],
        (-1,  0)    => STRAIGHT_MOVES[origin as usize][Directions::South as usize],
        ( 0, -1)    => STRAIGHT_MOVES[origin as usize][Directions::West as usize],
        ( 0,  1)    => STRAIGHT_MOVES[origin as usize][Directions::East as usize],
        _=> panic!("Error, tried to ray march an invalid direction ")
    };
    let delta = direction.0 * 8 + direction.1;

    let occupied = enemy_bitboard.0 | friendly_bitboard.0;
    if delta > 0{
        while bb.0 != 0{
            let bit_position = bb.0.trailing_zeros();

            if occupied & (1 << bit_position) != 0{
                if enemy_bitboard.0 & (1 << bit_position) != 0{
                    buffer.push(bit_position as u8);
                }
                return;
            }

            buffer.push(bit_position as u8);
            bb.0 ^= 1 << bit_position;
        }
    }else{
        while bb.0 != 0{
            let bit_position = 63 - bb.0.leading_zeros();

            if occupied & (1 << bit_position) != 0{
                if enemy_bitboard.0 & (1 << bit_position) != 0{
                    buffer.push(bit_position as u8);
                }
                return;
            }
            buffer.push(bit_position as u8);
            bb.0 ^= 1 << bit_position;
        }
    }
}

fn ray_march_captures_only(origin: u8, direction: (i16, i16), enemy_bitboard: Bitboard, friendly_bitboard: Bitboard, buffer: &mut MoveBuf){
    let mut bb = match direction {
        (-1,  1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::SouthEast as usize],
        ( 1,  1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::NorthEast as usize],
        ( 1, -1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::NorthWest as usize],
        (-1, -1)    => DIAGONAL_MOVES[origin as usize][DiagonalDirections::SouthWest as usize],
        ( 1,  0)    => STRAIGHT_MOVES[origin as usize][Directions::North as usize],
        (-1,  0)    => STRAIGHT_MOVES[origin as usize][Directions::South as usize],
        ( 0, -1)    => STRAIGHT_MOVES[origin as usize][Directions::West as usize],
        ( 0,  1)    => STRAIGHT_MOVES[origin as usize][Directions::East as usize],
        _=> panic!("Error, tried to ray march an invalid direction ")
    };
    let delta = direction.0 * 8 + direction.1;

    let occupied = enemy_bitboard.0 | friendly_bitboard.0;
    if delta > 0{
        while bb.0 != 0{
            let bit_position = bb.0.trailing_zeros();

            if occupied & (1 << bit_position) != 0{
                if enemy_bitboard.0 & (1 << bit_position) != 0{
                    buffer.push(bit_position as u8);
                }
                return;
            }
            bb.0 ^= 1 << bit_position;
        }
    }else{
        while bb.0 != 0{
            let bit_position = 63 - bb.0.leading_zeros();

            if occupied & (1 << bit_position) != 0{
                if enemy_bitboard.0 & (1 << bit_position) != 0{
                    buffer.push(bit_position as u8);
                }
                return;
            }
            bb.0 ^= 1 << bit_position;
        }
    }
}

// determines if a side is in check with a given board state
fn is_in_check(board: &Board, side: Side) -> bool {
    
    let enemy = if side == Side::White {Side::Black} else {Side::White};
    let king: u8 = board.bitboards[side as usize][PieceType::King as usize].0.trailing_zeros() as u8;
    is_square_attacked(board, king, enemy)
}

// determines if a given square is attacked by a given side
fn is_square_attacked(board: &Board, square: u8, side: Side) -> bool{
    // pawns?
    let dir: i16 = if side == Side::White {-8} else {8};
    if square % 8 != 0{
        if board.bitboards[side as usize][PieceType::Pawn as usize].0 & (1 << (square as i16 + dir - 1) as u8) != 0{
            return true;
        }
    }
    if square % 8 != 7{
        if board.bitboards[side as usize][PieceType::Pawn as usize].0 & (1 << (square as i16 + dir + 1) as u8) != 0{
            return true;
        }
    }

    //knights?
    let mut knight_bb = board.bitboards[side as usize][PieceType::Knight as usize];
    while knight_bb.0 != 0 {
        let bit_position = knight_bb.0.trailing_zeros();

        if KNIGHT_MOVES[bit_position as usize].0 & (1 << square) != 0{
            return true;
        }

        knight_bb.0 ^= 1 << bit_position;
    }

    // check straight lines for queens or rooks
    let our_side = if side == Side::White {Side::Black} else {Side::White};
    let friendly_bb = get_side_bitboard(board, our_side);
    let enemy_bb = get_side_bitboard(board, side);
    
    for dir in STRAIGHT_DIRS{
        let mut move_buffer = MoveBuf::new();
        ray_march_captures_only(square, dir, enemy_bb, friendly_bb, &mut move_buffer);
        if move_buffer.len != 0{
            let blocker = get_piece(board, move_buffer.data[0]);
            if blocker.piece_type == PieceType::Queen || blocker.piece_type == PieceType::Rook{
                return true;
            }
        }
    }
    

    // check diagonal lines for queens or bishops
    let our_side = if side == Side::White {Side::Black} else {Side::White};
    let friendly_bb = get_side_bitboard(board, our_side);
    let enemy_bb = get_side_bitboard(board, side);
    for dir in DIAGONAL_DIRS{
        let mut move_buffer = MoveBuf::new();
        ray_march_captures_only(square, dir, enemy_bb, friendly_bb, &mut move_buffer);
        if move_buffer.len != 0{
            let blocker = get_piece(board, move_buffer.data[0]);
            if blocker.piece_type == PieceType::Queen || blocker.piece_type == PieceType::Bishop{
                return true;
            }
        }
    }

    // king ? 
    if KING_MOVES[board.bitboards[side as usize][PieceType::King as usize].0.trailing_zeros() as usize].0 & (1 << square) != 0{
        return true;
    }

    false
}

fn make_move(board: &mut Board, old: u8, new: u8) -> Undo{
    if let Some(mut p) = Some(get_piece(board, old)) {
        let mut undo: Undo = Undo {last_move: Move {to: new, from: old},
                        moving_piece_before: p,
                        captured_piece: None, 
                        previous_en_passant_target: None,
                        previous_state: board.state};
        undo.previous_en_passant_target = board.en_passant_target;
        undo.captured_piece = if is_piece(board, new) {Some(get_piece(board, new))} else {None};
        let mut captured_bit = new;
        
        // remove en passant target
        board.en_passant_target = None;
        board.moves += 1;
        board.state ^= WHITE_TO_MOVE;
        if p.piece_type == PieceType::Pawn{
            
            let delta = new as i16 - old as i16;
            if  delta == 16 || delta == -16{
                board.en_passant_target = Some((old as i16 + delta / 2) as u8);
            }
            // check for en passant and for updating doubled moved
            if new % 8 != old % 8 {
                if !is_piece(board, new){
                    let captured_piece = get_piece(board, ((old / 8) * 8) + (new - (new/8) * 8));
                    undo.captured_piece = Some(captured_piece);
                    captured_bit = ((old / 8) * 8) + (new - (new/8) * 8);
                    board.bitboards[captured_piece.color as usize][captured_piece.piece_type as usize].0 &= !(1 << ((old / 8) * 8) + (new - (new/8) * 8));
                    board.occupied &= !(1 << ((old / 8) * 8) + (new - (new/8) * 8));
                } 
            }

            // check if promoted
            if new < 8 || new > 55{
                p.piece_type = PieceType::Queen;
            }
        }else if p.piece_type == PieceType::King {
            if p.color == Side::White{
                board.state &= !(WHITE_SHORT | WHITE_LONG);
            }else{
                board.state &= !(BLACK_SHORT | BLACK_LONG);
            }
            let delta = new as i16 - old as i16;
            let row = new / 8;
            if delta == 2{
                board.bitboards[p.color as usize][PieceType::Rook as usize].0 &= !(1 << row * 8 + 7);
                board.bitboards[p.color as usize][PieceType::Rook as usize].0 |= 1 << row * 8 + 5;
                board.occupied &= !(1 << row * 8 + 7);
                board.occupied |= 1 << row * 8 + 5;
            } else if delta == -2 {
                board.bitboards[p.color as usize][PieceType::Rook as usize].0 &= !(1 << row * 8 + 0);
                board.bitboards[p.color as usize][PieceType::Rook as usize].0 |= 1 << row * 8 + 3;
                board.occupied &= !(1 << row * 8 + 0);
                board.occupied |= 1 << row * 8 + 3;
            }
        } else if p.piece_type == PieceType::Rook {
            if p.color == Side::White {
                if undo.last_move.from == 0 { board.state &= !WHITE_LONG; }
                else if undo.last_move.from == 7 { board.state &= !WHITE_SHORT; }
            } else {
                if undo.last_move.from == 56 { board.state &= !BLACK_LONG; }
                else if undo.last_move.from == 63 { board.state &= !BLACK_SHORT; }
            }
        }

        if let Some(piece) = undo.captured_piece{
            if piece.piece_type == PieceType::Rook{
                if piece.color == Side::White{
                    if undo.last_move.to == 0 && undo.previous_state & WHITE_LONG != 0{
                        board.state ^= WHITE_LONG
                    }
                    else if undo.last_move.to == 7 && undo.previous_state & WHITE_SHORT != 0{
                        board.state ^= WHITE_SHORT
                    }
                }else{
                    if undo.last_move.to == 56 && undo.previous_state & BLACK_LONG != 0{
                        board.state ^= BLACK_LONG
                    }
                    else if undo.last_move.to == 63 && undo.previous_state & BLACK_SHORT != 0{
                        board.state ^= BLACK_SHORT;
                    }
                }
            }
        }

        board.bitboards[p.color as usize][p.piece_type as usize].0 |= 1 << new;
        board.bitboards[p.color as usize][undo.moving_piece_before.piece_type as usize].0 &= !(1 << old);
        board.occupied &= !(1 << old);
        if undo.captured_piece != None{
            board.bitboards[undo.captured_piece.unwrap().color as usize][undo.captured_piece.unwrap().piece_type as usize].0 &= !(1 << captured_bit);
            board.occupied &= !(1 << captured_bit);
        }
        board.occupied |= 1 << new;
        undo
    }else{
        panic!("CANNOT MOVE EMPTY SQUARE");
    }
    
}

fn undo_move(board: &mut Board, undo: Undo){
    board.state = undo.previous_state;
    board.moves -= 1;
    board.en_passant_target = undo.previous_en_passant_target;
    let current_piece = get_piece(board, undo.last_move.to);

    board.bitboards[undo.moving_piece_before.color as usize][undo.moving_piece_before.piece_type as usize].0 |= 1 << undo.last_move.from;
    board.bitboards[undo.moving_piece_before.color as usize][current_piece.piece_type as usize].0 &= !(1 << undo.last_move.to);
    board.occupied |= 1 << undo.last_move.from;
    board.occupied &= !(1 << undo.last_move.to); 
    
    if undo.captured_piece == None{
        if undo.moving_piece_before.piece_type == PieceType::King {
            //check for castling move
            let delta: i16 = undo.last_move.to as i16 - undo.last_move.from as i16;
            let row = undo.last_move.to / 8;
            if delta == 2 {
                board.bitboards[undo.moving_piece_before.color as usize][PieceType::Rook as usize].0 |= 1 << row * 8 + 7;
                board.bitboards[undo.moving_piece_before.color as usize][PieceType::Rook as usize].0 &= !(1 << row * 8 + 5);
                board.occupied |= 1 << row * 8 + 7;
                board.occupied &= !(1 << row * 8 + 5);
            } else if delta == -2 {
                board.bitboards[undo.moving_piece_before.color as usize][PieceType::Rook as usize].0 |= 1 << row * 8;
                board.bitboards[undo.moving_piece_before.color as usize][PieceType::Rook as usize].0 &= !(1 << row * 8 + 3);
                board.occupied |= 1 << row * 8;
                board.occupied &= !(1 << row * 8 + 3);
            }
        }
    }else{
        let is_en_passant = undo.moving_piece_before.piece_type == PieceType::Pawn
            && undo.previous_en_passant_target == Some(undo.last_move.to)
            && undo.last_move.to % 8 != undo.last_move.from % 8;

        if is_en_passant {
            let delta_col = (undo.last_move.to % 8) as i16 - (undo.last_move.from % 8) as i16;
            board.bitboards[undo.captured_piece.unwrap().color as usize][undo.captured_piece.unwrap().piece_type as usize].0 |= 1 << (undo.last_move.from as i16 + delta_col) as u8;
            board.occupied |= 1 << (undo.last_move.from as i16 + delta_col) as u8;
        } else {
            board.bitboards[undo.captured_piece.unwrap().color as usize][undo.captured_piece.unwrap().piece_type as usize].0 |= 1 << undo.last_move.to;
            board.occupied |= 1 << undo.last_move.to;
        }
    }
}

// gets all moves for a piece DOES NOT INCLUDE CHECKING THAT KING IS LEFT VISIBLE
fn get_pseudo_legal_moves(board: &Board, coord: u8, list: &mut MoveBuf){
    if is_piece(board, coord) {
        let p = get_piece(board, coord);
        match p.piece_type{
            PieceType::Bishop => {
                let mut move_buffer = MoveBuf::new();
                for dir in DIAGONAL_DIRS{
                    ray_march(coord, dir, get_side_bitboard(board, if p.color == Side::White {Side::Black} else {Side::White}), get_side_bitboard(board, p.color), &mut move_buffer);
                }
                for i in 0..move_buffer.len{
                    list.push(move_buffer.data[i]);
                }
                
            },
            PieceType::Knight => {
                let mut possible: Bitboard = KNIGHT_MOVES[coord as usize];
                let friendly = get_side_bitboard(board, p.color);
                while possible.0 != 0{
                    let bit = possible.0.trailing_zeros();
                    if !friendly.get(bit as u8){
                        list.push(bit as u8);
                    }
                    possible.0 ^= 1 << bit;
                }
            },
            PieceType::King => {
                let mut possible: Bitboard = KING_MOVES[coord as usize];
                let friendly = get_side_bitboard(board, p.color);
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
                let possible = PAWN_MOVES[p.color as usize][coord as usize];
                let side_to_move = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
                let ep_bit = if board.en_passant_target.is_some() && p.color == side_to_move {
                    1 << board.en_passant_target.unwrap()
                } else { 0 };

                let mut valid = (enemy | ep_bit) & possible.0;

                while valid != 0{
                    list.push(valid.trailing_zeros() as u8);
                    valid ^= 1 << valid.trailing_zeros();
                }

                // move forward
                let forward:i32 = if p.color == Side::White { 8 } else { -8 };
                let friendly = get_side_bitboard(board, p.color);

                if (enemy | friendly.0) & (1 << (coord as i32 + forward) as u8) == 0{
                    list.push((coord as i32 + forward) as u8);
                    let starting_rank = if p.color == Side::White { 1 } else { 6 };
                    if coord / 8 == starting_rank{
                        if (enemy | friendly.0) & (1 << ((coord as i32 + forward*2)) as u8) == 0{
                            list.push(((coord as i32 + forward*2)) as u8);
                        }
                    }
                    
                }

            },
            PieceType::Queen => {
                let mut move_buffer = MoveBuf::new();
                for dir in STRAIGHT_DIRS{
                    ray_march(coord, dir, get_side_bitboard(board, if p.color == Side::White {Side::Black} else {Side::White}), get_side_bitboard(board, p.color), &mut move_buffer);

                }
                for dir in DIAGONAL_DIRS{
                    ray_march(coord, dir, get_side_bitboard(board, if p.color == Side::White {Side::Black} else {Side::White}), get_side_bitboard(board, p.color), &mut move_buffer);
                }
                for i in 0..move_buffer.len{
                    list.push(move_buffer.data[i]);
                }
            }
            PieceType::Rook => {
                let mut move_buffer = MoveBuf::new();
                for dir in STRAIGHT_DIRS{
                    ray_march(coord, dir, get_side_bitboard(board, if p.color == Side::White {Side::Black} else {Side::White}), get_side_bitboard(board, p.color), &mut move_buffer);
                }
                for i in 0..move_buffer.len{
                    list.push(move_buffer.data[i]);
                }
            }      
        }
    }
}

// returns all valid moves for a piece 
fn get_valid_moves(board: &mut Board, coord: u8, currently_in_check: bool, move_buf: &mut MoveBuf){
    if !is_piece(board, coord) {return}
    let piece = get_piece(board, coord);
    let mut not_checked = MoveBuf::new();
    get_pseudo_legal_moves(&board, coord, &mut not_checked);

    // add castling move if neccesary
    if piece.piece_type == PieceType::King{
        // castling

        let opposite_side = if piece.color == Side::White { Side::Black } else { Side::White };
        let can_long = if piece.color == Side::White {board.state & WHITE_LONG != 0} else {board.state & BLACK_LONG != 0};
        let can_short = if piece.color == Side::White {board.state & WHITE_SHORT != 0} else {board.state & BLACK_SHORT != 0};
        // queenside

        let row = coord / 8;
        if can_long{
            if !is_piece(board, row * 8 + 1) && !is_piece(board, row * 8 + 2) && !is_piece(board, row * 8 + 3) {
                if 
                    !is_square_attacked(&board, row * 8 + 2 , opposite_side) 
                    && !is_square_attacked(&board, row * 8 + 3, opposite_side) {
                    if !currently_in_check {
                        not_checked.push(row * 8 + 2);
                    }
                }
            }
        }

        // kingside
        if can_short {
            if !is_piece(board, row * 8 + 6) && !is_piece(board, row * 8 + 5) {
                if !is_square_attacked(&board, row * 8 + 6, opposite_side) 
                    && !is_square_attacked(&board, row * 8 + 5, opposite_side) {
                    if !currently_in_check {
                        not_checked.push(row * 8 + 6);
                    }
                }
            }
        }
    }

    
    // check validity of each move
    // --- legality filtering ---
    let moving_color = piece.color;

    // Hoisted out of the loop: these don't change as we iterate destinations.
    let piece_is_king = piece.piece_type == PieceType::King;
    let piece_on_king_ray = is_on_ray_from(board.bitboards[moving_color as usize][PieceType::King as usize].0.trailing_zeros() as u8, coord);

    let ep_target = board.en_passant_target;

    let friendly = get_side_bitboard(board, moving_color);
    for i in 0..not_checked.len {
        if friendly.get(not_checked.data[i]) {
            continue;
        }

        // Is this move en passant? Pawn moving diagonally to the EP target,
        let is_en_passant = piece.piece_type == PieceType::Pawn
            && ep_target.is_some()
            && not_checked.data[i] == ep_target.unwrap();

        let needs_full_check = piece_is_king
            || currently_in_check
            || piece_on_king_ray
            || is_en_passant;

        if !needs_full_check {
            move_buf.push(not_checked.data[i]);
            continue;
        }

        // Slow path: actually try the move and see.
        let undo = make_move(board, coord, not_checked.data[i]);
        let in_check = is_in_check(board, moving_color);
        undo_move(board, undo);
        if !in_check {
            move_buf.push(not_checked.data[i]);
        }
    }
}

fn get_valid_moves_standalone(board: &mut Board, square: u8) -> Vec<u8> {
    let piece = if is_piece(board, square) { get_piece(board, square)} else {return Vec::new()};
    let in_check = is_in_check(board, piece.color);
    let mut buf = MoveBuf::new();
    get_valid_moves(board, square, in_check, &mut buf);

    let mut list = Vec::new();
    for i in 0..buf.len{
        list.push(buf.data[i]);
    }
    list
}


// returns only moves that capture pieces (including en passant) for a single piece
fn get_capture_moves(board: &mut Board, square: u8, captures: &mut MoveBuf){
    if !is_piece(board, square) { return }
    let piece = get_piece(board, square);
    let mut pseudo: MoveBuf = MoveBuf::new();
    get_pseudo_legal_moves(&board, square, &mut pseudo);

    let moving_color = piece.color;

    let currently_in_check = is_in_check(board, moving_color);
    let piece_is_king = piece.piece_type == PieceType::King;
    let piece_on_king_ray = is_on_ray_from(board.bitboards[piece.color as usize][PieceType::King as usize].0.trailing_zeros() as u8, square);

    for i in 0..pseudo.len {
        let is_en_passant = piece.piece_type == PieceType::Pawn
            && pseudo.data[i] % 8 != square % 8
            && if board.en_passant_target.is_some() {board.en_passant_target.unwrap() == pseudo.data[i]} else {false};

        let is_capture = is_piece(board, pseudo.data[i]) || is_en_passant;
        if !is_capture { continue; }

        let needs_full_check = piece_is_king
            || currently_in_check
            || piece_on_king_ray
            || is_en_passant;

        if !needs_full_check {
            captures.push(pseudo.data[i]);
            continue;
        }

        let undo = make_move(board, square, pseudo.data[i]);
        let in_check = is_in_check(board, moving_color);
        undo_move(board, undo);
        if !in_check {
            captures.push(pseudo.data[i]);
        }
    }
}

// gets all captures a side can make ((from), (to))
fn get_all_captures(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
    let mut friendly_pieces = get_side_bitboard(board, side);
    let mut buf = MoveBuf::new();
    while friendly_pieces.0 != 0{
        let bit = friendly_pieces.0.trailing_zeros();
        buf.len = 0;
        get_capture_moves(board, bit as u8, &mut buf);
        for i in 0..buf.len{
            buffer.push(bit as u8, buf.data[i]);
        }
        friendly_pieces.0 ^= 1 << bit;
    }
}

// gets all moves that a side can make ((from), (to))
fn get_all_moves(board: &mut Board, side: Side, buffer: &mut MoveListBuf){
    let currently_in_check = is_in_check(board, side);
    let mut friendly_pieces = get_side_bitboard(board, side);
    let mut buf = MoveBuf::new();
    while friendly_pieces.0 != 0{
        let bit = friendly_pieces.0.trailing_zeros();
        buf.len = 0;
        get_valid_moves(board, bit as u8, currently_in_check, &mut buf);
        for i in 0..buf.len{
            buffer.push(bit as u8, buf.data[i]);
        }
        friendly_pieces.0 ^= 1 << bit;
    }
}

// gets num of moves that a side can make
fn get_move_count(board: &mut Board, side: Side) -> u8{
    let currently_in_check = is_in_check(board, side);
    let mut friendly_pieces = get_side_bitboard(board, side);
    let mut buf = MoveBuf::new();
    let mut count = 0;
    while friendly_pieces.0 != 0{
        let bit = friendly_pieces.0.trailing_zeros();
        buf.len = 0;
        get_valid_moves(board, bit as u8, currently_in_check, &mut buf);
        count += buf.len;
        friendly_pieces.0 ^= 1 << bit;
    }
    count as u8
}

fn draw_board(tile_size: f32) {
    for row in 0..8 {
        for col in 0..8 {
            let color = if (row + col) % 2 == 0 {
                Color::from_rgba(255,219,187, 255) // light square
            } else {
                Color::from_rgba(214, 110, 100, 255)  // dark square
            };
            draw_rectangle(col as f32 * tile_size, row as f32 * tile_size, tile_size, tile_size, color);
        }
    }
}

fn draw_moves(tile_size: f32, board: &mut Board, selected_piece: u8, flipped: bool){
    let moves: Vec<u8> = get_valid_moves_standalone(board, selected_piece);
    let mut buf = MoveBuf::new();
    get_capture_moves(board, selected_piece, &mut buf);
    let mut captures = Vec::new();
    for i in 0..buf.len{
        captures.push(buf.data[i]);
    }
    let color = Color::from_rgba(100, 50, 50, 100);
    for mv in moves {
        let col = if flipped {7 - mv % 8} else {mv % 8};
        let row = if flipped {mv / 8} else {7 - (mv / 8)};

        if captures.contains(&mv){
            draw_circle_lines(col as f32 * tile_size + tile_size*0.5, row as f32 * tile_size + tile_size*0.5, tile_size/3.0, tile_size/8.0, color);
        }else{
            draw_circle(col as f32 * tile_size + tile_size*0.5, row as f32 * tile_size + tile_size*0.5, tile_size/4.0, color);
        }
    }
}

fn piece_label(piece: &Piece) -> &str {
    match (&piece.color, &piece.piece_type) {
        (Side::White, PieceType::King)   => "♔",
        (Side::White, PieceType::Queen)  => "♕",
        (Side::White, PieceType::Rook)   => "♖",
        (Side::White, PieceType::Bishop) => "♗",
        (Side::White, PieceType::Knight) => "♘",
        (Side::White, PieceType::Pawn)   => "♙",
        (Side::Black, PieceType::King)   => "♚",
        (Side::Black, PieceType::Queen)  => "♛",
        (Side::Black, PieceType::Rook)   => "♜",
        (Side::Black, PieceType::Bishop) => "♝",
        (Side::Black, PieceType::Knight) => "♞",
        (Side::Black, PieceType::Pawn)   => "♟",
    }
}

fn draw_pieces(board: &Board, font: &Font, tile_size: f32, flipped: bool) {
    for row in 0..8 {
        for col in 0..8 {
            if is_piece(board, row*8 + col){
                let piece = get_piece(board, row * 8 + col);
                let (draw_col, draw_row) = if flipped {
                    (7 - col, row)
                } else {
                    (col, 7 - row)
                };
                let x = draw_col as f32 * tile_size + tile_size * 0.1;
                let y = draw_row as f32 * tile_size + tile_size * 0.8125;
                draw_text_ex(
                    piece_label(&piece),
                    x, y,
                    TextParams {
                        font: Some(font),
                        font_size: tile_size as u16,
                        color: BLACK,
                        ..Default::default()
                    },
                );
            }
        }
    }
}

// --- Main Loop ---

#[macroquad::main("Chess Engine")]
async fn main() {
    let tile_size: f32 = WINDOW_SIZE / 8.0; 
    let font = load_ttf_font("assets/FreeSerif.ttf").await.unwrap();
    let mut board_flipped = false;
    let move_color:Color = Color::from_rgba(228,217,111, 100);
    let check_color: Color = Color::from_rgba(250, 50, 50, 180);

    //load sfx
    let move_check = load_sound("assets/move-check.wav").await.unwrap();
    let move_normal = load_sound("assets/move-self.wav").await.unwrap();
    let move_capture = load_sound("assets/capture.wav").await.unwrap();
    let move_castle = load_sound("assets/castle.wav").await.unwrap();
    let game_finished = load_sound("assets/game-end.wav").await.unwrap();

    let mut white_wins: f32 = 0.0;
    let mut black_wins: f32 = 0.0;
    let mut game_over = false;
    let mut winner: Option<bool> = None;

    let args: Vec<String> = env::args().collect();

    let depth = args.iter()
    .position(|a| a == "--depth")
    .and_then(|i| args.get(i + 1))
    .and_then(|d| d.parse::<usize>().ok())
    .unwrap_or(DEPTH);

    let default_fen = String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    let fen = args.iter()
        .position(|a| a == "--fen")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or(&default_fen);

    let default_white = String::from("human");
    let white = args.iter()
    .position(|a| a == "--white")
    .and_then(|i| args.get(i + 1))
    .map(String::as_str)
    .unwrap_or(&default_white);

    let default_black = String::from("minimax");
    let black = args.iter()
    .position(|a| a == "--black")
    .and_then(|i| args.get(i + 1))
    .map(String::as_str)
    .unwrap_or(&default_black);


    let mut board = from_fen(fen);
    println!("{}",white);
    println!("{}",black);
    // make sure player1 and player2 are correct options
    let player1: Arc<dyn Player + Send + Sync> = match white {
        "human"     => Arc::new(HumanPlayer),
        "random"    => Arc::new(RandomAI),
        "minimax"   => {Arc::new(MinimaxAI::new(depth))},
        _           => panic!("unknown player type: {white}"),
    };

    let player2: Arc<dyn Player + Send + Sync> = match black {
        "human"     => Arc::new(HumanPlayer),
        "random"    => Arc::new(RandomAI),
        "minimax"   => Arc::new(MinimaxAI::new(depth)),
        _           => panic!("unknown player type: {black}"),
    };

    let mut current_player = if board.state & WHITE_TO_MOVE != 0 { &player1} else { &player2};
    
    let mut thinking: Option<Receiver<(u8, u8)>> = None;

    // set screen size
    macroquad::window::request_new_screen_size(WINDOW_SIZE, WINDOW_SIZE);

    let mut selected_piece: Option<Piece> = None;
    let mut selected_coords: Option<u8> = None; 

    let mut move_history: Vec<Undo> = Vec::new();
    let mut last_move: Option<(u8, u8)> = None;


    loop {
        // restart
        if game_over{
            thread::sleep(Duration::from_secs_f32(5.0));
            board = new_board();
            current_player = &player1;
            game_over = false;

            if winner.is_some(){
                if winner.unwrap(){
                    white_wins += 1.0;
                }else{
                    black_wins += 1.0;
                }
            }else{
                white_wins += 0.5;
                black_wins += 0.5;
            }
            println!("white wins: {}\nblack wins: {}", white_wins, black_wins);  
            thinking = None;
            winner = None;
            last_move = None;
            move_history.clear();

            player1.reset();
            player2.reset();
        }


        if macroquad::input::is_key_pressed(KeyCode::F){
            board_flipped = !board_flipped;
        }

        // manually set draw
        if macroquad::input::is_key_pressed(KeyCode::D){
            game_over = true;
        }

        if current_player.as_any().is::<HumanPlayer>(){
            let (x,y) = mouse_position();

            // get any input
            if macroquad::input::is_mouse_button_pressed(MouseButton::Left){
                let col: usize = if board_flipped{7 - (x / tile_size) as usize}else{(x / tile_size) as usize};
                let row: usize = if board_flipped{(y / tile_size) as usize}else{ 7 - (y / tile_size) as usize};

                if selected_piece.is_none(){
                    if is_piece(&board, (row * 8 + col) as u8){
                        selected_piece = Some(get_piece(&board, (row * 8 + col) as u8));
                        selected_coords = Some((row * 8 + col) as u8);
                    }
                }else{
                    if get_valid_moves_standalone(&mut board, selected_coords.unwrap()).contains(&((row * 8  + col) as u8)) && get_piece(&board, selected_coords.unwrap()).color == if board.state & WHITE_TO_MOVE != 0 {Side::White} else {Side::Black}{
                        let move_info: Undo = make_move(&mut board, selected_coords.unwrap(), (row* 8 +  col) as u8);
                        move_history.push(move_info.clone());
                        println!("\nmove {}:", board.moves);
                        println!("eval: {}",minimax_ai::evaluate(&board));
                        last_move = Some(((row* 8 +  col) as u8, selected_coords.unwrap()));

                        let is_white = Arc::ptr_eq(&current_player, &player1); 
                        let opposite_side = if is_white {Side::Black} else {Side::White};

                        if get_move_count(&mut board, opposite_side) == 0{ 
                            if is_in_check(&board, opposite_side){ // checkmate
                                game_over = true;
                                winner = Some(is_white); // true if white, false if black
                                play_sound_once(&move_check); 
                            }else{ // draw
                                game_over = true;
                            }
                            play_sound_once(&game_finished);       
                        }
                        else{
                            if move_info.captured_piece.is_some(){
                                play_sound_once(&move_capture);
                            }else{
                                // check for castle
                                if move_info.moving_piece_before.piece_type == PieceType::King && (move_info.last_move.to as i32 - move_info.last_move.from as i32).abs() == 2{
                                    play_sound_once(&move_castle);
                                }else{
                                    play_sound_once(&move_normal);
                                }
                            }
                            if is_in_check(&board, opposite_side){
                                play_sound_once(&move_check);
                            }
                        }

                        //switch turns
                        if is_white{
                            current_player = &player2;
                        }else{
                            current_player = &player1;
                        }

                        selected_piece = None;
                        selected_coords = None;
                    }else{
                        selected_piece = if is_piece(&board, (row* 8 +  col) as u8) {Some(get_piece(&board, (row* 8 +  col) as u8))} else {None};
                        selected_coords = Some((row * 8 +  col) as u8);
                    }
    
                }
            }
        }else{ // AI
            if thinking.is_none() {
                let player = Arc::clone(&current_player);
                let board_snapshot = board;   
                let side = if Arc::ptr_eq(&current_player, &player1) {Side::White} else {Side::Black};
                let (tx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let mv = player.get_move(&board_snapshot, side);
                    let _ = tx.send(mv);
                });
                thinking = Some(rx);
            }

            if let Some(rx) = &thinking {
                if let Ok(mv) = rx.try_recv() {
                    let move_info = make_move(&mut board, mv.0, mv.1);
                    move_history.push(move_info.clone());
                    println!("move {}:", board.moves);
                    println!("eval: {}\n",minimax_ai::evaluate(&board));
                    last_move = Some(mv);

                    let is_white = Arc::ptr_eq(&current_player, &player1); 
                    let opposite_side = if is_white {Side::Black} else {Side::White};

                    if get_move_count(&mut board, opposite_side) == 0{ 
                        if is_in_check(&board, opposite_side){ // checkmate
                            game_over = true;
                            winner = Some(is_white); // true if white, false if black
                            play_sound_once(&move_check); 
                        }else{ // draw
                            game_over = true;
                        }
                        play_sound_once(&game_finished);       
                    }else{
                        if move_info.captured_piece.is_some(){
                            play_sound_once(&move_capture);
                        }else{
                            // check for castle
                            if move_info.moving_piece_before.piece_type == PieceType::King && (move_info.last_move.to as i32 - move_info.last_move.from as i32).abs() == 2{
                                play_sound_once(&move_castle);
                            }else{
                                play_sound_once(&move_normal);
                            }
                        }
                        if is_in_check(&board, opposite_side){
                            play_sound_once(&move_check);
                        }
                    }
                    
                    //switch turns
                    if is_white{
                        current_player = &player2;
                    }else{
                        current_player = &player1;
                    }

                    thinking = None;
                }
            }
        }
        

        

        // display visuals
        clear_background(WHITE);
        draw_board(tile_size);
        if last_move.is_some(){
            let lm = last_move.unwrap();
            if board_flipped{
                draw_rectangle((7 - (lm.0 % 8)) as f32 * tile_size, ((lm.0 / 8)) as f32 * tile_size, tile_size, tile_size, move_color);
                draw_rectangle((7 - (lm.1 % 8)) as f32 * tile_size, ((lm.1 / 8)) as f32 * tile_size, tile_size, tile_size, move_color);
            }else{
                draw_rectangle((lm.0 % 8) as f32 * tile_size, (7 - (lm.0 / 8) ) as f32 * tile_size, tile_size, tile_size, move_color);
                draw_rectangle((lm.1 % 8) as f32 * tile_size, (7 - (lm.1 / 8) ) as f32 * tile_size, tile_size, tile_size, move_color);
            }
        }
        if selected_coords.is_some() && is_piece(&board, selected_coords.unwrap()){
            if get_piece(&board, selected_coords.unwrap()).color == if board.state & WHITE_TO_MOVE != 0 {Side::White} else {Side::Black}{
                draw_moves(tile_size, &mut board, selected_coords.unwrap(), board_flipped);
            }
        }
        if is_in_check(&board, Side::White){
            let king_bit = board.bitboards[Side::White as usize][PieceType::King as usize].0.trailing_zeros();
            if board_flipped{
                draw_rectangle((7 - king_bit % 8) as f32 * tile_size, ( (king_bit / 8) ) as f32 * tile_size, tile_size, tile_size, check_color);
            }else{
                draw_rectangle( (king_bit % 8) as f32 * tile_size, (7 - (king_bit / 8) ) as f32 * tile_size, tile_size, tile_size, check_color);
            }
        }
        if is_in_check(&board, Side::Black){
            let king_bit = board.bitboards[Side::Black as usize][PieceType::King as usize].0.trailing_zeros();
            if board_flipped{
                draw_rectangle((7 - king_bit % 8) as f32 * tile_size, ( 7 - (king_bit / 8) * 8) as f32 * tile_size, tile_size, tile_size, check_color);
            }else{
                draw_rectangle( (king_bit % 8) as f32 * tile_size, ((king_bit / 8) * 8) as f32 * tile_size, tile_size, tile_size, check_color);
            }
        }
        draw_pieces(&board, &font, tile_size, board_flipped);

        if game_over{
            if winner.is_some(){
                if winner.unwrap(){
                    draw_text_ex(
                        "WHITE WINS",
                        100.0, 100.0,
                        TextParams {
                            font: Some(&font),
                            font_size: (tile_size) as u16,
                            color: BLACK,
                            ..Default::default()
                        },
                    );
                }else{
                    draw_text_ex(
                        "BLACK WINS",
                        100.0, 100.0,
                        TextParams {
                            font: Some(&font),
                            font_size: (tile_size) as u16,
                            color: BLACK,
                            ..Default::default()
                        },
                    );
                }
            }else{
                draw_text_ex(
                    "DRAW",
                    100.0, 100.0,
                    TextParams {
                        font: Some(&font),
                        font_size: (tile_size) as u16,
                        color: BLACK,
                        ..Default::default()
                    },
                );
            }
             
        }
        next_frame().await;

    }
}
