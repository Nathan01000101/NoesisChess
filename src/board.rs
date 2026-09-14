use std::fmt::Write;
use crate::types::{Piece, PieceType, Side, Board, Bitboard, Undo, Move, WHITE_LONG, WHITE_SHORT, BLACK_LONG, BLACK_SHORT, WHITE_TO_MOVE};

// board logic
pub fn new_board() -> Board {
    from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
}

pub fn get_piece(board: &Board, square: u8) -> Piece {
    board.mailbox[square as usize].expect("tried to fetch empty piece")
}

pub fn get_side_bitboard(board: &Board, side: Side) -> Bitboard{
    let mut bitboard = Bitboard::EMPTY;
    bitboard.0 |= board.bitboards[side as usize][PieceType::King as usize].0 | board.bitboards[side as usize][PieceType::Queen as usize].0 | board.bitboards[side as usize][PieceType::Rook as usize].0 | board.bitboards[side as usize][PieceType::Bishop as usize].0 | board.bitboards[side as usize][PieceType::Knight as usize].0 | board.bitboards[side as usize][PieceType::Pawn as usize].0;
    bitboard
}

pub fn from_fen(fen: &str) -> Board {
    let mut board = Board { bitboards: [[Bitboard::EMPTY; 6]; 2], mailbox: [None; 64], occupied: 0, moves: 0, en_passant_target: None, state: 0b10000 };

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
        board.mailbox[square as usize] = Some(Piece { piece_type: p_type, color: side });
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

pub fn make_move(board: &mut Board, old: u8, new: u8) -> Undo{
    if let Some(mut p) = Some(get_piece(board, old)) {
        let mut undo: Undo = Undo {last_move: Move {to: new, from: old},
                moving_piece_before: p,
                captured_piece: None, 
                previous_en_passant_target: None,
                previous_state: board.state};
        undo.previous_en_passant_target = board.en_passant_target;
        undo.captured_piece = if board.is_piece(new) {Some(get_piece(board, new))} else {None};
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
                if !board.is_piece(new){
                    let captured_piece = get_piece(board, ((old / 8) * 8) + (new - (new/8) * 8));
                    undo.captured_piece = Some(captured_piece);
                    captured_bit = ((old / 8) * 8) + (new - (new/8) * 8);
                    board.bitboards[captured_piece.color as usize][captured_piece.piece_type as usize].0 &= !(1 << ((old / 8) * 8) + (new - (new/8) * 8));
                    board.occupied &= !(1 << ((old / 8) * 8) + (new - (new/8) * 8));
                    board.mailbox[captured_bit as usize] = None;
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
                board.mailbox[(row * 8 + 7) as usize] = None;
                board.mailbox[(row * 8 + 5) as usize] = Some(Piece { piece_type: PieceType::Rook, color: p.color });
            } else if delta == -2 {
                board.bitboards[p.color as usize][PieceType::Rook as usize].0 &= !(1 << row * 8 + 0);
                board.bitboards[p.color as usize][PieceType::Rook as usize].0 |= 1 << row * 8 + 3;
                board.occupied &= !(1 << row * 8 + 0);
                board.occupied |= 1 << row * 8 + 3;
                board.mailbox[(row * 8) as usize] = None;
                board.mailbox[(row * 8 + 3) as usize] = Some(Piece { piece_type: PieceType::Rook, color: p.color });
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

        board.mailbox[old as usize] = None;
        board.mailbox[new as usize] = Some(Piece { piece_type: p.piece_type, color: p.color });

        undo
    }else{
        panic!("CANNOT MOVE EMPTY SQUARE");
    }
    
}

pub fn undo_move(board: &mut Board, undo: Undo){
    board.state = undo.previous_state;
    board.moves -= 1;
    board.en_passant_target = undo.previous_en_passant_target;
    let current_piece = get_piece(board, undo.last_move.to);

    board.bitboards[undo.moving_piece_before.color as usize][undo.moving_piece_before.piece_type as usize].0 |= 1 << undo.last_move.from;
    board.bitboards[undo.moving_piece_before.color as usize][current_piece.piece_type as usize].0 &= !(1 << undo.last_move.to);
    board.occupied |= 1 << undo.last_move.from;
    board.occupied &= !(1 << undo.last_move.to); 

    board.mailbox[undo.last_move.from as usize] = Some(undo.moving_piece_before);
    board.mailbox[undo.last_move.to as usize] = None;
    
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
                board.mailbox[(row * 8 + 7) as usize] = Some(Piece { piece_type: PieceType::Rook, color: undo.moving_piece_before.color });
                board.mailbox[(row * 8 + 5) as usize] = None;
            } else if delta == -2 {
                board.bitboards[undo.moving_piece_before.color as usize][PieceType::Rook as usize].0 |= 1 << row * 8;
                board.bitboards[undo.moving_piece_before.color as usize][PieceType::Rook as usize].0 &= !(1 << row * 8 + 3);
                board.occupied |= 1 << row * 8;
                board.occupied &= !(1 << row * 8 + 3);
                board.mailbox[(row * 8) as usize] = Some(Piece { piece_type: PieceType::Rook, color: undo.moving_piece_before.color });
                board.mailbox[(row * 8 + 3) as usize] = None;
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
            board.mailbox[(undo.last_move.from as i16 + delta_col) as u8 as usize] = Some(undo.captured_piece.unwrap());
        } else {
            board.bitboards[undo.captured_piece.unwrap().color as usize][undo.captured_piece.unwrap().piece_type as usize].0 |= 1 << undo.last_move.to;
            board.occupied |= 1 << undo.last_move.to;
            board.mailbox[undo.last_move.to as usize] = Some(undo.captured_piece.unwrap());
        }
    }
}