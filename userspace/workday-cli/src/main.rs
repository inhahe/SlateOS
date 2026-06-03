#![deny(clippy::all)]

//! workday-cli — OurOS Workday HCM + Financials
//!
//! Single personality: `workday`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: workday [OPTIONS]");
        println!("Workday 2024R2 (OurOS) — Cloud HCM + Financial Management");
        println!();
        println!("Options:");
        println!("  --tenant TENANT        Workday tenant name");
        println!("  --report ID            Run report");
        println!("  --eib FILE             Enterprise Interface Builder (data load)");
        println!("  --studio               Workday Studio (integration IDE)");
        println!("  --prism                Workday Prism Analytics");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Workday 2024R2 (Release 2024R2) (OurOS)"); return 0; }
    println!("Workday 2024R2 (OurOS)");
    println!("  Suites: Human Capital Management (HCM), Financial Management, Planning,");
    println!("          Spend Management, Adaptive Planning (FP&A), Peakon (engagement)");
    println!("  Architecture: object-based, in-memory, multi-tenant SaaS");
    println!("  Customer experience: 'Power of One' — single codeline for all customers");
    println!("  Updates: 2 major releases/year (R1/R2), weekly service updates");
    println!("  Extension: Workday Extend (custom apps), Studio (integrations)");
    println!("  AI: Workday AI (built-in), Workday Illuminate (GenAI roadmap)");
    println!("  Reporting: Custom reports, Worksheets, Discovery Boards, Prism Analytics");
    println!("  License: per-employee subscription (HCM), revenue-based (Financials)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "workday".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/workday"), "workday");
        assert_eq!(basename(r"C:\bin\workday.exe"), "workday.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("workday.exe"), "workday");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_wd(&["--help".to_string()], "workday"), 0);
        assert_eq!(run_wd(&["-h".to_string()], "workday"), 0);
        assert_eq!(run_wd(&["--version".to_string()], "workday"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_wd(&[], "workday"), 0);
    }
}
