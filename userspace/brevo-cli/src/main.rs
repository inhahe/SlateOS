#![deny(clippy::all)]

//! brevo-cli — OurOS Brevo (formerly Sendinblue, European SMB marketing platform)
//!
//! Single personality: `brevo` (also responds to `sendinblue`)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_brevo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: brevo [OPTIONS]");
        println!("Brevo (OurOS) — multi-channel marketing + sales + transactional from one platform");
        println!();
        println!("Options:");
        println!("  --free                 Free — unlimited contacts, 300 emails/day");
        println!("  --starter              Starter — from $9/mo (20K emails/mo)");
        println!("  --business             Business — from $18/mo (multi-user + automations)");
        println!("  --enterprise           Enterprise — custom (SLA + dedicated IP)");
        println!("  --conversations        Brevo Conversations — chat + helpdesk");
        println!("  --transactional        Transactional API (SMTP relay + REST API)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Brevo 2024 (OurOS)"); return 0; }
    println!("Brevo 2024 (OurOS)");
    println!("  Vendor: Brevo SAS (Paris, France — private, fka Sendinblue)");
    println!("  Founders: Armand Thiberge (CEO), Kapil Sharma (CTO), 2012");
    println!("          Thiberge previously ran a consulting agency in India serving European SMBs");
    println!("          built Sendinblue to solve his own clients' transactional email problems");
    println!("          rebranded Sendinblue → Brevo May 2023 to drop the 'sendin' (email-only) connotation");
    println!("  Founded: 2012 in Paris with operations in Delhi (engineering hub)");
    println!("          bootstrapped early, then raised €140M Bridgepoint Series C 2020 (mostly secondary)");
    println!("          ~€150M+ ARR estimated, profitable, 800+ employees");
    println!("          one of largest European SaaS companies (along with Mirakl, Aircall, Doctolib)");
    println!("  Strategic position: 'European HubSpot for SMBs — multi-channel + transactional in one':");
    println!("                    SMB sweet spot, especially in Europe + LATAM + India + SE Asia");
    println!("                    competes with Mailchimp (less automation), HubSpot (more expensive)");
    println!("                    competes with ActiveCampaign (similar pricing, less channels)");
    println!("                    competes with SendGrid for transactional");
    println!("                    pitch: 'one platform for marketing + transactional + chat + CRM'");
    println!("                    GDPR-native + European data residency");
    println!("  Pricing (volume-based, very transparent + email-count not contact-count):");
    println!("    Free — unlimited contacts (!), 300 emails/day with Brevo branding");
    println!("    Starter — from $9/mo (20K emails/mo) — no daily limit, no branding");
    println!("    Business — from $18/mo (20K emails/mo) — multi-user, A/B test, send time opt, automations");
    println!("    BrevoPlus (Enterprise) — custom — dedicated IP + advanced security + priority support");
    println!("    SMS + WhatsApp — separate pay-as-you-go pricing (€0.0445/SMS in France, etc.)");
    println!("    Transactional — separate (SendGrid-like): from $15/mo (20K) up to enterprise volumes");
    println!("    contact-count is NOT the pricing axis — email-count is (refreshing differentiator)");
    println!("  Channels (broad for SMB pricing):");
    println!("    - Email (own infrastructure + dedicated IP options)");
    println!("    - Transactional email (SMTP + REST API — competes with SendGrid + Postmark + Mailgun)");
    println!("    - SMS (international with carrier partnerships)");
    println!("    - WhatsApp Business (added 2023, growing fast)");
    println!("    - Web push notifications");
    println!("    - Chat (Brevo Conversations — built-in live chat widget)");
    println!("    - Meetings (Calendly-like booking page)");
    println!("  Automations:");
    println!("    - Visual workflow builder (added 2017, vastly improved 2022)");
    println!("    - Triggers: form sign-up, page visit, link click, attribute change, event from API");
    println!("    - Actions: send email, send SMS, add/remove from list, update field, webhook, score adjustment, wait");
    println!("    - Marketing automations library: welcome, anniversary, abandoned cart, post-purchase, win-back");
    println!("    - Site tracking with JS snippet");
    println!("    - Lead scoring (manual rules)");
    println!("  Brevo Sales Platform (CRM, was Sendinblue Sales Hub):");
    println!("    - Deal pipeline + activity tracking");
    println!("    - Tasks + reminders + email tracking");
    println!("    - Less mature than HubSpot CRM or ActiveCampaign Deals");
    println!("    - Free tier available — playing catch-up here");
    println!("  Brevo Conversations:");
    println!("    - Live chat widget (multi-language, mobile + web)");
    println!("    - Unified inbox: chat + email + WhatsApp + Instagram + Facebook Messenger");
    println!("    - Chatbots (rule-based + Brevo Conversations AI)");
    println!("    - Helpdesk ticketing (lightweight vs Zendesk/Freshdesk)");
    println!("    - Acquired Chatra in 2018 to bootstrap this");
    println!("  Transactional (the legacy core, still very strong):");
    println!("    - REST + SMTP relay APIs (used by 50K+ developers globally)");
    println!("    - Webhooks for delivery, open, click, bounce, spam, hard/soft bounce");
    println!("    - Template engine with variables + conditionals");
    println!("    - Inbound parse (incoming emails → webhook payload)");
    println!("    - Dedicated IP warming (managed automatically)");
    println!("    - Often replaces SendGrid for SMBs at lower cost");
    println!("  Integrations: 150+ apps + Zapier + Make");
    println!("              Shopify, WooCommerce, Magento, PrestaShop (massive in EU + LATAM)");
    println!("              WordPress (official plugin, very popular)");
    println!("              Salesforce, HubSpot, Pipedrive sync");
    println!("              Segment, mParticle");
    println!("              REST API + Webhooks + JS site SDK + native iOS/Android SDKs");
    println!("  Customers: 500,000+ paying customers worldwide");
    println!("            Louis Vuitton (LVMH, transactional), Carrefour, Michelin, Eurosport (some marketing)");
    println!("            massive SMB long-tail in France, Germany, Spain, Italy, India, Brazil");
    println!("            sweet spot: SMBs + dev shops wanting transactional + marketing in one bill");
    println!("  Critique: UI shows European/translated origin — sometimes inconsistent vs HubSpot's polish");
    println!("           Sales/CRM features less mature than dedicated SMB CRMs (ActiveCampaign, HubSpot)");
    println!("           rebrand from Sendinblue to Brevo confused legacy customers");
    println!("           AI features lag US competitors (Klaviyo, HubSpot, Braze all ahead)");
    println!("           deep e-commerce automation lags Klaviyo for high-volume e-commerce");
    println!("           less brand awareness in North America than in Europe");
    println!("  Differentiator: marketing + transactional + chat in one tool, generous pricing, GDPR-native, free unlimited contacts — best multi-channel value for SMBs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "brevo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_brevo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_brevo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/brevo"), "brevo");
        assert_eq!(basename(r"C:\bin\brevo.exe"), "brevo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("brevo.exe"), "brevo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_brevo(&["--help".to_string()], "brevo"), 0);
        assert_eq!(run_brevo(&["-h".to_string()], "brevo"), 0);
        assert_eq!(run_brevo(&["--version".to_string()], "brevo"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_brevo(&[], "brevo"), 0);
    }
}
