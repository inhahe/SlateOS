#![deny(clippy::all)]

//! deno — SlateOS Deno JavaScript/TypeScript runtime
//!
//! Single personality: `deno`

use std::env;
use std::process;

fn run_deno(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deno <SUBCOMMAND> [OPTIONS]");
        println!();
        println!("Subcommands:");
        println!("  run        Run a JavaScript or TypeScript program");
        println!("  serve      Run a server");
        println!("  task       Run a task defined in deno.json");
        println!("  test       Run tests");
        println!("  bench      Run benchmarks");
        println!("  compile    Compile script to executable");
        println!("  fmt        Format source files");
        println!("  lint       Lint source files");
        println!("  check      Type-check without running");
        println!("  repl       Start interactive session");
        println!("  eval       Evaluate code");
        println!("  install    Install script as executable");
        println!("  uninstall  Uninstall script");
        println!("  info       Show dependencies or cache info");
        println!("  doc        Generate documentation");
        println!("  upgrade    Upgrade Deno");
        println!("  --version  Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "--version" | "-V" => {
            println!("deno 1.44.0 (SlateOS)");
            println!("v8 12.6.228.9");
            println!("typescript 5.4.5");
        }
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("main.ts");
            println!("(running {} — simulated)", script);
        }
        "test" => {
            println!("running 3 tests from ./test.ts");
            println!("test basic ... ok (5ms)");
            println!("test async ... ok (12ms)");
            println!("test edge  ... ok (2ms)");
            println!();
            println!("ok | 3 passed | 0 failed (25ms)");
        }
        "fmt" => println!("Checked 5 files"),
        "lint" => println!("Checked 5 files (no diagnostics)"),
        "check" => println!("Check file:///project/main.ts (no errors)"),
        "repl" => {
            println!("Deno 1.44.0 (SlateOS)");
            println!("> (REPL — simulated)");
        }
        "eval" => {
            let code = args.get(1).map(|s| s.as_str()).unwrap_or("console.log('hello')");
            println!("(eval: {})", code);
        }
        "info" => {
            println!("DENO_DIR: /home/user/.cache/deno");
            println!("Remote modules cache: /home/user/.cache/deno/deps");
            println!("TypeScript compiler cache: /home/user/.cache/deno/gen");
        }
        "compile" | "bench" | "serve" | "task" | "install" | "uninstall" | "doc" | "upgrade" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown subcommand '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_deno(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_deno};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_deno(vec!["--help".to_string()]), 0);
        assert_eq!(run_deno(vec!["-h".to_string()]), 0);
        let _ = run_deno(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_deno(vec![]);
    }
}
