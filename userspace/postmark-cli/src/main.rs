#![deny(clippy::all)]
//! postmark-cli — SlateOS Postmark transactional email personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Postmark transactional email delivery.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Wildbit founders, ActiveCampaign acquisition");
    println!("    products        Transactional + Broadcasts + DMARC + Inbound");
    println!("    deliverability  Why Postmark wins on inbox placement");
    println!("    pricing         Volume tiers");
    println!("    customers       Notable users");
    println!("    differentiator  Separating transactional from marketing");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("Postmark — transactional email, opinionated and reliable.");
    println!();
    println!("Founded 2010 by Chris Nagele, Natalie Nagele, and the Wildbit team");
    println!("in Philadelphia, PA. Wildbit was a bootstrapped, profitable");
    println!("software studio that built Beanstalk (Git/SVN hosting, 2007) and");
    println!("Conveyor (deployment tool) before pivoting one of those products");
    println!("into Postmark — initially built to handle Beanstalk's own");
    println!("notification emails reliably, then productized when other devs");
    println!("kept asking how Wildbit was getting such high deliverability.");
    println!();
    println!("Bootstrapped throughout — no VC funding. Wildbit was famously");
    println!("'profitable from day one' and frequently cited by indie founders");
    println!("as a calm-company role model. ~50 employees at peak.");
    println!();
    println!("September 2022: ActiveCampaign acquired Wildbit (Postmark +");
    println!("Beanstalk + DMARC Digests) for an undisclosed sum, reportedly");
    println!("nine figures USD. The Postmark brand and engineering team");
    println!("continue as a unit inside ActiveCampaign — pricing and product");
    println!("strategy have remained largely independent post-acquisition.");
}

fn print_products() {
    println!("Postmark product line:");
    println!();
    println!("• Transactional Streams");
    println!("    The core: send transactional email via SMTP or REST API.");
    println!("    Receipts, password resets, magic links, account notifications.");
    println!("    Each Postmark 'server' has separate streams for transactional");
    println!("    vs. broadcast traffic so reputation issues don't bleed.");
    println!();
    println!("• Broadcast Streams (2021+)");
    println!("    Added marketing-email capability with proper segmentation.");
    println!("    Stored separately from transactional streams to avoid the");
    println!("    deliverability cross-contamination that destroys transactional");
    println!("    senders on combined platforms (looking at you, big-email-co).");
    println!();
    println!("• Inbound Email Parsing");
    println!("    Receive emails to a custom-domain mailbox, parse to JSON,");
    println!("    POST to your webhook URL. Powers reply-to-thread features,");
    println!("    email-to-ticket integrations.");
    println!();
    println!("• Message Streams (subdivision)");
    println!("    Within a Postmark server, create multiple named streams to");
    println!("    isolate workloads (e.g., 'product-app', 'admin-tools',");
    println!("    'monitoring-alerts') with separate IP warmup and metrics.");
    println!();
    println!("• Templates");
    println!("    HTML + plain-text email templates with mustachioed");
    println!("    variables, A/B testing, layouts (reusable headers/footers),");
    println!("    visual editor + API-driven template management.");
    println!();
    println!("• DMARC Digests (free standalone)");
    println!("    Wildbit's free DMARC report aggregation service. Helps");
    println!("    domains adopt DMARC enforcement by visualizing the report");
    println!("    XML in human-readable summaries.");
}

fn print_deliverability() {
    println!("Postmark's deliverability obsession.");
    println!();
    println!("Postmark's pitch — and the reason it commands a premium — is");
    println!("inbox placement. Their published statistics (median time-to-");
    println!("inbox under 10 seconds, sustained for years) are among the");
    println!("best in the industry. How they achieve it:");
    println!();
    println!("• Transactional-only IP pools. Postmark refuses to send marketing");
    println!("  blasts and bulk newsletters on transactional IPs. This kept");
    println!("  the IP reputation pristine for the first decade. Broadcast");
    println!("  streams now exist but use separate dedicated IPs.");
    println!();
    println!("• Aggressive sender curation. Postmark closes accounts that");
    println!("  send spammy traffic faster than competitors. Painful for");
    println!("  borderline senders; great for the rest of the customer base.");
    println!();
    println!("• Strict DMARC, SPF, DKIM enforcement. New domains can't send");
    println!("  through Postmark until proper alignment is configured.");
    println!();
    println!("• Bounce processing in real time. Hard bounces are auto-");
    println!("  suppressed; soft bounces tracked per recipient. Sending to");
    println!("  known-bad addresses fails fast, protecting reputation.");
    println!();
    println!("• Engagement-aware delivery for broadcast streams. Recent");
    println!("  openers prioritized; long-inactive recipients moved to a");
    println!("  slower send tier.");
    println!();
    println!("• Public transparency. Postmark's deliverability metrics are");
    println!("  posted publicly. Few competitors do this — most won't");
    println!("  publish median time-to-inbox because their numbers would be");
    println!("  embarrassing.");
}

fn print_pricing() {
    println!("Postmark pricing (USD, 2025):");
    println!();
    println!("• Free trial: 100 emails/month");
    println!();
    println!("• Outbound Email Plans (per server, monthly billed):");
    println!("    10K emails — $15/mo");
    println!("    50K emails — $50/mo");
    println!("    300K emails — $215/mo");
    println!("    1M emails — $545/mo");
    println!("    5M emails — $1,995/mo");
    println!("    Higher tiers — custom");
    println!();
    println!("  Includes: unlimited servers, unlimited templates, 45-day");
    println!("  message history, all team members, webhooks, REST + SMTP.");
    println!();
    println!("• Inbound Email: 1K parses/mo free with outbound plan, then");
    println!("  $1.25 per 10K parses.");
    println!();
    println!("• Dedicated IPs: $50/mo per IP for very-high-volume senders.");
    println!();
    println!("• Broadcast Streams: separate pricing per broadcast email,");
    println!("  typically $0.40-1.50 per 1K sends depending on volume tier.");
    println!();
    println!("Honest take: Postmark is not cheap. SES is 10-100x cheaper");
    println!("per-email. But Postmark's pricing is competitive among");
    println!("'managed' transactional providers (SendGrid, Mailgun, Postmark");
    println!("are all in the same range), and the deliverability premium is");
    println!("real for transactional traffic where every undelivered receipt");
    println!("is a support ticket.");
}

fn print_customers() {
    println!("Postmark customer references (public + observable):");
    println!();
    println!("  • Basecamp (37signals) — magic links + transactional");
    println!("  • Lightspeed Commerce — POS receipts and notifications");
    println!("  • InVision — design-tool notifications");
    println!("  • Coingecko — crypto price alerts");
    println!("  • Atlas / atlas.so — workspace transactional");
    println!("  • IndieHackers — community transactional");
    println!("  • Bento — many startups use Postmark");
    println!("  • Lemon Squeezy — receipts and license keys");
    println!("  • Pieces for Developers — magic links");
    println!("  • Userlist — peer email marketing (uses Postmark!)");
    println!();
    println!("Pattern: indie SaaS, dev-tool startups, calm-company-affiliated");
    println!("teams who care about deliverability and don't want to wrestle");
    println!("with SendGrid's complexity or AWS SES's bare-metal feel.");
}

fn print_differentiator() {
    println!("Why teams pick Postmark:");
    println!();
    println!("• Best-in-class deliverability for transactional email.");
    println!("  Published median time-to-inbox under 10 seconds.");
    println!();
    println!("• Hard separation between transactional and broadcast traffic.");
    println!("  Other providers smear them together, polluting reputation.");
    println!();
    println!("• Calm-company ethos. Reliable product, no aggressive upsells,");
    println!("  no surprise rate-limit incidents from over-provisioning.");
    println!();
    println!("• Excellent docs and support. Real engineers answering tickets.");
    println!("  Response time historically among the best in the industry.");
    println!();
    println!("• Beautiful, opinionated UI. Easy to navigate. Suppression");
    println!("  list, webhook configuration, template editing are all");
    println!("  pleasant rather than punishing.");
    println!();
    println!("• Wildbit/ActiveCampaign acquisition has preserved the");
    println!("  Postmark identity. Pricing has not jumped post-acquisition.");
    println!();
    println!("vs. SendGrid: SendGrid is cheaper at high volume but");
    println!("  deliverability has declined since Twilio acquisition; UX is");
    println!("  more enterprise-y. Postmark feels indie-friendly.");
    println!();
    println!("vs. Mailgun: Mailgun has stronger marketing-email features and");
    println!("  EU regions but transactional deliverability lags Postmark");
    println!("  in independent tests.");
    println!();
    println!("vs. SES: SES is dramatically cheaper but you do reputation");
    println!("  management, bounce processing, templating yourself. Postmark");
    println!("  is opinionated and managed. SES is build-it-yourself.");
    println!();
    println!("vs. Resend: Resend is the modern challenger with a polished");
    println!("  React Email integration and dev-first DX. Postmark has");
    println!("  longer track record and more enterprise features.");
}

fn print_critique() {
    println!("Honest critique of Postmark:");
    println!();
    println!("• Expensive vs. SES at high volume. Above 1M emails/month, the");
    println!("  delta is significant. Some teams move bulk traffic to SES");
    println!("  while keeping transactional on Postmark.");
    println!();
    println!("• Broadcast Streams are newer and less mature than dedicated");
    println!("  marketing platforms (Mailchimp, Customer.io, Klaviyo).");
    println!("  Postmark is great if you want transactional + light");
    println!("  newsletters; not great as a full marketing automation tool.");
    println!();
    println!("• No deep marketing automation. Behavioral triggers, drip");
    println!("  campaigns, cart abandonment flows — not Postmark's lane.");
    println!();
    println!("• Limited template marketplace. The visual editor is good but");
    println!("  there are few pre-designed templates vs. competitors.");
    println!();
    println!("• Inbound parsing fee is small but adds up at high volumes.");
    println!();
    println!("• Account suspension can be abrupt for accounts that violate");
    println!("  the AUP (sending unsolicited mail). Postmark's reputation");
    println!("  protection is strict; if your sending pattern is borderline,");
    println!("  you may get warnings or shutdowns that less strict providers");
    println!("  would tolerate.");
    println!();
    println!("• ActiveCampaign acquisition long-term direction is still");
    println!("  observable. So far so good — but the era of independent");
    println!("  Wildbit ownership is over.");
}

fn run_postmark(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "deliverability" | "deliver" => { print_deliverability(); 0 }
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
        .unwrap_or_else(|| "postmark".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_postmark(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/postmark"), "postmark"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("postmark.exe"), "postmark"); }
    #[test] fn t_help() { assert_eq!(run_postmark(&[], "postmark"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_postmark(&["xx".to_string()], "postmark"), 2); }
}
