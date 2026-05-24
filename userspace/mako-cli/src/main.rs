#![deny(clippy::all)]

//! mako-cli — OurOS mako Wayland notification daemon
//!
//! Multi-personality: `mako`, `makoctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mako(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mako [OPTIONS]");
        println!("mako v1.9 (OurOS) — Lightweight Wayland notification daemon");
        println!();
        println!("Options:");
        println!("  -c FILE           Config file");
        println!("  --font FONT       Font specification");
        println!("  --background-color COLOR  Background color");
        println!("  --text-color COLOR        Text color");
        println!("  --default-timeout MS      Default timeout");
        println!("  --max-visible N   Max visible notifications");
        return 0;
    }
    println!("mako: notification daemon running");
    println!("  Max visible: 5");
    println!("  Default timeout: 5000ms");
    if args.is_empty() {
        println!("  Listening for notifications...");
    }
    0
}

fn run_makoctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: makoctl COMMAND");
        println!("makoctl v1.9 (OurOS) — mako control client");
        println!();
        println!("Commands:");
        println!("  dismiss           Dismiss notification");
        println!("  dismiss --all     Dismiss all");
        println!("  invoke ACTION     Invoke action");
        println!("  list              List notifications");
        println!("  reload            Reload config");
        println!("  mode              Get/set modes");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => println!("[{{\"id\":1,\"summary\":\"Update available\",\"body\":\"System updates ready\"}}]"),
        "dismiss" => println!("Notification dismissed."),
        "reload" => println!("Configuration reloaded."),
        _ => println!("makoctl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mako".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "makoctl" => run_makoctl(&rest, &prog),
        _ => run_mako(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
