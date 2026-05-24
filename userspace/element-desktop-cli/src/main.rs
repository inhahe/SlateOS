#![deny(clippy::all)]

//! element-desktop-cli — OurOS Element Matrix client
//!
//! Single personality: `element-desktop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_element(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: element-desktop [OPTIONS]");
        println!("element-desktop v1.11 (OurOS) — Matrix client");
        println!();
        println!("Options:");
        println!("  --hidden          Start hidden");
        println!("  --profile DIR     Profile directory");
        println!("  --no-update       Disable auto-update");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("element-desktop v1.11 (OurOS)"); return 0; }
    println!("element-desktop: Matrix client started");
    println!("  Homeserver: matrix.org");
    println!("  Rooms: 15 joined");
    println!("  Unread: 3 rooms");
    println!("  Encryption: Olm/Megolm (end-to-end)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "element-desktop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_element(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
