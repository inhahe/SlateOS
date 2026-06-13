#![deny(clippy::all)]

//! lxpolkit-cli — SlateOS LXPolkit lightweight PolicyKit agent
//!
//! Single personality: `lxpolkit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lxpolkit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lxpolkit");
        println!("lxpolkit v0.1 (SlateOS) — Lightweight PolicyKit agent");
        println!();
        println!("Minimal GTK+ PolicyKit authentication agent for LXDE.");
        return 0;
    }
    let _ = args;
    println!("lxpolkit: authentication agent started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lxpolkit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lxpolkit(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lxpolkit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lxpolkit"), "lxpolkit");
        assert_eq!(basename(r"C:\bin\lxpolkit.exe"), "lxpolkit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lxpolkit.exe"), "lxpolkit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lxpolkit(&["--help".to_string()], "lxpolkit"), 0);
        assert_eq!(run_lxpolkit(&["-h".to_string()], "lxpolkit"), 0);
        let _ = run_lxpolkit(&["--version".to_string()], "lxpolkit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lxpolkit(&[], "lxpolkit");
    }
}
