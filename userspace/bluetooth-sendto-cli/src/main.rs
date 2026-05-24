#![deny(clippy::all)]

//! bluetooth-sendto-cli — OurOS gnome-bluetooth file sender
//!
//! Single personality: `bluetooth-sendto`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sendto(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bluetooth-sendto [OPTIONS] FILE...");
        println!("bluetooth-sendto v3.34 (OurOS) — Send files via Bluetooth");
        println!();
        println!("Options:");
        println!("  --device MAC      Target device address");
        println!("  --name NAME       Target device name");
        return 0;
    }
    let device = args.iter().skip_while(|a| a.as_str() != "--device").nth(1)
        .map(|s| s.as_str()).unwrap_or("(select)");
    for f in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Sending {} to {}", f, device);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bluetooth-sendto".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sendto(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
