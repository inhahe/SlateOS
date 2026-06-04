#![deny(clippy::all)]

//! wofi-cli — OurOS Wofi application launcher
//!
//! Single personality: `wofi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wofi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wofi [OPTIONS]");
        println!("wofi v1.4 (OurOS) — Application launcher for Wayland");
        println!();
        println!("Options:");
        println!("  -S MODE           Show mode: drun, run, dmenu");
        println!("  -W WIDTH          Window width (px or %)");
        println!("  -H HEIGHT         Window height (px or %)");
        println!("  -p PROMPT         Prompt text");
        println!("  -x X              X position");
        println!("  -y Y              Y position");
        println!("  -n                Normal window (no layer-shell)");
        println!("  -I                Show icons");
        println!("  -i                Case-insensitive matching");
        println!("  -s STYLE          CSS style file");
        println!("  -c CONFIG         Config file");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wofi v1.4 (OurOS)"); return 0; }
    let mode = args.iter().skip_while(|a| a.as_str() != "-S").nth(1)
        .map(|s| s.as_str()).unwrap_or("drun");
    println!("wofi: launcher (mode={})", mode);
    println!("  [Search...                    ]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wofi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wofi(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wofi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wofi"), "wofi");
        assert_eq!(basename(r"C:\bin\wofi.exe"), "wofi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wofi.exe"), "wofi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wofi(&["--help".to_string()], "wofi"), 0);
        assert_eq!(run_wofi(&["-h".to_string()], "wofi"), 0);
        let _ = run_wofi(&["--version".to_string()], "wofi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wofi(&[], "wofi");
    }
}
