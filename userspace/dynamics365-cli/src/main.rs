#![deny(clippy::all)]

//! dynamics365-cli — OurOS Microsoft Dynamics 365 ERP/CRM
//!
//! Single personality: `dynamics365`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_d365(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dynamics365 [OPTIONS]");
        println!("Microsoft Dynamics 365 (OurOS) — Modular cloud ERP/CRM + Power Platform");
        println!();
        println!("Options:");
        println!("  --app APP              Sales/Customer Service/Field Service/Marketing/...");
        println!("  --finance              D365 Finance (formerly AX)");
        println!("  --supply-chain         D365 Supply Chain Management");
        println!("  --business-central     D365 Business Central (formerly NAV)");
        println!("  --pac                  Power Platform CLI (pac)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft Dynamics 365 (2024 Wave 2) + pac CLI 1.34 (OurOS)"); return 0; }
    println!("Microsoft Dynamics 365 (2024 Wave 2) (OurOS)");
    println!("  CRM apps: Sales, Customer Service, Field Service, Marketing, Project Ops");
    println!("  ERP apps: Finance, Supply Chain Mgmt, Commerce, HR, Project Operations");
    println!("  SMB ERP: Business Central (formerly Navision/NAV)");
    println!("  Foundation: Dataverse (formerly Common Data Service), Power Platform");
    println!("  Power Platform: Power Apps, Power Automate, Power BI, Power Pages, Copilot");
    println!("  Language: X++ (Finance/SCM kernel), C# plugins, JS web resources, Power Fx");
    println!("  AI: Copilot in every app, Sales Copilot, Customer Service Copilot");
    println!("  Integration: Microsoft 365, Azure, Teams (deeply embedded)");
    println!("  Releases: 2 waves/year (Spring/Fall), 2-month preview windows");
    println!("  License: per-user (Sales/Service), Enterprise/Professional, base + attach");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dynamics365".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_d365(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_d365};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dynamics365"), "dynamics365");
        assert_eq!(basename(r"C:\bin\dynamics365.exe"), "dynamics365.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dynamics365.exe"), "dynamics365");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_d365(&["--help".to_string()], "dynamics365"), 0);
        assert_eq!(run_d365(&["-h".to_string()], "dynamics365"), 0);
        assert_eq!(run_d365(&["--version".to_string()], "dynamics365"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_d365(&[], "dynamics365"), 0);
    }
}
