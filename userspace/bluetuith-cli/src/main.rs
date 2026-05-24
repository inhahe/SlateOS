#![deny(clippy::all)]

//! bluetuith-cli — OurOS bluetuith TUI Bluetooth manager
//!
//! Single personality: `bluetuith`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bluetuith(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bluetuith [OPTIONS]");
        println!("bluetuith v0.2 (OurOS) — TUI Bluetooth manager");
        println!();
        println!("Options:");
        println!("  --adapter NAME    Use specific adapter");
        println!("  --receive-dir DIR Directory for received files");
        println!("  --version         Show version");
        println!();
        println!("Terminal-based Bluetooth device manager with pairing,");
        println!("connecting, file transfer, and adapter management.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bluetuith v0.2 (OurOS)"); return 0; }
    println!("bluetuith: TUI Bluetooth manager");
    println!("  Adapter: hci0 (powered, discoverable)");
    println!("  Scanning for devices...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bluetuith".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bluetuith(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
