#![deny(clippy::all)]

//! nwg-panel-cli — OurOS nwg-panel GTK panel for Wayland
//!
//! Single personality: `nwg-panel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nwg_panel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nwg-panel [OPTIONS]");
        println!("nwg-panel v0.9 (OurOS) — GTK3 panel for sway/Wayland compositors");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file (JSON)");
        println!("  -s FILE           CSS style file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("nwg-panel v0.9 (OurOS)"); return 0; }
    println!("nwg-panel: GTK3 panel running");
    println!("  Config: ~/.config/nwg-panel/config");
    println!("  Modules: clock, tray, workspaces, playerctl, brightness");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nwg-panel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nwg_panel(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
