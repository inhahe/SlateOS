#![deny(clippy::all)]

//! wavemon-cli — Slate OS wavemon wireless monitor
//!
//! Single personality: `wavemon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wavemon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wavemon [OPTIONS]");
        println!("wavemon v0.9 (Slate OS) — Wireless network monitor");
        println!();
        println!("Options:");
        println!("  -i IFACE       Interface (default: wlan0)");
        println!("  -d             Dump mode (non-interactive)");
        println!("  --version      Show version");
        println!();
        println!("ncurses-based wireless monitor showing signal, noise,");
        println!("statistics, AP info, and scan results.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wavemon v0.9 (Slate OS)"); return 0; }
    println!("wavemon: wireless monitor (wlan0)");
    println!("  SSID: HomeNetwork");
    println!("  Signal: -45 dBm (excellent)");
    println!("  Noise: -90 dBm");
    println!("  Channel: 36 (5180 MHz)");
    println!("  Mode: 802.11ac");
    println!("  Tx rate: 866.7 Mbps");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wavemon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wavemon(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wavemon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wavemon"), "wavemon");
        assert_eq!(basename(r"C:\bin\wavemon.exe"), "wavemon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wavemon.exe"), "wavemon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wavemon(&["--help".to_string()], "wavemon"), 0);
        assert_eq!(run_wavemon(&["-h".to_string()], "wavemon"), 0);
        let _ = run_wavemon(&["--version".to_string()], "wavemon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wavemon(&[], "wavemon");
    }
}
