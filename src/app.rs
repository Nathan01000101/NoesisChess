use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use std::{process, thread};

use macroquad::audio::play_sound_once;
use macroquad::prelude::*;

use crate::ai::Player;
use crate::assets::{self, Assets};
use crate::board::{from_fen, get_piece, make_move, new_board};
use crate::config::{self, Config};
use crate::engine::Engine;
use crate::human::HumanPlayer;
use crate::movegen::{find_legal_move, get_move_count, is_in_check};
use crate::render::{draw_board, draw_moves, draw_pieces};
use crate::types::{
    Board, CASTLE_MOVE, EN_PASSANT_MOVE, Move, MoveContext, PROMOTION_MOVE, Piece, PieceType, Side,
    Undo, WHITE_TO_MOVE,
};
use crate::*;

const ENGINE_NAME: &str = "Noesis 0.31";
const ENGINE_AUTHORS: &str = "Nathan E.";

struct MoveOutcome {
    game_over: bool,
    winner: Option<bool>, // true -> white, false -> black, None -> draw
}

fn handle_move_made(
    board: &mut Board,
    mv: &Move,
    move_info: &Undo,
    is_white: bool,
    assets: &Assets,
) -> MoveOutcome {
    let opposite_side = if is_white { Side::Black } else { Side::White };

    if get_move_count(board, opposite_side) == 0 {
        play_sound_once(&assets.game_finished);
        if is_in_check(board, opposite_side) {
            play_sound_once(&assets.move_check);
            return MoveOutcome {
                game_over: true,
                winner: Some(is_white),
            };
        }
        return MoveOutcome {
            game_over: true,
            winner: None,
        };
    }

    let move_type = mv.get_flags() & 0b0011;

    if move_type == types::CASTLE_MOVE {
        play_sound_once(&assets.move_castle);
    } else if move_type == types::NORMAL_MOVE {
        if move_info.captured_piece.is_some() {
            play_sound_once(&assets.move_capture);
        } else {
            play_sound_once(&assets.move_normal);
        }
    } else if move_type == types::EN_PASSANT_MOVE {
        play_sound_once(&assets.move_capture);
    } else if move_type == types::PROMOTION_MOVE {
        play_sound_once(&assets.move_normal);
    }

    if is_in_check(board, opposite_side) {
        play_sound_once(&assets.move_check);
    }

    MoveOutcome {
        game_over: false,
        winner: None,
    }
}

pub async fn run(config: Config) {
    let tile_size: f32 = WINDOW_SIZE / 8.0;
    let assets = assets::load().await;

    // 10 minutes + 0 s
    let wtime = std::time::Duration::from_millis(600_000);
    let btime = std::time::Duration::from_millis(600_000);
    let winc = std::time::Duration::from_millis(0);
    let binc = std::time::Duration::from_millis(0);

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

    let mut current_player = if board.state & WHITE_TO_MOVE != 0 {
        &player1
    } else {
        &player2
    };
    let mut thinking: Option<Receiver<Move>> = None;

    let mut move_history: Vec<Undo> = Vec::new();
    let mut last_move: Option<Move> = None;

    let mut start = std::time::Instant::now();
    loop {
        // restart
        if game_over {
            thread::sleep(Duration::from_secs_f32(5.0));
            board = new_board();
            current_player = &player1;
            game_over = false;

            if let Some(w) = winner {
                if w {
                    white_wins += 1.0;
                } else {
                    black_wins += 1.0;
                }
            } else {
                white_wins += 0.5;
                black_wins += 0.5;
            }
            println!("white wins: {}\nblack wins: {}", white_wins, black_wins);
            if (white_wins + black_wins) as usize >= config.game_limit {
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

        if is_key_pressed(KeyCode::F) {
            board_flipped = !board_flipped;
        }
        if is_key_pressed(KeyCode::D) {
            game_over = true;
        }

        if current_player.as_any().is::<HumanPlayer>() {
            let (x, y) = mouse_position();

            if is_mouse_button_pressed(MouseButton::Left) {
                let col: usize = if board_flipped {
                    7 - (x / tile_size) as usize
                } else {
                    (x / tile_size) as usize
                };
                let row: usize = if board_flipped {
                    (y / tile_size) as usize
                } else {
                    7 - (y / tile_size) as usize
                };
                let square = (row * 8 + col) as u8;

                if selected_piece.is_none() {
                    if board.is_piece(square) {
                        selected_piece = Some(get_piece(&board, square));
                        selected_coords = Some(square);
                    }
                } else {
                    let from = selected_coords.unwrap();

                    if let Some(mv) = find_legal_move(&mut board, from, square, None) {
                        let move_info = make_move(&mut board, mv);
                        move_history.push(move_info.clone());
                        last_move = Some(mv);

                        let is_white = Arc::ptr_eq(current_player, &player1);
                        let outcome =
                            handle_move_made(&mut board, &mv, &move_info, is_white, &assets);
                        game_over = outcome.game_over;
                        winner = outcome.winner;

                        current_player = if is_white { &player2 } else { &player1 };
                        if !game_over {
                            if is_white {
                                if wtime
                                    .as_millis()
                                    .saturating_sub(start.elapsed().as_millis())
                                    == 0
                                {
                                    game_over = true;
                                    winner = Some(false);
                                }
                            } else {
                                if btime
                                    .as_millis()
                                    .saturating_sub(start.elapsed().as_millis())
                                    == 0
                                {
                                    game_over = true;
                                    winner = Some(true);
                                }
                            }
                        }
                        selected_piece = None;
                        selected_coords = None;
                        start = std::time::Instant::now();
                    } else {
                        selected_piece = if board.is_piece(square) {
                            Some(get_piece(&board, square))
                        } else {
                            None
                        };
                        selected_coords = Some(square);
                    }
                }
            }
        } else {
            // AI
            if thinking.is_none() {
                let player = Arc::clone(current_player);
                let context = MoveContext {
                    board: board.clone(),
                    wtime: wtime,
                    btime: btime,
                    winc: winc,
                    binc: binc,
                    moves_to_go: -1,
                };
                let (tx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let mv = player.get_move(context);
                    let _ = tx.send(mv);
                });
                thinking = Some(rx);
            }

            if let Some(rx) = &thinking {
                if let Ok(mv) = rx.try_recv() {
                    let move_info = make_move(&mut board, mv);
                    move_history.push(move_info.clone());
                    last_move = Some(mv);

                    let is_white = Arc::ptr_eq(current_player, &player1);

                    let outcome = handle_move_made(&mut board, &mv, &move_info, is_white, &assets);
                    game_over = outcome.game_over;
                    winner = outcome.winner;

                    if !game_over {
                        if is_white {
                            if wtime
                                .as_millis()
                                .saturating_sub(start.elapsed().as_millis())
                                == 0
                            {
                                game_over = true;
                                winner = Some(false);
                            }
                        } else {
                            if btime
                                .as_millis()
                                .saturating_sub(start.elapsed().as_millis())
                                == 0
                            {
                                game_over = true;
                                winner = Some(true);
                            }
                        }
                    }

                    current_player = if is_white { &player2 } else { &player1 };
                    thinking = None;
                    start = std::time::Instant::now();
                }
            }
        }

        // display visuals
        clear_background(WHITE);
        draw_board(tile_size);
        if let Some(lm) = last_move {
            if board_flipped {
                draw_rectangle(
                    (7 - (lm.get_from() % 8)) as f32 * tile_size,
                    (lm.get_from() / 8) as f32 * tile_size,
                    tile_size,
                    tile_size,
                    move_color,
                );
                draw_rectangle(
                    (7 - (lm.get_to() % 8)) as f32 * tile_size,
                    (lm.get_to() / 8) as f32 * tile_size,
                    tile_size,
                    tile_size,
                    move_color,
                );
            } else {
                draw_rectangle(
                    (lm.get_from() % 8) as f32 * tile_size,
                    (7 - (lm.get_from() / 8)) as f32 * tile_size,
                    tile_size,
                    tile_size,
                    move_color,
                );
                draw_rectangle(
                    (lm.get_to() % 8) as f32 * tile_size,
                    (7 - (lm.get_to() / 8)) as f32 * tile_size,
                    tile_size,
                    tile_size,
                    move_color,
                );
            }
        }
        if let Some(coords) = selected_coords {
            if board.is_piece(coords) {
                let side_to_move = if board.state & WHITE_TO_MOVE != 0 {
                    Side::White
                } else {
                    Side::Black
                };
                if get_piece(&board, coords).color == side_to_move {
                    draw_moves(tile_size, &mut board, coords, board_flipped);
                }
            }
        }

        let stm = if board.state & WHITE_TO_MOVE != 0 {
            Side::White
        } else {
            Side::Black
        };
        if is_in_check(&board, stm) {
            let king_bit = board.bitboards[stm as usize][PieceType::King as usize]
                .0
                .trailing_zeros();
            let x = if board_flipped {
                (7 - king_bit % 8) as f32 * tile_size
            } else {
                (king_bit % 8) as f32 * tile_size
            };
            let y = if board_flipped {
                (king_bit / 8) as f32 * tile_size
            } else {
                (7 - (king_bit / 8)) as f32 * tile_size
            };
            draw_rectangle(x, y, tile_size, tile_size, check_color);
        }
        draw_pieces(&board, &assets.font, tile_size, board_flipped);

        if game_over {
            let label = match winner {
                Some(true) => "WHITE WINS",
                Some(false) => "BLACK WINS",
                None => "DRAW",
            };
            draw_text_ex(
                label,
                100.0,
                100.0,
                TextParams {
                    font: Some(&assets.font),
                    font_size: tile_size as u16,
                    color: BLACK,
                    ..Default::default()
                },
            );
        }

        next_frame().await;
    }
}

// UCI SUPPORT
pub fn run_headless(config: Config) {
    let ai_instance: Engine = Engine::new(config.depth);
    let mut board = new_board();

    loop {
        let mut input = String::new();

        std::io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

        let input = input.trim();
        let parts: Vec<&str> = input.split(' ').collect();

        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "uci" => {
                println!("id name {}", ENGINE_NAME);
                println!("id author {}", ENGINE_AUTHORS);
                println!("uciok")
            }
            "isready" => println!("readyok"),
            "ucinewgame" => ai_instance.reset(),
            "quit" => break,
            "position" => board = handle_uci_position(&parts[1..]),
            "go" => {
                let mv = handle_uci_go(&parts[1..], &board, &ai_instance);
                make_move(&mut board, mv);
            }
            _ => println!("command not found."),
        }
    }
}

pub fn move_to_uci(mv: Move) -> String {
    let to: String = index_move_to_uci(mv.get_to());
    let from: String = index_move_to_uci(mv.get_from());
    let mut result: String = from + &to;

    if mv.get_flags() & 0b11 == PROMOTION_MOVE {
        let promo_char = match mv.get_flags() & 0b1100 {
            types::PROMOTION_QUEEN => 'q',
            types::PROMOTION_ROOK => 'r',
            types::PROMOTION_BISHOP => 'b',
            types::PROMOTION_KNIGHT => 'n',
            _ => unreachable!(),
        };

        result.push(promo_char);
    }

    result
}

pub fn index_move_to_uci(square: u8) -> String {
    let r = square / 8; // can be left as number
    let c = square % 8; // needs to be a-h

    let column_char = match c {
        0 => "a",
        1 => "b",
        2 => "c",
        3 => "d",
        4 => "e",
        5 => "f",
        6 => "g",
        7 => "h",
        _ => panic!("could not parse column"),
    };
    return format!("{}{}", column_char, r + 1);
}

fn parse_uci_move(board: &mut Board, s: &str) -> Option<Move> {
    let b = s.as_bytes();
    if b.len() < 4 {
        return None;
    }
    let sq = |f: u8, r: u8| -> Option<u8> {
        if !(b'a'..=b'h').contains(&f) || !(b'1'..=b'8').contains(&r) {
            return None;
        }
        Some((r - b'1') * 8 + (f - b'a'))
    };
    let from = sq(b[0], b[1])?;
    let to = sq(b[2], b[3])?;
    let promo = match b.get(4) {
        Some(b'q') => Some(PieceType::Queen),
        Some(b'r') => Some(PieceType::Rook),
        Some(b'b') => Some(PieceType::Bishop),
        Some(b'n') => Some(PieceType::Knight),
        _ => None,
    };
    find_legal_move(board, from, to, promo)
}
fn handle_uci_position(tokens: &[&str]) -> Board {
    let mut board;
    let mut idx;

    if tokens[0] == "startpos" {
        board = new_board();
        idx = 1;
    } else if tokens[0] == "fen" {
        let fen = tokens[1..7].join(" "); // FEN is always 6 fields
        board = from_fen(&fen);
        idx = 7;
    } else {
        panic!("malformed position command");
    }

    if idx < tokens.len() && tokens[idx] == "moves" {
        idx += 1;
        for mv_str in &tokens[idx..] {
            let mv = parse_uci_move(&mut board, mv_str);
            make_move(&mut board, mv.unwrap());
        }
    }

    board
}

fn handle_uci_go(tokens: &[&str], board: &Board, ai: &Engine) -> Move {
    let mut wtime = Duration::from_millis(0);
    let mut btime = Duration::from_millis(0);
    let mut winc = Duration::from_millis(0);
    let mut binc = Duration::from_millis(0);
    let mut moves_to_go = 0;
    let mut i = 0;
    while i < tokens.len() {
        let val = tokens.get(i + 1).and_then(|s| s.parse::<u64>().ok());
        match tokens[i] {
            "wtime" => {
                if let Some(v) = val {
                    wtime = Duration::from_millis(v);
                }
                i += 2;
            }
            "btime" => {
                if let Some(v) = val {
                    btime = Duration::from_millis(v);
                }
                i += 2;
            }
            "winc" => {
                if let Some(v) = val {
                    winc = Duration::from_millis(v);
                }
                i += 2;
            }
            "binc" => {
                if let Some(v) = val {
                    binc = Duration::from_millis(v);
                }
                i += 2;
            }
            "movestogo" => {
                if let Some(v) = val {
                    moves_to_go = v
                }
                i += 2;
            }
            _ => {
                i += 1;
            } // movetime/depth/infinite not handled yet — ignored, not errored
        }
    }

    let context = MoveContext {
        board: board.clone(),
        wtime: wtime,
        btime: btime,
        winc: winc,
        binc: binc,
        moves_to_go: moves_to_go as i32,
    };
    let mv = ai.get_move(context);
    println!("bestmove {}", move_to_uci(mv));
    mv
}
