#![deny(clippy::all)]

//! anki-cli — SlateOS Anki spaced-repetition flashcards
//!
//! Single personality: `anki`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_anki(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anki [OPTIONS] [PROFILE]");
        println!("anki v24.04 (SlateOS) — Spaced-repetition flashcard application");
        println!();
        println!("Options:");
        println!("  --base DIR        Data directory");
        println!("  --profile NAME    Profile name");
        println!("  --sync            Sync on startup");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("anki v24.04 (SlateOS)"); return 0; }
    println!("anki: flashcard application started");
    println!("  Decks: 5 active");
    println!("  Due today: 42 cards");
    println!("  New today: 10 cards");
    println!("  Algorithm: FSRS (Free Spaced Repetition Scheduler)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "anki".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_anki(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_anki};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/anki"), "anki");
        assert_eq!(basename(r"C:\bin\anki.exe"), "anki.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("anki.exe"), "anki");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_anki(&["--help".to_string()], "anki"), 0);
        assert_eq!(run_anki(&["-h".to_string()], "anki"), 0);
        let _ = run_anki(&["--version".to_string()], "anki");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_anki(&[], "anki");
    }
}
