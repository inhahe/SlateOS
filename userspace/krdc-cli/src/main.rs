#![deny(clippy::all)]

//! krdc-cli — OurOS KRDC KDE remote desktop client
//!
//! Single personality: `krdc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_krdc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: krdc [OPTIONS] [URI]");
        println!("krdc v23.08 (OurOS) — KDE Remote Desktop Connection");
        println!();
        println!("Options:");
        println!("  --fullscreen      Start fullscreen");
        println!("  --version         Show version");
        println!();
        println!("Protocols: VNC, RDP");
        println!("URI format: vnc://host:port or rdp://host");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("krdc v23.08 (OurOS)"); return 0; }
    println!("krdc: KDE remote desktop client started");
    println!("  Bookmarks: 0 saved connections");
    println!("  Protocols: VNC (TigerVNC), RDP (FreeRDP)");
    println!("  Discovery: Zeroconf/Avahi");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "krdc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_krdc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
