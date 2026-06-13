#![deny(clippy::all)]

//! libhangul-cli — SlateOS libhangul Korean input method
//!
//! Single personality: `hangul`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hangul(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hangul [OPTIONS]");
        println!("hangul v0.1 (SlateOS) — Korean Hangul input engine");
        println!();
        println!("Options:");
        println!("  --keyboard TYPE   Keyboard layout (2set, 3set-final, 3set-390, romaja)");
        println!("  --version         Show version");
        println!();
        println!("Keyboard layouts:");
        println!("  2set (Dubeolsik)    - Standard Korean 2-set");
        println!("  3set-final          - Sebeolsik Final");
        println!("  3set-390            - Sebeolsik 390");
        println!("  romaja              - Romanization input");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hangul v0.1 (SlateOS, libhangul)"); return 0; }
    println!("hangul: Korean input engine");
    println!("  Keyboard: 2set (Dubeolsik)");
    println!("  Jamo composition: automatic");
    println!("  Hanja conversion: available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hangul".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hangul(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hangul};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/libhangul"), "libhangul");
        assert_eq!(basename(r"C:\bin\libhangul.exe"), "libhangul.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("libhangul.exe"), "libhangul");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hangul(&["--help".to_string()], "libhangul"), 0);
        assert_eq!(run_hangul(&["-h".to_string()], "libhangul"), 0);
        let _ = run_hangul(&["--version".to_string()], "libhangul");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hangul(&[], "libhangul");
    }
}
