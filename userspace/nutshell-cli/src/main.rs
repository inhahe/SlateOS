#![deny(clippy::all)]

//! nutshell-cli — OurOS Nutshell (small-team CRM from Ann Arbor)
//!
//! Single personality: `nutshell`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nutshell(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nutshell [OPTIONS]");
        println!("Nutshell (OurOS) — easy CRM for small teams");
        println!();
        println!("Options:");
        println!("  --foundation           Foundation $16/user/mo");
        println!("  --growth               Growth $42/user/mo");
        println!("  --pro                  Pro $52/user/mo");
        println!("  --business             Business $67/user/mo");
        println!("  --enterprise           Enterprise $79/user/mo");
        println!("  --campaigns            Nutshell Marketing — email campaigns add-on");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Nutshell 2024 (OurOS)"); return 0; }
    println!("Nutshell 2024 (OurOS)");
    println!("  Vendor: Nutshell, Inc. (Ann Arbor, MI — private)");
    println!("  Founders: Guy Suter, Andy Fowler, Ian Berry (2010)");
    println!("          all U-Michigan grads / Ann Arbor startup community");
    println!("          Guy Suter previously co-founded BitLeap (acquired by Barracuda 2008)");
    println!("  Founded: 2010 in Ann Arbor — bootstrapped from day one until 2020");
    println!("          acquired/recapitalized by Krause Holdings (Iowa family office) ~2020");
    println!("          ~$25M ARR estimated, ~80 employees");
    println!("  Mission: 'CRM should be the thing reps WANT to use, not the thing they hate'");
    println!("          opinionated about being light, fast, and Mac/iOS-native-feeling");
    println!("  Pricing: Foundation $16/user/mo (basic contacts + pipeline)");
    println!("          Growth $42/user/mo (workflow automation, custom reports)");
    println!("          Pro $52/user/mo (advanced reports, multiple pipelines)");
    println!("          Business $67/user/mo (engagement-based scoring, multiple currencies)");
    println!("          Enterprise $79/user/mo (audit logs, API limits raised, dedicated support)");
    println!("          all billed annually (monthly +20%)");
    println!("          Marketing add-on: from $5/mo per 100 contacts");
    println!("  Core CRM features:");
    println!("    - Visual pipeline (kanban-style drag-and-drop)");
    println!("    - List view + map view + chart view all for same data");
    println!("    - Email integration (Gmail + Outlook native, any IMAP)");
    println!("    - Click-to-call (with VoIP integrations: RingCentral, Aircall, Dialpad)");
    println!("    - Activity logging (calls, emails, meetings, notes)");
    println!("    - Customizable lead/opportunity stages per pipeline");
    println!("    - Multiple pipelines (Pro+)");
    println!("  Workflow automation (Growth+):");
    println!("    - Auto-assign leads on round-robin or by territory");
    println!("    - Trigger emails or tasks based on stage changes");
    println!("    - Email sequences (drip campaigns) for nurture");
    println!("    - Engagement scoring — how 'hot' is this lead");
    println!("  Reporting:");
    println!("    - Pipeline forecasting + win-rate analytics");
    println!("    - Activity reports per rep");
    println!("    - Custom reports with SQL-like filtering (Pro+)");
    println!("  Nutshell Marketing (add-on, ex-VTNS Solutions acquisition):");
    println!("    - Email broadcast + drip campaigns");
    println!("    - Landing pages");
    println!("    - Lead scoring based on email + web behavior");
    println!("    - Sends from CRM contact data (no separate list management)");
    println!("  Mobile: iOS + Android apps with offline mode + business card scanner");
    println!("        the business card scanner is genuinely one of the best in the category");
    println!("  Integrations: Gmail, Outlook, Office 365, Google Workspace, Slack, Zoom");
    println!("              QuickBooks, Mailchimp, Constant Contact, Zapier (5K+ apps)");
    println!("              RingCentral, Aircall, Dialpad for native dialing");
    println!("  Customers: 5,000+ companies, mostly 5-100 employees");
    println!("            heavy in construction, manufacturing, professional services, agencies");
    println!("            very strong customer support reputation (G2 leaders consistently)");
    println!("  Critique: less brand awareness vs HubSpot/Pipedrive");
    println!("           marketing automation thinner than HubSpot's Marketing Hub");
    println!("           reporting good but not Salesforce-deep");
    println!("  Differentiator: opinionated, fast, light CRM with possibly the friendliest support team in the category");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nutshell".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nutshell(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nutshell};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nutshell"), "nutshell");
        assert_eq!(basename(r"C:\bin\nutshell.exe"), "nutshell.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nutshell.exe"), "nutshell");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nutshell(&["--help".to_string()], "nutshell"), 0);
        assert_eq!(run_nutshell(&["-h".to_string()], "nutshell"), 0);
        assert_eq!(run_nutshell(&["--version".to_string()], "nutshell"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nutshell(&[], "nutshell"), 0);
    }
}
