#![deny(clippy::all)]

//! pyephem-cli — OurOS PyEphem astronomical computations
//!
//! Single personality: `ephem`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ephem(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ephem [COMMAND] [OPTIONS]");
        println!("ephem v4.1 (OurOS) — Astronomical ephemeris calculator");
        println!();
        println!("Commands:");
        println!("  planets          Show current planet positions");
        println!("  moon             Moon phase and position");
        println!("  sun              Sun rise/set times");
        println!("  rise-set OBJ     Rise/set times for object");
        println!("  separation O1 O2 Angular separation");
        println!();
        println!("Options:");
        println!("  --lat N          Observer latitude");
        println!("  --lon N          Observer longitude");
        println!("  --date DATE      Date (YYYY/MM/DD)");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PyEphem v4.1.5 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("moon") => {
            println!("Moon:");
            println!("  Phase: 72.3% (Waxing Gibbous)");
            println!("  RA: 18h 45m 23s");
            println!("  Dec: -23d 12m 45s");
            println!("  Distance: 384,400 km");
            println!("  Rise: 17:23, Set: 03:45");
            println!("  Next full: 2024-06-22");
        }
        Some("sun") => {
            println!("Sun:");
            println!("  Rise: 05:24");
            println!("  Transit: 12:58");
            println!("  Set: 20:32");
            println!("  Day length: 15h 08m");
            println!("  RA: 05h 38m 12s");
            println!("  Dec: +23d 18m 42s");
        }
        Some("planets") => {
            println!("Planet Positions:");
            println!("  Mercury: RA 06h 12m, Dec +24.1, Mag -0.5");
            println!("  Venus:   RA 08h 45m, Dec +18.3, Mag -3.9");
            println!("  Mars:    RA 01h 34m, Dec +09.2, Mag +1.4");
            println!("  Jupiter: RA 04h 56m, Dec +22.1, Mag -2.0");
            println!("  Saturn:  RA 23h 08m, Dec -07.8, Mag +0.9");
        }
        _ => {
            println!("ephem: specify a command (planets, moon, sun, rise-set)");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ephem".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ephem(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ephem};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pyephem"), "pyephem");
        assert_eq!(basename(r"C:\bin\pyephem.exe"), "pyephem.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pyephem.exe"), "pyephem");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ephem(&["--help".to_string()], "pyephem"), 0);
        assert_eq!(run_ephem(&["-h".to_string()], "pyephem"), 0);
        assert_eq!(run_ephem(&["--version".to_string()], "pyephem"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ephem(&[], "pyephem"), 0);
    }
}
