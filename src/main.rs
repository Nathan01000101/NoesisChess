mod ai;
mod app;
mod assets;
mod attacks;
mod board;
mod config;
mod datagen;
mod engine;
mod eval;
mod human;
mod movegen;
mod nnue;
mod render;
mod search;
mod tt;
mod types;
mod zobrist;

use std::io::{self, IsTerminal};

const WINDOW_SIZE: f32 = 800.0;
pub const MAX_DEPTH: usize = 40;

fn main() {
    if io::stdin().is_terminal() {
        println!("╔══════════════════════════════╗");
        println!("║          Ν Ο Η Σ Ι Σ         ║");
        println!("║          Version 0.5         ║");
        println!("╚══════════════════════════════╝");
    }
    let config = config::Config::from_args();

    if !config.gui {
        if !config.data_gen {
            app::run_headless(config);
        } else {
            datagen::start_data_gen();
        }
    } else {
        macroquad::Window::from_config(window_conf(), app::run(config));
    }
}

fn window_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "Noesis GUI".to_owned(),
        window_width: WINDOW_SIZE as i32,
        window_height: WINDOW_SIZE as i32,
        ..Default::default()
    }
}
