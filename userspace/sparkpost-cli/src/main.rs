#![deny(clippy::all)]
//! sparkpost-cli — SlateOS SparkPost / Bird Email personality CLI (historical).

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — SparkPost / Bird Email (historical + Momentum heritage).");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Message Systems / Momentum / SparkPost / Bird arc");
    println!("    products        Cloud + on-prem Momentum MTA");
    println!("    momentum        The Momentum on-prem MTA platform");
    println!("    pricing         Pre-EOL pricing structure");
    println!("    customers       Who SparkPost served");
    println!("    differentiator  MTA depth + analytics");
    println!("    critique        Honest critique + EOL story");
    println!("    sunset          SparkPost cloud sunset (April 2024) + migration");
    println!("    help / version");
}

fn print_about() {
    println!("SparkPost — once a leading email API, sunset under MessageBird/Bird.");
    println!();
    println!("Lineage:");
    println!("  • 1998 — Message Systems founded by John Stafford et al.,");
    println!("    building Momentum, an on-premise high-performance MTA");
    println!("    (Mail Transfer Agent) used by ISPs, telcos, and large");
    println!("    enterprises sending billions of emails. Acquired by Elliott");
    println!("    Associates 2014 and renamed.");
    println!("  • 2014 — Message Systems launches a cloud-API product on top");
    println!("    of Momentum technology, branded 'SparkPost.'");
    println!("  • 2017 — Message Systems formally renames itself SparkPost,");
    println!("    consolidating around the cloud product while continuing");
    println!("    Momentum on-prem licensing.");
    println!("  • 2021 — MessageBird acquires SparkPost for ~$600M cash + stock,");
    println!("    folding it into their omnichannel customer-comms portfolio");
    println!("    alongside their core SMS/WhatsApp/Voice APIs.");
    println!("  • 2023 — MessageBird rebrands corporate parent as 'Bird.'");
    println!("    SparkPost rebrands as 'Bird Email API.' Existing customer");
    println!("    confusion ensues.");
    println!("  • April 30, 2024 — Bird announces the SparkPost cloud service");
    println!("    end-of-life. Existing customers must migrate to alternate");
    println!("    providers or to MessageBird's other email infrastructure.");
    println!("  • Momentum on-prem MTA continues as a licensed product for");
    println!("    customers needing self-hosted high-volume MTA.");
    println!();
    println!("SparkPost's cloud sunset was one of the largest email-vendor");
    println!("EOL events in recent history. Major customers had to migrate");
    println!("to SendGrid, Mailgun, Postmark, Resend, or AWS SES on short");
    println!("notice. The episode is now a cautionary tale about depending");
    println!("on a single email vendor through ownership changes.");
}

fn print_products() {
    println!("SparkPost product line (at peak):");
    println!();
    println!("• SparkPost Cloud (Email API)");
    println!("    REST API + SMTP relay. Templates (Handlebars-flavored),");
    println!("    transactional + marketing send modes, real-time event");
    println!("    streaming (Webhooks, Amazon S3 batch, Kinesis), suppression");
    println!("    lists, A/B testing, scheduled sends, transactional templates.");
    println!("    EOL'd April 2024.");
    println!();
    println!("• Momentum on-prem MTA");
    println!("    The historical core. Self-hosted MTA software designed for");
    println!("    customers sending billions of emails/month. Used by ESPs,");
    println!("    cable/telco providers, Yahoo Mail-style high-volume senders.");
    println!("    Continues as licensed product under Bird.");
    println!();
    println!("• Signals (Deliverability Analytics)");
    println!("    Deep analytics across multi-vendor email sends, ISP-by-ISP");
    println!("    placement analysis, engagement metrics, infrastructure-");
    println!("    level diagnostics.");
    println!();
    println!("• PowerMTA (sister product, later)");
    println!("    Port25's PowerMTA acquired by SparkPost in 2017, integrated");
    println!("    as a competitor/complement to Momentum.");
    println!();
    println!("• Inbound (Relay Webhooks)");
    println!("    Receive emails and POST to webhook URLs. Less feature-rich");
    println!("    than competitors but functional.");
}

fn print_momentum() {
    println!("Momentum — the on-prem MTA platform.");
    println!();
    println!("Momentum is the original Message Systems product that SparkPost");
    println!("was built on top of. It remains a serious enterprise MTA");
    println!("product even after the SparkPost cloud sunset.");
    println!();
    println!("Key capabilities:");
    println!("  • Massive scale — multi-million-emails-per-hour per node");
    println!("  • Adaptive throttling per receiving ISP (back off when Gmail");
    println!("    pushes back, ramp up to Yahoo's preferences)");
    println!("  • Custom Lua scripting for message handling, header injection,");
    println!("    routing decisions");
    println!("  • Sophisticated bounce processing (DSN parsing, soft/hard");
    println!("    bounce categorization, automatic suppression)");
    println!("  • Multi-binding (one Momentum host can present as many");
    println!("    different sender IPs / domains)");
    println!("  • Feedback loop integration with all major ISPs");
    println!("  • Run as cluster across multiple nodes for HA");
    println!();
    println!("Typical customers:");
    println!("  • ISPs that send their own mail (Comcast, ATT, Sky, BT,");
    println!("    Telefónica, large national telcos)");
    println!("  • White-label ESPs that resell email APIs to customers");
    println!("  • Marketing platforms with their own infrastructure (e.g.,");
    println!("    some Salesforce Marketing Cloud installations historically)");
    println!("  • Companies sending >100M emails/month who can't afford the");
    println!("    SaaS markup");
    println!();
    println!("Pricing is enterprise licensing — typically six- to seven-figure");
    println!("USD per year depending on volume and deployment topology.");
    println!();
    println!("Under Bird, Momentum continues to be sold and supported. The");
    println!("on-prem product wasn't part of the SparkPost cloud EOL.");
}

fn print_pricing() {
    println!("SparkPost pricing (historical, pre-EOL):");
    println!();
    println!("Note: SparkPost cloud was shut down April 30, 2024. The");
    println!("below describes the legacy pricing for historical context.");
    println!();
    println!("• Test Account — Free");
    println!("    500 emails/month, all features, for evaluation only.");
    println!();
    println!("• Starter — from $20/month");
    println!("    50K emails/month, basic features.");
    println!();
    println!("• Premier — custom (enterprise tier)");
    println!("    Volume pricing for millions of emails/month, dedicated IPs,");
    println!("    advanced deliverability tools, premium support.");
    println!();
    println!("• Enterprise — custom contracts for very-high-volume senders");
    println!();
    println!("Momentum on-prem (current, under Bird):");
    println!("  • Per-node licensing or volume-based licensing");
    println!("  • Typically 6-figure USD/year for production deployments");
    println!("  • Professional services for installation, tuning, ISP");
    println!("    relationship management often part of the package");
}

fn print_customers() {
    println!("SparkPost customer base (historical):");
    println!();
    println!("Cloud customers (now migrated to alternatives):");
    println!("  • Pinterest — pin notifications");
    println!("  • Twitter (now X) — historically used SparkPost for some flows");
    println!("  • Comcast — telecom customer comms");
    println!("  • The New York Times — newsletter and notifications");
    println!("  • Zillow — listing alerts");
    println!("  • Many SaaS startups that wanted scale without SES complexity");
    println!();
    println!("Momentum on-prem customers (current):");
    println!("  • Comcast, ATT, Sky, BT (telco mail infrastructure)");
    println!("  • Various national postal services (statement printing/email)");
    println!("  • Large banks for statement delivery");
    println!("  • Cox Communications");
    println!("  • Verizon");
    println!("  • Several ESPs/CRMs using Momentum as their MTA layer");
    println!();
    println!("Pattern: SparkPost cloud served growth-stage SaaS that scaled");
    println!("beyond SendGrid Essentials but didn't want enterprise pricing.");
    println!("Momentum serves the absolute top tier of email-sending");
    println!("infrastructure — ISPs and Fortune 500 with custom MTA needs.");
}

fn print_differentiator() {
    println!("SparkPost's historical differentiators:");
    println!();
    println!("• Momentum MTA underneath. Few cloud-API competitors had the");
    println!("  depth of MTA technology that came from 15+ years of Message");
    println!("  Systems engineering. Adaptive throttling per ISP, bounce");
    println!("  classification accuracy, were genuine technical advantages.");
    println!();
    println!("• Signals analytics. Deep deliverability insights that some");
    println!("  customers found unmatched, including per-ISP per-sender-");
    println!("  domain placement tracking.");
    println!();
    println!("• Hybrid model. You could run SparkPost cloud + Momentum on-prem");
    println!("  side-by-side, with consistent analytics and configuration.");
    println!();
    println!("• Strong technical brand. SparkPost engineering team was");
    println!("  visible in the email deliverability community, contributing");
    println!("  to industry standards (DMARC, ARC, BIMI).");
    println!();
    println!("• Reasonable cloud pricing in the middle tier — cheaper than");
    println!("  SendGrid for some volumes, more enterprise-capable than");
    println!("  Mailgun/Postmark.");
    println!();
    println!("• Real-time event streaming via Webhooks + Kinesis was leading-");
    println!("  edge when introduced. Most competitors caught up over time.");
    println!();
    println!("Caveat (2025): SparkPost cloud no longer exists. These are");
    println!("historical differentiators. Momentum still exists as a");
    println!("differentiator for the small market of self-hosted MTA buyers.");
}

fn print_critique() {
    println!("Honest critique of SparkPost (now Bird Email):");
    println!();
    println!("• The cloud sunset (April 2024) was the largest failure mode.");
    println!("  Bird gave customers ~12 months to migrate. Many were caught");
    println!("  off-guard by the announcement. Trust in the brand collapsed.");
    println!("  This is now a cautionary tale for SaaS dependency planning.");
    println!();
    println!("• Brand transitions confused customers. Message Systems →");
    println!("  SparkPost → MessageBird → Bird Email API → EOL. Each rename");
    println!("  cost trust and SEO continuity.");
    println!();
    println!("• Roadmap velocity declined under MessageBird/Bird ownership.");
    println!("  Innovation moved to Bird's omnichannel positioning while");
    println!("  the cloud email product stagnated.");
    println!();
    println!("• Documentation became inconsistent across the rebrands.");
    println!("  Customer-facing API hostnames changed; SDK versions reflected");
    println!("  different stages of the brand journey.");
    println!();
    println!("• Smaller plugin/template ecosystem than SendGrid or Postmark.");
    println!();
    println!("• Mid-tier pricing strategy (~$200-2000/month for many customers)");
    println!("  was squeezed: SES cheaper at high volume, Postmark/Resend");
    println!("  better DX at low volume, SendGrid better brand. SparkPost");
    println!("  occupied a stable but unloved middle.");
    println!();
    println!("• On-prem Momentum continues but is a small market. Most");
    println!("  new email infrastructure decisions go to SaaS APIs.");
    println!();
    println!("• If you're evaluating email vendors in 2025, this entry is");
    println!("  more of a historical reference than a recommendation. Look");
    println!("  at SendGrid, Postmark, Resend, Mailgun, SES instead.");
}

fn print_sunset() {
    println!("SparkPost cloud sunset (April 30, 2024).");
    println!();
    println!("Timeline:");
    println!("  • 2023 Q4 — Bird announces sunset to customers via email +");
    println!("    customer success channels");
    println!("  • Customers given ~12 months to migrate");
    println!("  • Migration playbook published with guidance on alternative");
    println!("    providers (SendGrid, Mailgun, Postmark, Resend, AWS SES)");
    println!("  • Bird offered free migration to some MessageBird products");
    println!("    where it made sense");
    println!("  • April 30, 2024 — SparkPost cloud send APIs stop accepting");
    println!("    new traffic. Existing scheduled sends drain. Then full");
    println!("    shutdown of the cloud service.");
    println!();
    println!("Industry impact:");
    println!("  • Customer migrations triggered notable spikes in onboarding");
    println!("    for SendGrid, Postmark, Resend during early 2024");
    println!("  • Some customers used the transition as an opportunity to");
    println!("    re-evaluate vendors more broadly");
    println!("  • Several SaaS companies experienced deliverability issues");
    println!("    during the cutover (new IP warmup, DNS DKIM rotation,");
    println!("    suppression list re-import)");
    println!();
    println!("Lessons documented in the post-mortem community discussion:");
    println!("  • Always have a backup email vendor for failover");
    println!("  • Treat email vendor migration as a months-long project");
    println!("  • Keep SPF/DKIM/DMARC DNS records vendor-agnostic where");
    println!("    possible (use sender domains + multiple SPF includes)");
    println!("  • Export your suppression list regularly to avoid being");
    println!("    locked into vendor-specific state");
    println!();
    println!("Momentum on-prem remains operational under Bird and was not");
    println!("part of the cloud sunset.");
}

fn run_sparkpost(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (SlateOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "momentum" => { print_momentum(); 0 }
        "pricing" => { print_pricing(); 0 }
        "customers" => { print_customers(); 0 }
        "differentiator" | "diff" => { print_differentiator(); 0 }
        "critique" => { print_critique(); 0 }
        "sunset" | "eol" => { print_sunset(); 0 }
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
        .unwrap_or_else(|| "sparkpost".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_sparkpost(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/sparkpost"), "sparkpost"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("sparkpost.exe"), "sparkpost"); }
    #[test] fn t_help() { assert_eq!(run_sparkpost(&[], "sparkpost"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_sparkpost(&["xx".to_string()], "sparkpost"), 2); }
}
