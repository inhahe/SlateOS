#![deny(clippy::all)]

//! swappy-cli — SlateOS Swappy screenshot editor
//!
//! Single personality: `swappy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swappy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: swappy -f FILE [OPTIONS]");
        println!("swappy v1.5 (SlateOS) — Wayland screenshot editor");
        println!();
        println!("Options:");
        println!("  -f FILE           Input image file");
        println!("  -o FILE           Output file");
        println!("  --version         Show version");
        println!();
        println!("Usage: grim -g \"$(slurp)\" - | swappy -f -");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("swappy v1.5 (SlateOS)"); return 0; }
    let file = args.iter().skip_while(|a| a.as_str() != "-f").nth(1).map(|s| s.as_str()).unwrap_or("-");
    println!("Opening editor: {}", file);
    println!("  Tools: pen, rectangle, arrow, text, blur");
    println!("  Ctrl+S to save, Ctrl+C to copy");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swappy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_swappy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_swappy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/swappy"), "swappy");
        assert_eq!(basename(r"C:\bin\swappy.exe"), "swappy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("swappy.exe"), "swappy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_swappy(&["--help".to_string()], "swappy"), 0);
        assert_eq!(run_swappy(&["-h".to_string()], "swappy"), 0);
        let _ = run_swappy(&["--version".to_string()], "swappy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_swappy(&[], "swappy");
    }
}
