#![deny(clippy::all)]

//! activecampaign-cli — SlateOS ActiveCampaign (SMB marketing automation + CRM)
//!
//! Single personality: `activecampaign`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ac(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: activecampaign [OPTIONS]");
        println!("ActiveCampaign (SlateOS) — SMB marketing automation + sales CRM");
        println!();
        println!("Options:");
        println!("  --lite                 Lite — email marketing starter ($15/mo for 1K contacts)");
        println!("  --plus                 Plus — automations + CRM ($49/mo)");
        println!("  --professional         Professional — predictive + attribution ($79/mo)");
        println!("  --enterprise           Enterprise — custom, unlimited users");
        println!("  --automations          Visual automation builder");
        println!("  --crm                  Pipeline CRM (Deals)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ActiveCampaign 2024 (SlateOS)"); return 0; }
    println!("ActiveCampaign 2024 (SlateOS)");
    println!("  Vendor: ActiveCampaign, LLC (Chicago, IL — private)");
    println!("  Founders: Jason VandeBoom (CEO), 2003");
    println!("          VandeBoom started ActiveCampaign as a consulting/services biz");
    println!("          rebooted as SaaS pure-play around 2013");
    println!("          one of few SMB SaaS companies bootstrapped to scale before raising");
    println!("  Founded: 2003 in Chicago — bootstrapped first ~10 years");
    println!("          Susquehanna Growth Equity led $100M Series A in 2018 (yes, only A)");
    println!("          Silver Lake $240M Series B in 2021 at $3B+ valuation");
    println!("          ~$240M+ ARR estimated, profitable, ~1,000 employees");
    println!("          considered IPO 2022 but held off through downturn");
    println!("  Strategic position: 'marketing automation that didn't sell out to enterprise':");
    println!("                    SMB + mid-market sweet spot (under 50 employees most common)");
    println!("                    competes with HubSpot (more expensive, more polished)");
    println!("                    competes with Mailchimp + Klaviyo + Constant Contact (less automation depth)");
    println!("                    competes with Marketo + Eloqua (much smaller deals)");
    println!("                    'platform that grows with you from $15/mo to $5K/mo'");
    println!("  Pricing (transparent + contact-count based):");
    println!("    Lite (1K contacts) — $15/mo (email-only, no automations)");
    println!("    Plus (1K contacts) — $49/mo (automations + simple CRM + landing pages)");
    println!("    Professional (1K contacts) — $79/mo (predictive sending + attribution + split testing)");
    println!("    Enterprise (1K contacts) — $145/mo (single sign-on + custom report + dedicated rep)");
    println!("    scales by contact count: 5K contacts roughly doubles each tier");
    println!("    100K contacts on Plus is ~$549/mo, Pro ~$729/mo");
    println!("    unlimited sends per month on every plan (no overage charges)");
    println!("  Automation builder (the killer feature for SMB):");
    println!("    - Visual drag-and-drop automation editor — feels like a Lego board");
    println!("    - Triggers: site visit, form fill, tag added, email opened, deal stage changed, custom field, anniversary");
    println!("    - Actions: send email, add/remove tag, update field, notify sales rep, wait, if/else, webhook, goal");
    println!("    - Conditional branching: 'if user opened last email AND visited pricing page' → branch");
    println!("    - Site tracking + event tracking integrate with automations");
    println!("    - 800+ pre-built automation recipes shared by community in 'Automation Marketplace'");
    println!("    - Multi-channel: emails + SMS + site messages + Facebook custom audiences");
    println!("    - Math + date math in automations (rare for SMB tools)");
    println!("  Pipeline CRM (built-in, not a separate product):");
    println!("    - Deals organized into pipelines (drag-and-drop deal stages)");
    println!("    - Lead scoring (automation-driven, fully custom)");
    println!("    - Deal tasks + reminders auto-created from automations");
    println!("    - Sales automations: assign reps, set win probability, send notifications");
    println!("    - Reports: pipeline value, conversion rates per stage, sales velocity");
    println!("    - Mobile app for sales reps");
    println!("    - far simpler than Salesforce — designed for 1-20 person sales teams");
    println!("  Predictive features (Professional+):");
    println!("    - Predictive Sending — best time per contact (per-contact ML)");
    println!("    - Predictive Content — choose best variant per recipient");
    println!("    - Win Probability — likelihood deal closes");
    println!("    - Attribution — multi-touch revenue credit across email + site + SMS");
    println!("  Channels:");
    println!("    - Email (own infrastructure, very strong deliverability for SMB)");
    println!("    - SMS (US + international, à la carte pricing)");
    println!("    - Site messages (in-app popups + chat widgets)");
    println!("    - Facebook custom audiences (sync segments to Meta ads)");
    println!("    - Conversations — built-in live chat + unified inbox (Plus+)");
    println!("  Forms + landing pages:");
    println!("    - Drag-and-drop form builder (inline, modal, slide-in, floating)");
    println!("    - Landing pages with templates (Plus+)");
    println!("    - Behavior-triggered popups");
    println!("  Integrations: 900+ apps");
    println!("              Shopify, WooCommerce, BigCommerce, Square");
    println!("              WordPress, Webflow, Squarespace");
    println!("              Zapier, Make (Integromat), n8n");
    println!("              Salesforce, Pipedrive, HubSpot (yes, integrates with competitors)");
    println!("              REST API + webhooks + JS site SDK");
    println!("  Customers: 180,000+ paying customers in 170+ countries");
    println!("            sweet spot: solo entrepreneurs through ~50-employee SMBs");
    println!("            very strong in: coaches/consultants, course creators, agencies, indie e-commerce");
    println!("            agency-friendly: white-label + multi-account dashboard");
    println!("  Critique: UI shows its age (still rapidly improving) — feels less modern than HubSpot");
    println!("           reporting is functional but not as polished as Klaviyo or HubSpot");
    println!("           predictive features less sophisticated than Braze/Iterable at enterprise scale");
    println!("           deliverability good for SMB but lower-tier than Klaviyo for high-volume e-commerce");
    println!("           CRM lacks depth — no quoting, forecasting, or territory management");
    println!("           customer support response times can be slow on lower tiers");
    println!("  Differentiator: best automation builder at SMB pricing + integrated CRM + transparent pricing — 'pro automation for indie businesses'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "activecampaign".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ac(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ac};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/activecampaign"), "activecampaign");
        assert_eq!(basename(r"C:\bin\activecampaign.exe"), "activecampaign.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("activecampaign.exe"), "activecampaign");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ac(&["--help".to_string()], "activecampaign"), 0);
        assert_eq!(run_ac(&["-h".to_string()], "activecampaign"), 0);
        let _ = run_ac(&["--version".to_string()], "activecampaign");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ac(&[], "activecampaign");
    }
}
