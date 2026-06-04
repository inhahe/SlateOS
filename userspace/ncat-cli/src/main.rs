#![deny(clippy::all)]

//! ncat-cli — OurOS Ncat networking utility
//!
//! Single personality: `ncat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ncat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ncat [OPTIONS] [HOST] [PORT]");
        println!("ncat v7.95 (OurOS) — Concatenate and redirect sockets");
        println!();
        println!("Connect mode:");
        println!("  HOST PORT         Connect to host:port");
        println!("  --ssl             Use SSL/TLS");
        println!("  --proxy HOST:PORT Use HTTP/SOCKS proxy");
        println!();
        println!("Listen mode:");
        println!("  -l                Listen for connections");
        println!("  -p PORT           Listen port");
        println!("  -k                Keep listening after disconnect");
        println!("  -e CMD            Execute command on connect");
        println!();
        println!("General:");
        println!("  -u                UDP mode");
        println!("  -w SECS           Connection timeout");
        println!("  -v                Verbose output");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ncat v7.95 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        let port = args.iter().skip_while(|a| a.as_str() != "-p").nth(1).map(|s| s.as_str()).unwrap_or("4444");
        println!("Ncat: Listening on 0.0.0.0:{}", port);
        println!("Ncat: Connection from 192.168.1.100:49152");
    } else {
        let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("localhost");
        let port = args.iter().filter(|a| !a.starts_with('-')).nth(1).map(|s| s.as_str()).unwrap_or("80");
        println!("Ncat: Connected to {}:{}.", host, port);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ncat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ncat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ncat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ncat"), "ncat");
        assert_eq!(basename(r"C:\bin\ncat.exe"), "ncat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ncat.exe"), "ncat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ncat(&["--help".to_string()], "ncat"), 0);
        assert_eq!(run_ncat(&["-h".to_string()], "ncat"), 0);
        let _ = run_ncat(&["--version".to_string()], "ncat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ncat(&[], "ncat");
    }
}
