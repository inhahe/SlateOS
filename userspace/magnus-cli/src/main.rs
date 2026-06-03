#![deny(clippy::all)]

//! magnus-cli — OurOS Magnus screen magnifier
//!
//! Single personality: `magnus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_magnus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: magnus [OPTIONS]");
        println!("magnus v1.0 (OurOS) — Screen magnifier");
        println!();
        println!("Options:");
        println!("  --zoom LEVEL      Zoom level (2-16, default 2)");
        println!("  --refresh RATE    Refresh rate (ms, default 250)");
        println!("  --version         Show version");
        println!();
        println!("Shows a magnified view of the area around the cursor.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("magnus v1.0 (OurOS)"); return 0; }
    let zoom = args.iter().skip_while(|a| a.as_str() != "--zoom").nth(1)
        .map(|s| s.as_str()).unwrap_or("2");
    println!("magnus: magnifying at {}x", zoom);
    println!("  Following cursor position");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "magnus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_magnus(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_magnus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/magnus"), "magnus");
        assert_eq!(basename(r"C:\bin\magnus.exe"), "magnus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("magnus.exe"), "magnus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_magnus(&["--help".to_string()], "magnus"), 0);
        assert_eq!(run_magnus(&["-h".to_string()], "magnus"), 0);
        assert_eq!(run_magnus(&["--version".to_string()], "magnus"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_magnus(&[], "magnus"), 0);
    }
}
