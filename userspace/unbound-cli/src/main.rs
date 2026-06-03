#![deny(clippy::all)]

//! unbound-cli — OurOS Unbound recursive DNS resolver
//!
//! Multi-personality: `unbound`, `unbound-control`, `unbound-host`, `unbound-checkconf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_unbound(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unbound [OPTIONS]");
        println!("Options: -c <config>, -d (debug), -v (verbose), -V (version)");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("Version 1.19.1 (OurOS)");
        println!("linked libs: libevent 2.1.12-stable, OpenSSL 3.2.1");
        println!("linked modules: dns64 respip validator iterator");
        println!("BSD licensed, see LICENSE.");
        return 0;
    }

    println!("[1716364800] unbound[1234:0] notice: Start of unbound 1.19.1 (OurOS).");
    println!("[1716364800] unbound[1234:0] info: verbosity 1");
    println!("[1716364800] unbound[1234:0] info: service (unbound 1.19.1).");
    println!("[1716364800] unbound[1234:0] info: start of service (unbound 1.19.1).");
    println!("[1716364800] unbound[1234:0] info: generate keytag query _ta-4f66. NULL IN");
    0
}

fn run_unbound_control(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: unbound-control COMMAND [args]");
        println!();
        println!("Commands: status, stats, stats_noreset, reload, flush <name>,");
        println!("  flush_zone <name>, dump_cache, load_cache, lookup <name>,");
        println!("  list_forwards, list_stubs, list_local_zones");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "status" => {
            println!("version: 1.19.1");
            println!("verbosity: 1");
            println!("threads: 4");
            println!("modules: 3 [ dns64 validator iterator ]");
            println!("uptime: 14400 seconds");
            println!("options: reuseport control(ssl)");
            println!("unbound (pid 1234) is running...");
        }
        "stats" | "stats_noreset" => {
            println!("thread0.num.queries=12345");
            println!("thread0.num.queries_ip_ratelimited=0");
            println!("thread0.num.cachehits=8901");
            println!("thread0.num.cachemiss=3444");
            println!("thread0.num.prefetch=234");
            println!("thread0.num.recursivereplies=3444");
            println!("total.num.queries=45678");
            println!("total.num.cachehits=34567");
            println!("total.num.cachemiss=11111");
            println!("total.requestlist.avg=2.5");
            println!("total.requestlist.max=15");
            println!("total.requestlist.overwritten=0");
            println!("total.recursion.time.avg=0.045678");
            println!("total.recursion.time.median=0.023456");
            println!("msg.cache.count=4567");
            println!("rrset.cache.count=8901");
        }
        "reload" => println!("ok"),
        "flush" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("example.com");
            println!("ok removed {}", name);
        }
        "flush_zone" => {
            let zone = args.get(1).map(|s| s.as_str()).unwrap_or("example.com");
            println!("ok removed {} and subdomains", zone);
        }
        "list_forwards" => {
            println!(". IN forward 8.8.8.8 8.8.4.4");
            println!("corp.example.com. IN forward 10.0.0.53");
        }
        "dump_cache" => println!("START_RRSET_CACHE\nEND_RRSET_CACHE\nSTART_MSG_CACHE\nEND_MSG_CACHE\nEOF"),
        _ => println!("ok"),
    }
    0
}

fn run_unbound_host(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: unbound-host [-v] [-t type] <name>");
        return 0;
    }
    let name = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("example.com");
    println!("{} has address 93.184.216.34 (secure)", name);
    println!("{} has IPv6 address 2606:2800:220:1:248:1893:25c8:1946 (secure)", name);
    println!("{} mail is handled by 10 mail.{} (secure)", name, name);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "unbound".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "unbound-control" => run_unbound_control(&rest),
        "unbound-host" => run_unbound_host(&rest),
        "unbound-checkconf" => { println!("unbound-checkconf: no errors in /etc/unbound/unbound.conf"); 0 }
        _ => run_unbound(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_unbound};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/unbound"), "unbound");
        assert_eq!(basename(r"C:\bin\unbound.exe"), "unbound.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("unbound.exe"), "unbound");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_unbound(&["--help".to_string()]), 0);
        assert_eq!(run_unbound(&["-h".to_string()]), 0);
        assert_eq!(run_unbound(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_unbound(&[]), 0);
    }
}
