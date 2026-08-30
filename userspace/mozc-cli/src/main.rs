#![deny(clippy::all)]

//! mozc-cli — Slate OS Mozc Japanese input method
//!
//! Multi-personality: `mozc_server`, `mozc_tool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_server(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mozc_server [OPTIONS]");
        println!("mozc_server v2.29 (Slate OS) — Mozc conversion server");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Mozc is an open-source Japanese input method based on");
        println!("Google Japanese Input, providing intelligent conversion.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mozc_server v2.29 (Slate OS)"); return 0; }
    println!("mozc_server: Japanese conversion server started");
    println!("  Dictionary: system + user + suggestion");
    println!("  Prediction: context-aware");
    println!("  Cloud sync: disabled");
    0
}

fn run_tool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mozc_tool [OPTIONS]");
        println!("mozc_tool v2.29 (Slate OS) — Mozc configuration tool");
        println!();
        println!("Options:");
        println!("  --mode config_dialog   Open settings");
        println!("  --mode dictionary_tool Open dictionary editor");
        println!("  --mode word_register   Register word");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mozc_tool v2.29 (Slate OS)"); return 0; }
    println!("mozc_tool: configuration tool started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mozc_server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mozc_tool" => run_tool(&rest, &prog),
        _ => run_server(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_server};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mozc"), "mozc");
        assert_eq!(basename(r"C:\bin\mozc.exe"), "mozc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mozc.exe"), "mozc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_server(&["--help".to_string()], "mozc"), 0);
        assert_eq!(run_server(&["-h".to_string()], "mozc"), 0);
        let _ = run_server(&["--version".to_string()], "mozc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_server(&[], "mozc");
    }
}
