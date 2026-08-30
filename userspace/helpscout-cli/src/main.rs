#![deny(clippy::all)]

//! helpscout-cli — Slate OS Help Scout (email-first helpdesk that feels like real email)
//!
//! Single personality: `helpscout`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_helpscout(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: helpscout [OPTIONS]");
        println!("Help Scout (Slate OS) — invisible-helpdesk email support tool");
        println!();
        println!("Options:");
        println!("  --standard             Standard $25/user/mo (2 mailboxes)");
        println!("  --plus                 Plus $50/user/mo (unlimited mailboxes, custom fields)");
        println!("  --pro                  Pro $65/user/mo (HIPAA, enterprise security)");
        println!("  --beacon               Beacon — in-app chat + KB widget");
        println!("  --docs                 Docs (knowledge base)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Help Scout 2024 (Slate OS)"); return 0; }
    println!("Help Scout 2024 (Slate OS)");
    println!("  Vendor: Help Scout PBC (Boston, MA — fully remote, certified B-Corp)");
    println!("  Founders: Nick Francis (CEO), Jared McDaniel, Denny Swindle (2011)");
    println!("          all three previously ran a small web design agency in Nashville → Boston");
    println!("          frustrated with Zendesk's 'ticket' UI killing personal feel of customer email");
    println!("  Founded: 2011 in Boston — bootstrapped early, raised modest Series A 2018");
    println!("          structured as Public Benefit Corporation (PBC) — formal commitment to social good");
    println!("          'fully distributed' since pre-pandemic — ~140 employees in 80+ cities");
    println!("  Core thesis: 'every customer conversation should feel like personal email — not a ticket'");
    println!("            no ticket numbers visible to customers, no automated 'your case has been logged'");
    println!("            agents see the conversation as if it were Gmail, not a ticketing system");
    println!("  Pricing: Standard $25/user/mo (2 mailboxes, 50 saved replies, basic reports)");
    println!("          Plus $50/user/mo (unlimited mailboxes, custom fields, advanced API)");
    println!("          Pro $65/user/mo (HIPAA compliance, enterprise security, dedicated CSM)");
    println!("          5-user minimum on Pro; annual billing 20% off");
    println!("  Mailbox features:");
    println!("    - Looks like Gmail — threaded conversations, not 'tickets'");
    println!("    - Saved replies (templates) with merge fields");
    println!("    - Workflows (if-this-then-that rules, very simple UX)");
    println!("    - Collision detection — see who else is viewing/typing on the same conversation");
    println!("    - Internal notes (mentions, threads private to agents)");
    println!("    - Customer Properties — track custom attributes per customer");
    println!("    - Multi-language reply templates");
    println!("  Beacon (in-app widget):");
    println!("    - Live chat + offline messaging in a single widget");
    println!("    - Contextual KB article suggestions before user submits");
    println!("    - Customizable widget look (any site can match brand)");
    println!("    - Triggers — show widget based on URL/scroll/inactivity");
    println!("    - Customer profile sidebar inside Mailbox shows prior chats");
    println!("  Docs (knowledge base):");
    println!("    - Branded help site with collections + categories + articles");
    println!("    - Site-wide search with analytics");
    println!("    - Article ratings + 'related articles'");
    println!("    - Bulk article import + CSV export");
    println!("    - Restricted Docs sites (private to logged-in customers)");
    println!("  AI features (recently added):");
    println!("    - AI Summarize — collapse a long conversation into 1 paragraph for the next agent");
    println!("    - AI Assist — improve, shorten, or change tone of an agent draft");
    println!("    - AI Answers (beta) — draft reply from KB + prior conversation context");
    println!("    - launched Sep 2023, OpenAI-powered");
    println!("  Reporting:");
    println!("    - Volume by mailbox + channel + tag");
    println!("    - Response time + Handle time + Resolution time");
    println!("    - Happiness Score (built-in CSAT after every email)");
    println!("    - Agent productivity dashboards");
    println!("    - Customer effort score (CES) tracking");
    println!("  Integrations: 100+ apps");
    println!("              Slack, Salesforce, HubSpot, Jira, Trello, Asana, GitHub");
    println!("              Shopify, BigCommerce, WooCommerce (deep e-commerce focus)");
    println!("              Mailchimp, Webhooks, Zapier");
    println!("              REST API + webhooks + iOS/Android SDKs for Beacon");
    println!("  Customers: 12,000+ companies");
    println!("            Buffer (early adopter + case study), Reddit, Lemonade, Atlassian (parts), Trello");
    println!("            sweet spot: SaaS startups, e-commerce SMBs, mission-driven brands");
    println!("            very strong in 5-50 agent teams that hate enterprise helpdesk UX");
    println!("  Critique: doesn't scale comfortably past ~500 agents — built for small intimate teams");
    println!("           voice/phone support limited compared to Zendesk Talk or Freshcaller");
    println!("           workflow engine simpler than Zendesk Triggers — can hit ceiling");
    println!("           reporting customization less powerful than Explore");
    println!("  Differentiator: 'doesn't look like a helpdesk' — customers + agents both prefer it on small teams");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "helpscout".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_helpscout(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_helpscout};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/helpscout"), "helpscout");
        assert_eq!(basename(r"C:\bin\helpscout.exe"), "helpscout.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("helpscout.exe"), "helpscout");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_helpscout(&["--help".to_string()], "helpscout"), 0);
        assert_eq!(run_helpscout(&["-h".to_string()], "helpscout"), 0);
        let _ = run_helpscout(&["--version".to_string()], "helpscout");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_helpscout(&[], "helpscout");
    }
}
