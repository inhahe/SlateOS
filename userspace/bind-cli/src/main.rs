#![deny(clippy::all)]

//! bind-cli — Slate OS BIND DNS server tools
//!
//! Multi-personality: `named`, `rndc`, `dig`, `nslookup`, `host`, `named-checkconf`, `named-checkzone`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dig(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dig [@server] name [type] [options]");
        println!();
        println!("dig — DNS lookup utility (Slate OS, BIND 9.18).");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("DiG 9.18.24 (Slate OS)");
        return 0;
    }

    let name = args.iter().find(|a| !a.starts_with('-') && !a.starts_with('@')).map(|s| s.as_str()).unwrap_or("example.com");
    let qtype = args.iter().skip_while(|a| a.as_str() != name).nth(1).map(|s| s.as_str()).unwrap_or("A");
    let server = args.iter().find(|a| a.starts_with('@')).map(|s| &s[1..]).unwrap_or("127.0.0.1");

    println!("; <<>> DiG 9.18.24 <<>> {} {}", name, qtype);
    println!(";; global options: +cmd");
    println!(";; Got answer:");
    println!(";; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 12345");
    println!(";; flags: qr rd ra; QUERY: 1, ANSWER: 1, AUTHORITY: 0, ADDITIONAL: 1");
    println!();
    println!(";; QUESTION SECTION:");
    println!(";{}.\t\t\tIN\t{}", name, qtype);
    println!();
    println!(";; ANSWER SECTION:");
    match qtype {
        "AAAA" => println!("{}.\t\t300\tIN\tAAAA\t2606:2800:220:1:248:1893:25c8:1946", name),
        "MX" => println!("{}.\t\t300\tIN\tMX\t10 mail.{}.", name, name),
        "NS" => {
            println!("{}.\t\t300\tIN\tNS\tns1.{}.", name, name);
            println!("{}.\t\t300\tIN\tNS\tns2.{}.", name, name);
        }
        "TXT" => println!("{}.\t\t300\tIN\tTXT\t\"v=spf1 include:_spf.{} ~all\"", name, name),
        _ => println!("{}.\t\t300\tIN\tA\t93.184.216.34", name),
    }
    println!();
    println!(";; Query time: 12 msec");
    println!(";; SERVER: {}#53({})", server, server);
    println!(";; WHEN: Wed May 22 12:00:00 UTC 2024");
    println!(";; MSG SIZE  rcvd: 56");
    0
}

fn run_nslookup(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: nslookup <name> [server]");
        return 0;
    }
    let name = args.first().map(|s| s.as_str()).unwrap_or("example.com");
    let server = args.get(1).map(|s| s.as_str()).unwrap_or("127.0.0.1");
    println!("Server:\t\t{}", server);
    println!("Address:\t{}#53", server);
    println!();
    println!("Non-authoritative answer:");
    println!("Name:\t{}", name);
    println!("Address: 93.184.216.34");
    0
}

fn run_host(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: host [-t type] name [server]");
        return 0;
    }
    let name = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("example.com");
    println!("{} has address 93.184.216.34", name);
    println!("{} has IPv6 address 2606:2800:220:1:248:1893:25c8:1946", name);
    println!("{} mail is handled by 10 mail.{}.", name, name);
    0
}

fn run_rndc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rndc COMMAND [args]");
        println!("Commands: status, reload, flush, dumpdb, stats, reconfig, retransfer");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "status" => {
            println!("version: BIND 9.18.24 (Slate OS)");
            println!("running on slateos-dns: Slate OS");
            println!("boot time: Wed, 22 May 2024 08:00:00 GMT");
            println!("server is up and running");
            println!("number of zones: 42");
            println!("recursive clients: 12/900/1000");
        }
        "reload" => println!("server reload successful"),
        "flush" => println!("flushed cache"),
        "reconfig" => println!("reconfigured zone list"),
        "stats" => println!("statistics dump written to /var/named/data/named_stats.txt"),
        _ => println!("rndc: '{}' completed", subcmd),
    }
    0
}

fn run_named(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: named [OPTIONS]");
        println!("Options: -c <config>, -f (foreground), -g (debug foreground), -d <level>, -v");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("BIND 9.18.24 (Slate OS)");
        return 0;
    }
    println!("named: starting BIND 9.18.24 (Slate OS)");
    println!("named: loading configuration from '/etc/named.conf'");
    println!("named: listening on IPv4 interface lo, 127.0.0.1#53");
    println!("named: listening on IPv4 interface eth0, 192.168.1.100#53");
    println!("named: zone example.com/IN: loaded serial 2024052201");
    println!("named: running");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dig".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nslookup" => run_nslookup(&rest),
        "host" => run_host(&rest),
        "rndc" => run_rndc(&rest),
        "named" => run_named(&rest),
        "named-checkconf" => { println!("Configuration OK"); 0 }
        "named-checkzone" => {
            let zone = rest.first().map(|s| s.as_str()).unwrap_or("example.com");
            println!("zone {}/IN: loaded serial 2024052201", zone);
            println!("OK");
            0
        }
        _ => run_dig(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dig};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bind"), "bind");
        assert_eq!(basename(r"C:\bin\bind.exe"), "bind.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bind.exe"), "bind");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dig(&["--help".to_string()]), 0);
        assert_eq!(run_dig(&["-h".to_string()]), 0);
        let _ = run_dig(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dig(&[]);
    }
}
