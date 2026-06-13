#![deny(clippy::all)]

//! axiom-cli — Slate OS Axiom/FriCAS computer algebra system
//!
//! Multi-personality: `fricas`, `axiom`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fricas(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fricas [OPTIONS]");
        println!("  -nosman       Don't start HyperDoc");
        println!("  -nogr         No graphics");
        println!("  -noclef       No command-line editing");
        println!("  -eval CODE    Evaluate expression");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("FriCAS 1.3.10 (Slate OS)");
        println!("Based on Axiom");
        println!("SBCL 2.4.1");
        return 0;
    }
    if args.iter().any(|a| a == "-eval") {
        let code = args.windows(2).find(|w| w[0] == "-eval").map(|w| w[1].as_str()).unwrap_or("factor 2024");
        println!("(1) -> {}", code);
        println!("   (1)  [result]");
        println!("                                          Type: Expression Integer");
        return 0;
    }
    println!("                     FriCAS Computer Algebra System");
    println!("                       Version: FriCAS 1.3.10");
    println!("              Timestamp: Thu Jan 15 12:00:00 UTC 2024");
    println!("   Issue )copyright to view copyright notices.");
    println!("   Issue )summary for a summary of useful system commands.");
    println!("   Issue )quit to leave FriCAS.");
    println!();
    println!("(1) ->");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fricas".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fricas(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fricas};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/axiom"), "axiom");
        assert_eq!(basename(r"C:\bin\axiom.exe"), "axiom.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("axiom.exe"), "axiom");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fricas(&["--help".to_string()]), 0);
        assert_eq!(run_fricas(&["-h".to_string()]), 0);
        let _ = run_fricas(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fricas(&[]);
    }
}
