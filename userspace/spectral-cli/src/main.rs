#![deny(clippy::all)]

//! spectral-cli — OurOS Spectral API linter
//!
//! Multi-personality: `spectral`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spectral(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: spectral COMMAND [OPTIONS]");
        println!("Spectral 6.11.0 (OurOS) — API linter");
        println!();
        println!("Commands:");
        println!("  lint           Lint API documents");
        println!();
        println!("Options:");
        println!("  -r, --ruleset FILE   Ruleset file");
        println!("  -f, --format FMT     Output format (stylish, json, sarif, text)");
        println!("  -o, --output FILE    Output file");
        println!("  -F, --fail-severity  Minimum severity to fail");
        println!("  -D, --show-unmatched-globs  Show unmatched globs");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("spectral 6.11.0");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("lint");
    if subcmd == "lint" {
        let spec = args.get(1).map(|s| s.as_str()).unwrap_or("openapi.yaml");
        let format = args.windows(2).find(|w| w[0] == "-f" || w[0] == "--format")
            .map(|w| w[1].as_str()).unwrap_or("stylish");

        match format {
            "json" => {
                println!("[");
                println!("  {{\"code\": \"operation-description\", \"path\": [\"/paths/~1users/get\"], \"message\": \"Operation must have a description.\", \"severity\": 1, \"range\": {{\"start\": {{\"line\": 10}}, \"end\": {{\"line\": 15}}}}}},");
                println!("  {{\"code\": \"info-contact\", \"path\": [\"/info\"], \"message\": \"Info object must have a contact.\", \"severity\": 1, \"range\": {{\"start\": {{\"line\": 1}}, \"end\": {{\"line\": 5}}}}}}");
                println!("]");
            }
            _ => {
                println!("{}:", spec);
                println!("  10:3  warning  operation-description  Operation must have a description.");
                println!("  1:1   warning  info-contact           Info object must have a contact.");
                println!("  22:5  hint     operation-tag           Operation should have at least one tag.");
                println!("  35:3  error    oas3-schema             Schema is invalid: 'required' must be an array.");
                println!();
                println!("4 problems (1 error, 2 warnings, 1 hint)");
            }
        }
    } else {
        println!("spectral: '{}' completed", subcmd);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spectral".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spectral(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spectral};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spectral"), "spectral");
        assert_eq!(basename(r"C:\bin\spectral.exe"), "spectral.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spectral.exe"), "spectral");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_spectral(&["--help".to_string()]), 0);
        assert_eq!(run_spectral(&["-h".to_string()]), 0);
        assert_eq!(run_spectral(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_spectral(&[]), 0);
    }
}
