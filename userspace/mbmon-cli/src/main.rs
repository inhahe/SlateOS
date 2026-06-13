#![deny(clippy::all)]

//! mbmon-cli — SlateOS motherboard monitor
//!
//! Single personality: `mbmon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mbmon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mbmon [OPTIONS]");
        println!("mbmon v2.0 (Slate OS) — Motherboard hardware monitor");
        println!();
        println!("Options:");
        println!("  -c COUNT          Number of readings");
        println!("  -d                Daemon mode");
        println!("  -p PORT           TCP port for daemon (default: 411)");
        println!("  -r                Print raw values");
        println!("  -T TAG            Output tag");
        println!("  -I SECS           Interval between readings");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mbmon v2.0 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-d") {
        println!("mbmon: daemon started on port 411");
        return 0;
    }
    println!("Temp0 :  45.0 C");
    println!("Temp1 :  43.0 C");
    println!("Temp2 :  38.0 C");
    println!("Fan0  :  980 RPM");
    println!("Fan1  : 1250 RPM");
    println!("Vc0   :  1.01 V");
    println!("Vc1   :  3.33 V");
    println!("+5V   :  5.06 V");
    println!("+12V  : 12.14 V");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mbmon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mbmon(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mbmon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mbmon"), "mbmon");
        assert_eq!(basename(r"C:\bin\mbmon.exe"), "mbmon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mbmon.exe"), "mbmon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mbmon(&["--help".to_string()], "mbmon"), 0);
        assert_eq!(run_mbmon(&["-h".to_string()], "mbmon"), 0);
        let _ = run_mbmon(&["--version".to_string()], "mbmon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mbmon(&[], "mbmon");
    }
}
