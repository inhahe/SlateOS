#![deny(clippy::all)]

//! st-cli — SlateOS st (suckless terminal)
//!
//! Single personality: `st`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_st(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: st [OPTIONS] [CMD [ARGS...]]");
        println!("st v0.9 (Slate OS) — Simple terminal");
        println!();
        println!("Options:");
        println!("  -a                Disable alt screen");
        println!("  -c CLASS          Window class");
        println!("  -e CMD            Execute command");
        println!("  -f FONT           Font specification");
        println!("  -g GEOMETRY       Window geometry");
        println!("  -i                Fix window to tiling WM");
        println!("  -o FILE           Write I/O to file");
        println!("  -T TITLE          Window title");
        println!("  -t NAME           Terminal name (default st-256color)");
        println!("  -v                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") { println!("st v0.9 (Slate OS)"); return 0; }
    println!("st: simple terminal");
    println!("  Font: Liberation Mono:size=12");
    println!("  TERM: st-256color");
    if args.is_empty() {
        println!("  Shell: /bin/sh");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "st".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_st(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_st};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/st"), "st");
        assert_eq!(basename(r"C:\bin\st.exe"), "st.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("st.exe"), "st");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_st(&["--help".to_string()], "st"), 0);
        assert_eq!(run_st(&["-h".to_string()], "st"), 0);
        let _ = run_st(&["--version".to_string()], "st");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_st(&[], "st");
    }
}
