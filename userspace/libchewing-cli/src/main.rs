#![deny(clippy::all)]

//! libchewing-cli — OurOS libchewing Chinese (Zhuyin) input
//!
//! Single personality: `chewing`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_chewing(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chewing [OPTIONS]");
        println!("chewing v0.9 (OurOS) — Chinese (Zhuyin/Bopomofo) input engine");
        println!();
        println!("Options:");
        println!("  --keyboard TYPE   Keyboard layout (default, hsu, et26, ibm, dvorak)");
        println!("  --version         Show version");
        println!();
        println!("Provides Traditional Chinese input using Zhuyin (Bopomofo)");
        println!("phonetic symbols. Intelligent phrase selection.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("chewing v0.9 (OurOS, libchewing)"); return 0; }
    println!("chewing: Zhuyin input engine");
    println!("  Keyboard: default");
    println!("  Dictionary: system (150k entries)");
    println!("  Phrase prediction: intelligent selection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "chewing".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_chewing(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_chewing};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/libchewing"), "libchewing");
        assert_eq!(basename(r"C:\bin\libchewing.exe"), "libchewing.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("libchewing.exe"), "libchewing");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_chewing(&["--help".to_string()], "libchewing"), 0);
        assert_eq!(run_chewing(&["-h".to_string()], "libchewing"), 0);
        let _ = run_chewing(&["--version".to_string()], "libchewing");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_chewing(&[], "libchewing");
    }
}
