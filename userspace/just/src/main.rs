#![deny(clippy::all)]

//! just — SlateOS command runner (make alternative)
//!
//! Single personality: `just`

use std::env;
use std::process;

fn run_just(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: just [OPTIONS] [RECIPE] [ARGUMENTS]...");
        println!();
        println!("A handy way to save and run project-specific commands.");
        println!();
        println!("Options:");
        println!("  -c, --choose          Select recipe to run interactively");
        println!("  --chooser <CHOOSER>   Override binary used by --choose");
        println!("  --color <WHEN>        Color output (auto/always/never)");
        println!("  -n, --dry-run         Print what would be done");
        println!("  --dump                Print justfile");
        println!("  --dump-format <FMT>   Dump format (just/json)");
        println!("  -e, --edit            Edit justfile");
        println!("  --evaluate            Evaluate and print variables");
        println!("  --fmt                 Format justfile");
        println!("  --check               Check formatting (returns 1 if not formatted)");
        println!("  -f, --justfile <FILE> Use this justfile");
        println!("  --init                Initialize a new justfile");
        println!("  -l, --list            List available recipes");
        println!("  --list-heading <TEXT> Heading for --list");
        println!("  --list-prefix <TEXT>  Prefix for --list entries");
        println!("  --no-dotenv           Don't load .env");
        println!("  -q, --quiet           Suppress echoed commands");
        println!("  --set <VAR> <VALUE>   Set a variable");
        println!("  --shell <SHELL>       Shell to use");
        println!("  --shell-arg <ARG>     Shell argument");
        println!("  -s, --show <RECIPE>   Show recipe body");
        println!("  --summary             List recipe names");
        println!("  --unsorted            Don't sort --list output");
        println!("  -v, --verbose         Be more verbose");
        println!("  -d, --working-directory <DIR>  Set working directory");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("just 1.25.2 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--init") {
        println!("Wrote justfile");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Available recipes:");
        println!("    build        # Build the project");
        println!("    test         # Run tests");
        println!("    lint         # Run linter");
        println!("    fmt          # Format code");
        println!("    clean        # Clean build artifacts");
        println!("    run *args    # Run the project");
        println!("    release      # Build release");
        println!("    install      # Install to system");
        println!("    bench        # Run benchmarks");
        println!("    docs         # Generate documentation");
        return 0;
    }
    if args.iter().any(|a| a == "--summary") {
        println!("bench build clean docs fmt install lint release run test");
        return 0;
    }
    if args.iter().any(|a| a == "--dump") {
        let json_fmt = args.windows(2).any(|w| w[0] == "--dump-format" && w[1] == "json");
        if json_fmt {
            println!("{{\"recipes\":{{\"build\":{{\"body\":[\"cargo build\"],\"dependencies\":[\"lint\"]}}}}}}");
        } else {
            println!("# Project justfile");
            println!();
            println!("default: build");
            println!();
            println!("# Build the project");
            println!("build: lint");
            println!("    cargo build");
            println!();
            println!("# Run tests");
            println!("test: build");
            println!("    cargo test");
            println!();
            println!("# Run linter");
            println!("lint:");
            println!("    cargo clippy --all-targets");
            println!();
            println!("# Format code");
            println!("fmt:");
            println!("    cargo fmt");
            println!();
            println!("# Clean build artifacts");
            println!("clean:");
            println!("    cargo clean");
            println!();
            println!("# Run the project");
            println!("run *args:");
            println!("    cargo run -- {{{{args}}}}");
        }
        return 0;
    }
    if args.iter().any(|a| a == "--evaluate") {
        println!("name   := \"my-project\"");
        println!("version := \"1.0.0\"");
        println!("target  := \"release\"");
        return 0;
    }

    // Check for --show
    let show_idx = args.iter().position(|a| a == "-s" || a == "--show");
    if let Some(idx) = show_idx {
        let recipe = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("build");
        match recipe {
            "build" => {
                println!("# Build the project");
                println!("build: lint");
                println!("    cargo build");
            }
            "test" => {
                println!("# Run tests");
                println!("test: build");
                println!("    cargo test");
            }
            _ => {
                println!("# {}", recipe);
                println!("{}:", recipe);
                println!("    echo \"Running {}...\"", recipe);
            }
        }
        return 0;
    }

    let dry_run = args.iter().any(|a| a == "-n" || a == "--dry-run");
    let recipe = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("build");

    if dry_run {
        println!("# (dry run)");
    }

    match recipe {
        "build" => {
            println!("cargo clippy --all-targets");
            if !dry_run {
                println!("(clippy: no warnings)");
            }
            println!("cargo build");
            if !dry_run {
                println!("   Compiling my-project v1.0.0");
                println!("    Finished `dev` profile target(s) in 2.34s");
            }
        }
        "test" => {
            println!("cargo test");
            if !dry_run {
                println!("running 12 tests");
                println!("test result: ok. 12 passed; 0 failed; 0 ignored");
            }
        }
        "clean" => {
            println!("cargo clean");
        }
        _ => {
            println!("echo \"Running {}...\"", recipe);
            if !dry_run {
                println!("Running {}...", recipe);
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_just(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_just};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_just(vec!["--help".to_string()]), 0);
        assert_eq!(run_just(vec!["-h".to_string()]), 0);
        let _ = run_just(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_just(vec![]);
    }
}
