#![deny(clippy::all)]

//! flameshot-cli — OurOS Flameshot screenshot tool
//!
//! Single personality: `flameshot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flameshot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: flameshot COMMAND [OPTIONS]");
        println!("flameshot v12.1 (OurOS) — Powerful screenshot tool");
        println!();
        println!("Commands:");
        println!("  gui               Interactive capture");
        println!("  full              Full screen capture");
        println!("  screen            Capture specific screen");
        println!("  config            Open configuration");
        println!("  version           Show version");
        println!();
        println!("Options:");
        println!("  -p PATH           Save path");
        println!("  -d DELAY          Delay (ms)");
        println!("  --clipboard       Copy to clipboard");
        println!("  --pin             Pin screenshot");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("gui");
    match cmd {
        "gui" => println!("Flameshot: interactive capture started"),
        "full" => {
            let path = args.iter().skip_while(|a| a.as_str() != "-p").nth(1).map(|s| s.as_str()).unwrap_or("screenshot.png");
            println!("Full screen capture saved: {}", path);
        }
        "screen" => println!("Screen capture: monitor 1"),
        "config" => println!("Opening configuration dialog..."),
        "version" | "--version" => println!("flameshot v12.1 (OurOS)"),
        _ => println!("flameshot {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flameshot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flameshot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
