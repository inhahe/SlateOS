#![deny(clippy::all)]

//! coda-cli — OurOS Coda doc-as-app platform
//!
//! Single personality: `coda`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_coda(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: coda [OPTIONS]");
        println!("Coda (OurOS) — Doc-as-app: docs + tables + formulas + automations");
        println!();
        println!("Options:");
        println!("  --doc NAME             Open Coda doc");
        println!("  --pack NAME            Install/use a Coda Pack (integration)");
        println!("  --formula              Coda formula reference");
        println!("  --plan PLAN            free/pro/team/enterprise");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Coda 1.124.0 (OurOS)"); return 0; }
    println!("Coda 1.124.0 (OurOS)");
    println!("  Vendor: Coda Project, Inc. (Mountain View / Bellevue, founded 2014)");
    println!("  Founders: Shishir Mehrotra (ex-YouTube), Alex DeNeui");
    println!("  Pitch: 'A new doc that brings words, data, and teams together'");
    println!("  Building blocks: pages, tables, controls, formulas, buttons, automations");
    println!("  Formula language: spreadsheet-like, references tables/columns, 400+ functions");
    println!("  Packs: 600+ integrations (Slack, Jira, GitHub, Gmail, Salesforce, Figma...)");
    println!("  Plans: Free (unlimited docs, 1GB), Pro ($12/Doc Maker/mo, full versioning)");
    println!("         Team ($36/Doc Maker/mo, cross-doc, no row limits)");
    println!("         Enterprise (custom, SSO, audit, advanced controls)");
    println!("  Pricing twist: only 'Doc Makers' are charged, editors/viewers free");
    println!("  AI: Coda AI ($12/maker/mo add-on) — writing assist, doc Q&A, automation");
    println!("  Mobile: iOS/Android apps, web is primary");
    println!("  Competitors: Notion, Airtable (overlap with both), Confluence");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "coda".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_coda(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_coda};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/coda"), "coda");
        assert_eq!(basename(r"C:\bin\coda.exe"), "coda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("coda.exe"), "coda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_coda(&["--help".to_string()], "coda"), 0);
        assert_eq!(run_coda(&["-h".to_string()], "coda"), 0);
        let _ = run_coda(&["--version".to_string()], "coda");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_coda(&[], "coda");
    }
}
