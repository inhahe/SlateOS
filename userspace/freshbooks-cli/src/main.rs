#![deny(clippy::all)]

//! freshbooks-cli — OurOS FreshBooks (Toronto-founded invoicing & accounting for freelancers/agencies)
//!
//! Single personality: `freshbooks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: freshbooks [OPTIONS]");
        println!("FreshBooks (OurOS) — Cloud accounting for freelancers, agencies, service businesses");
        println!();
        println!("Options:");
        println!("  --lite                 Lite ($21/mo — 5 clients)");
        println!("  --plus                 Plus ($38/mo — 50 clients, double-entry)");
        println!("  --premium              Premium ($65/mo — unlimited clients)");
        println!("  --select               Select (custom — multi-user, dedicated AM)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FreshBooks 2024 (OurOS)"); return 0; }
    println!("FreshBooks 2024 (OurOS)");
    println!("  Vendor: 2ndSite Inc. dba FreshBooks (Toronto, Canada — founded 2003)");
    println!("  Founder: Mike McDerment (Toronto agency founder)");
    println!("          built FreshBooks because he accidentally invoiced a client over old QuickBooks → wanted simpler tool");
    println!("  History: built quietly for ~10 years, profitable, no VC for first decade");
    println!("          accepted growth investment 2015, ~$80M Series B 2018");
    println!("          2017 rebuild ('FreshBooks Classic' → 'New FreshBooks') controversial — lost some loyal users");
    println!("  Scale: 30M+ users across 160 countries lifetime");
    println!("        ~500 employees in Toronto");
    println!("  Strategy: freelancer + small service business focus");
    println!("           NOT a full accounting suite — invoice-centric, with bookkeeping added");
    println!("  Pricing: Lite $21, Plus $38, Premium $65, Select custom");
    println!("          Lite caps at 5 billable clients (push to Plus); Plus at 50");
    println!("  Features:");
    println!("    - Invoicing (the killer feature — beautiful, customizable, branded templates)");
    println!("    - Recurring invoices + late-payment reminders + late fees");
    println!("    - Online payments (Stripe + WePay + ACH)");
    println!("    - Time tracking (timer + per-project + billable hours auto-rolled to invoice)");
    println!("    - Expense tracking + mileage tracking (auto via mobile GPS)");
    println!("    - Project budgeting + collaboration");
    println!("    - Proposals + estimates → invoices");
    println!("    - Client retainers + advance billing");
    println!("    - Double-entry accounting (added 2018 — Plus and above)");
    println!("    - Bank feeds + reconciliation (Plus+)");
    println!("    - Financial reports (P&L, sales tax summary, expense reports)");
    println!("    - Mobile app with receipt capture + voice-to-invoice");
    println!("  Customers: independent contractors, lawyers, consultants, photographers, small agencies");
    println!("  Critique: weaker accounting depth vs QuickBooks/Xero (no inventory, weak fixed assets)");
    println!("           Lite tier's 5-client cap is aggressive");
    println!("  Differentiator: best invoicing UX in the market + time tracking → invoice loop for service businesses");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "freshbooks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/freshbooks"), "freshbooks");
        assert_eq!(basename(r"C:\bin\freshbooks.exe"), "freshbooks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("freshbooks.exe"), "freshbooks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fb(&["--help".to_string()], "freshbooks"), 0);
        assert_eq!(run_fb(&["-h".to_string()], "freshbooks"), 0);
        let _ = run_fb(&["--version".to_string()], "freshbooks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fb(&[], "freshbooks");
    }
}
