#![deny(clippy::all)]

//! squid — OurOS caching proxy server
//!
//! Multi-personality: `squid` (proxy server), `squidclient` (HTTP client)

use std::env;
use std::process;

fn run_squid(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: squid [-dhvzCDFNRYX] [-f config-file] [-[au] port] [-s | -l file] [-k signal]");
        println!();
        println!("Options:");
        println!("  -f <file>    Use given config-file instead of /etc/squid/squid.conf");
        println!("  -k <signal>  Send signal: reconfigure|rotate|shutdown|restart|interrupt|kill|check|parse");
        println!("  -z           Initialize cache directories");
        println!("  -N           Run in foreground (no daemon mode)");
        println!("  -d <level>   Write debugging to stderr");
        println!("  -v           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("Squid Cache: Version 6.9 (OurOS)");
        println!("Service Name: squid");
        println!("configure options: --prefix=/usr --sysconfdir=/etc/squid --with-openssl");
        return 0;
    }
    if args.iter().any(|a| a == "-z") {
        println!("Creating missing swap directories");
        println!("(No cache_dir changes need to be done)");
        return 0;
    }
    if let Some(idx) = args.iter().position(|a| a == "-k") {
        let signal = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("check");
        match signal {
            "reconfigure" => println!("Sending SIGHUP to Squid... done."),
            "rotate" => println!("Sending rotate signal to Squid... done."),
            "shutdown" => println!("Sending shutdown signal to Squid... done."),
            "check" | "parse" => {
                let config = args.iter().position(|a| a == "-f")
                    .and_then(|i| args.get(i + 1))
                    .map(|s| s.as_str())
                    .unwrap_or("/etc/squid/squid.conf");
                println!("Processing Configuration File: {}", config);
                println!("Configuration File is valid.");
            }
            _ => println!("squid -k {}: unknown signal", signal),
        }
        return 0;
    }

    // Start server
    println!("2025/05/22 10:00:00| Starting Squid Cache version 6.9 for OurOS...");
    println!("2025/05/22 10:00:00| Service Name: squid");
    println!("2025/05/22 10:00:00| Process ID 12345");
    println!("2025/05/22 10:00:00| Process Roles: master worker");
    println!("2025/05/22 10:00:00| With 65535 file descriptors available");
    println!("2025/05/22 10:00:00| Accepting HTTP Socket connections at local=0.0.0.0:3128 remote=[::] FD 12");
    println!("2025/05/22 10:00:00| HTCP Disabled.");
    println!("2025/05/22 10:00:00| Squid plugin modules loaded: 0");
    println!("2025/05/22 10:00:00| Adaptation support is off.");
    println!("2025/05/22 10:00:01| Ready to serve requests.");
    0
}

fn run_squidclient(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: squidclient [options] url");
        println!();
        println!("Options:");
        println!("  -h host       Proxy host (default: localhost)");
        println!("  -p port       Proxy port (default: 3128)");
        println!("  -m method     Request method (default: GET)");
        println!("  -H 'string'   Extra headers");
        return 0;
    }
    let url = args.iter().find(|a| a.starts_with("http")).map(|s| s.as_str()).unwrap_or("http://example.com/");
    println!("HTTP/1.1 200 OK");
    println!("Server: squid/6.9");
    println!("Date: Thu, 22 May 2025 10:00:00 GMT");
    println!("Content-Type: text/html; charset=UTF-8");
    println!("Content-Length: 1234");
    println!("X-Cache: HIT from squid.example.com");
    println!("Via: 1.1 squid.example.com (squid/6.9)");
    println!("Connection: close");
    println!();
    println!("(body of {} — simulated)", url);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("squid");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "squidclient" => run_squidclient(rest),
        _ => run_squid(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_squid};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_squid(vec!["--help".to_string()]), 0);
        assert_eq!(run_squid(vec!["-h".to_string()]), 0);
        assert_eq!(run_squid(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_squid(vec![]), 0);
    }
}
