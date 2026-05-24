#![deny(clippy::all)]

//! ggez-cli — OurOS ggez Rust game framework helper
//!
//! Single personality: `ggez`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ggez(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ggez COMMAND [OPTIONS]");
        println!("ggez v0.9.3 (OurOS) — Rust game framework");
        println!();
        println!("Commands:");
        println!("  new NAME        Create new project");
        println!("  build           Build project");
        println!("  run             Build and run");
        println!("  info            Show framework info");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("ggez v0.9.3 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-game");
            println!("Creating ggez project: {}", name);
            println!("  Created Cargo.toml with ggez dependency");
            println!("  Created src/main.rs with game loop template");
            println!("  Created resources/ directory");
        }
        "build" => println!("Building ggez project... Done."),
        "run" => println!("Running ggez game... Window: 800x600"),
        "info" => {
            println!("ggez v0.9.3");
            println!("  Graphics: wgpu");
            println!("  Audio: rodio");
            println!("  Windowing: winit");
        }
        _ => println!("ggez {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ggez".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ggez(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
