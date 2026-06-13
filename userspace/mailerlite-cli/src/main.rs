#![deny(clippy::all)]
//! mailerlite-cli — SlateOS MailerLite email marketing platform personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — MailerLite email marketing for creators and SMBs.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Lithuanian bootstrapped story");
    println!("    products        Email + sites + creator monetization");
    println!("    automation      MailerLite's automation builder");
    println!("    pricing         Per-subscriber tiers");
    println!("    customers       Creators and SMBs");
    println!("    differentiator  Affordable + clean for non-developers");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("MailerLite — bootstrapped email marketing out of Lithuania.");
    println!();
    println!("Founded 2010 in Vilnius, Lithuania by Ignas Rubežius and a small");
    println!("team under the parent company 'Igniterz' (later renamed");
    println!("'RemoteCompany' to reflect the fully-distributed work model).");
    println!("MailerLite originally started as a web-design agency that built");
    println!("an in-house email tool; the tool became the product when");
    println!("clients kept asking to use it.");
    println!();
    println!("Notable: 100% bootstrapped. No VC funding, no exit, no obligation");
    println!("to grow at venture pace. Profitable since approximately 2013.");
    println!("RemoteCompany now operates several products: MailerLite (email");
    println!("marketing), MailerSend (transactional email), MailerCheck (email");
    println!("verification), all under the same parent.");
    println!();
    println!("RemoteCompany is famous for being one of the early fully-remote-");
    println!("by-default companies (since 2017). Headquartered in Vilnius but");
    println!("staff distributed globally. The company is regularly cited in");
    println!("'remote-first culture' case studies.");
    println!();
    println!("Headcount ~120 across MailerLite + sister products. Revenue >$50M");
    println!("annually (estimated from public commentary by Ignas). MailerLite");
    println!("has ~1M customers worldwide, mostly SMBs, creators, and bloggers.");
}

fn print_products() {
    println!("MailerLite product line:");
    println!();
    println!("• Email Marketing");
    println!("    The flagship. Newsletter campaigns, RSS-to-email, A/B tests,");
    println!("    AI subject-line writer, segmentation, multi-list management,");
    println!("    drag-and-drop email editor, pre-designed templates, embed");
    println!("    forms and popups.");
    println!();
    println!("• Automation");
    println!("    Workflow builder for trigger-based email sequences. Triggers:");
    println!("    new subscriber, anniversary, segment join, form submit, link");
    println!("    click, custom date. Conditional branching, delays, exit");
    println!("    conditions.");
    println!();
    println!("• Websites & Landing Pages");
    println!("    Build a simple website or landing pages directly on MailerLite.");
    println!("    Useful for solopreneurs who don't have a separate site.");
    println!();
    println!("• Newsletter / Blog");
    println!("    Built-in newsletter publishing — write a post, publish to");
    println!("    your subscribers and also to a public blog page. Substack-");
    println!("    lite for creators.");
    println!();
    println!("• Sell Digital Products");
    println!("    Sell ebooks, digital downloads, courses directly to your");
    println!("    list with Stripe-integrated checkout. Subscriber gating.");
    println!();
    println!("• Paid Newsletters");
    println!("    Subscription/membership tier for creators — gated content,");
    println!("    Stripe-billed monthly/yearly memberships.");
    println!();
    println!("• Forms & Popups");
    println!("    Sign-up forms (embedded, popup, slide-in, full-screen)");
    println!("    that capture emails to your MailerLite lists.");
    println!();
    println!("• API + integrations");
    println!("    REST API for contact management, send, automation triggering.");
    println!("    Integrations with Shopify, WooCommerce, WordPress, Zapier,");
    println!("    Make, Stripe, Magento.");
}

fn print_automation() {
    println!("MailerLite Automation — the workflow engine.");
    println!();
    println!("The automation builder is one of MailerLite's strengths. Visual");
    println!("drag-and-drop canvas with triggers, conditions, actions:");
    println!();
    println!("Triggers:");
    println!("  • Joins a group / list");
    println!("  • Updates a field (e.g., status changes to 'customer')");
    println!("  • Completes a form");
    println!("  • Clicks a specific link");
    println!("  • Anniversary of a date field");
    println!("  • Specific date / scheduled");
    println!("  • API trigger from your app");
    println!("  • E-commerce: abandoned cart, post-purchase (with");
    println!("    integration), product viewed");
    println!();
    println!("Actions:");
    println!("  • Send email");
    println!("  • Wait (delay)");
    println!("  • Move to group");
    println!("  • Update field");
    println!("  • Conditional branch (yes/no fork)");
    println!("  • Multi-branch logic");
    println!("  • Webhook out");
    println!("  • A/B split test path");
    println!();
    println!("Common workflows:");
    println!("  • Welcome series (5-email onboarding for new signups)");
    println!("  • Re-engagement (inactive subscribers in 90 days)");
    println!("  • Birthday/anniversary discount");
    println!("  • Abandoned-cart recovery (e-commerce integrations)");
    println!("  • Course drip (lesson per day for purchasers)");
    println!();
    println!("UX is friendly to non-technical users. Bloggers, course");
    println!("creators, and SMB owners can build sophisticated flows without");
    println!("learning anything about programming.");
}

fn print_pricing() {
    println!("MailerLite pricing (USD, 2025):");
    println!();
    println!("• Free");
    println!("    1,000 subscribers, 12,000 emails/month, drag-and-drop");
    println!("    editor, automation, forms, sites (1), 30 days support.");
    println!("    Includes MailerLite-branded footer.");
    println!();
    println!("• Growing Business (per-subscriber tiers):");
    println!("    1K subs — $10/mo, 3K — $20/mo, 10K — $39/mo,");
    println!("    25K — $87/mo, 50K — $146/mo, 100K — $284/mo,");
    println!("    200K — $573/mo. Unlimited emails. Removes branding.");
    println!();
    println!("• Advanced (per-subscriber):");
    println!("    Same subscriber tiers + ~30% premium. Adds: AI assistant,");
    println!("    multi-step automations, custom HTML editor, multiple users,");
    println!("    multiple sites, priority support.");
    println!();
    println!("• Enterprise — custom");
    println!("    100K+ subscribers, dedicated success manager, SLA, advanced");
    println!("    security and compliance.");
    println!();
    println!("Pricing is notably below Mailchimp's pay-as-you-grow tiers. The");
    println!("free tier is genuinely useful for early-stage creators who");
    println!("haven't hit 1K subscribers yet.");
}

fn print_customers() {
    println!("MailerLite customer base:");
    println!();
    println!("  • 1M+ accounts (mostly small / mid-size businesses)");
    println!("  • Many bloggers, podcasters, course creators");
    println!("  • Small e-commerce shops (Shopify + WooCommerce)");
    println!("  • Bootstrapped SaaS founders");
    println!("  • Non-profits and community organizations");
    println!("  • Author/writer audiences (newsletter-as-marketing)");
    println!();
    println!("Notable public references:");
    println!("  • Kim Komando (tech journalist)");
    println!("  • Many independent newsletter publishers");
    println!("  • Course creators who chose MailerLite over Mailchimp");
    println!("  • International SMBs in EU/LATAM where pricing matters");
    println!();
    println!("Pattern: SMBs, creators, bloggers, anyone for whom Mailchimp");
    println!("became too expensive or for whom Klaviyo/Iterable is overkill.");
    println!("Strongest in EU and emerging markets where affordable SaaS");
    println!("pricing wins over enterprise polish.");
}

fn print_differentiator() {
    println!("Why teams pick MailerLite:");
    println!();
    println!("• Affordable. Pricing per-subscriber is half or less of");
    println!("  Mailchimp's equivalent tiers. The free tier covers real");
    println!("  early-stage usage.");
    println!();
    println!("• Clean, modern UI. Substantially less cluttered than Mailchimp");
    println!("  or Constant Contact. Doesn't feel like 90s-era marketing.");
    println!();
    println!("• Built-in website + landing pages + sell-digital-products +");
    println!("  paid newsletters. One platform for solopreneur needs without");
    println!("  bolting together five tools.");
    println!();
    println!("• Strong automation builder for the price tier. Comparable to");
    println!("  ConvertKit, ActiveCampaign at lower cost.");
    println!();
    println!("• Bootstrapped + remote-first parent company. No VC pressure");
    println!("  for aggressive monetization. Roadmap reflects customer needs.");
    println!();
    println!("• MailerSend pairing. Sister product MailerSend handles");
    println!("  transactional email — convenient for SMBs to keep marketing");
    println!("  + transactional under one parent vendor.");
    println!();
    println!("vs. Mailchimp: MailerLite is cheaper and feels less cluttered.");
    println!("  Mailchimp has more integrations and bigger brand recognition.");
    println!();
    println!("vs. ConvertKit (Kit): both creator-friendly. ConvertKit has");
    println!("  stronger creator-marketing positioning; MailerLite has");
    println!("  broader SMB feature set and lower price.");
    println!();
    println!("vs. Brevo/Sendinblue: similar feature scope. MailerLite is");
    println!("  more focused on email; Brevo adds SMS, CRM, chat.");
    println!();
    println!("vs. Loops/Customer.io: those are SaaS-focused with sequences");
    println!("  + transactional. MailerLite is marketing-newsletter-focused.");
}

fn print_critique() {
    println!("Honest critique of MailerLite:");
    println!();
    println!("• Not for developers. The API and integrations are functional");
    println!("  but the product is built for non-technical users. SaaS");
    println!("  founders typically prefer Loops/Customer.io/Resend.");
    println!();
    println!("• Account approval can be friction. New accounts go through");
    println!("  manual review (anti-spam measure). Legitimate users sometimes");
    println!("  experience delays or rejections.");
    println!();
    println!("• Free tier requires social media login or higher-friction");
    println!("  signup verification. Some find this surprising.");
    println!();
    println!("• Advanced automation (deep behavioral, custom event triggers)");
    println!("  is less powerful than Customer.io / Klaviyo. MailerLite is");
    println!("  optimized for simpler creator/SMB flows.");
    println!();
    println!("• Limited e-commerce depth. The Shopify/Woo integrations work");
    println!("  but Klaviyo dominates e-commerce email marketing for good");
    println!("  reason — deeper revenue attribution and segmentation.");
    println!();
    println!("• Deliverability is generally good but has historically been");
    println!("  more variable than Postmark/Mailchimp due to shared-IP pool");
    println!("  policies and the broad SMB customer base (some bad actors).");
    println!();
    println!("• Multi-step automations require Advanced plan. Lower tiers");
    println!("  cap at single-step automations.");
    println!();
    println!("• Brand 'MailerLite' is sometimes confused with 'MailerSend'");
    println!("  (same parent, different products). Customer confusion at");
    println!("  the marketing layer.");
}

fn run_mailerlite(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "automation" | "auto" => { print_automation(); 0 }
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
        .unwrap_or_else(|| "mailerlite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_mailerlite(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/mailerlite"), "mailerlite"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("mailerlite.exe"), "mailerlite"); }
    #[test] fn t_help() { assert_eq!(run_mailerlite(&[], "mailerlite"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_mailerlite(&["xx".to_string()], "mailerlite"), 2); }
}
