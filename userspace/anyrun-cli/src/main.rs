#![deny(clippy::all)]

//! anyrun-cli — SlateOS anyrun Wayland runner
//!
//! Single personality: `anyrun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_anyrun(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anyrun [OPTIONS]");
        println!("anyrun v0.2 (Slate OS) — Wayland-native runner (krunner-like)");
        println!();
        println!("Options:");
        println!("  -c CONFIG         Config file path");
        println!("  --override KEY=VALUE  Override config values");
        println!("  --version         Show version");
        println!();
        println!("Plugin-based runner with modules:");
        println!("  Applications, Shell, Symbols, Translate, Calculator,");
        println!("  Dictionary, Websearch, Randr, Stdin, Kidex");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("anyrun v0.2 (Slate OS)"); return 0; }
    println!("anyrun: Wayland runner");
    println!("  Plugins: applications, shell, calculator, websearch");
    println!("  > ");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "anyrun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_anyrun(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_anyrun};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/anyrun"), "anyrun");
        assert_eq!(basename(r"C:\bin\anyrun.exe"), "anyrun.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("anyrun.exe"), "anyrun");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_anyrun(&["--help".to_string()], "anyrun"), 0);
        assert_eq!(run_anyrun(&["-h".to_string()], "anyrun"), 0);
        let _ = run_anyrun(&["--version".to_string()], "anyrun");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_anyrun(&[], "anyrun");
    }
}
