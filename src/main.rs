mod app;
mod commands;
mod config;
mod engine;
mod error;
mod storage;

fn main() {
    if let Err(error) = commands::start() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
