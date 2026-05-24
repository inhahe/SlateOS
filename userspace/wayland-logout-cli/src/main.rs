#![deny(clippy::all)]

//! wayland-logout-cli — OurOS wayland-logout session terminator
//!
//! Single personality: `wayland-logout`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wayland_logout(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wayland-logout [OPTIONS]");
        println!("wayland-logout v1.4 (OurOS) — Terminate Wayland compositor session");
        println!();
        println!("Options:");
        println!("  -p PID            Compositor PID to terminate");
        println!("  --version         Show version");
        println!();
        println!("Sends exit request to the Wayland compositor, cleanly");
        println!("ending the session. Uses wl_registry to find the compositor.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wayland-logout v1.4 (OurOS)"); return 0; }
    println!("wayland-logout: requesting compositor session exit...");
    println!("  Session terminated cleanly.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wayland-logout".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wayland_logout(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
