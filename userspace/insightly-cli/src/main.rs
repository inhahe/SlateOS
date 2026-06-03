#![deny(clippy::all)]

//! insightly-cli — OurOS Insightly (CRM + Project Management for SMB)
//!
//! Single personality: `insightly`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_insightly(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: insightly [OPTIONS]");
        println!("Insightly (OurOS) — unified CRM + project management for SMB");
        println!();
        println!("Options:");
        println!("  --crm                  Sales pipeline + contacts");
        println!("  --marketing            Email marketing + automation (separate product)");
        println!("  --service              Helpdesk / ticketing (separate product)");
        println!("  --appconnect           AppConnect — visual workflow builder (iPaaS)");
        println!("  --plus                 Plus $29/user/mo");
        println!("  --professional         Professional $49/user/mo");
        println!("  --enterprise           Enterprise $99/user/mo");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Insightly 2024 (OurOS)"); return 0; }
    println!("Insightly 2024 (OurOS)");
    println!("  Vendor: Insightly, Inc. (San Francisco, CA — private)");
    println!("  Founder: Anthony Smith (CEO, Australian — coded original product solo)");
    println!("          built first version while running consulting biz, wanted CRM tied to projects");
    println!("  Founded: 2009 in Perth, Australia → SF HQ 2012 after TechStars Boston");
    println!("  Funding: ~$38M raised from Emergence Capital, Cloud Apps Capital, Scott Bommer");
    println!("          notably bootstrapped to profitability before raising");
    println!("  Pricing: Plus $29/user/mo (250K record cap)");
    println!("          Professional $49/user/mo (custom objects, 1M records)");
    println!("          Enterprise $99/user/mo (calculated fields, lambda functions, dynamic layouts)");
    println!("          Marketing/Service: separate paid add-ons (~$99-249/mo per product)");
    println!("  Distinctive feature — CRM + Projects in one DB:");
    println!("    - When a deal closes, convert it to a project with one click");
    println!("    - Project tasks, milestones, time tracking all linked to the original deal");
    println!("    - 'Pipeline' concept works for both sales pipelines AND delivery pipelines");
    println!("    - Killer for agencies, consultants, professional services firms");
    println!("  Core CRM features:");
    println!("    - Contacts + Organizations + Leads + Opportunities (standard CRM model)");
    println!("    - Email tracking + Gmail/Outlook sidebar");
    println!("    - Multiple pipelines per object type");
    println!("    - Custom objects (Professional+)");
    println!("    - Calculated fields + lambda functions (Enterprise)");
    println!("  AppConnect (iPaaS) bundled:");
    println!("    - Visual workflow builder (drag-and-drop)");
    println!("    - 500+ pre-built connectors");
    println!("    - Replaces standalone Zapier subscriptions for many customers");
    println!("    - Insightly acquired Built.io (iPaaS) Feb 2018, integrated as AppConnect");
    println!("  Integrations: Gmail, Outlook, Office 365, Google Workspace, Slack");
    println!("              QuickBooks, Xero, MailChimp, Mailgun, DocuSign, Dropbox, OneDrive");
    println!("              full REST API + webhooks");
    println!("  Customers: agencies, consultancies, real estate, nonprofits, small manufacturers");
    println!("            ~25,000+ paying customers");
    println!("            sweet spot 10-200 employees");
    println!("            ESRI, AAA, Bloomberg (smaller workgroups), Habitat for Humanity");
    println!("  Critique: UI feels dated next to HubSpot/Pipedrive — long-running modernization");
    println!("           reporting capabilities lag behind Salesforce/HubSpot Pro");
    println!("           customer support reportedly inconsistent (mixed reviews)");
    println!("           Marketing/Service add-ons less polished than core CRM");
    println!("  Differentiator: only major SMB CRM that natively unifies sales pipeline + project delivery");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "insightly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_insightly(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_insightly};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/insightly"), "insightly");
        assert_eq!(basename(r"C:\bin\insightly.exe"), "insightly.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("insightly.exe"), "insightly");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_insightly(&["--help".to_string()], "insightly"), 0);
        assert_eq!(run_insightly(&["-h".to_string()], "insightly"), 0);
        assert_eq!(run_insightly(&["--version".to_string()], "insightly"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_insightly(&[], "insightly"), 0);
    }
}
