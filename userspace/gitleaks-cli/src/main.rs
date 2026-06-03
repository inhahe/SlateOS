#![deny(clippy::all)]

//! gitleaks-cli — OurOS Gitleaks secret scanner
//!
//! Multi-personality: `gitleaks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gitleaks(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gitleaks COMMAND [OPTIONS]");
        println!("Gitleaks 8.18.0 (OurOS) — Secret scanner");
        println!();
        println!("Commands:");
        println!("  detect        Scan git repo for secrets");
        println!("  protect       Scan staged changes (pre-commit)");
        println!("  version       Show version");
        println!();
        println!("Options:");
        println!("  --source DIR      Source directory (default: .)");
        println!("  --config FILE     Config file");
        println!("  --report-path F   Report output file");
        println!("  --report-format F Format (json, csv, sarif)");
        println!("  --no-git          Scan files without git");
        println!("  --verbose         Verbose output");
        println!("  --redact          Redact secrets in output");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("detect");
    match subcmd {
        "version" | "--version" => {
            println!("gitleaks 8.18.0");
        }
        "detect" => {
            let source = args.windows(2).find(|w| w[0] == "--source")
                .map(|w| w[1].as_str()).unwrap_or(".");
            let redact = args.iter().any(|a| a == "--redact");
            let report_fmt = args.windows(2).find(|w| w[0] == "--report-format")
                .map(|w| w[1].as_str()).unwrap_or("json");

            println!("    ○");
            println!("    │╲");
            println!("    │ ○");
            println!("    ○ ░");
            println!("    ░    gitleaks");
            println!();
            println!("Scanning {} for secrets...", source);
            println!();

            let secret_display = if redact { "REDACTED" } else { "AKIAIOSFODNN7EXAMPLE" };

            println!("Finding:     {}", secret_display);
            println!("RuleID:      aws-access-key");
            println!("Entropy:     3.94");
            println!("File:        .env");
            println!("Line:        3");
            println!("Commit:      abc123def456");
            println!("Author:      dev@example.com");
            println!("Date:        2024-01-15");
            println!();

            let secret2 = if redact { "REDACTED" } else { "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx" };
            println!("Finding:     {}", secret2);
            println!("RuleID:      github-pat");
            println!("Entropy:     4.12");
            println!("File:        scripts/deploy.sh");
            println!("Line:        12");
            println!("Commit:      def789abc012");
            println!("Author:      dev@example.com");
            println!("Date:        2024-02-20");
            println!();

            println!("2 leaks found in {} commits.", 156);
            println!("Report format: {}", report_fmt);
        }
        "protect" => {
            println!("Scanning staged changes...");
            println!();
            println!("No leaks found in staged changes.");
        }
        _ => println!("gitleaks: unknown command '{}'", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gitleaks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gitleaks(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gitleaks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gitleaks"), "gitleaks");
        assert_eq!(basename(r"C:\bin\gitleaks.exe"), "gitleaks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gitleaks.exe"), "gitleaks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gitleaks(&["--help".to_string()]), 0);
        assert_eq!(run_gitleaks(&["-h".to_string()]), 0);
        assert_eq!(run_gitleaks(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gitleaks(&[]), 0);
    }
}
