use crate::types::{Bitboard, Side, PieceType, Board, MoveBuf, MoveListBuf, WHITE_TO_MOVE, WHITE_LONG, WHITE_SHORT, BLACK_LONG, BLACK_SHORT};
use crate::board::{get_piece, make_move, undo_move, get_side_bitboard};
use crate::attacks::{KNIGHT_MOVES, PAWN_MOVES, KING_MOVES, ROOK_MASKS, ROOK_SHIFTS, ROOK_MAGIC_TABLE, STRAIGHT_VALID_MOVES, BISHOP_MASKS, BISHOP_SHIFTS, BISHOP_MAGIC_TABLE, DIAGONAL_VALID_MOVES};

// Returns true if `sq` lies on the same rank, file, or diagonal as `target`.
pub fn is_on_ray_from(target: u8, sq: u8) -> bool {
    if target == sq { return false; }
    let dr = (sq / 8 )as i16 - (target / 8) as i16;
    let dc = (sq - (sq / 8) * 8 ) as i16 - (target - (target / 8) * 8 ) as i16;
    // Same rank, same file, or same diagonal (|dr| == |dc|).
    dr == 0 || dc == 0 || dr.abs() == dc.abs()
}

// determines if a side is in check with a given board state
pub fn is_in_check(board: &Board, side: Side) -> bool {
    
    let enemy = if side == Side::White {Side::Black} else {Side::White};
    let king: u8 = board.bitboards[side as usize][PieceType::King as usize].0.trailing_zeros() as u8;
    is_square_attacked(board, king, enemy)
}

// determines if a given square is attacked by a given side
pub fn is_square_attacked(board: &Board, square: u8, side: Side) -> bool{
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
    
    let s_mask = ROOK_MASKS[square as usize];
    let s_shift = ROOK_SHIFTS[square as usize];
    let s_key = (((board.occupied & s_mask).wrapping_mul(ROOK_MAGIC_TABLE[square as usize])) >> s_shift) as usize;
    let s_moves = STRAIGHT_VALID_MOVES[square as usize][s_key].0 & !friendly_bb.0 ;

    if s_moves & board.bitboards[side as usize][PieceType::Rook as usize].0 != 0 || s_moves & board.bitboards[side as usize][PieceType::Queen as usize].0 != 0{
        return true;
    }

    // check diagonal lines for queens or bishops
    let d_mask = BISHOP_MASKS[square as usize];
    let d_shift = BISHOP_SHIFTS[square as usize];
    let d_key = (((board.occupied & d_mask).wrapping_mul(BISHOP_MAGIC_TABLE[square as usize])) >> d_shift) as usize;
    let d_moves = DIAGONAL_VALID_MOVES[square as usize][d_key].0 & !friendly_bb.0 ;

    if d_moves & board.bitboards[side as usize][PieceType::Bishop as usize].0 != 0 || d_moves & board.bitboards[side as usize][PieceType::Queen as usize].0 != 0{
        return true;
    }

    // king ? 
    if KING_MOVES[board.bitboards[side as usize][PieceType::King as usize].0.trailing_zeros() as usize].0 & (1 << square) != 0{
        return true;
    }

    false
}

pub fn get_pseudo_legal_moves(board: &Board, square: u8, list: &mut MoveBuf){
    if board.is_piece(square) {
        let p = get_piece(board, square);
        let friendly = get_side_bitboard(board, p.color);
        match p.piece_type{
            PieceType::Bishop => {
                let mask = BISHOP_MASKS[square as usize];
                let shift = BISHOP_SHIFTS[square as usize];
                let key = (((board.occupied & mask).wrapping_mul(BISHOP_MAGIC_TABLE[square as usize])) >> shift) as usize;
                let mut moves = DIAGONAL_VALID_MOVES[square as usize][key].0 & !friendly.0 ;

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
                let s_mask = ROOK_MASKS[square as usize];
                let s_shift = ROOK_SHIFTS[square as usize];
                let s_key = (((board.occupied & s_mask).wrapping_mul(ROOK_MAGIC_TABLE[square as usize])) >> s_shift) as usize;
                let mut s_moves = STRAIGHT_VALID_MOVES[square as usize][s_key].0 & !friendly.0 ;

                while s_moves != 0{
                    let bit = s_moves.trailing_zeros();
                    list.push(bit as u8);
                    s_moves ^= 1 << bit;
                }

                let d_mask = BISHOP_MASKS[square as usize];
                let d_shift = BISHOP_SHIFTS[square as usize];
                let d_key = (((board.occupied & d_mask).wrapping_mul(BISHOP_MAGIC_TABLE[square as usize])) >> d_shift) as usize;
                let mut d_moves = DIAGONAL_VALID_MOVES[square as usize][d_key].0 & !friendly.0 ;

                while d_moves != 0{
                    let bit = d_moves.trailing_zeros();
                    list.push(bit as u8);
                    d_moves ^= 1 << bit;
                }
            }
            PieceType::Rook => {
                let mask = ROOK_MASKS[square as usize];
                let shift = ROOK_SHIFTS[square as usize];
                let key = (((board.occupied & mask).wrapping_mul(ROOK_MAGIC_TABLE[square as usize])) >> shift) as usize;
                let mut moves = STRAIGHT_VALID_MOVES[square as usize][key].0 & !friendly.0 ;

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
        // castling

        let opposite_side = if piece.color == Side::White { Side::Black } else { Side::White };
        let can_long = if piece.color == Side::White {board.state & WHITE_LONG != 0} else {board.state & BLACK_LONG != 0};
        let can_short = if piece.color == Side::White {board.state & WHITE_SHORT != 0} else {board.state & BLACK_SHORT != 0};
        // queenside

        let row = square / 8;
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
    let piece_on_king_ray = is_on_ray_from(board.bitboards[moving_color as usize][PieceType::King as usize].0.trailing_zeros() as u8, square);

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


// returns only moves that capture pieces (including en passant) for a single piece
pub fn get_capture_moves(board: &mut Board, square: u8, captures: &mut MoveBuf){
    if !board.is_piece(square) { return }
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
pub fn get_all_captures(board: &mut Board, side: Side, buffer: &mut MoveListBuf) {
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
pub fn get_all_moves(board: &mut Board, side: Side, buffer: &mut MoveListBuf){
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
pub fn get_move_count(board: &mut Board, side: Side) -> u8{
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