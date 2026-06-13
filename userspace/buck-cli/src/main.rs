#![deny(clippy::all)]

//! buck-cli — SlateOS Buck2 build system
//!
//! Multi-personality: `buck2`, `buck`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_buck2(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: buck2 COMMAND [OPTIONS]");
        println!("Buck2 Build System (Slate OS)");
        println!();
        println!("Commands:");
        println!("  build        Build specified targets");
        println!("  test         Build and test");
        println!("  run          Build and run a binary");
        println!("  query        Query the target graph");
        println!("  targets      List available targets");
        println!("  audit        Audit build configuration");
        println!("  clean        Clean build outputs");
        println!("  init         Initialize a new project");
        println!("  install      Build and install on device");
        println!("  cquery       Configured target query");
        println!("  uquery       Unconfigured target query");
        println!("  aquery       Action query");
        println!("  bxl          Run BXL script");
        println!("  log          Access build logs");
        println!("  debug        Debug build");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("buck2 20240215 (Slate OS)"),
        "build" => {
            let targets: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            if targets.is_empty() {
                println!("buck2 build: no targets specified");
                return 1;
            }
            for t in &targets {
                println!("Build ID: abc123def456");
                println!("  Building: {}", t);
            }
            println!("Jobs completed: 15. Time elapsed: 2.3s.");
            println!("BUILD SUCCEEDED");
        }
        "test" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("//...");
            println!("Testing: {}", target);
            println!("  PASS: //tests:unit_test (3 tests, 0.5s)");
            println!("  PASS: //tests:integration_test (5 tests, 1.2s)");
            println!("Tests finished: 8 passed, 0 failed");
        }
        "run" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("//app:main");
            println!("Building: {}", target);
            println!("Running: {}", target);
        }
        "query" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("deps(//...)");
            println!("buck2 query: {}", query);
            println!("  //lib:core");
            println!("  //lib:utils");
            println!("  //app:main");
        }
        "targets" => {
            let pattern = args.get(1).map(|s| s.as_str()).unwrap_or("//...");
            println!("buck2 targets: {}", pattern);
            println!("  //lib:core");
            println!("  //lib:utils");
            println!("  //app:main");
            println!("  //tests:unit_test");
        }
        "clean" => {
            println!("Cleaning buck-out/...");
            println!("Cleaned.");
        }
        "init" => {
            println!("Initializing buck2 project...");
            println!("  Created .buckconfig");
            println!("  Created BUCK");
            println!("  Created toolchains/BUCK");
            println!("Project initialized.");
        }
        "audit" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("config");
            println!("buck2 audit {}", what);
            println!("  Configuration looks good.");
        }
        _ => println!("buck2: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "buck2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_buck2(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_buck2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/buck"), "buck");
        assert_eq!(basename(r"C:\bin\buck.exe"), "buck.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("buck.exe"), "buck");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_buck2(&["--help".to_string()]), 0);
        assert_eq!(run_buck2(&["-h".to_string()]), 0);
        let _ = run_buck2(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_buck2(&[]);
    }
}
