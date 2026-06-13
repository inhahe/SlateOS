#![deny(clippy::all)]
//! mailersend-cli — SlateOS MailerSend transactional email personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — MailerSend transactional email (sister of MailerLite).");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           RemoteCompany family, transactional spinoff");
    println!("    products        Email API, SMS, Inbound, Templates");
    println!("    architecture    Multi-region, dedicated IPs");
    println!("    pricing         Volume tiers, generous free tier");
    println!("    customers       SMBs and developers");
    println!("    differentiator  Transactional with sibling MailerLite synergy");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("MailerSend — transactional email from the RemoteCompany family.");
    println!();
    println!("Launched 2020 by RemoteCompany (Vilnius, Lithuania), the same");
    println!("parent that owns MailerLite (marketing email) and MailerCheck");
    println!("(email verification). MailerSend was created to fill the");
    println!("transactional-email gap in the family portfolio — MailerLite");
    println!("had always focused on marketing, but customers kept asking for");
    println!("'a transactional sister product that integrates with our");
    println!("MailerLite account.'");
    println!();
    println!("Founding leadership: Ilma Nausėdaitė (Product), Ignas Rubežius");
    println!("(RemoteCompany founder), and the existing engineering team");
    println!("from MailerLite. Built on lessons learned from MailerLite's");
    println!("decade of email-deliverability operations.");
    println!();
    println!("Same bootstrapped, remote-first DNA as MailerLite. No VC funding.");
    println!("RemoteCompany operates the whole portfolio profitably. Headcount");
    println!("of the MailerSend team is ~30-40 engineers / product / support.");
    println!();
    println!("MailerSend grew quickly thanks to the trust capital from");
    println!("MailerLite — existing MailerLite customers signed up for");
    println!("MailerSend at high rates. By 2024 MailerSend was a credible");
    println!("competitor to SendGrid and Mailgun in the SMB transactional");
    println!("segment, sending billions of emails per month.");
}

fn print_products() {
    println!("MailerSend product line:");
    println!();
    println!("• Transactional Email API");
    println!("    REST + SMTP for programmatic sending. Single-send, bulk-");
    println!("    send (up to 500 messages per API call), templates with");
    println!("    variable substitution, attachments.");
    println!();
    println!("• Templates with drag-and-drop editor");
    println!("    Visual block-based editor for transactional templates.");
    println!("    Variables, conditional blocks, internationalization (i18n)");
    println!("    via per-locale template versions.");
    println!();
    println!("• Inbound Email Parsing");
    println!("    Receive emails to a domain configured with MailerSend's");
    println!("    MX records, parse into JSON, POST to your webhook. Supports");
    println!("    routing rules by recipient pattern.");
    println!();
    println!("• SMS API (newer)");
    println!("    Programmatic SMS sending via integration with carrier");
    println!("    networks. Twilio competitor for SMB scale.");
    println!();
    println!("• Webhooks");
    println!("    Event delivery for sent, delivered, hard/soft bounced,");
    println!("    spam complained, opened, clicked, unsubscribed. Signed,");
    println!("    retry-with-backoff.");
    println!();
    println!("• Suppression Management");
    println!("    Automatic global and per-domain suppression lists. Hard");
    println!("    bounces auto-suppressed. Complaints auto-suppressed. API");
    println!("    + UI to manage exclusions.");
    println!();
    println!("• Sub-accounts");
    println!("    For agencies and platforms: child accounts under a parent");
    println!("    with per-account quotas and separate reporting.");
    println!();
    println!("• Domain Verification");
    println!("    SPF, DKIM, DMARC, MX, return-path setup wizard with auto-");
    println!("    verification. Multiple domains per account.");
}

fn print_architecture() {
    println!("MailerSend architecture.");
    println!();
    println!("Multi-region infrastructure:");
    println!("  • US-East (Virginia)");
    println!("  • EU-Central (Frankfurt) — choose at account creation for");
    println!("    GDPR data residency compliance");
    println!();
    println!("Shared IP pools per region by default. Dedicated IPs available");
    println!("on higher plans — important for high-volume senders who need");
    println!("to manage their own sender reputation.");
    println!();
    println!("Deliverability features:");
    println!("  • Automatic IP warming for new dedicated IPs (gradually ramps");
    println!("    sending volume over ~30 days to build reputation)");
    println!("  • Engagement-based throttling (deprioritizes addresses that");
    println!("    haven't engaged recently)");
    println!("  • Real-time bounce processing with hard/soft classification");
    println!("  • Feedback loop processing with major ISPs (Gmail postmaster");
    println!("    tools, Yahoo CFL, etc.)");
    println!("  • Pre-flight content scanning (spam-trigger detection)");
    println!();
    println!("API:");
    println!("  • RESTful, JSON-based, OpenAPI spec");
    println!("  • Rate limits: 50 req/sec on lower tiers, higher on premium");
    println!("  • Bulk-send endpoint for up to 500 messages per call");
    println!("  • Idempotency keys for safe retries");
    println!();
    println!("SDKs: Node, Python, PHP, Ruby, Go, .NET, Java — all official.");
    println!();
    println!("Integrations: Zapier, Make, n8n, Pipedream, plus direct");
    println!("integrations with WordPress, Magento, WooCommerce, Shopify.");
}

fn print_pricing() {
    println!("MailerSend pricing (USD, 2025):");
    println!();
    println!("• Free");
    println!("    3,000 emails/month free forever, 1 verified domain, 5 user");
    println!("    seats, all features, generous trial. Includes MailerSend");
    println!("    footer branding.");
    println!();
    println!("• Premium tiers (per-email pricing above included quota):");
    println!("    Hobby: $24/mo (includes 50K emails)");
    println!("    Starter: $32/mo (includes 50K emails + advanced features)");
    println!("    Professional: $99/mo (includes 100K + dedicated IP option)");
    println!("    Enterprise: custom (custom volume + dedicated success)");
    println!();
    println!("• Per-email overage: $0.90 per 1K emails above plan quota.");
    println!();
    println!("• SMS pricing: pay per message at carrier-cost-pass-through");
    println!("  rates, typically $0.0075-0.025/SMS depending on destination.");
    println!();
    println!("• Inbound parsing: 100 routes free per month, then ~$1.25/1K");
    println!("  parses.");
    println!();
    println!("Honest take: MailerSend is one of the most generous free tiers");
    println!("in transactional email — 3K emails/month forever-free with no");
    println!("trial expiry, just branding required. The pay tiers are");
    println!("competitive with SendGrid/Mailgun at SMB scale.");
}

fn print_customers() {
    println!("MailerSend customer references:");
    println!();
    println!("  • Many MailerLite customers cross-using MailerSend for");
    println!("    transactional");
    println!("  • Small-to-mid SaaS startups");
    println!("  • E-commerce shops (Shopify, WooCommerce integrations)");
    println!("  • European companies for GDPR-friendly transactional");
    println!("  • Various agency / freelance shops managing client email");
    println!("  • PostgreSQL Conference (Europe) — event transactional");
    println!("  • IndieHackers + bootstrapped SaaS community");
    println!();
    println!("Pattern: SMBs that already use MailerLite, indie SaaS founders,");
    println!("European companies preferring EU vendors. Similar customer");
    println!("profile to Postmark and Resend but generally smaller volume");
    println!("per customer.");
}

fn print_differentiator() {
    println!("Why teams pick MailerSend:");
    println!();
    println!("• Generous free tier. 3K emails/month forever-free with no");
    println!("  expiry, just a small branding footer. Few competitors match.");
    println!();
    println!("• MailerLite synergy. If you already use MailerLite for");
    println!("  marketing, MailerSend for transactional integrates under one");
    println!("  account, billing, and support contract.");
    println!();
    println!("• EU data center option (Frankfurt). GDPR-friendly for");
    println!("  European customers concerned about US-only competitors.");
    println!();
    println!("• Clean modern UI. Less cluttered than SendGrid's dashboard.");
    println!("  Suppression management, template editing, webhook config");
    println!("  are pleasant rather than tedious.");
    println!();
    println!("• Drag-and-drop template editor for transactional — most");
    println!("  competitors require HTML coding for templates. MailerSend");
    println!("  lets non-engineers edit transactional templates.");
    println!();
    println!("• Bootstrapped + remote-first parent. No VC pressure, calm");
    println!("  roadmap, customer-focused.");
    println!();
    println!("• Built-in SMS adds omnichannel capability without integrating");
    println!("  a separate Twilio account.");
    println!();
    println!("vs. SendGrid: MailerSend is smaller and cheaper at SMB scale.");
    println!("  SendGrid has more enterprise capabilities and bigger brand.");
    println!();
    println!("vs. Mailgun: similar size and positioning. MailerSend has");
    println!("  better UI; Mailgun has stronger developer-tier docs.");
    println!();
    println!("vs. Postmark: Postmark has slightly better deliverability");
    println!("  reputation and broader enterprise features. MailerSend is");
    println!("  cheaper and has the visual template editor.");
    println!();
    println!("vs. Resend: Resend has better React/modern-stack DX.");
    println!("  MailerSend has SMS, EU regional, and the MailerLite tie-in.");
}

fn print_critique() {
    println!("Honest critique of MailerSend:");
    println!();
    println!("• Younger product than the 10-year incumbents. While built on");
    println!("  RemoteCompany's email-ops history (MailerLite), the");
    println!("  transactional-specific stack is newer.");
    println!();
    println!("• Brand confusion with MailerLite. Many users confuse 'lite'");
    println!("  vs. 'send,' which are entirely separate products despite");
    println!("  shared parent.");
    println!();
    println!("• Smaller feature surface than Postmark/SendGrid for enterprise");
    println!("  customers. Advanced sub-user permissions, audit logs, SSO");
    println!("  are still maturing.");
    println!();
    println!("• Deliverability is good but not industry-leading. Shared-IP");
    println!("  pools sometimes carry noise from less reputable senders");
    println!("  given the broad customer base.");
    println!();
    println!("• SMS is newer than competitors' (Twilio, MessageBird). Feature");
    println!("  parity and global coverage still developing.");
    println!();
    println!("• Some legacy MailerLite users find MailerSend redundant when");
    println!("  MailerLite itself can send transactional via campaigns —");
    println!("  product positioning between sibling products takes explanation.");
    println!();
    println!("• Smaller marketing/community presence than industry leaders.");
    println!("  Brand awareness in North America especially trails.");
    println!();
    println!("• Pricing tier between Free and Hobby ($0 to $24) has a gap");
    println!("  where light senders without 50K emails/month find no fit.");
}

fn run_mailersend(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (SlateOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "architecture" | "arch" => { print_architecture(); 0 }
        "pricing" => { print_pricing(); 0 }
        "customers" => { print_customers(); 0 }
        "differentiator" | "diff" => { print_differentiator(); 0 }
        "critique" => { print_critique(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for usage.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mailersend".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_mailersend(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/mailersend"), "mailersend"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("mailersend.exe"), "mailersend"); }
    #[test] fn t_help() { assert_eq!(run_mailersend(&[], "mailersend"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_mailersend(&["xx".to_string()], "mailersend"), 2); }
}
