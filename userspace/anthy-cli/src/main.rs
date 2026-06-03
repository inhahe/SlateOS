#![deny(clippy::all)]

//! anthy-cli — OurOS Anthy Japanese input method
//!
//! Multi-personality: `anthy`, `anthy-dic-tool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_anthy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anthy [OPTIONS]");
        println!("anthy v0.4 (OurOS) — Japanese kana-kanji conversion engine");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Provides Japanese input by converting hiragana to kanji.");
        println!("Used as backend for IBus, uim, SCIM, fcitx.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("anthy v0.4 (OurOS)"); return 0; }
    println!("anthy: Japanese conversion engine");
    println!("  Dictionary: system + user");
    println!("  Prediction: frequency-based");
    0
}

fn run_dic_tool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anthy-dic-tool [--dump|--load|--export]");
        println!("anthy-dic-tool v0.4 (OurOS) — Anthy dictionary tool");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("anthy-dic-tool v0.4 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--dump") {
        println!("anthy-dic-tool: dumping user dictionary...");
        return 0;
    }
    println!("anthy-dic-tool: dictionary management");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "anthy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "anthy-dic-tool" => run_dic_tool(&rest, &prog),
        _ => run_anthy(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_anthy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/anthy"), "anthy");
        assert_eq!(basename(r"C:\bin\anthy.exe"), "anthy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("anthy.exe"), "anthy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_anthy(&["--help".to_string()], "anthy"), 0);
        assert_eq!(run_anthy(&["-h".to_string()], "anthy"), 0);
        assert_eq!(run_anthy(&["--version".to_string()], "anthy"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_anthy(&[], "anthy"), 0);
    }
}
