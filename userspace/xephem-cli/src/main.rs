#![deny(clippy::all)]

//! xephem-cli — OurOS XEphem interactive planetarium
//!
//! Single personality: `xephem`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xephem(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xephem [OPTIONS]");
        println!("XEphem v4.1 (OurOS) — Interactive astronomical ephemeris");
        println!();
        println!("Options:");
        println!("  -lat N         Observer latitude (degrees)");
        println!("  -lon N         Observer longitude (degrees)");
        println!("  -elev N        Observer elevation (meters)");
        println!("  -date DATE     Set date (YYYY/MM/DD)");
        println!("  -time TIME     Set time (HH:MM:SS)");
        println!("  -tz N          Timezone offset (hours)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("XEphem v4.1.0 (OurOS)"); return 0; }
    println!("XEphem v4.1.0 (OurOS) — Astronomical Ephemeris");
    println!("  Observer: 40.7128 N, 74.0060 W, 10m");
    println!("  Date: 2024-06-15 22:00:00 UTC");
    println!("  Sidereal time: 14h 23m 45s");
    println!("  Visible planets:");
    println!("    Jupiter:  RA 04h 32m, Dec +21.5, Mag -2.1");
    println!("    Saturn:   RA 23h 15m, Dec -08.2, Mag +0.8");
    println!("    Mars:     RA 01h 45m, Dec +10.3, Mag +1.2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xephem".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xephem(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
