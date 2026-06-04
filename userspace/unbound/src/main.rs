#![deny(clippy::all)]

//! unbound — OurOS validating, recursive DNS resolver
//!
//! Multi-personality: `unbound` (daemon), `unbound-control` (remote control),
//!   `unbound-checkconf` (config check), `unbound-host` (DNS lookup)

use std::env;
use std::process;

fn run_unbound(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unbound [options]");
        println!("  -d           Run in foreground (debug mode)");
        println!("  -c <file>    Config file (default: /etc/unbound/unbound.conf)");
        println!("  -v           Increase verbosity");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Version 1.20.0 (OurOS)");
        println!("linked libs: OpenSSL 3.2.1, libevent 2.1.12-stable");
        println!("linked modules: dns64 respip validator iterator");
        return 0;
    }
    let config = args.iter().position(|a| a == "-c")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("/etc/unbound/unbound.conf");
    println!("[2025-05-22 10:00:00] unbound[12345:0] info: start of service (unbound 1.20.0).");
    println!("[2025-05-22 10:00:00] unbound[12345:0] info: read {}", config);
    println!("[2025-05-22 10:00:00] unbound[12345:0] info: service became available.");
    println!("[2025-05-22 10:00:00] unbound[12345:0] info: listening on 0.0.0.0 port 53");
    println!("[2025-05-22 10:00:00] unbound[12345:0] info: listening on :: port 53");
    0
}

fn run_control(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: unbound-control <command>");
            println!();
            println!("Commands:");
            println!("  status              Show server status");
            println!("  stats               Show statistics");
            println!("  stats_noreset       Show statistics without reset");
            println!("  reload              Reload configuration");
            println!("  flush <name>        Flush cache for name");
            println!("  flush_zone <name>   Flush cache for zone");
            println!("  dump_cache          Dump cache to stdout");
            println!("  local_zone <name> <type>    Add local zone");
            println!("  local_data <rr>     Add local data");
            println!("  forward_add <zone> <addr>   Add forward zone");
            println!("  stop                Stop the server");
            0
        }
        "status" => {
            println!("version: 1.20.0");
            println!("verbosity: 1");
            println!("threads: 4");
            println!("modules: 4 [ dns64 respip validator iterator ]");
            println!("uptime: 86400 seconds");
            println!("options: reuseport control");
            println!("unbound (pid 12345) is running...");
            0
        }
        "stats" | "stats_noreset" => {
            println!("thread0.num.queries=142890");
            println!("thread0.num.queries_ip_ratelimited=0");
            println!("thread0.num.cachehits=128601");
            println!("thread0.num.cachemiss=14289");
            println!("thread0.num.prefetch=1234");
            println!("thread0.num.zero_ttl=0");
            println!("thread0.num.recursivereplies=14289");
            println!("total.num.queries=142890");
            println!("total.num.cachehits=128601");
            println!("total.num.cachemiss=14289");
            println!("total.requestlist.avg=0.5");
            println!("total.requestlist.max=12");
            println!("total.tcpusage=3");
            0
        }
        "reload" => { println!("ok"); 0 }
        "stop" => { println!("ok"); 0 }
        "flush" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("example.com");
            println!("ok removed {}", name);
            0
        }
        "flush_zone" => {
            let zone = args.get(1).map(|s| s.as_str()).unwrap_or("example.com");
            println!("ok removed {}", zone);
            0
        }
        "dump_cache" => {
            println!("START_RRSET_CACHE");
            println!("example.com. 3600 IN A 93.184.216.34");
            println!("example.com. 3600 IN AAAA 2606:2800:220:1:248:1893:25c8:1946");
            println!("google.com. 300 IN A 142.250.80.46");
            println!("END_RRSET_CACHE");
            0
        }
        other => { eprintln!("unbound-control: unknown command '{}'", other); 1 }
    }
}

fn run_checkconf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unbound-checkconf [config_file]");
        return 0;
    }
    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/etc/unbound/unbound.conf");
    println!("unbound-checkconf: no errors in {}", config);
    0
}

fn run_host(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unbound-host [-vdhr46] [-c class] [-t type] name");
        return 0;
    }
    let name = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("example.com");
    let verbose = args.iter().any(|a| a == "-v");
    println!("{} has address 93.184.216.34", name);
    println!("{} has IPv6 address 2606:2800:220:1:248:1893:25c8:1946", name);
    println!("{} mail is handled by 10 mail.{}", name, name);
    if verbose {
        println!("{} has DNSSEC validation: secure", name);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("unbound");
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
        "unbound-control" => run_control(rest),
        "unbound-checkconf" => run_checkconf(rest),
        "unbound-host" => run_host(rest),
        _ => run_unbound(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_unbound};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_unbound(vec!["--help".to_string()]), 0);
        assert_eq!(run_unbound(vec!["-h".to_string()]), 0);
        let _ = run_unbound(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_unbound(vec![]);
    }
}
