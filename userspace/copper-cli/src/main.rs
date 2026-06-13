#![deny(clippy::all)]

//! copper-cli — SlateOS Copper (CRM built natively inside Google Workspace)
//!
//! Single personality: `copper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_copper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: copper [OPTIONS]");
        println!("Copper (SlateOS) — CRM built for Google Workspace");
        println!();
        println!("Options:");
        println!("  --starter              Starter $9/user/mo (billed annually)");
        println!("  --basic                Basic $23/user/mo");
        println!("  --professional         Professional $59/user/mo");
        println!("  --business             Business $99/user/mo");
        println!("  --gmail-sidebar        Chrome extension shows CRM inside Gmail");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Copper 2024 (SlateOS)"); return 0; }
    println!("Copper 2024 (SlateOS)");
    println!("  Vendor: Copper CRM, Inc. (San Francisco, CA — private)");
    println!("  Founder: Dennis Fois (CEO from 2017, ex-AdRoll, Newscred)");
    println!("          original founder Jon Aniano (ex-Salesforce VP product)");
    println!("  Founded: 2011 as ProsperWorks → rebrand to Copper Mar 2018");
    println!("          rebrand because 'every salesperson prospers' was too generic");
    println!("  Funding: Series D ~$93M total — Norwest Venture Partners, Google Ventures");
    println!("          GV invested specifically because of deep Google Workspace integration");
    println!("  Pricing: Starter $9/user/mo (1,000 contacts cap)");
    println!("          Basic $23/user/mo (2,500 contacts)");
    println!("          Professional $59/user/mo (15,000 contacts, workflow automation)");
    println!("          Business $99/user/mo (unlimited, advanced reporting + leaderboards)");
    println!("          all prices billed annually; month-to-month +20%");
    println!("  Killer integration:");
    println!("    - 'Recommended by Google for Workspace' — top-tier partner badge");
    println!("    - Gmail Chrome extension shows CRM panel inside every email");
    println!("    - 1-click 'Add to Copper' from Gmail header");
    println!("    - Auto-syncs Google Contacts + Calendar bidirectionally");
    println!("    - Logs every email + meeting to the right contact/deal automatically");
    println!("    - Google Drive attachments link to opportunities");
    println!("  Core features:");
    println!("    - Visual pipeline (multiple pipelines per workspace)");
    println!("    - Lead enrichment from public web data (LinkedIn pulls, etc.)");
    println!("    - Activity tracking (calls, emails, meetings auto-logged)");
    println!("    - Task management with due dates + assignments");
    println!("    - Reports + dashboards (drag-and-drop builder)");
    println!("    - Workflow automation (Pro+) — trigger emails, create tasks, update fields");
    println!("    - Goal tracking + leaderboards (Business)");
    println!("    - Project management module (post-sale work)");
    println!("    - Mass email send (limited cadences vs HubSpot/Outreach)");
    println!("  Integrations: Slack, Zoom, DocuSign, Mailchimp, QuickBooks, Xero");
    println!("              Zapier (gateway to 5,000+ apps)");
    println!("              built-in dialer via partner integrations");
    println!("  Customers: small businesses + agencies + consultancies");
    println!("            sweet spot: 5-50 employees, heavy Google Workspace users");
    println!("            Uber Freight, Hello Fresh (early), Houzz, Swell");
    println!("  Competitive position:");
    println!("    - vs HubSpot: simpler, deeper Google integration, weaker marketing tools");
    println!("    - vs Pipedrive: similar pricing, Google-native vs platform-agnostic");
    println!("    - vs Salesforce: a tiny fraction of the complexity AND the price");
    println!("  Critique: weak email marketing — no real Mailchimp alternative built-in");
    println!("           limited custom object support — extending data model is awkward");
    println!("           if you switch off Google Workspace, you lose 80% of the value");
    println!("           reporting feels last-decade compared to modern competitors");
    println!("  Differentiator: literally lives inside Gmail — zero context-switching tax");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "copper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_copper(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_copper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/copper"), "copper");
        assert_eq!(basename(r"C:\bin\copper.exe"), "copper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("copper.exe"), "copper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_copper(&["--help".to_string()], "copper"), 0);
        assert_eq!(run_copper(&["-h".to_string()], "copper"), 0);
        let _ = run_copper(&["--version".to_string()], "copper");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_copper(&[], "copper");
    }
}
