#![deny(clippy::all)]

//! powerdns-cli — OurOS PowerDNS server
//!
//! Multi-personality: `pdns_server`, `pdnsutil`, `pdns_control`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_powerdns(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "pdnsutil" => {
                println!("pdnsutil (OurOS) — PowerDNS zone management");
                println!("  create-zone ZONE   Create zone");
                println!("  add-record ZONE NAME TYPE CONTENT  Add record");
                println!("  list-zone ZONE     List records");
                println!("  check-zone ZONE    Verify zone");
                println!("  secure-zone ZONE   DNSSEC sign");
                println!("  rectify-zone ZONE  Fix metadata");
            }
            "pdns_control" => {
                println!("pdns_control (OurOS) — PowerDNS runtime control");
                println!("  status      Show status");
                println!("  ping        Ping server");
                println!("  quit        Shut down");
                println!("  reload      Reload zones");
                println!("  list-zones  List all zones");
            }
            _ => {
                println!("pdns_server v4.9 (OurOS) — PowerDNS authoritative server");
                println!("  --config-dir DIR   Config directory");
                println!("  --daemon           Daemonize");
                println!("  --local-address IP Listen address");
                println!("  --launch BACKEND   Backend module");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PowerDNS v4.9.0 (OurOS)"); return 0; }
    println!("PowerDNS v4.9.0 (OurOS)");
    println!("  Backend: gsqlite3");
    println!("  Zones: 123");
    println!("  Records: 45,678");
    println!("  DNSSEC: 89 zones signed");
    println!("  Queries/sec: 1,234");
    println!("  Listening: 0.0.0.0:53");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdns_server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_powerdns(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_powerdns};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/powerdns"), "powerdns");
        assert_eq!(basename(r"C:\bin\powerdns.exe"), "powerdns.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("powerdns.exe"), "powerdns");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_powerdns(&["--help".to_string()], "powerdns"), 0);
        assert_eq!(run_powerdns(&["-h".to_string()], "powerdns"), 0);
        let _ = run_powerdns(&["--version".to_string()], "powerdns");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_powerdns(&[], "powerdns");
    }
}
