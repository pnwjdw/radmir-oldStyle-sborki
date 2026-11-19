#![windows_subsystem = "windows"]

mod commands;
#[allow(non_snake_case)]
mod windows;

use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let result = commands::run(&root, &args);
    if result.is_ok() {
        let _ = std::fs::write(root.join("launcher-error.txt"), "");
    }
    let quiet = args.iter().any(|s| s == "--quiet");
    let failed = result.is_err();
    let message = result.unwrap_or_else(|error| {
        let _ = std::fs::write(root.join("launcher-error.txt"), &error);
        error
    });
    if !quiet {
        windows::show_message(&message, failed);
    }
    if failed {
        std::process::exit(1);
    }
}
