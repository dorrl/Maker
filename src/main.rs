#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod text;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    if let Err(error) = app::run() {
        win::message(0, &error.to_string(), "Maker", win::MB_ICONERROR);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Maker requires Windows.");
}