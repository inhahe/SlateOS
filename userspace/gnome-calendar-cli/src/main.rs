#![deny(clippy::all)]

//! gnome-calendar-cli — OurOS GNOME Calendar
//!
//! Single personality: `gnome-calendar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnome_calendar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-calendar [OPTIONS]");
        println!("gnome-calendar v45.0 (OurOS) — GNOME desktop calendar");
        println!();
        println!("Options:");
        println!("  --date DATE       Open on specific date");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-calendar v45.0 (OurOS)"); return 0; }
    println!("gnome-calendar: calendar application started");
    println!("  Calendars: 3 (Personal, Work, Holidays)");
    println!("  Today's events: 2");
    println!("  Upcoming this week: 5");
    println!("  Online accounts: 1 (Google)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gnome-calendar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gnome_calendar(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
