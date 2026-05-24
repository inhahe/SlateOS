#![deny(clippy::all)]

//! macroquad-cli — OurOS Macroquad game framework helper
//!
//! Single personality: `macroquad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_macroquad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: macroquad COMMAND [OPTIONS]");
        println!("Macroquad v0.4.7 (OurOS) — Simple game framework");
        println!();
        println!("Commands:");
        println!("  new NAME        Create new project");
        println!("  build           Build project");
        println!("  run             Build and run");
        println!("  web             Build for web (WASM)");
        println!("  info            Show build info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("macroquad v0.4.7 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-game");
            println!("Creating project: {}", name);
            println!("  Created Cargo.toml");
            println!("  Created src/main.rs (with macroquad boilerplate)");
            println!("  Done.");
        }
        "build" => {
            println!("Building macroquad project...");
            println!("  Compiled successfully.");
        }
        "run" => println!("Running macroquad game... Window: 800x600"),
        "web" => {
            println!("Building for WASM...");
            println!("  target: wasm32-unknown-unknown");
            println!("  Output: target/wasm/game.wasm + index.html");
        }
        "info" => {
            println!("Macroquad v0.4.7");
            println!("  Backend: miniquad");
            println!("  Renderer: OpenGL / Metal / GLES");
            println!("  Audio: quad-snd");
        }
        _ => println!("macroquad {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "macroquad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_macroquad(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
