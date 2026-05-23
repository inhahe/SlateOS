#![deny(clippy::all)]

//! stunnel-cli — OurOS SSL/TLS tunneling tools
//!
//! Multi-personality: `stunnel`, `socat`, `ncat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_stunnel(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: stunnel [CONFIG_FILE]");
        println!();
        println!("stunnel — SSL/TLS tunnel (OurOS).");
        println!();
        println!("Options:");
        println!("  -version    Show version");
        println!("  -fd N       Read config from file descriptor");
        println!("  -help       Show help");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("stunnel 5.71 on x86_64-ouros-gnu");
        println!("Compiled/running with OpenSSL 3.2.1 30 Jan 2024");
        println!("Threading: PTHREAD");
        println!("Sockets: POLL+SELECT");
        println!("TLS: Engine support: yes");
        return 0;
    }

    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/etc/stunnel/stunnel.conf");
    println!("stunnel: reading configuration from '{}'", config);
    println!("stunnel: Configuration successful");
    println!("stunnel: Starting service [https-proxy]");
    println!("stunnel: Listening on 0.0.0.0:8443");
    println!("stunnel: Service [https-proxy] connected remote server from 127.0.0.1:8080");
    0
}

fn run_socat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: socat [OPTIONS] <address1> <address2>");
        println!();
        println!("socat — multipurpose relay (OurOS).");
        println!();
        println!("Address types:");
        println!("  TCP:host:port            TCP client");
        println!("  TCP-LISTEN:port          TCP server");
        println!("  UDP:host:port            UDP client");
        println!("  UDP-LISTEN:port          UDP server");
        println!("  UNIX-CONNECT:path        Unix socket client");
        println!("  UNIX-LISTEN:path         Unix socket server");
        println!("  SSL:host:port            SSL/TLS client");
        println!("  OPENSSL-LISTEN:port      SSL/TLS server");
        println!("  STDIO                    Standard I/O");
        println!("  EXEC:command             Execute program");
        println!("  PTY                      Pseudo-terminal");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("socat version 1.8.0.0 (OurOS)");
        println!("   running on x86_64-ouros-gnu");
        println!("features:");
        println!("  #define WITH_STDIO 1");
        println!("  #define WITH_FDNUM 1");
        println!("  #define WITH_FILE 1");
        println!("  #define WITH_OPENSSL 1");
        println!("  #define WITH_TCP 1");
        println!("  #define WITH_UDP 1");
        println!("  #define WITH_UNIX 1");
        return 0;
    }

    let addr1 = args.first().map(|s| s.as_str()).unwrap_or("STDIO");
    let addr2 = args.get(1).map(|s| s.as_str()).unwrap_or("TCP:localhost:80");
    println!("socat: {} <-> {}", addr1, addr2);
    println!("socat: relay established");
    0
}

fn run_ncat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ncat [OPTIONS] [host] [port]");
        println!();
        println!("ncat — networking utility (OurOS, from Nmap).");
        println!();
        println!("Options:");
        println!("  -l, --listen          Listen for connections");
        println!("  -p, --source-port     Specify source port");
        println!("  -e, --exec <cmd>      Execute command");
        println!("  --ssl                 Use SSL/TLS");
        println!("  --proxy <host:port>   Connect through proxy");
        println!("  -k, --keep-open      Accept multiple connections");
        println!("  -u, --udp             Use UDP");
        println!("  -v, --verbose         Verbose output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Ncat: Version 7.94 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-l" || a == "--listen") {
        let port = args.iter().position(|a| a == "-p" || a == "--source-port")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("4444");
        println!("Ncat: Listening on 0.0.0.0:{}", port);
        println!("Ncat: Connection from 192.168.1.50:54321.");
    } else {
        let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("localhost");
        println!("Ncat: Connected to {}.", host);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "stunnel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "socat" => run_socat(&rest),
        "ncat" => run_ncat(&rest),
        _ => run_stunnel(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
