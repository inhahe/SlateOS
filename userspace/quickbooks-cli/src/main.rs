#![deny(clippy::all)]

//! quickbooks-cli — OurOS Intuit QuickBooks accounting
//!
//! Single personality: `quickbooks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: quickbooks [OPTIONS] [FILE]");
        println!("Intuit QuickBooks Desktop Enterprise 24.0 / QuickBooks Online (OurOS)");
        println!();
        println!("Options:");
        println!("  --online               QuickBooks Online (cloud)");
        println!("  --desktop FILE         Desktop .QBW company file");
        println!("  --edition ED           Pro/Premier/Enterprise/Accountant/Online");
        println!("  --iif FILE             Import IIF transaction file");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Intuit QuickBooks Desktop Enterprise Solutions 24.0 (OurOS)"); return 0; }
    println!("Intuit QuickBooks Desktop Enterprise Solutions 24.0 (OurOS)");
    println!("  Editions: Online (QBO), Desktop Pro/Premier/Enterprise/Accountant");
    println!("  QBO Tiers: Simple Start, Essentials, Plus, Advanced, Self-Employed");
    println!("  Features: Invoicing, Bills, Banking, Payroll, Inventory, Reports, 1099");
    println!("  Format: .QBW (company file), .QBB (backup), .IIF (text import), .QBO/.QFX");
    println!("  Integrations: QuickBooks Payments, Payroll, Time (TSheets), Capital");
    println!("  Apps: 750+ app marketplace (CRM, e-commerce, project mgmt, time tracking)");
    println!("  API: REST API v3 (QBO), SDK (Desktop) for ISV apps");
    println!("  License: subscription (QBO) / annual upgrades (Enterprise) — SMB dominant");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "quickbooks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/quickbooks"), "quickbooks");
        assert_eq!(basename(r"C:\bin\quickbooks.exe"), "quickbooks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("quickbooks.exe"), "quickbooks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qb(&["--help".to_string()], "quickbooks"), 0);
        assert_eq!(run_qb(&["-h".to_string()], "quickbooks"), 0);
        let _ = run_qb(&["--version".to_string()], "quickbooks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qb(&[], "quickbooks");
    }
}
