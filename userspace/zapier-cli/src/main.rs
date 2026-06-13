#![deny(clippy::all)]

//! zapier-cli — SlateOS Zapier (SMB/prosumer automation, San Francisco, fully remote, profitable)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zapier(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zapier [OPTIONS]");
        println!("Zapier (Slate OS) — workflow automation for SMB + prosumers (private, profitable)");
        println!();
        println!("Options:");
        println!("  --zaps                 Zaps (automation workflows: trigger + action)");
        println!("  --tables               Zapier Tables (built-in database)");
        println!("  --interfaces           Zapier Interfaces (form builder + workflows UI)");
        println!("  --chatbots             Zapier Chatbots (no-code AI chatbot builder)");
        println!("  --ai-actions           AI Actions (LLM tool-calling into Zapier)");
        println!("  --canvas               Canvas (visual workflow map)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zapier 2024 (Slate OS)"); return 0; }
    println!("Zapier 2024 (Slate OS) — Automation for the Rest of Us");
    println!("  Vendor: Zapier, Inc. (incorporated Delaware, HQ San Francisco — fully remote since 2011)");
    println!("  Founders: Wade Foster + Bryan Helmig + Mike Knoop, 2011");
    println!("          founded in Columbia, MO during Y Combinator (W12 batch)");
    println!("          name 'Zapier' = the 'zap' (automation event) + suffix '-ier'");
    println!("          Wade Foster: long-time CEO + remote-work evangelist");
    println!("          Bryan Helmig: CTO, technical co-founder");
    println!("          Mike Knoop: co-founder, left 2023 to start AGI lab (Lab 42, ARC Prize)");
    println!("          one of the iconic fully-remote companies in tech (no HQ, distributed workforce 800+)");
    println!("  Private funding (notably: bootstrapped + profitable):");
    println!("         Y Combinator W12 + ~$1.4M seed (2012) — Bessemer, A16Z early backers");
    println!("         Series A 2014 ($1.2M Bessemer)");
    println!("         NO Series B or C — bootstrapped through profitability");
    println!("         Sequoia-led $5B tender offer secondary 2021 (employee liquidity)");
    println!("         valued $5B at that round, no primary capital");
    println!("         Profitable for ~10 years (rare in SaaS)");
    println!("         estimated $300M+ ARR (private)");
    println!("  Strategic position: 'easiest way to automate work — no-code for SMB + prosumers':");
    println!("                    pitch: 'connect 7,000+ apps with Zaps that anyone can build'");
    println!("                    target: SMB + freelancer + marketer + ops person (NOT IT department)");
    println!("                    primary competitor: Make (was Integromat), n8n, Microsoft Power Automate (lower-tier)");
    println!("                    secondary (enterprise tier): Workato, Tray.io, Boomi (Zapier moves upmarket slowly)");
    println!("                    Zapier's wedge: 7,000+ app integrations (most in industry) + simplicity + free tier");
    println!("                    'the app store of automation' — most app integrations of any iPaaS");
    println!("  Pricing (per-task model — friendly for SMB):");
    println!("    Free tier: 100 tasks/month + 2-step Zaps");
    println!("    Starter: $19.99/mo (750 tasks, multi-step Zaps, conditions)");
    println!("    Professional: $49/mo (2K tasks + paths + filters + advanced features)");
    println!("    Team: $69/mo (50K tasks, shared workspaces, premier support)");
    println!("    Company: custom (SSO, governance, audit logs)");
    println!("    typically 10-100x cheaper than enterprise iPaaS for similar volumes");
    println!("  Product portfolio (the Zapier Platform):");
    println!("    1. Zaps (the core automation):");
    println!("       - Trigger (when X happens in app A) → Action (do Y in app B)");
    println!("       - Multi-step Zaps (chain many actions)");
    println!("       - Paths (conditional branching: if/then/else)");
    println!("       - Filters, formatters, delays, loops");
    println!("       - 7,000+ supported apps (largest in industry)");
    println!("    2. Zapier Tables (2023 — built-in database):");
    println!("       - Lightweight database for storing data between Zap runs");
    println!("       - Compete with: Airtable, Smartsheet, NocoDB");
    println!("    3. Zapier Interfaces (2023 — form builder + custom UIs):");
    println!("       - Forms, kanban boards, dashboards");
    println!("       - Trigger Zaps from custom UIs");
    println!("       - Compete with: Softr, Glide, Bubble (lighter-weight)");
    println!("    4. Zapier Chatbots (2023 — AI chatbot builder):");
    println!("       - No-code AI chatbots powered by GPT-4 / Claude");
    println!("       - Embed on websites or trigger Zaps");
    println!("       - Compete with: Voiceflow, Intercom Fin, Drift");
    println!("    5. AI Actions / Zapier MCP (2024):");
    println!("       - Expose Zapier integrations as 'tools' for LLM tool-calling");
    println!("       - ChatGPT, Claude can trigger Zaps via natural language");
    println!("       - Model Context Protocol (MCP) server: Zapier as universal LLM-tool layer");
    println!("    6. Canvas (workflow visualization):");
    println!("       - Diagrams of all Zaps + dependencies");
    println!("       - Useful for ops teams managing hundreds of Zaps");
    println!("    7. Transfer (bulk data import — was retired/integrated)");
    println!("    8. Webhooks by Zapier:");
    println!("       - Generic webhook trigger + action");
    println!("       - Most-used 'integration' on the platform (universal)");
    println!("  Zapier ecosystem:");
    println!("    - 7,000+ apps integrated (vs MuleSoft 200, Workato 1,000)");
    println!("    - 'Zapier Platform' for app developers to build their own integration");
    println!("    - Featured integration partner for many SaaS startups (Zapier integration = legitimacy)");
    println!("    - 2.2M+ users across 170+ countries");
    println!("    - 'Built-in Apps' (Tables, Interfaces, Webhooks, Chatbots) plus 3rd-party");
    println!("  Integrations (the long tail — 7,000+ apps):");
    println!("    - Mainstream SaaS: Salesforce, HubSpot, Gmail, Slack, Microsoft 365, Google Sheets, Notion");
    println!("    - Niche tools: every podcast platform, every form builder, every newsletter tool");
    println!("    - CRMs: Pipedrive, Close, ActiveCampaign, Mailchimp, HubSpot");
    println!("    - Forms: Typeform, JotForm, Wufoo, Google Forms");
    println!("    - Calendars: Google Calendar, Outlook, Calendly, Cal.com");
    println!("    - Sheets: Google Sheets, Excel, Airtable, Smartsheet");
    println!("    - Storage: Google Drive, Dropbox, OneDrive, Box");
    println!("    - Project mgmt: Trello, Asana, ClickUp, Monday, Notion");
    println!("    - Devops: GitHub, GitLab, PagerDuty, Linear");
    println!("    - AI: OpenAI, Anthropic, Perplexity, Pinecone, Replicate (recent additions)");
    println!("  Zapier CLI usage:");
    println!("    zapier login");
    println!("    zapier zap list --status enabled");
    println!("    zapier zap create --trigger 'gmail.new_email' --action 'slack.post_message'");
    println!("    zapier zap run --zap-id ABC123 --test");
    println!("    zapier tables create --name leads --columns 'name,email,company'");
    println!("    zapier ai-actions enable --app slack --action post_message");
    println!("  Customers (2.2M+ users):");
    println!("    - SMB + freelancer + solopreneur sweet spot");
    println!("    - Marketing teams, ops teams, founders at startups");
    println!("    - Enterprise expansion: ~10K paying enterprise customers");
    println!("    - Major: Adobe, Spotify, Asana (use Zapier internally for ops automation)");
    println!("    - International: ~70% of users outside US (broadest geography of iPaaS)");
    println!("  Critique: scaling concerns for high-volume automations (task pricing balloons)");
    println!("           enterprise features lighter than Workato/Boomi (governance, SSO, audit)");
    println!("           Microsoft Power Automate's Office 365 bundling threatens prosumer base");
    println!("           Make (Integromat) increasingly competitive on visual workflow UX");
    println!("           AI Chatbots + Interfaces feel like adjacent bets, not core competence");
    println!("           Mike Knoop departure 2023 = loss of co-founder energy");
    println!("           bootstrapped + profitable means slower feature velocity than VC-funded competitors");
    println!("           expected IPO uncertain — no public capital strategy");
    println!("  Differentiator: 7,000+ app integrations (most in industry) + free tier + bootstrapped profitable for ~10 years + ~$5B last-round secondary valuation + 2.2M+ users + 'Zapier Platform' for SaaS partners + Zapier MCP for LLM tool-calling (2024 AI-era pivot) + fully-remote since founding — the prosumer/SMB automation platform that lets anyone connect 7,000 apps without writing code");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zapier".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zapier(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zapier};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zapier"), "zapier");
        assert_eq!(basename(r"C:\bin\zapier.exe"), "zapier.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zapier.exe"), "zapier");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zapier(&["--help".to_string()], "zapier"), 0);
        assert_eq!(run_zapier(&["-h".to_string()], "zapier"), 0);
        let _ = run_zapier(&["--version".to_string()], "zapier");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zapier(&[], "zapier");
    }
}
