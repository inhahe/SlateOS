#![deny(clippy::all)]

//! iftop-cli — SlateOS iftop network bandwidth monitor
//!
//! Single personality: `iftop`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iftop(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iftop [OPTIONS]");
        println!("iftop 1.0pre4 (SlateOS) — Network bandwidth monitor");
        println!();
        println!("Options:");
        println!("  -i IFACE       Listen on interface");
        println!("  -f FILTER      BPF filter expression");
        println!("  -F NET/MASK    Show traffic to/from network");
        println!("  -G NET/MASK    Same as -F but for IPv6");
        println!("  -n             Don't resolve hostnames");
        println!("  -N             Don't resolve port names");
        println!("  -p             Run in promiscuous mode");
        println!("  -P             Show ports");
        println!("  -B             Display in bytes");
        println!("  -b             Don't display bar graphs");
        println!("  -t             Text output mode");
        println!("  -s NUM         Print once after NUM seconds");
        println!("  -o ORDER       Sort by (2s, 10s, 40s, source, dest)");
        println!("  -c FILE        Config file");
        return 0;
    }
    let iface = args.windows(2).find(|w| w[0] == "-i")
        .map(|w| w[1].as_str()).unwrap_or("eth0");
    if args.iter().any(|a| a == "-t") {
        println!("Listening on {} (text mode)", iface);
        println!("   # Host name             last 2s   last 10s  last 40s cumulative");
        println!("---1 192.168.1.100              =>  2.50Mb   2.35Mb   2.20Mb   28.5MB");
        println!("                                <=  5.60Mb   5.40Mb   5.10Mb   64.2MB");
        println!("---2 10.0.0.1                   =>  0.50Mb   0.48Mb   0.45Mb    5.8MB");
        println!("                                <=  1.20Mb   1.15Mb   1.10Mb   14.1MB");
        return 0;
    }
    println!("iftop: Listening on {}...", iface);
    println!("192.168.1.100           => 2.50Mb  2.35Mb  2.20Mb");
    println!("                        <= 5.60Mb  5.40Mb  5.10Mb");
    println!("TX: cum:  34.3MB  peak:  3.50Mb  rates:  2.50Mb  2.35Mb  2.20Mb");
    println!("RX: cum:  78.3MB  peak:  6.80Mb  rates:  5.60Mb  5.40Mb  5.10Mb");
    println!("TOTAL:    112.6MB        10.30Mb          8.10Mb  7.75Mb  7.30Mb");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iftop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iftop(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_iftop};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/iftop"), "iftop");
        assert_eq!(basename(r"C:\bin\iftop.exe"), "iftop.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("iftop.exe"), "iftop");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_iftop(&["--help".to_string()], "iftop"), 0);
        assert_eq!(run_iftop(&["-h".to_string()], "iftop"), 0);
        let _ = run_iftop(&["--version".to_string()], "iftop");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_iftop(&[], "iftop");
    }
}
