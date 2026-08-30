#![deny(clippy::all)]

//! modula-cli — Slate OS Modula-2/Oberon compiler
//!
//! Multi-personality: `gm2`, `obc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gm2(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gm2 [OPTIONS] FILE.mod [FILE.mod ...]");
        println!("GNU Modula-2 14.0.0 (Slate OS)");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file");
        println!("  -c            Compile only");
        println!("  -O            Optimize");
        println!("  -g            Debug info");
        println!("  -fiso         ISO Modula-2 dialect");
        println!("  -fpim         PIM Modula-2 dialect");
        println!("  -I DIR        Module search path");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gm2 (GCC) 14.0.0");
        println!("Copyright (C) Free Software Foundation, Inc.");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".mod") || a.ends_with(".def"))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("gm2: no input files");
        return 1;
    }
    for f in &files {
        let base = f.rsplit_once('.').map_or(*f, |(b, _)| b);
        println!("gm2: compiling {} -> {}.o", f, base);
    }
    if !args.iter().any(|a| a == "-c") {
        println!("gm2: linking -> a.out");
    }
    0
}

fn run_obc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: obc [OPTIONS] FILE.ob [FILE.ob ...]");
        println!("Oxford Oberon-2 Compiler 3.2 (Slate OS)");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file");
        println!("  -O            Optimize");
        println!("  -g            Debug info");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("obc 3.2 (Slate OS)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".ob") || a.ends_with(".ob2") || a.ends_with(".Mod"))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("obc: no input files");
        return 1;
    }
    for f in &files {
        println!("obc: compiling {}", f);
    }
    println!("obc: linking...");
    println!("obc: done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gm2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "obc" => run_obc(&rest),
        _ => run_gm2(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gm2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/modula"), "modula");
        assert_eq!(basename(r"C:\bin\modula.exe"), "modula.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("modula.exe"), "modula");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gm2(&["--help".to_string()]), 0);
        assert_eq!(run_gm2(&["-h".to_string()]), 0);
        let _ = run_gm2(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gm2(&[]);
    }
}
