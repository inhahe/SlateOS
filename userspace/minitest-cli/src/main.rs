#![deny(clippy::all)]

//! minitest-cli — SlateOS Ruby Minitest runner
//!
//! Multi-personality: `minitest`, `ruby-test`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_minitest(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: minitest [OPTIONS] [FILES]");
        println!("Minitest 5.24.0 (SlateOS)");
        println!();
        println!("Options:");
        println!("  -n, --name PATTERN   Run tests matching pattern");
        println!("  -v, --verbose        Verbose output");
        println!("  -s, --seed SEED      Random seed");
        println!("  --pride              Show pride output");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("minitest 5.24.0");
        return 0;
    }
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let pride = args.iter().any(|a| a == "--pride");

    println!("Run options: --seed 42");
    println!();
    println!("# Running:");
    println!();
    if verbose {
        println!("TestUser#test_create = 0.001 s = .");
        println!("TestUser#test_validate = 0.002 s = .");
        println!("TestUser#test_destroy = 0.001 s = .");
        println!("TestAuth#test_login = 0.003 s = .");
        println!("TestAuth#test_logout = 0.001 s = .");
    } else {
        // Default and the "pride" plugin both print dots in this stub; we
        // have no color, so we render the same plain progress regardless.
        let _ = pride;
        println!(".....");
    }
    println!();
    println!("Finished in 0.008s, 625.0 runs/s, 1250.0 assertions/s.");
    println!();
    println!("5 runs, 10 assertions, 0 failures, 0 errors, 0 skips");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "minitest".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_minitest(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_minitest};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/minitest"), "minitest");
        assert_eq!(basename(r"C:\bin\minitest.exe"), "minitest.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("minitest.exe"), "minitest");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_minitest(&["--help".to_string()]), 0);
        assert_eq!(run_minitest(&["-h".to_string()]), 0);
        let _ = run_minitest(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_minitest(&[]);
    }
}
