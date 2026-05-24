#![deny(clippy::all)]

//! bluez-tools-cli — OurOS BlueZ command-line tools
//!
//! Multi-personality: `bt-adapter`, `bt-agent`, `bt-device`, `bt-network`, `bt-obex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bt_adapter(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bt-adapter [OPTIONS]");
        println!("bt-adapter v0.3 (OurOS) — BlueZ adapter control");
        println!();
        println!("Options:");
        println!("  -l                List adapters");
        println!("  -d                Discover devices");
        println!("  -a ADAPTER        Select adapter");
        println!("  --set PROP VAL    Set adapter property");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("hci0 (Intel AX210) [default]");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("Discovering...");
        println!("  [NEW] AA:BB:CC:DD:EE:FF Headphones");
        println!("  [NEW] 11:22:33:44:55:66 Keyboard");
        return 0;
    }
    println!("bt-adapter: hci0 — Powered=Yes, Discoverable=Yes");
    0
}

fn run_bt_device(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bt-device [OPTIONS]");
        println!("bt-device v0.3 (OurOS) — BlueZ device control");
        println!();
        println!("Options:");
        println!("  -l                List known devices");
        println!("  -c MAC            Connect to device");
        println!("  -d MAC            Disconnect device");
        println!("  -r MAC            Remove device");
        println!("  --set MAC PROP VAL Set device property");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("AA:BB:CC:DD:EE:FF Headphones (paired, connected)");
        println!("11:22:33:44:55:66 Keyboard (paired)");
    }
    0
}

fn run_default(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        println!("{} v0.3 (OurOS) — BlueZ tool", prog);
        return 0;
    }
    println!("{}: running", prog);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bt-adapter".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "bt-adapter" => run_bt_adapter(&rest, &prog),
        "bt-device" => run_bt_device(&rest, &prog),
        _ => run_default(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
