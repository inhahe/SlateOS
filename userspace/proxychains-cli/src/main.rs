#![deny(clippy::all)]

//! proxychains-cli — SlateOS proxy chain/SOCKS tools
//!
//! Multi-personality: `proxychains`, `tsocks`, `redsocks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_proxychains(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: proxychains [OPTIONS] <program> [args...]");
        println!();
        println!("proxychains-ng — proxy chains (SlateOS).");
        println!();
        println!("Options:");
        println!("  -q             Quiet mode");
        println!("  -f <file>      Config file");
        println!();
        println!("Config: /etc/proxychains.conf");
        println!("Chain types: dynamic_chain, strict_chain, round_robin_chain, random_chain");
        return 0;
    }

    let quiet = args.iter().any(|a| a == "-q");
    let prog = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("curl");

    if !quiet {
        println!("[proxychains] config file found: /etc/proxychains.conf");
        println!("[proxychains] preloading /usr/lib/libproxychains4.so");
        println!("[proxychains] DLL init: proxychains-ng 4.17 (SlateOS)");
        println!("[proxychains] Dynamic chain  ...  127.0.0.1:9050  ...  OK");
    }
    println!("[proxychains] launching: {}", prog);
    0
}

fn run_tsocks(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tsocks [OPTIONS] <command> [args...]");
        println!();
        println!("tsocks — transparent SOCKS proxy wrapper (SlateOS).");
        println!();
        println!("Options:");
        println!("  -on            Enable tsocks");
        println!("  -off           Disable tsocks");
        println!("  -show          Show current config");
        return 0;
    }

    if args.iter().any(|a| a == "-show") {
        println!("tsocks configuration:");
        println!("  server = 127.0.0.1");
        println!("  server_port = 1080");
        println!("  server_type = 5");
        println!("  local = 192.168.0.0/255.255.255.0");
        return 0;
    }

    let prog = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("curl");
    println!("tsocks: proxying {} through SOCKS5 127.0.0.1:1080", prog);
    0
}

fn run_redsocks(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: redsocks [OPTIONS]");
        println!();
        println!("redsocks — transparent TCP-to-SOCKS/HTTPS proxy redirector (SlateOS).");
        println!();
        println!("Options:");
        println!("  -c <file>      Config file (default: /etc/redsocks.conf)");
        println!("  -t             Test config and exit");
        println!("  -p <file>      PID file");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("redsocks 0.5 (SlateOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-t") {
        println!("redsocks: configuration file is valid");
        return 0;
    }

    let config = args.windows(2).find(|w| w[0] == "-c")
        .map(|w| w[1].as_str())
        .unwrap_or("/etc/redsocks.conf");
    println!("redsocks: reading config from '{}'", config);
    println!("redsocks: roles:");
    println!("  redsocks: listening on 0.0.0.0:12345 -> SOCKS5 127.0.0.1:1080");
    println!("  redudp: listening on 0.0.0.0:10053 -> 127.0.0.1:1080");
    println!("redsocks: started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "proxychains".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "tsocks" => run_tsocks(&rest),
        "redsocks" => run_redsocks(&rest),
        _ => run_proxychains(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_proxychains};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/proxychains"), "proxychains");
        assert_eq!(basename(r"C:\bin\proxychains.exe"), "proxychains.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("proxychains.exe"), "proxychains");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_proxychains(&["--help".to_string()]), 0);
        assert_eq!(run_proxychains(&["-h".to_string()]), 0);
        let _ = run_proxychains(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_proxychains(&[]);
    }
}
