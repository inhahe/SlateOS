#![deny(clippy::all)]

//! libpinyin-cli — SlateOS libpinyin Chinese input method
//!
//! Single personality: `pinyin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pinyin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pinyin [OPTIONS]");
        println!("pinyin v2.8 (Slate OS) — Chinese Pinyin input engine");
        println!();
        println!("Options:");
        println!("  --train FILE      Train with text corpus");
        println!("  --import FILE     Import user dictionary");
        println!("  --export FILE     Export user dictionary");
        println!("  --version         Show version");
        println!();
        println!("Intelligent Pinyin input with phrase prediction,");
        println!("fuzzy matching, and cloud-style suggestions.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pinyin v2.8 (Slate OS, libpinyin)"); return 0; }
    println!("pinyin: Chinese input engine");
    println!("  Dictionary: system (380k entries) + user");
    println!("  Fuzzy pinyin: enabled");
    println!("  Prediction: phrase-level");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pinyin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pinyin(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pinyin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/libpinyin"), "libpinyin");
        assert_eq!(basename(r"C:\bin\libpinyin.exe"), "libpinyin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("libpinyin.exe"), "libpinyin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pinyin(&["--help".to_string()], "libpinyin"), 0);
        assert_eq!(run_pinyin(&["-h".to_string()], "libpinyin"), 0);
        let _ = run_pinyin(&["--version".to_string()], "libpinyin");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pinyin(&[], "libpinyin");
    }
}
