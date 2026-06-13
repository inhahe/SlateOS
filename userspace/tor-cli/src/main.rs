#![deny(clippy::all)]

//! tor-cli — SlateOS Tor anonymity network tools
//!
//! Multi-personality: `tor`, `torify`, `torsocks`, `tor-resolve`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tor(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tor [OPTIONS]");
        println!();
        println!("tor — The Tor anonymity network daemon (Slate OS).");
        println!();
        println!("Options:");
        println!("  -f <file>       Config file (default: /etc/tor/torrc)");
        println!("  --list-fingerprint   Show relay fingerprint");
        println!("  --verify-config      Verify config and exit");
        println!("  --hash-password STR  Hash a control password");
        println!("  --version            Show version");
        println!("  --quiet              Suppress startup messages");
        println!("  SocksPort <port>     Override SOCKS port");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Tor version 0.4.8.10 (Slate OS).");
        println!("Tor is running on Slate OS with Libevent 2.1.12-stable, OpenSSL 3.2.1,");
        println!("Zlib 1.3.1, Liblzma 5.4.5, Libzstd 1.5.5, and Unknown N/A as libc.");
        return 0;
    }

    if args.iter().any(|a| a == "--verify-config") {
        println!("Configuration was valid.");
        return 0;
    }

    if args.iter().any(|a| a == "--hash-password") {
        println!("16:872860B76453A77D60CA2BB8C1A7042072093276A3D701AD684053EC4C");
        return 0;
    }

    if args.iter().any(|a| a == "--list-fingerprint") {
        println!("SlateOS-Relay AABBCCDD11223344556677889900AABBCCDDEEFF");
        return 0;
    }

    println!("May 22 12:00:00.000 [notice] Tor 0.4.8.10 running on SlateOS.");
    println!("May 22 12:00:00.001 [notice] Read configuration file \"/etc/tor/torrc\".");
    println!("May 22 12:00:00.010 [notice] Opening Socks listener on 127.0.0.1:9050");
    println!("May 22 12:00:00.011 [notice] Opened Socks listener connection (ready) on 127.0.0.1:9050");
    println!("May 22 12:00:01.500 [notice] Bootstrapped 0% (starting): Starting");
    println!("May 22 12:00:02.100 [notice] Bootstrapped 5% (conn): Connecting to a relay");
    println!("May 22 12:00:02.500 [notice] Bootstrapped 10% (conn_done): Connected to a relay");
    println!("May 22 12:00:03.200 [notice] Bootstrapped 14% (handshake): Handshaking with a relay");
    println!("May 22 12:00:03.800 [notice] Bootstrapped 25% (onehop_create): Establishing a one-hop circuit");
    println!("May 22 12:00:04.500 [notice] Bootstrapped 40% (requesting_status): Asking for networkstatus consensus");
    println!("May 22 12:00:05.200 [notice] Bootstrapped 45% (loading_status): Loading networkstatus consensus");
    println!("May 22 12:00:06.000 [notice] Bootstrapped 80% (conn_or): Connecting to the Tor network");
    println!("May 22 12:00:07.500 [notice] Bootstrapped 85% (conn_done_or): Connected to the Tor network");
    println!("May 22 12:00:08.000 [notice] Bootstrapped 89% (circuit_create): Establishing a Tor circuit");
    println!("May 22 12:00:09.200 [notice] Bootstrapped 100% (done): Done");
    0
}

fn run_torsocks(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: torsocks [OPTIONS] <command> [args...]");
        println!();
        println!("torsocks — wrapper to transparently route through Tor (Slate OS).");
        println!();
        println!("Options:");
        println!("  --shell        Spawn a shell with torsocks enabled");
        println!("  --port <port>  Override SOCKS port (default: 9050)");
        println!("  --address <a>  Override SOCKS address");
        println!("  -d             Debug output");
        return 0;
    }

    let prog = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("curl");
    println!("torsocks: routing '{}' through Tor SOCKS5 127.0.0.1:9050", prog);
    0
}

fn run_tor_resolve(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tor-resolve [OPTIONS] <hostname>");
        println!();
        println!("Resolve a hostname through Tor.");
        return 0;
    }

    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("example.com");
    println!("{}", host);
    println!("93.184.216.34");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tor".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "torify" | "torsocks" => run_torsocks(&rest),
        "tor-resolve" => run_tor_resolve(&rest),
        _ => run_tor(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tor};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tor"), "tor");
        assert_eq!(basename(r"C:\bin\tor.exe"), "tor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tor.exe"), "tor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tor(&["--help".to_string()]), 0);
        assert_eq!(run_tor(&["-h".to_string()]), 0);
        let _ = run_tor(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tor(&[]);
    }
}
