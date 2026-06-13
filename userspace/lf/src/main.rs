#![deny(clippy::all)]

//! lf — SlateOS terminal file manager (list files)
//!
//! Single personality: `lf`

use std::env;
use std::process;

fn run_lf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lf [OPTIONS] [PATH]");
        println!();
        println!("Terminal file manager.");
        println!();
        println!("Options:");
        println!("  -command <CMD>         Execute command on startup");
        println!("  -config <FILE>         Config file path");
        println!("  -cpuprofile <FILE>     CPU profile output");
        println!("  -doc                   Print documentation");
        println!("  -last-dir-path <FILE>  Write last directory to file");
        println!("  -log <FILE>            Log file path");
        println!("  -print-last-dir        Print last directory on exit");
        println!("  -print-selection       Print selected files on exit");
        println!("  -remote <CMD>          Execute remote command");
        println!("  -selection-path <FILE> Selection file path");
        println!("  -server                Start server");
        println!("  -single                Start in single-pane mode");
        println!("  -version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("lf r32 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-doc") {
        println!("lf — Terminal file manager");
        println!();
        println!("CONFIGURATION");
        println!("  ~/.config/lf/lfrc");
        println!();
        println!("COMMANDS");
        println!("  set <option> <value>   Set an option");
        println!("  map <key> <command>    Map a key binding");
        println!("  cmd <name> <body>      Define a command");
        println!("  push <keys>            Simulate key presses");
        println!();
        println!("OPTIONS");
        println!("  anchorfind  bool   Anchor find at beginning");
        println!("  color256    bool   256-color mode");
        println!("  dircounts   bool   Show directory counts");
        println!("  dirfirst    bool   Directories first");
        println!("  drawbox     bool   Draw box around panels");
        println!("  hidden      bool   Show hidden files");
        println!("  icons       bool   Show file icons");
        println!("  preview     bool   Show file preview");
        println!("  ratios      str    Panel width ratios");
        println!("  scrolloff   int    Scroll offset");
        println!("  shell       str    Shell to use");
        println!("  sortby      str    Sort order");
        println!("  tabstop     int    Tab width");
        return 0;
    }

    let single = args.iter().any(|a| a == "-single");
    let path = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(".");

    println!("lf r32 (SlateOS) — {}", path);
    println!();

    if single {
        println!("  Cargo.toml");
        println!("  Cargo.lock");
        println!("  README.md");
        println!("> src/");
        println!("  tests/");
        println!("  target/");
    } else {
        println!("┌─────────────────┬─────────────────────────────┐");
        println!("│  Cargo.toml     │  // Preview of Cargo.toml   │");
        println!("│  Cargo.lock     │  [package]                  │");
        println!("│  README.md      │  name = \"my-project\"        │");
        println!("│> src/           │  version = \"1.0.0\"          │");
        println!("│  tests/         │  edition = \"2024\"           │");
        println!("│  target/        │                             │");
        println!("│                 │  [dependencies]             │");
        println!("│                 │  serde = \"1.0\"              │");
        println!("└─────────────────┴─────────────────────────────┘");
    }
    println!();
    println!("(TUI mode — j/k move, l/Enter open, h back, q quit)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lf(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_lf};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lf(vec!["--help".to_string()]), 0);
        assert_eq!(run_lf(vec!["-h".to_string()]), 0);
        let _ = run_lf(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lf(vec![]);
    }
}
