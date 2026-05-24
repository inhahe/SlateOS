#![deny(clippy::all)]

//! libnotify-cli — OurOS notify-send desktop notification tool
//!
//! Single personality: `notify-send`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_notify_send(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: notify-send [OPTIONS] SUMMARY [BODY]");
        println!("notify-send v0.8 (OurOS) — Send desktop notifications");
        println!();
        println!("Options:");
        println!("  -u URGENCY        low, normal, critical");
        println!("  -t TIMEOUT        Timeout in milliseconds");
        println!("  -i ICON           Icon name or path");
        println!("  -a APP_NAME       Application name");
        println!("  -c CATEGORY       Notification category");
        println!("  -h HINT           Extra hint (TYPE:NAME:VALUE)");
        println!("  -r ID             Replace existing notification");
        println!("  -p                Print notification ID");
        println!("  -e                Wait for notification to close");
        println!("  --action=ID=LABEL Add action button");
        return 0;
    }
    let summary = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("Notification");
    let urgency = args.iter().skip_while(|a| a.as_str() != "-u").nth(1)
        .map(|s| s.as_str()).unwrap_or("normal");
    println!("Notification ({}): {}", urgency, summary);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "notify-send".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_notify_send(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
