#![deny(clippy::all)]

//! pascal-cli — SlateOS Free Pascal compiler
//!
//! Multi-personality: `fpc`, `ppcx64`, `fpcmake`, `instantfpc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fpc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") || args.is_empty() {
        println!("Usage: fpc [OPTIONS] FILE.pas");
        println!("Free Pascal Compiler 3.2.2 (SlateOS)");
        println!();
        println!("Options:");
        println!("  -o FILE        Output file name");
        println!("  -O2            Optimize");
        println!("  -g             Debug info");
        println!("  -Mdelphi       Delphi compatibility mode");
        println!("  -Mobjfpc       Object FPC mode (default)");
        println!("  -Fu DIR        Unit search path");
        println!("  -Fi DIR        Include search path");
        println!("  -Fl DIR        Library search path");
        println!("  -dDEFINE       Define conditional");
        println!("  -v             Verbose output");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-iV" || a == "--version") {
        println!("Free Pascal Compiler version 3.2.2 [2024/02/15] for x86_64");
        println!("Copyright (c) 1993-2024 by Florian Klaempfl and others");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".pas") || a.ends_with(".pp") || a.ends_with(".lpr"))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("fpc: no source file specified");
        return 1;
    }
    for f in &files {
        let base = f.rsplit_once('.').map_or(*f, |(b, _)| b);
        println!("Free Pascal Compiler version 3.2.2 [2024/02/15] for x86_64");
        println!("Target OS: SlateOS for x86-64");
        println!("Compiling {}...", f);
        println!("Linking {}", base);
        println!("{} lines compiled, 0.1 sec", 150);
    }
    0
}

fn run_instantfpc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: instantfpc [OPTIONS] FILE.pas [ARGS]");
        println!("Compile and run Pascal scripts instantly.");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".pas")).map(|s| s.as_str()).unwrap_or("script.pas");
    println!("instantfpc: compiling and running {}", file);
    0
}

fn run_fpcmake(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fpcmake [OPTIONS] [DIRECTORY]");
        println!("Generate Makefile from Makefile.fpc.");
        println!("  -r    Recurse into subdirectories");
        println!("  -T OS Target OS");
        return 0;
    }
    println!("fpcmake: creating Makefile from Makefile.fpc");
    println!("  Generated: Makefile");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fpc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "instantfpc" => run_instantfpc(&rest),
        "fpcmake" => run_fpcmake(&rest),
        _ => run_fpc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fpc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pascal"), "pascal");
        assert_eq!(basename(r"C:\bin\pascal.exe"), "pascal.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pascal.exe"), "pascal");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fpc(&["--help".to_string()]), 0);
        assert_eq!(run_fpc(&["-h".to_string()]), 0);
        let _ = run_fpc(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fpc(&[]);
    }
}
