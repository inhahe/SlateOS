#![deny(clippy::all)]

//! pipedrive-cli — Slate OS Pipedrive (sales-first visual pipeline CRM)
//!
//! Single personality: `pipedrive`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pipedrive(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pipedrive [OPTIONS]");
        println!("Pipedrive (Slate OS) — sales-first visual pipeline CRM");
        println!();
        println!("Options:");
        println!("  --essential            Essential $14/user/mo");
        println!("  --advanced             Advanced $34/user/mo");
        println!("  --professional         Professional $49/user/mo");
        println!("  --power                Power $64/user/mo");
        println!("  --enterprise           Enterprise $99/user/mo");
        println!("  --leadbooster          LeadBooster add-on (chatbot, forms, prospector)");
        println!("  --campaigns            Campaigns add-on (email marketing)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pipedrive 2024 (Slate OS)"); return 0; }
    println!("Pipedrive 2024 (Slate OS)");
    println!("  Vendor: Pipedrive OU (HQ Tallinn, Estonia + New York, NY)");
    println!("  Founders: Timo Rein, Urmas Purde, Ragnar Sass, Martin Henk, Martin Tajur (2010)");
    println!("          all ex-salespeople — built the CRM THEY wanted (visual pipeline first)");
    println!("  Founded: 2010 in Tallinn — bootstrapped early, raised Series A from Bessemer 2015");
    println!("  Funding: Vista Equity Partners acquired majority Nov 2020 ($1.5B valuation)");
    println!("          previously: Bessemer, Atomico, Insight Partners ~$90M before Vista");
    println!("  Scale: 100,000+ companies in 179 countries");
    println!("        ~$200M+ ARR (Vista doesn't disclose post-buyout)");
    println!("        ~1,000 employees");
    println!("  Pricing: Essential $14/user/mo (basic pipeline)");
    println!("          Advanced $34/user/mo (automations, email sync)");
    println!("          Professional $49/user/mo (forecasts, e-sign, reports)");
    println!("          Power $64/user/mo (project mgmt, scheduler, phone)");
    println!("          Enterprise $99/user/mo (security, customization, SSO)");
    println!("  Add-ons:");
    println!("    - LeadBooster ($32.5/co/mo) — chatbot, live chat, web forms, prospector");
    println!("    - Web Visitors ($41/co/mo) — identify companies visiting your site");
    println!("    - Campaigns ($13.33/co/mo) — bulk email marketing");
    println!("    - Smart Docs ($32.5/co/mo) — quotes, contracts, e-sign");
    println!("    - Projects ($6.7/user/mo) — post-deal project tracking");
    println!("  Core features:");
    println!("    - Visual drag-and-drop pipeline (their original killer feature)");
    println!("    - Activity-based selling — focus on next action, not deal value");
    println!("    - Email sync (2-way Gmail/Outlook) + email tracking");
    println!("    - Caller (built-in VoIP) — click-to-call from contact records");
    println!("    - Smart Contact Data — auto-enrich contacts from web data");
    println!("    - Mobile-first design — iOS + Android apps highly rated");
    println!("    - Workflow Automation — trigger-based actions (Advanced+)");
    println!("    - Goals + Insights dashboards");
    println!("    - AI Sales Assistant — recommends next actions, flags stuck deals");
    println!("  Integrations: 350+ Marketplace apps");
    println!("              Slack, Google Workspace, Microsoft 365, Zapier, Make, Trello, Asana");
    println!("              Mailchimp, Intercom, Zoom, DocuSign, QuickBooks, Xero, Calendly");
    println!("  Customers: SMB sales teams (5-200 sales reps)");
    println!("            Vimeo, Skyscanner, Re/Max, Festo");
    println!("            very popular in Europe + Latin America (Estonia origin shows)");
    println!("  Critique: weaker on marketing automation vs HubSpot");
    println!("           reporting/forecasting less powerful than Salesforce");
    println!("           Vista ownership has slowed product cadence vs founder era");
    println!("           add-on pricing nickel-and-dimes at smaller seat counts");
    println!("  Differentiator: simplest, most beautiful sales-only pipeline tool — reps actually use it");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pipedrive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pipedrive(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pipedrive};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pipedrive"), "pipedrive");
        assert_eq!(basename(r"C:\bin\pipedrive.exe"), "pipedrive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pipedrive.exe"), "pipedrive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pipedrive(&["--help".to_string()], "pipedrive"), 0);
        assert_eq!(run_pipedrive(&["-h".to_string()], "pipedrive"), 0);
        let _ = run_pipedrive(&["--version".to_string()], "pipedrive");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pipedrive(&[], "pipedrive");
    }
}
