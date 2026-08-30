#![deny(clippy::all)]

//! gnome-calendar-cli — Slate OS GNOME Calendar
//!
//! Single personality: `gnome-calendar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gnome_calendar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gnome-calendar [OPTIONS]");
        println!("gnome-calendar v45.0 (Slate OS) — GNOME desktop calendar");
        println!();
        println!("Options:");
        println!("  --date DATE       Open on specific date");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gnome-calendar v45.0 (Slate OS)"); return 0; }
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
mod tests {
    use super::{basename, strip_ext, run_gnome_calendar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gnome-calendar"), "gnome-calendar");
        assert_eq!(basename(r"C:\bin\gnome-calendar.exe"), "gnome-calendar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gnome-calendar.exe"), "gnome-calendar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gnome_calendar(&["--help".to_string()], "gnome-calendar"), 0);
        assert_eq!(run_gnome_calendar(&["-h".to_string()], "gnome-calendar"), 0);
        let _ = run_gnome_calendar(&["--version".to_string()], "gnome-calendar");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gnome_calendar(&[], "gnome-calendar");
    }
}
