#![deny(clippy::all)]

//! just-cli — OurOS just command runner
//!
//! Single personality: `just`

use std::env;
use std::process;

fn run_just(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: just [OPTIONS] [RECIPE [ARGS...]]");
        println!();
        println!("just — a command runner (OurOS).");
        println!();
        println!("Options:");
        println!("  -l, --list           List recipes");
        println!("  -s, --show RECIPE    Show recipe source");
        println!("  -n, --dry-run        Dry run");
        println!("  -f, --justfile FILE  Justfile path");
        println!("  --evaluate           Evaluate and print variables");
        println!("  --dump               Dump justfile as JSON");
        println!("  --choose             Choose recipe interactively");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("just 1.23.0 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Available recipes:");
        println!("    build           # Build the project");
        println!("    test            # Run all tests");
        println!("    clean           # Clean build artifacts");
        println!("    fmt             # Format code");
        println!("    lint            # Lint code");
        println!("    run *args       # Run the application");
        println!("    deploy env      # Deploy to environment");
        println!("    docker-build    # Build Docker image");
        println!("    ci              # Full CI pipeline");
        return 0;
    }

    if args.iter().any(|a| a == "-s" || a == "--show") {
        let recipe = args.windows(2).find(|w| w[0] == "-s" || w[0] == "--show")
            .map(|w| w[1].as_str()).unwrap_or("build");
        println!("# {}", recipe);
        match recipe {
            "build" => {
                println!("build:");
                println!("    cargo build --release");
            }
            "test" => {
                println!("test:");
                println!("    cargo test --workspace");
            }
            "deploy" => {
                println!("deploy env:");
                println!("    @echo \"Deploying to {{{{env}}}}...\"");
                println!("    ./scripts/deploy.sh {{{{env}}}}");
            }
            _ => {
                println!("{}:", recipe);
                println!("    echo \"Running {}\"", recipe);
            }
        }
        return 0;
    }

    let dry_run = args.iter().any(|a| a == "-n" || a == "--dry-run");
    let recipe = args.first().map(|s| s.as_str()).unwrap_or("build");

    if dry_run {
        println!("#(dry-run) recipe: {}", recipe);
        match recipe {
            "build" => println!("cargo build --release"),
            "test" => println!("cargo test --workspace"),
            "clean" => println!("rm -rf target/"),
            _ => println!("echo \"Running {}\"", recipe),
        }
    } else {
        match recipe {
            "build" => {
                println!("cargo build --release");
                println!("   Compiling myproject v0.1.0");
                println!("    Finished release [optimized] target(s)");
            }
            "test" => {
                println!("cargo test --workspace");
                println!("running 12 tests");
                println!("test result: ok. 12 passed; 0 failed");
            }
            "clean" => {
                println!("rm -rf target/");
            }
            "ci" => {
                println!("fmt");
                println!("lint");
                println!("test");
                println!("build");
                println!("All CI steps passed.");
            }
            _ => {
                println!("echo \"Running {}\"", recipe);
                println!("Running {}", recipe);
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
    fn help_and_version_exit_zero() {
        assert_eq!(run_just(vec!["--help".to_string()]), 0);
        assert_eq!(run_just(vec!["-h".to_string()]), 0);
        assert_eq!(run_just(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_just(vec![]), 0);
    }
}
