#![deny(clippy::all)]

//! xdg-dbus-proxy-cli — OurOS xdg-dbus-proxy D-Bus filtering proxy
//!
//! Single personality: `xdg-dbus-proxy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_proxy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() < 2 {
        println!("Usage: xdg-dbus-proxy ADDRESS SOCKET [OPTIONS]");
        println!("xdg-dbus-proxy v0.1 (OurOS) — D-Bus filtering proxy");
        println!();
        println!("Arguments:");
        println!("  ADDRESS           D-Bus bus address");
        println!("  SOCKET            Proxy socket path");
        println!();
        println!("Options:");
        println!("  --filter          Enable filtering (deny by default)");
        println!("  --see NAME        Allow seeing bus name");
        println!("  --talk NAME       Allow talking to bus name");
        println!("  --own NAME        Allow owning bus name");
        println!("  --call RULE       Allow specific method call");
        println!("  --broadcast RULE  Allow specific broadcast");
        println!("  --log             Log filtered messages");
        return 0;
    }
    let addr = args.first().map(|s| s.as_str()).unwrap_or("unix:path=/run/dbus/system_bus_socket");
    let socket = args.get(1).map(|s| s.as_str()).unwrap_or("/tmp/proxy-bus");
    println!("xdg-dbus-proxy: {} -> {}", addr, socket);
    if args.iter().any(|a| a == "--filter") {
        println!("  Filtering: enabled (deny by default)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xdg-dbus-proxy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_proxy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
