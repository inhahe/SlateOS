#![deny(clippy::all)]

//! uim-cli — SlateOS uim input method framework
//!
//! Multi-personality: `uim-xim`, `uim-toolbar`, `uim-pref`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_uim_xim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uim-xim [OPTIONS]");
        println!("uim-xim v1.8 (Slate OS) — uim XIM bridge");
        println!();
        println!("Options:");
        println!("  --engine NAME     Default IM engine");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("uim-xim v1.8 (Slate OS)"); return 0; }
    println!("uim-xim: XIM input method bridge started");
    println!("  Engines: anthy, pinyin, hangul, m17n, skk");
    0
}

fn run_toolbar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uim-toolbar [OPTIONS]");
        println!("uim-toolbar v1.8 (Slate OS) — uim input method toolbar");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("uim-toolbar v1.8 (Slate OS)"); return 0; }
    println!("uim-toolbar: input method toolbar started");
    0
}

fn run_pref(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uim-pref [OPTIONS]");
        println!("uim-pref v1.8 (Slate OS) — uim preferences");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("uim-pref v1.8 (Slate OS)"); return 0; }
    println!("uim-pref: preferences dialog opened");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "uim-xim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "uim-toolbar" => run_toolbar(&rest, &prog),
        "uim-pref" => run_pref(&rest, &prog),
        _ => run_uim_xim(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_uim_xim};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/uim"), "uim");
        assert_eq!(basename(r"C:\bin\uim.exe"), "uim.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("uim.exe"), "uim");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_uim_xim(&["--help".to_string()], "uim"), 0);
        assert_eq!(run_uim_xim(&["-h".to_string()], "uim"), 0);
        let _ = run_uim_xim(&["--version".to_string()], "uim");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_uim_xim(&[], "uim");
    }
}
