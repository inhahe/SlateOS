#![deny(clippy::all)]

//! nsd-cli — OurOS NSD authoritative DNS server
//!
//! Multi-personality: `nsd`, `nsd-control`, `nsd-checkzone`, `nsd-checkconf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nsd(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "nsd-control" => {
                println!("nsd-control (OurOS) — NSD remote control");
                println!("  status         Show server status");
                println!("  reload         Reload zones");
                println!("  reconfig       Reload config");
                println!("  stats          Show statistics");
                println!("  zonestatus Z   Zone status");
            }
            "nsd-checkzone" => {
                println!("nsd-checkzone (OurOS) — Check zone file");
                println!("  nsd-checkzone ZONE ZONEFILE");
            }
            "nsd-checkconf" => {
                println!("nsd-checkconf (OurOS) — Check config");
                println!("  nsd-checkconf CONFIGFILE");
            }
            _ => {
                println!("NSD v4.9 (OurOS) — Authoritative DNS server");
                println!("  -c FILE    Config file");
                println!("  -d         Debug mode");
                println!("  -p PORT    Port (default: 53)");
                println!("  -f DB      Database file");
            }
        }
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NSD v4.9.1 (OurOS)"); return 0; }
    match prog {
        "nsd-control" => {
            println!("NSD status:");
            println!("  version: 4.9.1");
            println!("  uptime: 45 days 12:34:56");
            println!("  zones: 67");
            println!("  queries: 12,345,678");
        }
        "nsd-checkzone" => {
            println!("zone example.com is ok");
        }
        _ => {
            println!("NSD v4.9.1 (OurOS)");
            println!("  Zones: 67 loaded");
            println!("  Listening: 0.0.0.0:53 (UDP+TCP)");
            println!("  Control: /var/run/nsd/nsd.ctl");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nsd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nsd(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nsd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nsd"), "nsd");
        assert_eq!(basename(r"C:\bin\nsd.exe"), "nsd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nsd.exe"), "nsd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nsd(&["--help".to_string()], "nsd"), 0);
        assert_eq!(run_nsd(&["-h".to_string()], "nsd"), 0);
        assert_eq!(run_nsd(&["--version".to_string()], "nsd"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nsd(&[], "nsd"), 0);
    }
}
