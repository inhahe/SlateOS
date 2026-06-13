#![deny(clippy::all)]

//! freshdesk-cli — SlateOS Freshdesk (Freshworks helpdesk, Zendesk's main SMB competitor)
//!
//! Single personality: `freshdesk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_freshdesk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: freshdesk [OPTIONS]");
        println!("Freshdesk (Slate OS) — Freshworks helpdesk + omnichannel support");
        println!();
        println!("Options:");
        println!("  --free                 Free plan (up to 10 agents)");
        println!("  --growth               Growth $15/agent/mo");
        println!("  --pro                  Pro $49/agent/mo");
        println!("  --enterprise           Enterprise $79/agent/mo");
        println!("  --omnichannel          Omnichannel bundle (Support + Messaging + Contact Center)");
        println!("  --freddy               Freddy AI add-on (auto-triage + reply suggestions)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Freshdesk 2024 (Slate OS)"); return 0; }
    println!("Freshdesk 2024 (Slate OS)");
    println!("  Vendor: Freshworks Inc. (San Mateo, CA + Chennai, India — NASDAQ:FRSH)");
    println!("  Origin story: Girish Mathrubootham read a Hacker News thread about Zendesk's 60-300% price hike");
    println!("              built Freshdesk as the cheaper, friendlier alternative — launched 2011");
    println!("              entire Freshworks empire grew from this single product");
    println!("  Founders: Girish Mathrubootham (CEO) + Shan Krishnasamy (CTO)");
    println!("           founded in a small Chennai office — Series A from Accel + Tiger 2012");
    println!("  Company: IPO Sep 2021 NASDAQ:FRSH at $36 — popped to $47, now ~$13-18");
    println!("          ~$700M+ revenue, 67,000+ customers");
    println!("          one of largest Indian SaaS IPOs (Indian-American dual HQ)");
    println!("          ~7,400 employees globally");
    println!("  Pricing: Free up to 10 agents (basic email ticketing + KB)");
    println!("          Growth $15/agent/mo (automations, marketplace apps, time tracking)");
    println!("          Pro $49/agent/mo (custom roles, multi-product, advanced reporting, sandbox)");
    println!("          Enterprise $79/agent/mo (audit logs, IP allow-list, custom objects, agent shifts)");
    println!("          Omnichannel bundles: Growth+ $29/agent/mo, Pro+ $69/agent/mo, Enterprise+ $109/agent/mo");
    println!("  Core Freshdesk features:");
    println!("    - Ticketing across email, web form, social (FB/X/Insta DM), phone, chat");
    println!("    - SLA policies with escalation rules");
    println!("    - Automations (dispatcher 'observer' triggers)");
    println!("    - Scenario automations (multi-action canned macros)");
    println!("    - Round-robin + load-based ticket assignment");
    println!("    - Multi-product (one workspace, multiple product KBs/teams)");
    println!("    - Parent-child + sibling tickets for complex cases");
    println!("    - Customer Portal (self-service ticket history + KB)");
    println!("    - CSAT surveys");
    println!("  Freshworks suite (often bundled with Freshdesk):");
    println!("    - Freshchat — live chat + bots (Pro+)");
    println!("    - Freshcaller — cloud phone / call center");
    println!("    - Freshservice — IT service mgmt (ITSM, separate ITIL product)");
    println!("    - Freshmarketer — email marketing automation");
    println!("    - Freshsales — sales CRM");
    println!("  Freddy AI (Freshworks AI brand):");
    println!("    - Auto-triage tickets (classify type, urgency, language)");
    println!("    - Suggest canned responses to agents based on similar past tickets");
    println!("    - Predicted CSAT for in-flight conversations");
    println!("    - Freddy Self-Service bot for customer-facing chat deflection");
    println!("    - Freddy Copilot (2024) — generative AI reply drafts (GPT-powered)");
    println!("  Knowledge Base:");
    println!("    - Multi-language KB with translation workflows");
    println!("    - Article feedback (thumbs up/down) + analytics");
    println!("    - Article suggester (inline in agent reply form)");
    println!("    - Search optimization via 'Search Trends' analytics");
    println!("  Integrations: 1,000+ marketplace apps");
    println!("              Slack, Salesforce, Hubspot, Jira, Office 365, Google Workspace, Shopify");
    println!("              public REST API + webhooks");
    println!("              SDK for embeddable widget");
    println!("  Customers: 67,000+ across Freshworks, ~30,000+ specifically on Freshdesk");
    println!("            Bridgestone, Klarna, Decathlon, Pearson, American Express (parts), Honda");
    println!("            sweet spot: SMB + mid-market, 10-1,000 agents");
    println!("            strong globally — Asia, Europe, LATAM (vs Zendesk's NA strength)");
    println!("  Critique: AI features still maturing — Freddy lags Intercom's Fin in deflection numbers");
    println!("           UI improvements lag Zendesk Agent Workspace");
    println!("           pricing tier jumps non-linear — Enterprise needed for many 'enterprise' basics");
    println!("           Freshworks broader story (sales + IT + support combined) inconsistent in execution");
    println!("  Differentiator: best free tier in helpdesk category + global price-competitive vs Zendesk");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "freshdesk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_freshdesk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_freshdesk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/freshdesk"), "freshdesk");
        assert_eq!(basename(r"C:\bin\freshdesk.exe"), "freshdesk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("freshdesk.exe"), "freshdesk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_freshdesk(&["--help".to_string()], "freshdesk"), 0);
        assert_eq!(run_freshdesk(&["-h".to_string()], "freshdesk"), 0);
        let _ = run_freshdesk(&["--version".to_string()], "freshdesk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_freshdesk(&[], "freshdesk");
    }
}
