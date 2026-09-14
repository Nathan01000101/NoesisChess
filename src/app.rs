use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::{process, thread};
use std::time::Duration;

use macroquad::prelude::*;
use macroquad::audio::play_sound_once;

use crate::*; 
use crate::types::{WHITE_TO_MOVE, Board, Undo, PieceType, Piece, Side};
use crate::board::{get_piece, from_fen, new_board, make_move};
use crate::movegen::{get_move_count, is_in_check, get_valid_moves_standalone};
use crate::render::{draw_board, draw_moves, draw_pieces};
use crate::config::{self, Config};
use crate::assets::{self, Assets};
use crate::human::HumanPlayer;
use crate::ai::Player;
use crate::minimax_ai;

struct MoveOutcome {
    game_over: bool,
    winner: Option<bool>, // true -> white, false -> black, None -> draw
}

fn handle_move_made(board: &mut Board, move_info: &Undo, is_white: bool, assets: &Assets) -> MoveOutcome {
    let opposite_side = if is_white { Side::Black } else { Side::White };

    if get_move_count(board, opposite_side) == 0 {
        play_sound_once(&assets.game_finished);
        if is_in_check(board, opposite_side) {
            play_sound_once(&assets.move_check);
            return MoveOutcome { game_over: true, winner: Some(is_white) };
        }
        return MoveOutcome { game_over: true, winner: None };
    }

    if move_info.captured_piece.is_some() {
        play_sound_once(&assets.move_capture);
    } else if move_info.moving_piece_before.piece_type == PieceType::King
        && (move_info.last_move.to as i32 - move_info.last_move.from as i32).abs() == 2
    {
        play_sound_once(&assets.move_castle);
    } else {
        play_sound_once(&assets.move_normal);
    }

    if is_in_check(board, opposite_side) {
        play_sound_once(&assets.move_check);
    }

    MoveOutcome { game_over: false, winner: None }
}

pub async fn run(config: Config) {
    let tile_size: f32 = WINDOW_SIZE / 8.0;
    let assets = assets::load().await;

    let player1: Arc<dyn Player + Send + Sync> = config::make_player(&config.white, config.depth);
    let player2: Arc<dyn Player + Send + Sync> = config::make_player(&config.black, config.depth);

    let move_color: Color = Color::from_rgba(228, 217, 111, 100);
    let check_color: Color = Color::from_rgba(250, 50, 50, 180);

    let mut board = from_fen(&config.fen);
    let mut board_flipped = false;

    let mut selected_piece: Option<Piece> = None;
    let mut selected_coords: Option<u8> = None;

    let mut white_wins: f32 = 0.0;
    let mut black_wins: f32 = 0.0;
    let mut game_over = false;
    let mut winner: Option<bool> = None;

    let mut current_player = if board.state & WHITE_TO_MOVE != 0 { &player1 } else { &player2 };
    let mut thinking: Option<Receiver<(u8, u8)>> = None;

    let mut move_history: Vec<Undo> = Vec::new();
    let mut last_move: Option<(u8, u8)> = None;

    loop {
        
        // restart
        if game_over {
            thread::sleep(Duration::from_secs_f32(5.0));
            board = new_board();
            current_player = &player1;
            game_over = false;

            if let Some(w) = winner {
                if w { white_wins += 1.0; } else { black_wins += 1.0; }
            } else {
                white_wins += 0.5;
                black_wins += 0.5;
            }
            println!("white wins: {}\nblack wins: {}", white_wins, black_wins);
            if (white_wins + black_wins) as usize >= config.game_limit{
                println!("Game Limit Reached");
                process::exit(0);
            }
            thinking = None;
            winner = None;
            last_move = None;
            move_history.clear();

            player1.reset();
            player2.reset();
        }

        if is_key_pressed(KeyCode::F) { board_flipped = !board_flipped; }
        if is_key_pressed(KeyCode::D) { game_over = true; }

        if current_player.as_any().is::<HumanPlayer>() {
            let (x, y) = mouse_position();

            if is_mouse_button_pressed(MouseButton::Left) {
                let col: usize = if board_flipped { 7 - (x / tile_size) as usize } else { (x / tile_size) as usize };
                let row: usize = if board_flipped { (y / tile_size) as usize } else { 7 - (y / tile_size) as usize };
                let square = (row * 8 + col) as u8;

                if selected_piece.is_none() {
                    if board.is_piece(square) {
                        selected_piece = Some(get_piece(&board, square));
                        selected_coords = Some(square);
                    }
                } else {
                    let from = selected_coords.unwrap();
                    let side_to_move = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };

                    if get_valid_moves_standalone(&mut board, from).contains(&square)
                        && get_piece(&board, from).color == side_to_move
                    {
                        let move_info = make_move(&mut board, from, square);
                        move_history.push(move_info.clone());
                        println!("\nmove {}:", board.moves);
                        println!("eval: {}", minimax_ai::evaluate(&board));
                        last_move = Some((square, from));

                        let is_white = Arc::ptr_eq(current_player, &player1);
                        let outcome = handle_move_made(&mut board, &move_info, is_white, &assets);
                        game_over = outcome.game_over;
                        winner = outcome.winner;

                        current_player = if is_white { &player2 } else { &player1 };
                        selected_piece = None;
                        selected_coords = None;
                    } else {
                        selected_piece = if board.is_piece(square) { Some(get_piece(&board, square)) } else { None };
                        selected_coords = Some(square);
                    }
                }
            }
        } else {
            // AI
            if thinking.is_none() {
                let player = Arc::clone(current_player);
                let board_snapshot = board;
                let side = if Arc::ptr_eq(current_player, &player1) { Side::White } else { Side::Black };
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
                    println!("eval: {}\n", minimax_ai::evaluate(&board));
                    last_move = Some(mv);

                    let is_white = Arc::ptr_eq(current_player, &player1);
                    let outcome = handle_move_made(&mut board, &move_info, is_white, &assets);
                    game_over = outcome.game_over;
                    winner = outcome.winner;

                    current_player = if is_white { &player2 } else { &player1 };
                    thinking = None;
                }
            }
        }

        // display visuals
        clear_background(WHITE);
        draw_board(tile_size);
        if let Some(lm) = last_move {
            if board_flipped {
                draw_rectangle((7 - (lm.0 % 8)) as f32 * tile_size, (lm.0 / 8) as f32 * tile_size, tile_size, tile_size, move_color);
                draw_rectangle((7 - (lm.1 % 8)) as f32 * tile_size, (lm.1 / 8) as f32 * tile_size, tile_size, tile_size, move_color);
            } else {
                draw_rectangle((lm.0 % 8) as f32 * tile_size, (7 - (lm.0 / 8)) as f32 * tile_size, tile_size, tile_size, move_color);
                draw_rectangle((lm.1 % 8) as f32 * tile_size, (7 - (lm.1 / 8)) as f32 * tile_size, tile_size, tile_size, move_color);
            }
        }
        if let Some(coords) = selected_coords {
            if board.is_piece(coords) {
                let side_to_move = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
                if get_piece(&board, coords).color == side_to_move {
                    draw_moves(tile_size, &mut board, coords, board_flipped);
                }
            }
        }
        if is_in_check(&board, Side::White) {
            let king_bit = board.bitboards[Side::White as usize][PieceType::King as usize].0.trailing_zeros();
            if board_flipped {
                draw_rectangle((7 - king_bit % 8) as f32 * tile_size, (king_bit / 8) as f32 * tile_size, tile_size, tile_size, check_color);
            } else {
                draw_rectangle((king_bit % 8) as f32 * tile_size, (7 - (king_bit / 8)) as f32 * tile_size, tile_size, tile_size, check_color);
            }
        }
        if is_in_check(&board, Side::Black) {
            let king_bit = board.bitboards[Side::Black as usize][PieceType::King as usize].0.trailing_zeros();
            if board_flipped {
                draw_rectangle((7 - king_bit % 8) as f32 * tile_size, (7 - (king_bit / 8) * 8) as f32 * tile_size, tile_size, tile_size, check_color);
            } else {
                draw_rectangle((king_bit % 8) as f32 * tile_size, ((king_bit / 8) * 8) as f32 * tile_size, tile_size, tile_size, check_color);
            }
        }
        draw_pieces(&board, &assets.font, tile_size, board_flipped);

        if game_over {
            let label = match winner {
                Some(true) => "WHITE WINS",
                Some(false) => "BLACK WINS",
                None => "DRAW",
            };
            draw_text_ex(
                label, 100.0, 100.0,
                TextParams { font: Some(&assets.font), font_size: tile_size as u16, color: BLACK, ..Default::default() },
            );
        }

        next_frame().await;
    }
}

pub fn run_headless(config: Config) {
    let mut board = from_fen(&config.fen);
    let player1 = config::make_player(&config.white, config.depth);
    let player2 = config::make_player(&config.black, config.depth);

    loop {
        let side = if board.state & WHITE_TO_MOVE != 0 { Side::White } else { Side::Black };
        let player = if side == Side::White { &player1 } else { &player2 };

        let mv = player.get_move(&board, side);
        make_move(&mut board, mv.0, mv.1);

        let opponent = if side == Side::White { Side::Black } else { Side::White };
        if get_move_count(&mut board, opponent) == 0 {
            if is_in_check(&board, opponent) {
                println!("{:?} wins", side);
            } else {
                println!("draw");
            }
            break;
        }
    }
}