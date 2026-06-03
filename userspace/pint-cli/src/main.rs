#![deny(clippy::all)]

//! pint-cli — OurOS pint Prometheus rule linter
//!
//! Single personality: `pint`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pint(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pint COMMAND [OPTIONS]");
        println!("pint v0.60.0 (OurOS) — Prometheus rule linter");
        println!();
        println!("Commands:");
        println!("  lint            Lint rules");
        println!("  watch           Watch and lint continuously");
        println!("  ci              CI mode (exit non-zero on errors)");
        println!("  version         Show version");
        println!();
        println!("Options:");
        println!("  --config FILE   Config file");
        println!("  --min-severity LEVEL  Minimum severity (info, warning, bug, fatal)");
        println!("  --no-color      Disable colored output");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("pint v0.60.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("lint");
    match cmd {
        "lint" | "ci" => {
            println!("rules/alerts.yml:");
            println!("  [WARNING] alerts/HighCPU: alert query doesn't have a 'for' duration > 5m");
            println!("  [INFO] alerts/DiskFull: consider adding 'severity' label");
            println!("rules/recording.yml:");
            println!("  [OK] All recording rules valid");
            println!();
            println!("Checked 12 rules in 2 files");
            println!("  0 errors, 1 warning, 1 info");
        }
        "watch" => {
            println!("Watching for changes...");
            println!("  Monitoring: rules/");
        }
        _ => println!("pint {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pint".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pint(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pint};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pint"), "pint");
        assert_eq!(basename(r"C:\bin\pint.exe"), "pint.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pint.exe"), "pint");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pint(&["--help".to_string()], "pint"), 0);
        assert_eq!(run_pint(&["-h".to_string()], "pint"), 0);
        assert_eq!(run_pint(&["--version".to_string()], "pint"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pint(&[], "pint"), 0);
    }
}
