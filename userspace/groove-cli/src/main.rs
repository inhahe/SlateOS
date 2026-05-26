#![deny(clippy::all)]
//! groove-cli — personality CLI for Groove, the bootstrapped small-team
//! help desk and famous "Journey to $500K MRR" startup blog.
//!
//! Founded 2011 by Alex Turnbull in Newport, Rhode Island as a deliberately-
//! bootstrapped competitor to Zendesk + Help Scout at the SMB end of the
//! market. The defining piece of company-as-content marketing in the
//! help-desk SaaS world: Turnbull's "Journey to $500K MRR" blog series
//! transparently published Groove's ARR, churn rate, support metrics, and
//! marketing experiments throughout the company's growth — turning the
//! marketing-blog itself into Groove's most effective top-of-funnel asset
//! and a widely-studied bootstrapped-SaaS case study.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Groove bootstrapped small-team help-desk personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Alex Turnbull 2011 Newport RI; bootstrapped");
    println!("    journey       'Journey to $500K MRR' transparency blog series");
    println!("    inbox         Shared inbox + assignment + collision detection");
    println!("    kb            Knowledge-base + customer-facing help-centre");
    println!("    reports       Reporting + happiness scoring + agent metrics");
    println!("    ai            Newer AI features layered on the original product");
    println!("    pricing       Per-user-per-month tiered pricing");
    println!("    customers     Small-team SMB customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("groove-cli 0.1.0 (bootstrapped-help-desk personality build)"); }

fn run_about() {
    println!("Groove (GrooveHQ, Inc.).");
    println!("  Founded:    2011, Newport, Rhode Island.");
    println!("  Founder:    Alex Turnbull (CEO; still founder-led).");
    println!("  Status:     Bootstrapped — no significant VC funding ever taken.");
    println!("  Positioning: 'simple help desk for small businesses' — purposely");
    println!("              avoids competing with Zendesk on enterprise + automation depth.");
    println!("  Famously associated with the 'Journey to $500K MRR' transparency blog.");
    println!("  Modern scale: profitable, several-thousand-customer SaaS in the");
    println!("              high-seven-to-low-eight-figure ARR band.");
}

fn run_journey() {
    println!("'Journey to $500K MRR' blog series.");
    println!("  Alex Turnbull began publishing Groove's internal SaaS metrics openly:");
    println!("  monthly recurring revenue, churn rates, customer-acquisition cost,");
    println!("  marketing-channel performance, support-team workload.");
    println!("  Each post was a transparent dispatch about what was + wasn't working.");
    println!("  Became one of the most-read SaaS marketing blogs of the early-mid 2010s,");
    println!("  studied widely by bootstrappers, Indie Hackers, MicroConf community.");
    println!("  Effective inversion: the marketing strategy *was* publicly explaining");
    println!("  the marketing strategy — and that authenticity drove signups.");
    println!("  Frequently cited as the canonical example of 'build in public' marketing");
    println!("  before that term was popularised.");
}

fn run_inbox() {
    println!("Shared inbox.");
    println!("  Multiple email mailboxes (support@, billing@, etc.) consolidated into");
    println!("  one team queue.");
    println!("  Assignment to teammates with status indicators.");
    println!("  Collision detection: live indicator when another teammate is replying");
    println!("  to the same conversation.");
    println!("  Internal notes for teammate-only discussion next to the customer thread.");
    println!("  Canned responses + saved replies for common questions.");
    println!("  Mobile apps for iOS + Android for on-the-go inbox triage.");
}

fn run_kb() {
    println!("Knowledge base + help-centre.");
    println!("  Public-facing customer help-centre with branded subdomain.");
    println!("  Article authoring with rich text + media + categories.");
    println!("  In-app embed: chat widget surfaces relevant KB articles before the");
    println!("  customer escalates to a real agent.");
    println!("  Article-effectiveness reporting: which KB pages deflect tickets, which");
    println!("  ones lead to follow-up conversations.");
    println!("  Multi-brand: separate KBs per brand under one Groove account.");
}

fn run_reports() {
    println!("Reporting + happiness.");
    println!("  Conversation-volume + response-time + resolution-time dashboards.");
    println!("  Per-agent workload + handling-time metrics.");
    println!("  Customer happiness ratings (CSAT) collected via post-resolution surveys.");
    println!("  Tag-based reporting for theme analysis ('what % of tickets are about");
    println!("  refunds vs. shipping vs. product questions?').");
    println!("  Exportable to CSV for further BI tooling; modest native analytics depth");
    println!("  (deliberate — Groove explicitly targets teams that don't need Tableau).");
}

fn run_ai() {
    println!("Newer AI features.");
    println!("  AI Draft Replies: LLM-generated response drafts an agent reviews + sends.");
    println!("  AI Summarisation: long threads collapsed to key points + customer intent.");
    println!("  AI Article Suggestions: KB-recommendation surfaces in agent UI.");
    println!("  Tone-of-voice training: AI replies match the brand voice configured");
    println!("  per Groove account.");
    println!("  Newer additions reflecting category-wide LLM adoption — Groove was not an");
    println!("  early mover on AI but has caught up since 2023.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Standard:  ~$12 per user per month, basic shared inbox + KB.");
    println!("  Plus:      ~$20 per user per month, adds reporting + custom-fields + chat.");
    println!("  Pro:       ~$40 per user per month, adds automation + AI + multi-brand.");
    println!("  Annual prepay discount ~20%%; no implementation fees.");
    println!("  Significantly cheaper than Zendesk + Help Scout at the equivalent tier,");
    println!("  matching the 'help desk for small teams' positioning.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 1-10 person customer-support teams in SMBs.");
    println!("  Heavy share of indie SaaS founders, Shopify store operators, agencies,");
    println!("  e-commerce brands, niche software vendors.");
    println!("  Customers frequently came from Turnbull's blog — direct attribution to");
    println!("  content marketing rather than paid acquisition.");
    println!("  Common pattern: small bootstrapped or low-VC company that chose Groove");
    println!("  specifically because its values + economics matched the buyer's company.");
    println!("  Rarely competes for enterprise deals — those go to Zendesk / Salesforce.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "groove-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "journey" => run_journey(),
        "inbox" => run_inbox(),
        "kb" => run_kb(),
        "reports" => run_reports(),
        "ai" => run_ai(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_journey();
        run_inbox();
        run_kb();
        run_reports();
        run_ai();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("groove-cli");
        print_version();
    }
}
