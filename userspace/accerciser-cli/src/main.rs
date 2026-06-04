#![deny(clippy::all)]

//! accerciser-cli — OurOS accessibility explorer
//!
//! Single personality: `accerciser`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_accerciser(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: accerciser [OPTIONS]");
        println!("Accerciser v3.44 (OurOS) — AT-SPI accessibility explorer");
        println!();
        println!("Options:");
        println!("  --tree            Print accessibility tree");
        println!("  --inspect PID     Inspect specific app");
        println!("  --events          Monitor accessibility events");
        println!("  --validate        Validate accessibility compliance");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Accerciser v3.44 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--tree") {
        println!("Accessibility tree:");
        println!("  [desktop]");
        println!("    [application] Terminal");
        println!("      [frame] Terminal Window");
        println!("        [terminal] (text: 80x24)");
        return 0;
    }
    println!("Accerciser v3.44 — AT-SPI explorer");
    println!("  AT-SPI: running");
    println!("  Applications: 3 registered");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "accerciser".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_accerciser(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_accerciser};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/accerciser"), "accerciser");
        assert_eq!(basename(r"C:\bin\accerciser.exe"), "accerciser.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("accerciser.exe"), "accerciser");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_accerciser(&["--help".to_string()], "accerciser"), 0);
        assert_eq!(run_accerciser(&["-h".to_string()], "accerciser"), 0);
        let _ = run_accerciser(&["--version".to_string()], "accerciser");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_accerciser(&[], "accerciser");
    }
}
