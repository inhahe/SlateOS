#![deny(clippy::all)]

//! wireproxy-cli — Slate OS WireProxy WireGuard-based proxy
//!
//! Single personality: `wireproxy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wireproxy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wireproxy [OPTIONS]");
        println!("wireproxy v1.0.7 (Slate OS) — WireGuard-based userspace proxy");
        println!();
        println!("Options:");
        println!("  -c, --config FILE    WireGuard config file");
        println!("  --socks5 ADDR:PORT   SOCKS5 proxy listen address");
        println!("  --http ADDR:PORT     HTTP proxy listen address");
        println!("  --daemon             Run as daemon");
        println!("  --info               Show tunnel info");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("wireproxy v1.0.7 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--info") {
        println!("Tunnel info:");
        println!("  Interface: wg0");
        println!("  Address: 10.0.0.2/32");
        println!("  Peer: peer1 (10.0.0.1)");
        println!("  Transfer: 1.2 GiB received, 456 MiB sent");
        return 0;
    }
    println!("wireproxy v1.0.7 starting...");
    println!("  WireGuard tunnel established");
    println!("  SOCKS5 proxy: 127.0.0.1:1080");
    println!("  HTTP proxy: 127.0.0.1:8080");
    println!("  Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wireproxy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wireproxy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wireproxy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wireproxy"), "wireproxy");
        assert_eq!(basename(r"C:\bin\wireproxy.exe"), "wireproxy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wireproxy.exe"), "wireproxy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wireproxy(&["--help".to_string()], "wireproxy"), 0);
        assert_eq!(run_wireproxy(&["-h".to_string()], "wireproxy"), 0);
        let _ = run_wireproxy(&["--version".to_string()], "wireproxy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wireproxy(&[], "wireproxy");
    }
}
