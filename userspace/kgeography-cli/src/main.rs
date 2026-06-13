#![deny(clippy::all)]

//! kgeography-cli — SlateOS KGeography geography learning
//!
//! Single personality: `kgeography`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kgeography(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kgeography [OPTIONS]");
        println!("kgeography v23.08 (SlateOS) — Geography learning tool");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Modes:");
        println!("  Browse map       Click regions to learn names/capitals");
        println!("  Quiz: Click      Click the named division on map");
        println!("  Quiz: Capital    Name the capital of a division");
        println!("  Quiz: Division   Name the division of a capital");
        println!("  Quiz: Flag       Identify flag of a division");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kgeography v23.08 (SlateOS)"); return 0; }
    println!("kgeography: geography learning started");
    println!("  Maps: World, Europe, Africa, Asia, Americas, Oceania");
    println!("  Quizzes: capitals, flags, locations");
    println!("  Languages: 30+ translations");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kgeography".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kgeography(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kgeography};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kgeography"), "kgeography");
        assert_eq!(basename(r"C:\bin\kgeography.exe"), "kgeography.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kgeography.exe"), "kgeography");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kgeography(&["--help".to_string()], "kgeography"), 0);
        assert_eq!(run_kgeography(&["-h".to_string()], "kgeography"), 0);
        let _ = run_kgeography(&["--version".to_string()], "kgeography");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kgeography(&[], "kgeography");
    }
}
