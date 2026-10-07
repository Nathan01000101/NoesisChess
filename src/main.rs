mod ai;
mod app;
mod assets;
mod attacks;
mod board;
mod config;
mod engine;
mod eval;
mod human;
mod movegen;
mod random_ai;
mod render;
mod tests;
mod types;

use std::io::{self, IsTerminal};

const WINDOW_SIZE: f32 = 800.0;
pub const MAX_DEPTH: usize = 33;

fn main() {
    if io::stdin().is_terminal() {
        println!("╔══════════════════════════════╗");
        println!("║          Ν Ο Η Σ Ι Σ         ║");
        println!("║          Version 0.4         ║");
        println!("╚══════════════════════════════╝");
    }
    let config = config::Config::from_args();

    if !config.gui {
        app::run_headless(config);
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
