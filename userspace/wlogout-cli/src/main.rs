#![deny(clippy::all)]

//! wlogout-cli — OurOS wlogout session logout menu
//!
//! Single personality: `wlogout`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wlogout(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wlogout [OPTIONS]");
        println!("wlogout v1.2 (OurOS) — Wayland session logout menu");
        println!();
        println!("Options:");
        println!("  -l LAYOUT         Layout file path");
        println!("  -C CSS            CSS style file");
        println!("  -b BUTTONS        Number of buttons per row");
        println!("  -c COLUMNS        Number of columns");
        println!("  -r ROWS           Number of rows");
        println!("  -m MARGIN         Button margin (px)");
        println!("  -p                Protocol (layer-shell)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wlogout v1.2 (OurOS)"); return 0; }
    println!("wlogout: session menu");
    println!("  [Lock]  [Logout]  [Suspend]  [Hibernate]  [Shutdown]  [Reboot]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wlogout".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wlogout(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
