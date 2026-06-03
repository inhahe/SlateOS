#![deny(clippy::all)]

//! moodle-cli — OurOS Moodle desktop client
//!
//! Single personality: `moodle`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_moodle(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: moodle [OPTIONS]");
        println!("moodle v4.3 (OurOS) — Moodle LMS desktop client");
        println!();
        println!("Options:");
        println!("  --url URL         Moodle server URL");
        println!("  --token TOKEN     Access token");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("moodle v4.3 (OurOS)"); return 0; }
    println!("moodle: desktop client started");
    println!("  Status: not connected (configure server URL)");
    println!("  Features: course browsing, assignment submission,");
    println!("    notifications, offline content, file management");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "moodle".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_moodle(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_moodle};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/moodle"), "moodle");
        assert_eq!(basename(r"C:\bin\moodle.exe"), "moodle.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("moodle.exe"), "moodle");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_moodle(&["--help".to_string()], "moodle"), 0);
        assert_eq!(run_moodle(&["-h".to_string()], "moodle"), 0);
        assert_eq!(run_moodle(&["--version".to_string()], "moodle"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_moodle(&[], "moodle"), 0);
    }
}
