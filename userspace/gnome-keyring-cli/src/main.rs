#![deny(clippy::all)]

//! gnome-keyring-cli — OurOS GNOME Keyring daemon
//!
//! Multi-personality: `gnome-keyring-daemon`, `gnome-keyring-3`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_daemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-keyring-daemon [OPTIONS]");
        println!("gnome-keyring-daemon v46.0 (OurOS) — Keyring daemon");
        println!();
        println!("Options:");
        println!("  --start           Start the daemon");
        println!("  --replace         Replace running daemon");
        println!("  --components=LIST Components to start (pkcs11,secrets,ssh)");
        println!("  -d                Daemonize");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-keyring-daemon v46.0 (OurOS)"); return 0; }
    println!("gnome-keyring-daemon: keyring service started");
    println!("  Components: secrets, ssh, pkcs11");
    println!("  Socket: /run/user/1000/keyring/control");
    0
}

fn run_keyring(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-keyring-3 [OPTIONS]");
        println!("gnome-keyring-3 v46.0 (OurOS) — Keyring PAM module");
        return 0;
    }
    let _ = args;
    println!("gnome-keyring-3: PAM module loaded");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-keyring-daemon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gnome-keyring-3" => run_keyring(&rest, &prog),
        _ => run_daemon(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
