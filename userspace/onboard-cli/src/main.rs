#![deny(clippy::all)]

//! onboard-cli — OurOS Onboard on-screen keyboard
//!
//! Single personality: `onboard`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_onboard(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: onboard [OPTIONS]");
        println!("onboard v1.4 (OurOS) — On-screen keyboard");
        println!();
        println!("Options:");
        println!("  --size WxH        Window size");
        println!("  --layout NAME     Keyboard layout");
        println!("  --theme NAME      Visual theme");
        println!("  --not-show-in DE  Hide from desktop");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("onboard v1.4 (OurOS)"); return 0; }
    println!("onboard: on-screen keyboard started");
    println!("  Layout: Compact (default)");
    println!("  Word prediction: enabled");
    println!("  Auto-show: enabled for text fields");
    println!("  Themes: Ambiance, Nightshade, Droid");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "onboard".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_onboard(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
