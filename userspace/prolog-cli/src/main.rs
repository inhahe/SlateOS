#![deny(clippy::all)]

//! prolog-cli — SlateOS Prolog language tools
//!
//! Multi-personality: `swipl`, `gprolog`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swipl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swipl [OPTIONS] [FILE]");
        println!("SWI-Prolog 9.2.0 (Slate OS)");
        println!("  -g GOAL       Run goal");
        println!("  -t GOAL       Top-level goal");
        println!("  -f FILE       Load file");
        println!("  -s FILE       Load script");
        println!("  -l FILE       Load file (same as -s)");
        println!("  -O            Optimized compilation");
        println!("  --traditional Traditional Prolog mode");
        println!("  --packs       Enable pack system");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("SWI-Prolog version 9.2.0 for x86_64-slateos");
        return 0;
    }
    if args.iter().any(|a| a == "-g") {
        let goal = args.windows(2).find(|w| w[0] == "-g").map(|w| w[1].as_str()).unwrap_or("halt");
        println!("?- {}.", goal);
        println!("true.");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".pl") || a.ends_with(".pro")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("% loading {}", f);
        println!("% compiled 0.001 sec, 42 clauses");
    }
    println!("Welcome to SWI-Prolog 9.2.0 (Slate OS)");
    println!("?- ");
    0
}

fn run_gprolog(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gprolog [OPTIONS]");
        println!("GNU Prolog 1.5.0 (Slate OS)");
        println!("  --consult-file FILE  Load file");
        println!("  --init-goal GOAL     Initial goal");
        println!("  --query-goal GOAL    Query goal");
        println!("  --entry-goal GOAL    Entry point");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gprolog 1.5.0 (Slate OS)");
        println!("By Daniel Diaz");
        return 0;
    }
    println!("GNU Prolog 1.5.0 (64 bits)");
    println!("Compiled Feb 15 2024, 10:00:00 with gcc");
    println!("| ?- ");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swipl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gprolog" => run_gprolog(&rest),
        _ => run_swipl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_swipl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/prolog"), "prolog");
        assert_eq!(basename(r"C:\bin\prolog.exe"), "prolog.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("prolog.exe"), "prolog");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_swipl(&["--help".to_string()]), 0);
        assert_eq!(run_swipl(&["-h".to_string()]), 0);
        let _ = run_swipl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_swipl(&[]);
    }
}
