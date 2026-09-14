use macroquad::{prelude::*};
use crate::types::{Side, Board, MoveBuf, Piece, PieceType};
use crate::movegen::{get_valid_moves_standalone, get_capture_moves};
use crate::board::get_piece;

pub fn draw_board(tile_size: f32) {
    for row in 0..8 {
        for col in 0..8 {
            let color = if (row + col) % 2 == 0 {
                Color::from_rgba(223, 227, 230, 255) // light square
            } else {
                Color::from_rgba(141,162,173, 255)  // dark square
            };
            draw_rectangle(col as f32 * tile_size, row as f32 * tile_size, tile_size, tile_size, color);
        }
    }
}

pub fn draw_moves(tile_size: f32, board: &mut Board, selected_piece: u8, flipped: bool){
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

pub fn draw_pieces(board: &Board, font: &Font, tile_size: f32, flipped: bool) {
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