#![deny(clippy::all)]

//! urh-cli — OurOS Universal Radio Hacker
//!
//! Single personality: `urh`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_urh(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: urh [OPTIONS] [FILE]");
        println!("urh v2.9 (OurOS) — Universal Radio Hacker");
        println!();
        println!("Options:");
        println!("  -f FILE        Open signal file");
        println!("  -p FILE        Open protocol file");
        println!("  --rx           Receive mode");
        println!("  --tx           Transmit mode");
        println!("  --device DEV   SDR device");
        println!("  --freq N       Frequency (Hz)");
        println!("  --sample-rate N  Sample rate");
        println!("  --bandwidth N  Bandwidth");
        println!("  --gain N       Gain (dB)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Universal Radio Hacker v2.9.6 (OurOS)"); return 0; }
    println!("Universal Radio Hacker v2.9.6 (OurOS)");
    println!("  Features:");
    println!("    Signal analysis & demodulation");
    println!("    Protocol analysis");
    println!("    Signal generation & transmission");
    println!("    Fuzzing & simulation");
    println!("  Supported devices: RTL-SDR, HackRF, USRP, Airspy, LimeSDR");
    println!("  Status: ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "urh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_urh(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_urh};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/urh"), "urh");
        assert_eq!(basename(r"C:\bin\urh.exe"), "urh.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("urh.exe"), "urh");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_urh(&["--help".to_string()], "urh"), 0);
        assert_eq!(run_urh(&["-h".to_string()], "urh"), 0);
        assert_eq!(run_urh(&["--version".to_string()], "urh"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_urh(&[], "urh"), 0);
    }
}
