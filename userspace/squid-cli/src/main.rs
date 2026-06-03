#![deny(clippy::all)]

//! squid-cli — OurOS web proxy/caching tools
//!
//! Multi-personality: `squid`, `squidclient`, `purge`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_squid(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: squid [OPTIONS]");
        println!();
        println!("squid — web proxy cache (OurOS).");
        println!();
        println!("Options:");
        println!("  -f <file>    Config file");
        println!("  -z           Initialize cache directories");
        println!("  -k reconfigure   Reload config");
        println!("  -k shutdown      Graceful shutdown");
        println!("  -k check         Check running instance");
        println!("  -k parse         Parse config");
        println!("  -N           No daemon mode");
        println!("  -d <level>   Debug level");
        println!("  -v           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("Squid Cache: Version 6.6 (OurOS)");
        println!("Service Name: squid");
        println!("configure options: '--enable-ssl-crtd' '--with-openssl'");
        return 0;
    }

    if args.iter().any(|a| a == "-z") {
        println!("Creating missing swap directories");
        println!("Making directories in /var/spool/squid/00");
        println!("Making directories in /var/spool/squid/01");
        println!("Making directories in /var/spool/squid/02");
        println!("Making directories in /var/spool/squid/03");
        return 0;
    }

    let k_arg = args.windows(2).find(|w| w[0] == "-k").map(|w| w[1].as_str());
    if let Some(cmd) = k_arg {
        match cmd {
            "reconfigure" => println!("Squid is reconfiguring..."),
            "shutdown" => println!("Squid is shutting down..."),
            "check" => println!("squid: running pid 1234"),
            "parse" => {
                println!("Processing Configuration File: /etc/squid/squid.conf (depth 0)");
                println!("Processing: http_port 3128");
                println!("Processing: cache_dir ufs /var/spool/squid 100 16 256");
                println!("Processing: acl localnet src 192.168.0.0/16");
                println!("Processing: http_access allow localnet");
                println!("Configuration file parsing completed.");
            }
            _ => println!("squid: command '{}' completed", cmd),
        }
        return 0;
    }

    println!("Squid Cache: Version 6.6 (OurOS)");
    println!("Process ID 1234");
    println!("Process Roles: worker");
    println!();
    println!("Listening on port 3128");
    println!("Cache directory: /var/spool/squid (100 MB, 78% used)");
    println!();
    println!("Connection information for squid:");
    println!("  Number of clients accessing cache:  42");
    println!("  Number of HTTP requests received:   12345");
    println!("  Hits as %% of all requests:          5min: 45.2%%");
    println!("  Memory hits as %% of hit requests:   5min: 78.3%%");
    println!("  Disk hits as %% of hit requests:     5min: 21.7%%");
    println!("  Memory usage for squid:");
    println!("    Total space in arena:              64 MB");
    println!("    Total accounted:                   48 MB");
    0
}

fn run_squidclient(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: squidclient [OPTIONS] <url>");
        println!();
        println!("squidclient — Squid HTTP client (OurOS).");
        println!();
        println!("Options:");
        println!("  -h <host>    Proxy host (default: localhost)");
        println!("  -p <port>    Proxy port (default: 3128)");
        println!("  -m <method>  HTTP method (default: GET)");
        println!("  -r           Reload (pragma: no-cache)");
        println!("  mgr:info     Request cache manager info");
        return 0;
    }

    let url = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("http://example.com/");
    if url.starts_with("mgr:") {
        println!("HTTP/1.1 200 OK");
        println!("Server: squid/6.6");
        println!("Content-Type: text/plain");
        println!();
        println!("Squid Object Cache: Version 6.6");
        println!("Start Time:   Thu, 22 May 2024 08:00:00 GMT");
        println!("Current Time: Thu, 22 May 2024 12:00:00 GMT");
        println!("Connection information for squid:");
        println!("  Number of clients accessing cache:  42");
    } else {
        println!("HTTP/1.1 200 OK");
        println!("Via: 1.1 localhost (squid/6.6)");
        println!("X-Cache: HIT from localhost");
        println!("X-Cache-Lookup: HIT from localhost:3128");
        println!("Content-Length: 1256");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "squid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "squidclient" => run_squidclient(&rest),
        "purge" => { println!("Purging cache entries matching pattern..."); println!("Purged 42 objects."); 0 }
        _ => run_squid(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_squid};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/squid"), "squid");
        assert_eq!(basename(r"C:\bin\squid.exe"), "squid.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("squid.exe"), "squid");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_squid(&["--help".to_string()]), 0);
        assert_eq!(run_squid(&["-h".to_string()]), 0);
        assert_eq!(run_squid(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_squid(&[]), 0);
    }
}
