#![deny(clippy::all)]

//! looker-cli — SlateOS Google Cloud Looker BI
//!
//! Single personality: `looker`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_looker(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: looker [OPTIONS] [SUBCMD]");
        println!("Google Cloud Looker 24.18 (Slate OS) — Modern BI + embedded analytics");
        println!();
        println!("Options:");
        println!("  --instance URL         Looker instance URL");
        println!("  --client-id ID         API client ID");
        println!("  --client-secret SEC    API client secret");
        println!("  lookml-validate        Validate LookML model");
        println!("  --studio               Looker Studio (formerly Data Studio)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Google Cloud Looker 24.18 (Slate OS)"); return 0; }
    println!("Google Cloud Looker 24.18 (Slate OS)");
    println!("  Products: Looker (governed BI), Looker Studio (free self-serve)");
    println!("  LookML: declarative semantic modeling language (Looker Markup Language)");
    println!("  Architecture: in-database analytics — queries pushed to warehouse");
    println!("  Connectors: 50+ DB dialects via JDBC (BigQuery first-class)");
    println!("  Development: Git-versioned LookML projects, IDE in browser");
    println!("  Embedded: powered embed, signed embed, private embed, iframe");
    println!("  API: REST + SDK (Python/TypeScript/Kotlin/Swift/Go/Ruby/Java)");
    println!("  Integration: Google Cloud, BigQuery (native), Vertex AI, Gemini");
    println!("  License: per-user (Standard/Premium/Enterprise) + capacity");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "looker".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_looker(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_looker};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/looker"), "looker");
        assert_eq!(basename(r"C:\bin\looker.exe"), "looker.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("looker.exe"), "looker");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_looker(&["--help".to_string()], "looker"), 0);
        assert_eq!(run_looker(&["-h".to_string()], "looker"), 0);
        let _ = run_looker(&["--version".to_string()], "looker");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_looker(&[], "looker");
    }
}
