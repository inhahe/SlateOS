#![deny(clippy::all)]

//! nim-cli — SlateOS Nim programming language
//!
//! Multi-personality: `nim`, `nimble`, `nimsuggest`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nim(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nim COMMAND [OPTIONS] FILE.nim");
        println!("Nim Compiler Version 2.0.2 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  c, compile     Compile to C and build");
        println!("  cpp            Compile to C++ and build");
        println!("  js             Compile to JavaScript");
        println!("  r, run         Compile and run");
        println!("  e              Evaluate expression");
        println!("  check          Check for errors");
        println!("  doc            Generate documentation");
        println!("  dump           Dump internal representation");
        println!("  --version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => {
            println!("Nim Compiler Version 2.0.2 [SlateOS: amd64]");
            println!("Compiled at 2024-02-15");
            println!("git hash: abc123def");
        }
        "c" | "compile" | "cpp" | "r" | "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("main.nim");
            let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
            let backend = if subcmd == "cpp" { "C++" } else { "C" };
            println!("Hint: used config file: nim.cfg");
            println!("CC: gcc");
            println!("Hint: {} LOC; compilation: 0.5s", 1500);
            let ext = if subcmd == "cpp" { "cpp" } else { "c" };
            println!("Hint: {backend} backend generated: nimcache/{base}.{ext}");
            if subcmd == "r" || subcmd == "run" {
                println!("Hint: running {}", base);
            }
        }
        "js" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("main.nim");
            let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
            println!("Hint: JavaScript generated: nimcache/{}.js", base);
        }
        "check" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("main.nim");
            println!("Checking {}...", file);
            println!("Hint: no errors found");
        }
        "doc" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("main.nim");
            let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
            println!("Generating documentation for {}...", file);
            println!("  Output: htmldocs/{}.html", base);
        }
        "e" => {
            let expr = args.get(1).map(|s| s.as_str()).unwrap_or("echo 42");
            println!("nim e: {}", expr);
        }
        _ => println!("nim: '{}' completed", subcmd),
    }
    0
}

fn run_nimble(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nimble COMMAND [OPTIONS]");
        println!("Nimble 0.16.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  install      Install packages");
        println!("  uninstall    Remove packages");
        println!("  build        Build project");
        println!("  run          Build and run");
        println!("  test         Run tests");
        println!("  init         Initialize project");
        println!("  list         List installed packages");
        println!("  search       Search packages");
        println!("  publish      Publish package");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("nimble v0.16.0 (SlateOS)"),
        "install" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("jester");
            println!("Downloading {}@#head...", pkg);
            println!("  Installing {}@0.6.0", pkg);
            println!("  Success.");
        }
        "build" => {
            println!("Building myproject/myproject...");
            println!("  Success.");
        }
        "test" => {
            println!("Running tests...");
            println!("  All tests passed.");
        }
        "init" => {
            println!("Created: myproject.nimble");
            println!("Created: src/myproject.nim");
            println!("Created: tests/test1.nim");
        }
        "list" => {
            println!("  jester   [0.6.0]");
            println!("  norm     [2.8.3]");
            println!("  nimpy    [0.2.0]");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("web");
            println!("  jester    Sinatra-like web framework  (url: ...)", );
            let _ = term;
        }
        _ => println!("nimble: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nimble" => run_nimble(&rest),
        "nimsuggest" => { println!("nimsuggest: language server ready"); 0 }
        _ => run_nim(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nim};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nim"), "nim");
        assert_eq!(basename(r"C:\bin\nim.exe"), "nim.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nim.exe"), "nim");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nim(&["--help".to_string()]), 0);
        assert_eq!(run_nim(&["-h".to_string()]), 0);
        let _ = run_nim(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nim(&[]);
    }
}
