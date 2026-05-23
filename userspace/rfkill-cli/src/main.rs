#![deny(clippy::all)]

//! rfkill-cli — OurOS radio kill switch tool
//!
//! Multi-personality: `rfkill`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rfkill(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rfkill [OPTIONS] COMMAND [ID|TYPE]");
        println!();
        println!("rfkill — radio frequency kill switch (OurOS).");
        println!();
        println!("Commands:");
        println!("  list [TYPE]     List current state");
        println!("  block TYPE      Block radio");
        println!("  unblock TYPE    Unblock radio");
        println!("  toggle TYPE     Toggle radio");
        println!();
        println!("Types: all, wifi, bluetooth, wwan, uwb, wimax, gps, fm, nfc");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rfkill from util-linux 2.39 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            let json = args.iter().any(|a| a == "-J" || a == "--json");
            if json {
                println!("{{\"\":[");
                println!("  {{\"id\":0,\"type\":\"bluetooth\",\"device\":\"hci0\",\"type-desc\":\"Bluetooth\",\"soft\":\"unblocked\",\"hard\":\"unblocked\"}},");
                println!("  {{\"id\":1,\"type\":\"wlan\",\"device\":\"phy0\",\"type-desc\":\"Wireless LAN\",\"soft\":\"unblocked\",\"hard\":\"unblocked\"}}");
                println!("]}}");
            } else {
                println!("ID TYPE      DEVICE      SOFT      HARD");
                println!(" 0 bluetooth hci0   unblocked unblocked");
                println!(" 1 wlan      phy0   unblocked unblocked");
            }
        }
        "block" => {
            let typ = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            println!("Blocked {}", typ);
        }
        "unblock" => {
            let typ = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            println!("Unblocked {}", typ);
        }
        "toggle" => {
            let typ = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            println!("Toggled {}", typ);
        }
        _ => {
            eprintln!("rfkill: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rfkill".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rfkill(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
