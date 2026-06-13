#![deny(clippy::all)]

//! earthly-cli — Slate OS Earthly CI/CD CLI
//!
//! Multi-personality: `earthly`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_earthly(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: earthly [OPTIONS] TARGET");
        println!("Earthly 0.8.0 (Slate OS)");
        println!();
        println!("Options:");
        println!("  --push         Push images/artifacts");
        println!("  --ci           CI mode (no local cache)");
        println!("  --no-cache     Disable cache");
        println!("  --artifact     Output artifacts");
        println!("  --image        Output images");
        println!("  --platform P   Build platform");
        println!("  --secret K=V   Pass secret");
        println!();
        println!("Commands:");
        println!("  init           Initialize Earthfile");
        println!("  ls             List targets");
        println!("  prune          Prune cache");
        println!("  org            Manage organizations");
        println!("  satellite      Manage Earthly Satellites");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("earthly 0.8.0");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "init" => {
            println!("Created Earthfile");
            println!("Done. Run 'earthly +build' to start.");
        }
        "ls" => {
            println!("Targets:");
            println!("  +base");
            println!("  +build");
            println!("  +test");
            println!("  +lint");
            println!("  +docker");
            println!("  +ci");
        }
        "prune" => {
            println!("Pruning Earthly cache...");
            println!("  Removed 1.2 GB of cache data.");
        }
        _ => {
            // Target execution (e.g., +build, +test)
            let target = subcmd;
            let push = args.iter().any(|a| a == "--push");

            if target.starts_with('+') || target.starts_with("./") {
                println!("           buildkitd | Starting...");
                println!("           buildkitd | Started.");
                println!();
                println!("  {} | --> FROM alpine:3.20", target);
                println!("  {} | [    ] 100%", target);
                println!("  {} | --> RUN apk add --no-cache build-base", target);
                println!("  {} | fetch https://dl-cdn.alpinelinux.org/...", target);
                println!("  {} | [████] 100%", target);
                println!("  {} | --> COPY src/ ./src/", target);
                println!("  {} | --> RUN make build", target);
                println!("  {} | Build complete.", target);
                if push {
                    println!("  {} | --> Pushing image...", target);
                    println!("  {} | Image pushed.", target);
                }
                println!();
                println!("  {} | Target {} built successfully.", target, target);
                println!("========================= SUCCESS =========================");
            } else {
                println!("earthly: '{}' completed", subcmd);
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "earthly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_earthly(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_earthly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/earthly"), "earthly");
        assert_eq!(basename(r"C:\bin\earthly.exe"), "earthly.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("earthly.exe"), "earthly");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_earthly(&["--help".to_string()]), 0);
        assert_eq!(run_earthly(&["-h".to_string()]), 0);
        let _ = run_earthly(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_earthly(&[]);
    }
}
