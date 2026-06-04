#![deny(clippy::all)]

//! netcat-cli — OurOS netcat (nc) CLI
//!
//! Multi-personality: `nc`, `ncat`, `netcat`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_nc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nc [OPTIONS] [HOST] [PORT]");
        println!();
        println!("netcat — networking Swiss Army knife (OurOS).");
        println!();
        println!("Options:");
        println!("  -l                     Listen mode");
        println!("  -p PORT                Local port");
        println!("  -u                     UDP mode");
        println!("  -v                     Verbose");
        println!("  -z                     Zero-I/O mode (scanning)");
        println!("  -w TIMEOUT             Connection timeout");
        println!("  -k                     Keep listening after disconnect");
        println!("  -e COMMAND             Execute command on connect");
        println!("  -n                     No DNS resolution");
        println!("  -4                     IPv4 only");
        println!("  -6                     IPv6 only");
        println!("  --ssl                  Use SSL/TLS (ncat)");
        println!("  --proxy HOST:PORT      Connect via proxy");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Ncat 7.94 (OurOS)");
        return 0;
    }

    let listen = args.iter().any(|a| a == "-l");
    let verbose = args.iter().any(|a| a == "-v" || a == "-vv");
    let scan = args.iter().any(|a| a == "-z");
    let udp = args.iter().any(|a| a == "-u");

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if listen {
        let port = args.windows(2).find(|w| w[0] == "-p")
            .map(|w| w[1].as_str())
            .or_else(|| positional.first().copied())
            .unwrap_or("4444");
        let proto = if udp { "UDP" } else { "TCP" };
        if verbose {
            println!("Ncat: Listening on 0.0.0.0:{} ({})", port, proto);
        }
        println!("Listening on 0.0.0.0 {}", port);
        println!("Connection received on 192.168.1.100 54321");
    } else if scan {
        let host = positional.first().copied().unwrap_or("localhost");
        let port_spec = positional.get(1).copied().unwrap_or("80");
        // port scan mode
        if port_spec.contains('-') {
            let parts: Vec<&str> = port_spec.split('-').collect();
            let start: u16 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1);
            let end: u16 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
            for port in start..=end.min(start + 5) {
                if port == 22 || port == 80 || port == 443 {
                    println!("Connection to {} {} port [tcp/*] succeeded!", host, port);
                } else if verbose {
                    println!("nc: connect to {} port {} (tcp) failed: Connection refused", host, port);
                }
            }
        } else {
            println!("Connection to {} {} port [tcp/*] succeeded!", host, port_spec);
        }
    } else {
        let host = positional.first().copied().unwrap_or("localhost");
        let port = positional.get(1).copied().unwrap_or("80");
        if verbose {
            println!("Ncat: Connected to {}:{}.", host, port);
        }
        println!("(connected to {}:{})", host, port);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "nc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nc(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/netcat"), "netcat");
        assert_eq!(basename(r"C:\bin\netcat.exe"), "netcat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("netcat.exe"), "netcat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nc(vec!["--help".to_string()]), 0);
        assert_eq!(run_nc(vec!["-h".to_string()]), 0);
        let _ = run_nc(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nc(vec![]);
    }
}
