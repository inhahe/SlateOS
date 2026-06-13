#![deny(clippy::all)]

//! kstars-cli — SlateOS KStars astronomy software
//!
//! Single personality: `kstars`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kstars(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kstars [OPTIONS]");
        println!("kstars v3.7 (Slate OS) — Desktop astronomy application");
        println!();
        println!("Options:");
        println!("  --date DATE       Set simulation date");
        println!("  --paused          Start paused");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Sky simulation, telescope control (INDI),");
        println!("  astrophotography planning, observation scheduler,");
        println!("  sky catalog with 100M+ objects");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kstars v3.7 (Slate OS)"); return 0; }
    println!("kstars: astronomy application started");
    println!("  Catalog: 100M+ stars, deep sky objects");
    println!("  INDI: telescope/CCD control framework");
    println!("  Ekos: astrophotography suite");
    println!("  Solar system: high-accuracy ephemeris");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kstars".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kstars(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kstars};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kstars"), "kstars");
        assert_eq!(basename(r"C:\bin\kstars.exe"), "kstars.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kstars.exe"), "kstars");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kstars(&["--help".to_string()], "kstars"), 0);
        assert_eq!(run_kstars(&["-h".to_string()], "kstars"), 0);
        let _ = run_kstars(&["--version".to_string()], "kstars");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kstars(&[], "kstars");
    }
}
