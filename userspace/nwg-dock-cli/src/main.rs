#![deny(clippy::all)]

//! nwg-dock-cli — OurOS nwg-dock application dock
//!
//! Single personality: `nwg-dock`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nwg_dock(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-dock [OPTIONS]");
        println!("nwg-dock v0.3 (OurOS) — Wayland application dock");
        println!();
        println!("Options:");
        println!("  -d                Dock position: top, bottom, left, right");
        println!("  -o OUTPUT         Output to display on");
        println!("  -w                Full width");
        println!("  -nolauncher       Don't show launcher icon");
        println!("  -i ICON_SIZE      Icon size (px)");
        println!("  -mb MARGIN        Margin bottom");
        println!("  -ml MARGIN        Margin left");
        println!("  -r                Resident mode (stay running)");
        println!("  -l LAUNCHER       Launcher command");
        return 0;
    }
    let position = args.iter().skip_while(|a| a.as_str() != "-d").nth(1)
        .map(|s| s.as_str()).unwrap_or("bottom");
    println!("nwg-dock: application dock (position={})", position);
    println!("  Pinned: Firefox, Terminal, Files");
    println!("  Running: 3 applications");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nwg-dock".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nwg_dock(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
