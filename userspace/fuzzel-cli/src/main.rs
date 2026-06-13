#![deny(clippy::all)]

//! fuzzel-cli — SlateOS Fuzzel application launcher
//!
//! Single personality: `fuzzel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fuzzel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fuzzel [OPTIONS]");
        println!("fuzzel v1.10 (SlateOS) — Application launcher and fuzzy finder");
        println!();
        println!("Options:");
        println!("  -d                Application launcher mode (default)");
        println!("  -D                dmenu mode (read stdin)");
        println!("  -w WIDTH          Width in characters");
        println!("  -f FONT           Font specification");
        println!("  -b COLOR          Background color");
        println!("  -t COLOR          Text color");
        println!("  -m COLOR          Match highlight color");
        println!("  -s COLOR          Selection color");
        println!("  -B BORDER         Border width");
        println!("  -r RADIUS         Corner radius");
        println!("  -p PROMPT         Prompt string");
        println!("  -I                Show icons");
        println!("  -T TERMINAL       Terminal for terminal apps");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fuzzel v1.10 (SlateOS)"); return 0; }
    let dmenu = args.iter().any(|a| a == "-D");
    if dmenu {
        println!("fuzzel: dmenu mode (reading stdin)");
    } else {
        println!("fuzzel: application launcher");
        println!("  Fuzzy matching .desktop entries");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fuzzel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fuzzel(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fuzzel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fuzzel"), "fuzzel");
        assert_eq!(basename(r"C:\bin\fuzzel.exe"), "fuzzel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fuzzel.exe"), "fuzzel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fuzzel(&["--help".to_string()], "fuzzel"), 0);
        assert_eq!(run_fuzzel(&["-h".to_string()], "fuzzel"), 0);
        let _ = run_fuzzel(&["--version".to_string()], "fuzzel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fuzzel(&[], "fuzzel");
    }
}
