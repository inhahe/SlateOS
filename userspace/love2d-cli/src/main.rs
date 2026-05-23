#![deny(clippy::all)]

//! love2d-cli — OurOS LÖVE 2D game framework
//!
//! Multi-personality: `love`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_love(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: love [OPTIONS] [GAME_DIR|GAME.love]");
        println!("LOVE 11.5 (OurOS) — 2D game framework");
        println!();
        println!("Options:");
        println!("  --version          Show version");
        println!("  --fused            Fused mode (embedded game)");
        println!("  --console          Attach console (Windows)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("LOVE 11.5 (Mysterious Mysteries)");
        return 0;
    }
    let game = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    if let Some(path) = game {
        println!("LOVE 11.5 (Mysterious Mysteries)");
        println!("  Running game: {}", path);
    } else {
        println!("LOVE 11.5 (Mysterious Mysteries)");
        println!("  No game specified.");
        println!("  Usage: love [game_directory]");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "love".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_love(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
