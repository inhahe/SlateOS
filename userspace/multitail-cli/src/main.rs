#![deny(clippy::all)]

//! multitail-cli — OurOS MultiTail multi-log viewer
//!
//! Single personality: `multitail`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_multitail(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: multitail [OPTIONS] FILE [FILE...]");
        println!("multitail v7.1 (OurOS) — Multiple log file viewer");
        println!();
        println!("Options:");
        println!("  -s N              Split vertically into N columns");
        println!("  -sw H,H,...       Set window heights");
        println!("  -e REGEX          Highlight matching lines");
        println!("  -cS SCHEME        Color scheme");
        println!("  --version         Show version");
        println!();
        println!("View multiple log files in split-screen terminal windows.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("multitail v7.1 (OurOS)"); return 0; }
    println!("multitail: viewing {} log file(s)", args.len());
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "multitail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_multitail(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_multitail};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/multitail"), "multitail");
        assert_eq!(basename(r"C:\bin\multitail.exe"), "multitail.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("multitail.exe"), "multitail");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_multitail(&["--help".to_string()], "multitail"), 0);
        assert_eq!(run_multitail(&["-h".to_string()], "multitail"), 0);
        assert_eq!(run_multitail(&["--version".to_string()], "multitail"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_multitail(&[], "multitail"), 0);
    }
}
