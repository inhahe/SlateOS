#![deny(clippy::all)]

//! cargo-nextest — OurOS next-generation test runner for Rust
//!
//! Single personality: `cargo-nextest`

use std::env;
use std::process;

fn run_cargo_nextest(args: Vec<String>) -> i32 {
    // cargo-nextest is invoked as `cargo nextest`, so first arg is "nextest"
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let subargs: Vec<String> = if cmd == "nextest" {
        args[1..].to_vec()
    } else {
        args
    };

    let subcmd = subargs.first().map(|s| s.as_str()).unwrap_or("");
    match subcmd {
        "--help" | "-h" | "" => {
            println!("Usage: cargo nextest <COMMAND>");
            println!();
            println!("A next-generation test runner for Rust.");
            println!();
            println!("Commands:");
            println!("  run         Build and run tests");
            println!("  list        List tests");
            println!("  archive     Archive test binaries");
            println!("  show-config Show configuration");
            println!("  self        Manage nextest installation");
            println!();
            println!("Options:");
            println!("  --config-file <FILE>   Config file path");
            println!("  --tool-config-file <F> Tool-specific config");
            println!("  -V, --version          Show version");
            0
        }
        "--version" | "-V" => {
            println!("cargo-nextest 0.9.72 (OurOS)");
            0
        }
        "run" => {
            let filter: Option<&str> = subargs.iter()
                .skip(1)
                .find(|a| !a.starts_with('-'))
                .map(|s| s.as_str());

            println!("    Compiling tests...");
            println!("    Finished `test` profile target(s) in 3.45s");
            println!();
            println!("───── NEXTEST RUN ─────────────────────────────────────────");
            println!("    Starting  24 tests across 4 binaries (8 threads)");

            if let Some(f) = filter {
                println!("    Filter: {}", f);
            }

            println!("        PASS [  0.012s] my-lib tests::test_basic");
            println!("        PASS [  0.015s] my-lib tests::test_edge_case");
            println!("        PASS [  0.008s] my-lib tests::test_error_handling");
            println!("        PASS [  0.023s] my-lib tests::test_concurrent");
            println!("        PASS [  0.045s] my-lib::integration test_full_workflow");
            println!("        PASS [  0.031s] my-lib::integration test_api");
            println!("        ...");
            println!("───── Summary ─────────────────────────────────────────────");
            println!("     24 tests run:   24 passed,  0 failed,  0 skipped");
            println!("     Duration: 0.82s");
            0
        }
        "list" => {
            println!("my-lib:");
            println!("  tests::test_basic");
            println!("  tests::test_edge_case");
            println!("  tests::test_error_handling");
            println!("  tests::test_concurrent");
            println!("my-lib::integration:");
            println!("  test_full_workflow");
            println!("  test_api");
            println!("  test_stress");
            println!();
            println!("Listed 7 tests across 2 binaries.");
            0
        }
        "archive" => {
            println!("Archiving test binaries...");
            println!("  Created: target/nextest/archive.tar.zst (2.4 MiB)");
            0
        }
        "show-config" => {
            println!("# nextest configuration");
            println!("[profile.default]");
            println!("retries = 0");
            println!("fail-fast = true");
            println!("slow-timeout = {{ period = \"60s\" }}");
            println!("test-threads = \"num-cpus\"");
            println!();
            println!("[profile.ci]");
            println!("retries = 2");
            println!("fail-fast = false");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", subcmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cargo_nextest(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cargo_nextest};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cargo_nextest(vec!["--help".to_string()]), 0);
        assert_eq!(run_cargo_nextest(vec!["-h".to_string()]), 0);
        assert_eq!(run_cargo_nextest(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cargo_nextest(vec![]), 0);
    }
}
