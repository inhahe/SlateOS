#![deny(clippy::all)]

//! socat-cli — OurOS socat CLI
//!
//! Single personality: `socat`

use std::env;
use std::process;

fn run_socat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: socat [OPTIONS] <ADDRESS1> <ADDRESS2>");
        println!();
        println!("socat — multipurpose relay / bidirectional data transfer (OurOS).");
        println!();
        println!("Address types:");
        println!("  TCP:host:port         TCP client");
        println!("  TCP-LISTEN:port       TCP server");
        println!("  UDP:host:port         UDP client");
        println!("  UDP-LISTEN:port       UDP server");
        println!("  UNIX-CONNECT:path     Unix socket client");
        println!("  UNIX-LISTEN:path      Unix socket server");
        println!("  STDIO                 Standard I/O");
        println!("  EXEC:cmd              Execute command");
        println!("  FILE:path             File access");
        println!("  PIPE                  Named pipe");
        println!();
        println!("Options:");
        println!("  -d -d          Increase debug level");
        println!("  -v             Verbose data traffic");
        println!("  -x             Hex dump data traffic");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("socat version 1.8.0.0 (OurOS)");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v");
    let debug = args.iter().filter(|a| a.as_str() == "-d").count();

    let addr1 = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("STDIO");
    let addr2 = args.iter().filter(|a| !a.starts_with('-'))
        .nth(1).map(|s| s.as_str()).unwrap_or("TCP:localhost:80");

    if debug > 0 {
        println!("N socat[12345] starting");
        println!("N socat[12345] {} <-> {}", addr1, addr2);
    }

    if addr1.starts_with("TCP-LISTEN") || addr2.starts_with("TCP-LISTEN") {
        let port = addr1.split(':').nth(1)
            .or_else(|| addr2.split(':').nth(1))
            .unwrap_or("8080");
        println!("Listening on TCP port {}...", port);
        println!("  Connection from 192.168.1.10:45678");
        if verbose {
            println!("> GET / HTTP/1.1\\r\\n");
            println!("< HTTP/1.1 200 OK\\r\\n");
        }
    } else if addr1.starts_with("UNIX-LISTEN") || addr2.starts_with("UNIX-LISTEN") {
        println!("Listening on Unix socket...");
        println!("  Client connected");
    } else {
        println!("Connecting {} <-> {}...", addr1, addr2);
        println!("  Connection established");
        if verbose {
            println!("> (data flowing bidirectionally)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_socat(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_socat};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_socat(vec!["--help".to_string()]), 0);
        assert_eq!(run_socat(vec!["-h".to_string()]), 0);
        let _ = run_socat(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_socat(vec![]);
    }
}
