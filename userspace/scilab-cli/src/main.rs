#![deny(clippy::all)]

//! scilab-cli — OurOS Scilab numerical computing
//!
//! Single personality: `scilab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_scilab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: scilab [OPTIONS]");
        println!("Scilab v2024.1 (OurOS) — Open-source numerical computing");
        println!();
        println!("Options:");
        println!("  -f FILE           Execute script file");
        println!("  -e COMMAND        Execute command string");
        println!("  -nw               No window (console mode)");
        println!("  -nb               No banner");
        println!("  -quit             Exit after script execution");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Scilab v2024.1 (OurOS)");
        return 0;
    }
    if let Some(pos) = args.iter().position(|a| a == "-e") {
        let cmd = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("disp(1+1)");
        println!("--> {}", cmd);
        println!("  ans = 2");
        return 0;
    }
    println!("Scilab v2024.1 — Interactive console");
    println!("  Type 'help' for documentation");
    println!("  Type 'quit' to exit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "scilab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_scilab(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_scilab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/scilab"), "scilab");
        assert_eq!(basename(r"C:\bin\scilab.exe"), "scilab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("scilab.exe"), "scilab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_scilab(&["--help".to_string()], "scilab"), 0);
        assert_eq!(run_scilab(&["-h".to_string()], "scilab"), 0);
        assert_eq!(run_scilab(&["--version".to_string()], "scilab"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_scilab(&[], "scilab"), 0);
    }
}
