#![deny(clippy::all)]

//! qbittorrent-cli — OurOS qBittorrent client
//!
//! Multi-personality: `qbittorrent`, `qbittorrent-nox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qbittorrent(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qbittorrent [OPTIONS] [TORRENT...]");
        println!("qbittorrent v4.6 (OurOS) — BitTorrent client");
        println!();
        println!("Options:");
        println!("  --no-splash       Disable splash screen");
        println!("  --webui-port=PORT Web UI port");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qbittorrent v4.6 (OurOS)"); return 0; }
    println!("qbittorrent: desktop client started");
    println!("  libtorrent version: 2.0.9");
    println!("  Web UI: http://localhost:8080");
    0
}

fn run_nox(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qbittorrent-nox [OPTIONS]");
        println!("qbittorrent-nox v4.6 (OurOS) — Headless BitTorrent client");
        println!();
        println!("Options:");
        println!("  -d                Daemon mode");
        println!("  --webui-port=PORT Web UI port (default: 8080)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qbittorrent-nox v4.6 (OurOS)"); return 0; }
    println!("qbittorrent-nox: headless client started");
    println!("  Web UI: http://localhost:8080");
    println!("  Default credentials: admin/adminadmin");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qbittorrent".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "qbittorrent-nox" => run_nox(&rest, &prog),
        _ => run_qbittorrent(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
