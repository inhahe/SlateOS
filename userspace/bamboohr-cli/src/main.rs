#![deny(clippy::all)]

//! bamboohr-cli — OurOS BambooHR (people-data HRIS focused on SMB, Utah-based)
//!
//! Single personality: `bamboohr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bhr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bamboohr [OPTIONS]");
        println!("BambooHR (OurOS) — HRIS for small/medium business");
        println!();
        println!("Options:");
        println!("  --essentials           Essentials tier (per-employee/mo)");
        println!("  --advantage            Advantage tier (recruiting, performance, training)");
        println!("  --payroll              BambooHR Payroll (US only)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("BambooHR 2024 (OurOS)"); return 0; }
    println!("BambooHR 2024 (OurOS)");
    println!("  Vendor: Bamboo HR LLC (Lindon, Utah — founded 2008)");
    println!("  Founders: Ben Peterson + Ryan Sanders (in Ben's basement)");
    println!("           bootstrapped — no VC funding for first decade");
    println!("           accepted growth investment from Vista Equity 2019 (~$200M est.)");
    println!("  Strategy: 'one source of truth for employee data' — HRIS first, payroll/benefits later");
    println!("           Utah 'Silicon Slopes' culture, employee-centric branding");
    println!("  Scale: 30,000+ companies, 3 million+ employees");
    println!("        sweet spot: 25-1000 employees");
    println!("        ~1,500 internal employees");
    println!("  Pricing: per-employee/mo, undisclosed publicly — typically $5-10/employee/mo Essentials");
    println!("          Advantage tier: ~$8-12/employee/mo (adds applicant tracking + performance)");
    println!("          Payroll add-on: ~$5/employee/mo + base fee (US only, all 50 states)");
    println!("  Core features:");
    println!("    - Employee records (master file with custom fields, history, document storage)");
    println!("    - Org charts + people directory + 'Who's Out' calendar");
    println!("    - PTO + holiday tracking with approval workflows");
    println!("    - Onboarding workflows + e-signature offer letters");
    println!("    - Offboarding workflows + alumni tracking");
    println!("    - Performance management (goals + reviews + 360s)");
    println!("    - Applicant Tracking System (ATS) — careers page, candidate pipeline");
    println!("    - eNPS surveys, employee satisfaction tracking, employee Net Promoter Score");
    println!("    - Compensation tracking + salary band visualization");
    println!("    - Time tracking + timesheets");
    println!("    - Reports + workforce analytics (turnover, headcount, retention)");
    println!("  Marketplace: ~125 integrations (Slack, Greenhouse, Lever, Calendly, payroll vendors)");
    println!("  Brand: 'Set People Free' tagline, strong Utah 'family-friendly tech' culture");
    println!("  Critique: payroll is recent add-on (vs ADP/Paychex/Gusto's deep US payroll DNA)");
    println!("           weak in benefits administration vs Gusto/Rippling");
    println!("           sales-led pricing (no public pricing) feels behind transparent competitors");
    println!("  Differentiator: cleanest HRIS UX in SMB market, employee-self-serve, fast onboarding");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bamboohr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bhr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bhr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bamboohr"), "bamboohr");
        assert_eq!(basename(r"C:\bin\bamboohr.exe"), "bamboohr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bamboohr.exe"), "bamboohr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bhr(&["--help".to_string()], "bamboohr"), 0);
        assert_eq!(run_bhr(&["-h".to_string()], "bamboohr"), 0);
        let _ = run_bhr(&["--version".to_string()], "bamboohr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bhr(&[], "bamboohr");
    }
}
