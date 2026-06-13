#![deny(clippy::all)]

//! livechat-cli — Slate OS LiveChat (Polish-listed pure-play live chat platform)
//!
//! Single personality: `livechat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_livechat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: livechat [OPTIONS]");
        println!("LiveChat (Slate OS) — pure-play live chat from Wroclaw, Poland");
        println!();
        println!("Options:");
        println!("  --starter              Starter $20/agent/mo");
        println!("  --team                 Team $41/agent/mo");
        println!("  --business             Business $59/agent/mo");
        println!("  --enterprise           Enterprise (custom)");
        println!("  --chatbot              ChatBot.com — sister product, $52/mo standalone");
        println!("  --helpdesk             HelpDesk.com — sister product, ticketing");
        println!("  --knowledgebase        KnowledgeBase.com — sister KB product");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LiveChat 2024 (Slate OS)"); return 0; }
    println!("LiveChat 2024 (Slate OS)");
    println!("  Vendor: Text S.A. (Wroclaw, Poland — WSE:TXT, formerly LiveChat Software S.A.)");
    println!("        rebranded from LiveChat Software → Text in 2023 to reflect multi-product portfolio");
    println!("  Founders: Mariusz Cieply + Maciej Jarzębowski + Urszula Jarzębowska, 2002 (!!)");
    println!("          one of the longest-running live chat companies in the world");
    println!("          originally bought existing live chat code from US devs, modernized iteratively");
    println!("  Founded: 2002 in Wroclaw — bootstrapped to profitability before IPO");
    println!("          IPO 2014 on Warsaw Stock Exchange (WSE:TXT, formerly WSE:LVC)");
    println!("          ~$70M+ ARR, ~37,000 paying customer accounts");
    println!("          notably profitable + dividend-paying tech company (rare among SaaS)");
    println!("  Product portfolio (Text S.A. roof brand):");
    println!("    - LiveChat — flagship live chat (this product)");
    println!("    - ChatBot.com — visual flow bot builder ($52/mo entry)");
    println!("    - HelpDesk.com — ticketing add-on ($29/agent/mo)");
    println!("    - KnowledgeBase.com — KB product ($49/mo entry)");
    println!("    - OpenWidget.com — free embeddable customer engagement widget");
    println!("  Pricing: Starter $20/agent/mo (60-day chat history)");
    println!("          Team $41/agent/mo (unlimited history, multiple branding, advanced reports)");
    println!("          Business $59/agent/mo (work scheduler, staffing prediction, SSO)");
    println!("          Enterprise custom (HIPAA, dedicated CSM, SLAs)");
    println!("          all billed annually; monthly +20%");
    println!("  Live chat features:");
    println!("    - Embeddable widget (customizable: colors, position, eye-catcher, mobile)");
    println!("    - Multi-language widget (auto-detect or manual)");
    println!("    - Customer details sidebar (geo, browser, current page, past chats)");
    println!("    - File sharing in chat (images, PDFs)");
    println!("    - Canned responses (typing '#refund' expands template)");
    println!("    - Tagging + categorization");
    println!("    - Chat surveys (pre + post chat)");
    println!("    - Sneak peek — see what customer is typing BEFORE they hit send");
    println!("    - Ticketing (escalate chat to ticket via HelpDesk integration)");
    println!("  Agent workspace:");
    println!("    - Multi-chat handling (3-10 concurrent depending on tier)");
    println!("    - Skill-based routing (auto-route by topic, language, customer tag)");
    println!("    - Internal chat (agent-to-agent without leaving customer view)");
    println!("    - Chat transfer + invitation (loop in colleagues)");
    println!("    - Activity reports per agent (chats handled, response time, CSAT)");
    println!("  Automation:");
    println!("    - Targeted messages (rule-based, similar to Intercom)");
    println!("    - 'Eye-catcher' floating CTA (configurable per page/visitor)");
    println!("    - Goals (track chat conversions to specific URLs)");
    println!("    - Routing rules (groups, departments, agent skills)");
    println!("  AI features (recent additions):");
    println!("    - AI Assist — reply suggestions to agents based on past resolutions");
    println!("    - Tone of voice adjustment");
    println!("    - Multi-language live translation");
    println!("    - integration with ChatBot.com for deflection (single auth, shared customer record)");
    println!("  Integrations: 200+ marketplace apps");
    println!("              Shopify, Magento, WooCommerce, BigCommerce, Squarespace, Wix");
    println!("              Salesforce, HubSpot, Pipedrive, Mailchimp, Slack, Microsoft Teams");
    println!("              Zapier (5K+ apps)");
    println!("              REST API + webhooks + JavaScript SDK for custom widgets");
    println!("  Customers: 37,000+ companies (heavy SMB + e-commerce)");
    println!("            Mercedes-Benz (regional sites), Pearson, Adobe (some teams), Atlassian (parts)");
    println!("            McDonald's (regional), Allianz (parts), AccuWeather, Joybird");
    println!("            sweet spot: SMB + mid-market doing high-conversion website chat");
    println!("            popular in Europe, Latin America, and Asia");
    println!("  Critique: pure-play chat ceiling — less of a fit if you want unified email/chat/social helpdesk");
    println!("           pricing higher than Tidio/Tawk.to for solo entrepreneurs");
    println!("           AI features lag Intercom Fin / Zendesk Advanced AI");
    println!("           Text S.A. rebrand created some brand confusion (LiveChat vs Text vs ChatBot.com)");
    println!("  Differentiator: 22 years of pure focus on chat — deepest chat-only widget feature set + profitable Polish public company");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "livechat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_livechat(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_livechat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/livechat"), "livechat");
        assert_eq!(basename(r"C:\bin\livechat.exe"), "livechat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("livechat.exe"), "livechat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_livechat(&["--help".to_string()], "livechat"), 0);
        assert_eq!(run_livechat(&["-h".to_string()], "livechat"), 0);
        let _ = run_livechat(&["--version".to_string()], "livechat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_livechat(&[], "livechat");
    }
}
