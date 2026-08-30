#![deny(clippy::all)]

//! salesforce-cli — Slate OS Salesforce CRM platform (sf / sfdx CLI)
//!
//! Single personality: `salesforce`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: salesforce [OPTIONS] [SUBCMD]");
        println!("Salesforce Platform (Slate OS) — CRM + Lightning Platform");
        println!();
        println!("Options:");
        println!("  sf org login web       Authenticate to org");
        println!("  sf project deploy start Deploy metadata");
        println!("  sf data query          Run SOQL query");
        println!("  sf apex run            Run anonymous Apex");
        println!("  --lwc                  Lightning Web Components CLI");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Salesforce CLI sf v2.69.6 / Spring '25 release (Slate OS)"); return 0; }
    println!("Salesforce Platform (Slate OS)");
    println!("  Clouds: Sales, Service, Marketing (Pardot), Commerce, Experience,");
    println!("          Industries, Data Cloud, Slack, Mulesoft, Tableau, Heroku");
    println!("  Platform: Lightning (Aura + LWC), Visualforce (legacy), Apex (Java-like)");
    println!("  Language: Apex (server), Lightning Web Components (LWC, JS), SOQL/SOSL");
    println!("  Einstein: AI features (Einstein GPT, Copilot, Prompt Builder)");
    println!("  Releases: 3/year (Spring/Summer/Winter), every org auto-updated");
    println!("  DX: Salesforce CLI (sf), VS Code extensions, Scratch Orgs, source tracking");
    println!("  Marketplace: AppExchange (10000+ apps)");
    println!("  License: per-user (Essentials/Pro/Enterprise/Unlimited/Industries)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "salesforce".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/salesforce"), "salesforce");
        assert_eq!(basename(r"C:\bin\salesforce.exe"), "salesforce.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("salesforce.exe"), "salesforce");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sf(&["--help".to_string()], "salesforce"), 0);
        assert_eq!(run_sf(&["-h".to_string()], "salesforce"), 0);
        let _ = run_sf(&["--version".to_string()], "salesforce");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sf(&[], "salesforce");
    }
}
