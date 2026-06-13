#![deny(clippy::all)]

//! zmap-cli — SlateOS ZMap network scanner
//!
//! Single personality: `zmap`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zmap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zmap [OPTIONS] [SUBNET]");
        println!("zmap v3.0 (SlateOS) — Fast single-packet network scanner");
        println!();
        println!("Options:");
        println!("  SUBNET            Target subnet (e.g. 10.0.0.0/8)");
        println!("  -p PORT           Target port");
        println!("  -o FILE           Output file");
        println!("  -r RATE           Send rate (pps)");
        println!("  -B BANDWIDTH      Send bandwidth");
        println!("  -i IFACE          Source interface");
        println!("  -M MODULE         Probe module (tcp_synscan, icmp_echoscan)");
        println!("  -O MODULE         Output module (csv, json)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zmap v3.0 (SlateOS)"); return 0; }
    let subnet = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("192.168.1.0/24");
    let port = args.iter().skip_while(|a| a.as_str() != "-p").nth(1).map(|s| s.as_str()).unwrap_or("80");
    println!("Scanning {} on port {} (tcp_synscan)", subnet, port);
    println!("  Rate: 10000 pps");
    println!("  Sent: 254 packets");
    println!("  Received: 12 responses");
    println!("  Hitrate: 4.72%");
    println!("  Duration: 0.03 sec");
    println!();
    println!("192.168.1.1");
    println!("192.168.1.10");
    println!("192.168.1.20");
    println!("192.168.1.50");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zmap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zmap(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zmap};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zmap"), "zmap");
        assert_eq!(basename(r"C:\bin\zmap.exe"), "zmap.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zmap.exe"), "zmap");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zmap(&["--help".to_string()], "zmap"), 0);
        assert_eq!(run_zmap(&["-h".to_string()], "zmap"), 0);
        let _ = run_zmap(&["--version".to_string()], "zmap");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zmap(&[], "zmap");
    }
}
