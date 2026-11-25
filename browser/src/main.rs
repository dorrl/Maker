#![cfg_attr(
    all(windows, not(test), not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(windows)]
mod net;
#[cfg(windows)]
mod page;
#[cfg(windows)]
mod script;
#[cfg(any(windows, test))]
mod tabs;
#[cfg(windows)]
mod window;

#[cfg(windows)]
fn main() {
    if let Err(error) = window::run() {
        window::show_error(&error);
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Maker Browser requires Windows.");
}
