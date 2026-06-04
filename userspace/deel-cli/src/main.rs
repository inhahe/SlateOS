#![deny(clippy::all)]

//! deel-cli — OurOS Deel (global hiring / EOR / contractor payments)
//!
//! Single personality: `deel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_deel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: deel [OPTIONS]");
        println!("Deel (OurOS) — Global hiring + payroll + contractors + EOR");
        println!();
        println!("Options:");
        println!("  --contractor           Contractor management ($49/contractor/mo)");
        println!("  --eor                  Employer of Record (~$599/employee/mo)");
        println!("  --global-payroll       Direct global payroll");
        println!("  --hr                   Deel HR (free HRIS for all customers)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Deel 2024 (OurOS)"); return 0; }
    println!("Deel 2024 (OurOS)");
    println!("  Vendor: Deel Inc. (San Francisco — founded 2019)");
    println!("  Founders: Alex Bouaziz + Shuo Wang (MIT)");
    println!("  Funding: Andreessen Horowitz, Spark Capital, Coatue + others");
    println!("          $679M+ raised, $12.1B valuation 2023");
    println!("  Strategy: 'pay anyone, anywhere, in any currency, compliantly'");
    println!("           ride the post-COVID remote-work wave");
    println!("  Growth: $1M ARR in 2020 → $100M in 2021 → $500M+ in 2023");
    println!("         one of the fastest-growing SaaS companies in history");
    println!("  Scale: 25,000+ customers, ~3,500 employees, 150+ countries supported");
    println!("        Deel itself is fully remote — operating in every country it supports");
    println!("  Pricing:");
    println!("    Contractor management: $49/contractor/mo (mass contractor payments + agreements + tax forms)");
    println!("    EOR (Employer of Record): from $599/employee/mo (Deel becomes legal employer abroad)");
    println!("    Global Payroll: custom (direct payroll where Deel has own entities)");
    println!("    Deel HR: FREE forever (HRIS up to 200 employees) — gateway-drug strategy");
    println!("  Killer features:");
    println!("    - 150+ countries — locally-compliant employment agreements out-of-box");
    println!("    - Withdraw earnings in 15+ currencies + crypto (USDC) + Wise/PayPal/bank transfer");
    println!("    - Tax forms generation: W-9/W-8BEN (US), 1099, equivalents in every country");
    println!("    - Misclassification risk assessment (AI-based contractor-vs-employee classifier)");
    println!("    - Equipment shipping / visa support / relocation services");
    println!("    - Background checks in 200+ countries");
    println!("  Rippling lawsuit: 2024 — Rippling sued Deel for paying a Rippling employee");
    println!("                    to steal Rippling's customer + sales data");
    println!("                    Deel denies, high-profile HR-tech rivalry");
    println!("  Critique: aggressive sales culture, customer service complaints at scale");
    println!("           EOR pricing high; product breadth (HR/payroll) less mature than dedicated tools");
    println!("  Differentiator: largest global EOR + contractor coverage — global hiring in 150+ countries");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "deel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_deel(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_deel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/deel"), "deel");
        assert_eq!(basename(r"C:\bin\deel.exe"), "deel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("deel.exe"), "deel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_deel(&["--help".to_string()], "deel"), 0);
        assert_eq!(run_deel(&["-h".to_string()], "deel"), 0);
        let _ = run_deel(&["--version".to_string()], "deel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_deel(&[], "deel");
    }
}
