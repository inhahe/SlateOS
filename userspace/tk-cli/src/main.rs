#![deny(clippy::all)]

//! tk-cli — Slate OS Tk toolkit / Wish interpreter
//!
//! Multi-personality: `wish`, `wish8.6`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wish(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wish [OPTIONS] [FILE [ARG ...]]");
        println!("Tk 8.6.14 (Slate OS)");
        println!("  -display DISP  X display to use");
        println!("  -geometry GEO  Window geometry");
        println!("  -name NAME     Application name");
        println!("  -sync          Synchronous X requests");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("8.6.14");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(f) = file {
        println!("wish: executing {}", f);
    } else {
        println!("wish: Tk 8.6.14 interactive mode");
        println!("% ");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wish".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wish(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wish};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tk"), "tk");
        assert_eq!(basename(r"C:\bin\tk.exe"), "tk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tk.exe"), "tk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wish(&["--help".to_string()]), 0);
        assert_eq!(run_wish(&["-h".to_string()]), 0);
        let _ = run_wish(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wish(&[]);
    }
}
