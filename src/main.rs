mod ai;
mod app;
mod assets;
mod attacks;
mod board;
mod config;
mod human;
mod minimax_ai;
mod movegen;
mod render;
mod random_ai;
mod tests;
mod types;


const WINDOW_SIZE: f32 = 600.0;
pub const MAX_DEPTH: usize = 12;

fn main() {
println!(r#" Thank you for using
     __                _     
  /\ \ \___   ___  ___(_)___ 
 /  \/ / _ \ / _ \/ __| / __|
/ /\  / (_) |  __/\__ \ \__ \
\_\ \/ \___/ \___||___/_|___/
"#);
println!("\n Engine built by: Nathan E.");
println!("  Version 0.29-dev 2026-09-14");
    let config = config::Config::from_args();

    if !config.gui {
        app::run_headless(config);
    } else {
        macroquad::Window::from_config(window_conf(), app::run(config));
    }

}

fn window_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "Chess Engine".to_owned(),
        window_width: WINDOW_SIZE as i32,
        window_height: WINDOW_SIZE as i32,
        ..Default::default()
    }
}
