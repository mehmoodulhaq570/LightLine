#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_app::run() {
        eprintln!("LightLine: {error}");
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LightLine currently supports Windows only.");
}
