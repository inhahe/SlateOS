#![deny(clippy::all)]

//! zoho-cli — OurOS Zoho One business operating system
//!
//! Single personality: `zoho`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zoho(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zoho [OPTIONS]");
        println!("Zoho One (OurOS) — All-in-one business operating system");
        println!();
        println!("Options:");
        println!("  --app NAME             crm/mail/writer/sheet/show/projects/books/people");
        println!("  --crm                  Zoho CRM (Bigin/Standard/Plus/Enterprise/Ultimate)");
        println!("  --workplace            Zoho Workplace (Mail + Office suite)");
        println!("  --one                  Zoho One bundle (45+ apps, $37 per user/mo)");
        println!("  --zia                  Zia AI assistant");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zoho One 2024.11 (OurOS)"); return 0; }
    println!("Zoho One (OurOS)");
    println!("  Vendor: Zoho Corporation (Chennai, India + Pleasanton, CA)");
    println!("  Founded: 1996 (originally AdventNet); rebranded Zoho 2009");
    println!("  Privately held — no IPO, no outside investors (Sridhar Vembu, founder/CEO)");
    println!("  Catalog: 55+ apps across sales/marketing/email/finance/HR/IT/BI");
    println!("  Sales/CRM: CRM, Bigin, Bookings, CommercePlus, Forms");
    println!("  Marketing: Campaigns, MarketingHub, Social, Survey, SalesIQ, PageSense");
    println!("  Email/collab: Mail, Workplace, Cliq, Meeting, Connect, Notebook, WorkDrive");
    println!("  Office: Writer, Sheet, Show, Notebook, Sign — full Office alternative");
    println!("  Finance: Books, Invoice, Inventory, Expense, Subscriptions, Payroll, Checkout");
    println!("  HR: People, Recruit, Workerly");
    println!("  Dev/IT: Creator (low-code), Catalyst, Analytics, Desk, Assist");
    println!("  Zoho One: entire suite for $37/user/mo (flexible) or $105 (all-employee)");
    println!("  Stance: pro-privacy, anti-VC, 'rural revival' (offices in Indian villages)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zoho".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zoho(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zoho};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zoho"), "zoho");
        assert_eq!(basename(r"C:\bin\zoho.exe"), "zoho.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zoho.exe"), "zoho");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zoho(&["--help".to_string()], "zoho"), 0);
        assert_eq!(run_zoho(&["-h".to_string()], "zoho"), 0);
        let _ = run_zoho(&["--version".to_string()], "zoho");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zoho(&[], "zoho");
    }
}
