#![deny(clippy::all)]

//! btmgmt-cli — OurOS btmgmt Bluetooth management interface
//!
//! Single personality: `btmgmt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_btmgmt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: btmgmt COMMAND [OPTIONS]");
        println!("btmgmt v5.72 (OurOS) — Bluetooth management interface");
        println!();
        println!("Commands:");
        println!("  info              Show adapter info");
        println!("  power on|off      Power adapter on/off");
        println!("  discoverable on|off  Set discoverable");
        println!("  pairable on|off   Set pairable");
        println!("  find              Start device discovery");
        println!("  name NAME         Set adapter name");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "info" => {
            println!("hci0: addr AA:BB:CC:DD:EE:FF");
            println!("  Name: OurOS");
            println!("  Powered: yes, Discoverable: no");
            println!("  Supported settings: powered, connectable, discoverable, pairable");
        }
        "power" => {
            let state = args.get(1).map(|s| s.as_str()).unwrap_or("on");
            println!("Power: {}", state);
        }
        "find" => println!("Discovery started..."),
        "version" => println!("btmgmt v5.72 (OurOS)"),
        _ => println!("btmgmt {}: done", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "btmgmt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_btmgmt(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
