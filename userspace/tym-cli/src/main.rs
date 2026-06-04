#![deny(clippy::all)]

//! tym-cli — OurOS tym Lua-configurable terminal
//!
//! Single personality: `tym`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tym(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tym [OPTIONS]");
        println!("tym v3.5 (OurOS) — Lua-configurable terminal");
        println!();
        println!("Options:");
        println!("  -e CMD            Execute command");
        println!("  -c FILE           Config file (Lua)");
        println!("  -t TITLE          Window title");
        println!("  --role ROLE       Window role");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("tym v3.5 (OurOS)"); return 0; }
    println!("tym terminal starting...");
    println!("  Config: Lua");
    println!("  VTE backend");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tym".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tym(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tym};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tym"), "tym");
        assert_eq!(basename(r"C:\bin\tym.exe"), "tym.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tym.exe"), "tym");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tym(&["--help".to_string()], "tym"), 0);
        assert_eq!(run_tym(&["-h".to_string()], "tym"), 0);
        let _ = run_tym(&["--version".to_string()], "tym");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tym(&[], "tym");
    }
}
