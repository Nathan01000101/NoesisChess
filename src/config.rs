use std::sync::Arc;

use crate::ai::Player;
use crate::human::HumanPlayer;
use crate::random_ai::RandomAI;
use crate::minimax_ai::{MinimaxAI};
use crate::*;

pub struct Config {
    pub depth: usize,
    pub fen: String,
    pub white: String,
    pub black: String,
    pub headless: bool,
}

impl Config {
    pub fn from_args() -> Config {
        let args: Vec<String> = std::env::args().collect();

        let default_fen = String::from("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        let default_white = String::from("human");
        let default_black = String::from("minimax");

        Config {
            depth: args.iter()
        .position(|a| a == "--depth")
        .and_then(|i| args.get(i + 1))
        .and_then(|d| d.parse::<usize>().ok())
        .unwrap_or(FALLBACK_DEPTH),

            fen: String::from(args.iter()
        .position(|a| a == "--fen")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or(&default_fen)),

            white: String::from(args.iter()
        .position(|a| a == "--white")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or(&default_white)),

            black: String::from(args.iter()
        .position(|a| a == "--black")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or(&default_black)),

            headless: args.iter().any(|a| a == "--headless"),
        }
    }
}

pub fn make_player(kind: &str, depth: usize) -> Arc<dyn Player + Send + Sync> {
    match kind {
        "human"   => Arc::new(HumanPlayer),
        "random"  => Arc::new(RandomAI),
        "minimax" => Arc::new(MinimaxAI::new(depth)),
        _ => panic!("unknown player type: {kind}"),
    }
}

