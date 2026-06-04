#![deny(clippy::all)]

//! zendesksell-cli — OurOS Zendesk Sell (formerly Base CRM, acquired 2018)
//!
//! Single personality: `zsell`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zsell(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zsell [OPTIONS]");
        println!("Zendesk Sell (OurOS) — sales CRM, sister to Zendesk Support");
        println!();
        println!("Options:");
        println!("  --team                 Sell Team $19/user/mo");
        println!("  --growth               Sell Growth $55/user/mo");
        println!("  --professional         Sell Professional $115/user/mo");
        println!("  --enterprise           Sell Enterprise $169/user/mo");
        println!("  --voice                Built-in Voice (call from CRM)");
        println!("  --reach                Reach add-on (prospecting + enrichment)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zendesk Sell 2024 (OurOS)"); return 0; }
    println!("Zendesk Sell 2024 (OurOS)");
    println!("  Vendor: Zendesk, Inc. (San Francisco, CA — private since 2022)");
    println!("  Origin: 'Sell' was originally Base CRM (Mountain View, CA)");
    println!("        Base founded 2009 by Uzi Shmilovici, Pawel Niznik, Tomek Buszewski");
    println!("        Polish founders — engineering largely in Krakow");
    println!("        Base raised $53M from Index, Social+Capital, OCA Ventures");
    println!("  Acquisition: Zendesk acquired Base Sep 2018 for ~$50M → rebranded 'Zendesk Sell' 2019");
    println!("              Zendesk itself taken private June 2022 by Hellman & Friedman + Permira for $10.2B");
    println!("  Strategy: Zendesk's bet to compete in CRM (Sell) + service (Support) bundles");
    println!("           positioning: 'service-first' sales tool — natural fit if you already use Zendesk Support");
    println!("  Pricing: Team $19/user/mo (basic pipeline, mobile)");
    println!("          Growth $55/user/mo (forecasting, goals, advanced reporting)");
    println!("          Professional $115/user/mo (lead scoring, advanced perms, voice)");
    println!("          Enterprise $169/user/mo (custom roles, sandbox, dedicated support)");
    println!("          all billed annually (monthly +25%)");
    println!("  Sell features:");
    println!("    - Visual pipeline with drag-and-drop deals");
    println!("    - Email integration (Gmail + Outlook 2-way sync)");
    println!("    - Built-in voice dialer + recording (no integration needed)");
    println!("    - Click-to-call from any phone field");
    println!("    - SMS messaging (Voice add-on)");
    println!("    - Mobile app with offline mode + geo-tagged check-ins (best-in-class for field sales)");
    println!("    - Sales sequences (cadences) with automated steps");
    println!("    - Lead scoring + smart lists (Professional+)");
    println!("    - Forecasting + goals tracking");
    println!("  Reach add-on:");
    println!("    - 20M+ verified business contacts database");
    println!("    - Auto-enrich existing records with title/phone/email");
    println!("    - Build prospecting lists by industry/size/title");
    println!("  Zendesk integration:");
    println!("    - Native sync with Zendesk Support — sales rep sees customer's open tickets");
    println!("    - One unified customer view across sales + service");
    println!("    - Shared user directory + SSO");
    println!("  Other integrations: Mailchimp, Slack, Pandadoc, HubSpot Marketing, Quickbooks, Xero");
    println!("                     Zapier connector for 5K+ apps");
    println!("                     Sunshine platform (Zendesk's CDP layer)");
    println!("  Customers: SMB-to-mid-market sales teams that also use Zendesk Support");
    println!("            ~10,000+ paying companies on Sell specifically (Zendesk overall: 100,000+)");
    println!("            sweet spot: 10-500 employees with field sales or inside sales");
    println!("  Critique: feels like an acquisition that didn't fully integrate — separate UX from Support");
    println!("           less mindshare than HubSpot/Pipedrive in pure-play CRM evaluations");
    println!("           reporting weaker than Salesforce Reports + Dashboards");
    println!("           customers complain Zendesk has deprioritized Sell investment since PE buyout");
    println!("  Differentiator: best-in-class for field/mobile sales teams + native ticketing connection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zsell".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zsell(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zsell};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zendesksell"), "zendesksell");
        assert_eq!(basename(r"C:\bin\zendesksell.exe"), "zendesksell.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zendesksell.exe"), "zendesksell");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zsell(&["--help".to_string()], "zendesksell"), 0);
        assert_eq!(run_zsell(&["-h".to_string()], "zendesksell"), 0);
        let _ = run_zsell(&["--version".to_string()], "zendesksell");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zsell(&[], "zendesksell");
    }
}
