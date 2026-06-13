#![deny(clippy::all)]

//! bazel-cli — Slate OS Bazel build system CLI
//!
//! Single personality: `bazel`

use std::env;
use std::process;

fn run_bazel(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: bazel <COMMAND> [OPTIONS] [TARGETS...]");
        println!();
        println!("Bazel build system (Slate OS).");
        println!();
        println!("Commands:");
        println!("  build        Build targets");
        println!("  test         Run tests");
        println!("  run          Run a target");
        println!("  query        Query the build graph");
        println!("  clean        Clean build outputs");
        println!("  info         Show build info");
        println!("  fetch        Fetch external dependencies");
        println!("  coverage     Generate coverage report");
        return 0;
    }
    if args.iter().any(|a| a == "version") {
        println!("bazel 7.0.1 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "build" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("//...");
            println!("INFO: Analyzed target {} (12 packages loaded, 234 targets configured).", target);
            println!("INFO: Found 5 targets...");
            println!("[3 / 8] Compiling src/main.cc");
            println!("[5 / 8] Compiling src/utils.cc");
            println!("[7 / 8] Linking bin/myapp");
            println!("[8 / 8] Building output");
            println!("INFO: Elapsed time: 4.567s, Critical Path: 2.3s");
            println!("INFO: 8 processes: 3 internal, 5 local.");
            println!("INFO: Build completed successfully, 8 total actions");
            0
        }
        "test" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("//...");
            println!("INFO: Analyzed target {} (12 packages loaded).", target);
            println!("INFO: Found 3 test targets...");
            println!("//src:unit_test                                          PASSED in 0.8s");
            println!("//src:integration_test                                   PASSED in 2.1s");
            println!("//src:e2e_test                                           PASSED in 3.5s");
            println!();
            println!("Executed 3 out of 3 tests: 3 tests pass.");
            0
        }
        "run" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("//src:myapp");
            println!("INFO: Analyzed target {}.", target);
            println!("INFO: Build completed successfully.");
            println!("INFO: Running {}", target);
            0
        }
        "query" => {
            let expr = args.get(1).map(|s| s.as_str()).unwrap_or("deps(//src:myapp)");
            println!("Query: {}", expr);
            println!("//src:myapp");
            println!("//src:mylib");
            println!("//third_party:abseil");
            println!("//third_party:protobuf");
            println!("@bazel_tools//tools/cpp:toolchain");
            0
        }
        "clean" => {
            let expunge = args.iter().any(|a| a == "--expunge");
            if expunge {
                println!("INFO: Starting clean (--expunge).");
                println!("INFO: Removed entire output base.");
            } else {
                println!("INFO: Starting clean.");
                println!("INFO: Removed build outputs.");
            }
            0
        }
        "info" => {
            println!("bazel-bin: /home/user/.cache/bazel/execroot/myproject/bazel-out/k8-fastbuild/bin");
            println!("bazel-genfiles: /home/user/.cache/bazel/execroot/myproject/bazel-out/k8-fastbuild/genfiles");
            println!("bazel-testlogs: /home/user/.cache/bazel/execroot/myproject/bazel-out/k8-fastbuild/testlogs");
            println!("output_base: /home/user/.cache/bazel");
            println!("workspace: /home/user/project");
            println!("server_pid: 12345");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: bazel <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bazel(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_bazel};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bazel(vec!["--help".to_string()]), 0);
        assert_eq!(run_bazel(vec!["-h".to_string()]), 0);
        let _ = run_bazel(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bazel(vec![]);
    }
}
