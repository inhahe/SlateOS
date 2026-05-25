#![deny(clippy::all)]
//! ses-cli — OurOS AWS Simple Email Service personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — AWS SES (Simple Email Service).");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           SES origin January 2011");
    println!("    products        v1 vs v2 API, SES Receiving, MailManager");
    println!("    architecture    Regions, sandbox, production access");
    println!("    sandbox         The infamous SES sandbox approval");
    println!("    pricing         Pay-per-email cheap");
    println!("    customers       Everyone running email on AWS");
    println!("    differentiator  Cheap + flexible + AWS-integrated");
    println!("    critique        What SES doesn't do for you");
    println!("    help / version");
}

fn print_about() {
    println!("AWS SES — Amazon's bulk-emergent transactional email service.");
    println!();
    println!("Launched January 25, 2011 as Amazon SES (Simple Email Service).");
    println!("Built and operated by the AWS team in Seattle (and originally");
    println!("with significant engineering at AWS Dublin). SES emerged from");
    println!("Amazon's own internal email-sending needs — the retail business");
    println!("sends billions of order confirmations, shipping notifications,");
    println!("promotional emails per month. Productizing that capability for");
    println!("external customers was a natural extension.");
    println!();
    println!("Original positioning: 'commodity email infrastructure.' Not the");
    println!("polished UX of SendGrid or the deliverability obsession of");
    println!("Postmark — instead, the cheapest possible per-email cost, the");
    println!("highest possible scale, native AWS IAM integration, and the");
    println!("expectation that customers would build their own templating,");
    println!("analytics, and dashboards on top.");
    println!();
    println!("Over 15 years SES has grown features (event publishing,");
    println!("templates, configuration sets, SES Receiving for inbound,");
    println!("Mail Manager for advanced inbound rules) but the philosophy");
    println!("remains: cheap pipe, AWS-integrated, BYO everything else.");
    println!();
    println!("Strategic role for AWS: SES is critical infrastructure for");
    println!("countless SaaS startups, mobile apps, e-commerce sites. The");
    println!("'AWS does email' default makes AWS stickier — moving away");
    println!("from SES means moving cron jobs, Lambda functions, and IAM");
    println!("policies that depend on SES integration.");
}

fn print_products() {
    println!("SES product surface (2025):");
    println!();
    println!("• SES API v1 — legacy");
    println!("    Original 2011 API. Still supported. Slower than v2, fewer");
    println!("    features. Use SES API v2 for new code.");
    println!();
    println!("• SES API v2");
    println!("    Modern API. Bulk-send support, configuration sets, virtual");
    println!("    deliverability manager, list management, contact lists.");
    println!();
    println!("• SMTP Relay");
    println!("    Standard SMTP gateway for legacy applications. Use IAM SMTP");
    println!("    credentials. Same backend as the API.");
    println!();
    println!("• Configuration Sets");
    println!("    Named bundles of settings: which IPs to send from, which");
    println!("    event destinations (CloudWatch, SNS, Kinesis Firehose,");
    println!("    EventBridge), suppression list scope, tracking-domain");
    println!("    overrides. Apply per-send via headers.");
    println!();
    println!("• Templates");
    println!("    JSON-defined templates stored in SES with Mustache-style");
    println!("    variables. SES Templated Email API renders + sends in one");
    println!("    call. Limited compared to Handlebars/Liquid but native.");
    println!();
    println!("• Dedicated IPs (paid add-on)");
    println!("    Standard or Managed (auto-warmup). $24.95/month per");
    println!("    standard IP. Managed IPs cost more but handle warming.");
    println!();
    println!("• Virtual Deliverability Manager (VDM, 2023+)");
    println!("    Insights into sender reputation, dashboards, recommendations.");
    println!("    Paid add-on tier.");
    println!();
    println!("• SES Receiving / Mail Manager");
    println!("    Inbound email infrastructure. Receive at @your-domain.com,");
    println!("    apply rules (forward to S3, invoke Lambda, send to SNS),");
    println!("    Mail Manager (2024) adds richer rule engine.");
    println!();
    println!("• Contact Lists (newer)");
    println!("    Stored contact lists with topic-based subscriptions.");
    println!("    Generic unsubscribe handling. Helps build basic newsletter");
    println!("    workflows without a separate ESP.");
}

fn print_architecture() {
    println!("SES architecture and regions.");
    println!();
    println!("Regions (selected):");
    println!("  • us-east-1 (N. Virginia)");
    println!("  • us-west-2 (Oregon)");
    println!("  • us-east-2 (Ohio)");
    println!("  • eu-west-1 (Ireland)");
    println!("  • eu-central-1 (Frankfurt)");
    println!("  • eu-west-2 (London)");
    println!("  • eu-south-1 (Milan)");
    println!("  • ap-southeast-1 (Singapore)");
    println!("  • ap-southeast-2 (Sydney)");
    println!("  • ap-northeast-1 (Tokyo)");
    println!("  • ap-northeast-2 (Seoul)");
    println!("  • ap-south-1 (Mumbai)");
    println!("  • ca-central-1, sa-east-1, me-south-1, and more");
    println!();
    println!("Each region has its own IP pool, sandbox status, verified");
    println!("identities, sending quotas, reputation. Multi-region SES means");
    println!("you onboard the domain separately in each region.");
    println!();
    println!("Sending limits (per region):");
    println!("  • Sandbox: 200 emails / 24h, 1/sec, only to verified addresses");
    println!("  • Production: requested via AWS Support, increases over time");
    println!("  • Default production: 50K/day, 14/sec — increases via");
    println!("    automatic ramp-ups based on reputation");
    println!("  • Very large customers: millions/hour, custom quotas");
    println!();
    println!("Quotas are watched: if your bounce rate exceeds 5% or complaint");
    println!("rate exceeds 0.1%, you'll receive a warning. Sustained breach");
    println!("triggers automated 'review' (temporary pause). The Reputation");
    println!("Dashboard in the SES console shows your trend lines.");
    println!();
    println!("Event publishing: per-send events (Send, Delivery, Bounce,");
    println!("Complaint, Open, Click, RenderingFailure, etc.) can be routed");
    println!("to CloudWatch, SNS, Kinesis Firehose, EventBridge, or");
    println!("Pinpoint. This is how you build real-time dashboards and");
    println!("automatic suppression handling.");
}

fn print_sandbox() {
    println!("The infamous SES sandbox approval.");
    println!();
    println!("Every new SES region/account starts in 'sandbox' mode:");
    println!("  • 200 emails per 24 hours");
    println!("  • 1 email per second");
    println!("  • Can only send to verified addresses (you must verify each");
    println!("    test recipient)");
    println!();
    println!("To exit sandbox, you submit a 'Request Production Access' case");
    println!("to AWS Support describing:");
    println!("  • What kind of emails you'll send (transactional/marketing/mix)");
    println!("  • How you collect recipient consent");
    println!("  • How you handle bounces and complaints (must point to a");
    println!("    real monitoring/suppression mechanism)");
    println!("  • Expected sending volume and pattern");
    println!("  • Anti-abuse measures (rate limiting, signup verification, etc.)");
    println!();
    println!("Approval usually takes 24-48 hours. AWS Support is strict on");
    println!("the bounce/complaint handling description — vague answers");
    println!("('we'll figure it out later') often get rejected with");
    println!("requests for more detail.");
    println!();
    println!("Common rejection reasons:");
    println!("  • No clear consent collection mechanism");
    println!("  • Description suggests marketing without explicit opt-in");
    println!("  • No monitoring for bounces/complaints");
    println!("  • Domain age is very new and reputation is unknown");
    println!();
    println!("Once out of sandbox, ramping up volume requires demonstrating");
    println!("low bounce + complaint rates. AWS auto-increases your quotas");
    println!("if you maintain good reputation; auto-decreases (or 'reviews')");
    println!("if you don't.");
    println!();
    println!("The sandbox annoyance is real for new customers but it does");
    println!("protect SES's IP pools from abuse — which is part of why SES");
    println!("deliverability is reasonable despite the bargain pricing.");
}

fn print_pricing() {
    println!("SES pricing (USD, 2025):");
    println!();
    println!("• Outbound email from EC2 / Lambda:");
    println!("    First 62,000 emails/month — FREE (when sent from EC2/Lambda)");
    println!("    Above 62K — $0.10 per 1,000 emails");
    println!();
    println!("• Outbound email from non-AWS sources:");
    println!("    $0.10 per 1,000 emails (no free tier)");
    println!();
    println!("• Attachments: $0.12 per GB of attachments sent");
    println!();
    println!("• Dedicated IPs:");
    println!("    Standard dedicated IP: $24.95/month per IP");
    println!("    Managed dedicated IP (with automated warmup):");
    println!("      $200/month minimum for managed IP pool");
    println!();
    println!("• Inbound email (SES Receiving):");
    println!("    First 1,000 emails/month — FREE");
    println!("    Above 1K — $0.10 per 1K");
    println!("    Attachments: $0.09 per GB received");
    println!();
    println!("• Virtual Deliverability Manager: $1,500/month (advanced");
    println!("  insights + recommendations engine)");
    println!();
    println!("Pricing math: 1M emails/month from EC2 = ~$94. From non-AWS");
    println!("= $100. Same volume on SendGrid Essentials = $90 (but SendGrid");
    println!("includes templates, marketing campaigns, etc.). Postmark");
    println!("equivalent = $545/month. Resend equivalent (Scale tier) = ~$700.");
    println!();
    println!("SES is the cheapest serious email-sending option, by an order");
    println!("of magnitude at high volume. But you build everything else.");
}

fn print_customers() {
    println!("SES customer base:");
    println!();
    println!("  • Netflix — viewing notifications");
    println!("  • Reddit — comment + DM notifications");
    println!("  • Dropbox — file-sharing notifications");
    println!("  • Yelp — review + response notifications");
    println!("  • Almost every AWS-hosted startup at some point");
    println!("  • Many ESPs use SES under the hood as a backup or supplement");
    println!("  • Salesforce uses SES regionally");
    println!("  • Customer.io, Mailgun, SparkPost — competitors that also");
    println!("    leverage SES for certain workloads");
    println!();
    println!("Pattern: any AWS-native architecture sending more than a token");
    println!("number of emails. The default 'we send some email' option for");
    println!("anyone already running on AWS.");
}

fn print_differentiator() {
    println!("Why teams pick SES:");
    println!();
    println!("• Cheapest. By an order of magnitude at high volume.");
    println!();
    println!("• AWS-native. IAM-controlled. CloudWatch metrics. EventBridge");
    println!("  events. Lambda triggers. VPC endpoints. If your stack is");
    println!("  AWS, SES is the most idiomatic email choice.");
    println!();
    println!("• Reliable at scale. Has been delivering billions of emails per");
    println!("  day for over a decade. Outages exist but are infrequent.");
    println!();
    println!("• Multi-region. EU, APAC, US, MENA — pick a region for data");
    println!("  residency or latency.");
    println!();
    println!("• Flexible. You build the templating, sequences, marketing UI");
    println!("  on top — but you have total control over how that looks.");
    println!();
    println!("• Combines well with Amazon Pinpoint for marketing layers.");
    println!();
    println!("• No vendor lock-in beyond AWS. If you're already on AWS,");
    println!("  SES adds no incremental lock-in.");
    println!();
    println!("vs. SendGrid: SendGrid is more polished, more features, costs");
    println!("  more. SES is bare-bones, cheap, scale-friendly.");
    println!();
    println!("vs. Postmark/Resend: those are managed + opinionated. SES is");
    println!("  build-it-yourself. Some teams use SES under the hood and");
    println!("  layer a thin templating + scheduling app on top.");
}

fn print_critique() {
    println!("Honest critique of SES:");
    println!();
    println!("• You build everything else. Templates are weak. There's no");
    println!("  visual editor, no contact-list segmentation beyond basics,");
    println!("  no marketing UI, no A/B testing tool. SES is a SMTP+API");
    println!("  pipe; you build the rest.");
    println!();
    println!("• Bounce / complaint handling is your responsibility. SES will");
    println!("  publish events; you must consume them and suppress addresses.");
    println!("  Forget this and your sender reputation tanks.");
    println!();
    println!("• Sandbox onboarding annoyance. Production access approval");
    println!("  takes time and rejected requests require iteration.");
    println!();
    println!("• Reputation can crash hard. SES will pause your account if");
    println!("  bounce/complaint rates exceed thresholds. Recovery is painful");
    println!("  — you may need to contact support and explain the incident.");
    println!();
    println!("• Per-region quotas. Multi-region deployment requires onboarding,");
    println!("  reputation building, quota requests separately in each region.");
    println!();
    println!("• Limited deliverability insight without VDM ($1,500/mo).");
    println!("  Basic reputation dashboard is useful but lacks the granular");
    println!("  ISP-by-ISP analytics that Postmark / Sinch Inbox Tools provide.");
    println!();
    println!("• Template engine (Mustache + ses:Template) is minimal. No");
    println!("  conditionals, no helpers, no nesting. For real templating");
    println!("  most users render to MIME themselves before calling SendEmail.");
    println!();
    println!("• Email Authentication setup (SPF, DKIM, DMARC) is your");
    println!("  responsibility. SES provides DKIM key publishing helpers, but");
    println!("  you configure SPF and DMARC at your DNS provider.");
    println!();
    println!("• No inbound webhook to your URL (you must route through S3,");
    println!("  Lambda, or SNS — there's no 'POST to my HTTPS endpoint' mode).");
}

fn run_ses(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "architecture" | "arch" => { print_architecture(); 0 }
        "sandbox" => { print_sandbox(); 0 }
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
        .unwrap_or_else(|| "ses".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_ses(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/ses"), "ses"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("ses.exe"), "ses"); }
    #[test] fn t_help() { assert_eq!(run_ses(&[], "ses"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_ses(&["xx".to_string()], "ses"), 2); }
}
