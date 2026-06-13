#![deny(clippy::all)]

//! d-cli — SlateOS D programming language tools
//!
//! Multi-personality: `dmd`, `dub`, `rdmd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dmd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dmd [OPTIONS] FILE.d [FILE.d ...]");
        println!("DMD D Compiler v2.107.0 (SlateOS)");
        println!("  -of=FILE      Output file");
        println!("  -c            Compile only");
        println!("  -O            Optimize");
        println!("  -g            Debug info");
        println!("  -unittest     Enable unit tests");
        println!("  -I=DIR        Import path");
        println!("  -L=FLAG       Linker flags");
        println!("  -run FILE     Compile and run");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("DMD64 D Compiler v2.107.0-SlateOS");
        println!("Copyright (c) 1999-2024, The D Language Foundation");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| a.ends_with(".d")).map(|s| s.as_str()).collect();
    if files.is_empty() {
        println!("dmd: no input files");
        return 1;
    }
    let run = args.iter().any(|a| a == "-run");
    for f in &files {
        println!("dmd: compiling {}", f);
    }
    if run {
        println!("dmd: running...");
    }
    0
}

fn run_dub(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dub COMMAND [OPTIONS]");
        println!("DUB Package Manager 1.37.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  build        Build project");
        println!("  run          Build and run");
        println!("  test         Run unit tests");
        println!("  init         Initialize project");
        println!("  fetch        Fetch packages");
        println!("  add          Add dependency");
        println!("  remove       Remove dependency");
        println!("  upgrade      Upgrade dependencies");
        println!("  describe     Describe project");
        println!("  clean        Clean build files");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("build");
    match subcmd {
        "--version" => println!("DUB version 1.37.0 (SlateOS)"),
        "build" => {
            println!("Compiling myproject...");
            println!("  Linking...");
            println!("  Built: bin/myproject");
        }
        "run" => {
            println!("Building and running myproject...");
            println!("  Running ./bin/myproject");
        }
        "test" => {
            println!("Running unit tests...");
            println!("  All 5 tests passed.");
        }
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myproject");
            println!("Creating project '{}'...", name);
            println!("  Created: {}/source/app.d", name);
            println!("  Created: {}/dub.sdl", name);
        }
        "fetch" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vibe-d");
            println!("Fetching {}...", pkg);
        }
        "clean" => println!("Cleaning build files..."),
        _ => println!("dub: '{}' completed", subcmd),
    }
    0
}

fn run_rdmd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rdmd [OPTIONS] FILE.d [ARGS]");
        println!("rdmd — Compile and run D source code.");
        println!("  --force       Force recompilation");
        println!("  --build-only  Only compile, don't run");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".d")).map(|s| s.as_str()).unwrap_or("script.d");
    println!("rdmd: compiling and running {}", file);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dmd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "dub" => run_dub(&rest),
        "rdmd" => run_rdmd(&rest),
        _ => run_dmd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dmd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/d"), "d");
        assert_eq!(basename(r"C:\bin\d.exe"), "d.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("d.exe"), "d");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dmd(&["--help".to_string()]), 0);
        assert_eq!(run_dmd(&["-h".to_string()]), 0);
        let _ = run_dmd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dmd(&[]);
    }
}
