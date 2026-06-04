#![deny(clippy::all)]

//! make-cli — OurOS Make.com (visual automation, Prague + global, Notion Capital backed)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_make(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: make [OPTIONS]");
        println!("Make.com (OurOS) — visual workflow automation (was Integromat)");
        println!();
        println!("Options:");
        println!("  --scenarios            Scenarios (the visual workflows)");
        println!("  --templates            Template library");
        println!("  --custom-apps          Custom Apps (build your own integrations)");
        println!("  --make-ai              Make AI (LLM-augmented scenarios)");
        println!("  --make-grid            Make Grid (the visual canvas)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Make 2024 (OurOS)"); return 0; }
    println!("Make.com 2024 (OurOS) — Visual Automation Platform");
    println!("  Vendor: Make.com (Celonis subsidiary since 2020 — Prague, Czech Republic + global)");
    println!("  Founder: Patrik Šimek + Adam Tilton + Ondrej Andrlik, 2012 (as 'Integromat')");
    println!("          founded in Brno, Czech Republic");
    println!("          'Integromat' name 2012-2022 — rebranded to 'Make' in early 2022");
    println!("          Differentiator from day one: visual canvas with bubbles + connecting lines");
    println!("          richer logic than Zapier (loops, routers, aggregators, iterators)");
    println!("  Corporate history:");
    println!("         Acquired by Celonis April 2020 (terms undisclosed, estimated ~$80M)");
    println!("         Celonis = German process-mining unicorn ($13B valuation 2022)");
    println!("         Strategic for Celonis: process-mining + iPaaS = full process-execution platform");
    println!("         Patrik Šimek remains as CEO of Make division");
    println!("         estimated $100M+ ARR (private — within Celonis)");
    println!("  Strategic position: 'visual automation — more powerful than Zapier, simpler than Workato':");
    println!("                    pitch: 'build complex automations visually — no code, but more than no-code'");
    println!("                    target: SMB to mid-market — sweet spot for marketing/ops teams + agencies");
    println!("                    primary competitor: Zapier (lower-cost), n8n (open-source), Workato (enterprise)");
    println!("                    secondary: Microsoft Power Automate (E5 bundle), Pipedream, Tray.io");
    println!("                    Make's wedge: visual canvas UI + sophisticated logic primitives + lower price than enterprise iPaaS");
    println!("                    'Zapier on steroids' positioning resonates with power users");
    println!("  Pricing (per-operation model — friendly):");
    println!("    Free: 1,000 ops/month, 2 active scenarios");
    println!("    Core: $9/mo (10K ops/month, unlimited active scenarios)");
    println!("    Pro: $16/mo (10K ops + advanced features: custom variables, full execution logs)");
    println!("    Teams: $29/mo (10K ops + team workspaces + RBAC)");
    println!("    Enterprise: custom (SSO, audit logs, SLA)");
    println!("    typically 30-50% cheaper than equivalent Zapier plan for high-volume");
    println!("  Product portfolio:");
    println!("    1. Scenarios (the visual workflows):");
    println!("       - Visual canvas: modules connected by data-flow lines");
    println!("       - 'Bubbles' = modules; 'connectors' = data flow");
    println!("       - Routers (if/then branches with multiple paths)");
    println!("       - Iterators (loop over arrays)");
    println!("       - Aggregators (collect items into arrays)");
    println!("       - Error handlers (fallback routes)");
    println!("       - Sleep + Repeater + Resume On Error semantics");
    println!("    2. Template library:");
    println!("       - Pre-built scenario templates");
    println!("       - Searchable by app + use case");
    println!("    3. Custom Apps (build your own integration):");
    println!("       - Define an app via REST API config");
    println!("       - Publish + share with community");
    println!("       - 200+ community apps in addition to ~1,500 official apps");
    println!("    4. Make AI (2023 — LLM features):");
    println!("       - 'AI Scenario Builder' — generate scenarios from natural language");
    println!("       - OpenAI + Anthropic + Azure OpenAI built-in modules");
    println!("       - 'Mistral by Make' module = native LLM step");
    println!("    5. Make Grid (the visual canvas):");
    println!("       - Bubble-style flow visualization");
    println!("       - Visual diff between scenario versions");
    println!("       - Strong UX advantage over linear (Zapier-style) builders");
    println!("    6. Make Apps SDK:");
    println!("       - Build paid + free integration apps");
    println!("       - Distribute via Make App Store");
    println!("    7. Make for Slack / Teams (built-in notifications + commands):");
    println!("       - Trigger scenarios from chat");
    println!("    8. Make On-Prem / Single-tenant:");
    println!("       - Enterprise tier for regulated industries (rare)");
    println!("  Visual canvas (the UX differentiator):");
    println!("    - Modules drawn as labelled circles ('bubbles')");
    println!("    - Data flow shown as lines connecting modules");
    println!("    - Routers visualized as branching paths");
    println!("    - Iterators + aggregators shown as containers");
    println!("    - Strong visual intuition for complex flows");
    println!("    - Inspired the 'Make Grid' visual aesthetic now widely copied");
    println!("  Celonis synergy:");
    println!("    - Celonis Process Mining + Make Automation = end-to-end process execution");
    println!("    - 'Process discovery → process optimization → process automation'");
    println!("    - Celonis Execution Management System (EMS) embeds Make for action layer");
    println!("    - Make standalone customers remain core business");
    println!("  Integrations (~1,500 official + 200 community apps):");
    println!("    - Mainstream SaaS: Salesforce, HubSpot, Slack, Microsoft 365, Google Workspace");
    println!("    - Marketing: Mailchimp, ActiveCampaign, ConvertKit, Klaviyo, Iterable");
    println!("    - E-commerce: Shopify, WooCommerce, BigCommerce, Stripe, PayPal");
    println!("    - Forms: Typeform, JotForm, Wufoo, Google Forms");
    println!("    - Sheets + DB: Google Sheets, Airtable, Notion, MySQL, PostgreSQL");
    println!("    - File storage: Google Drive, Dropbox, Box, OneDrive");
    println!("    - DevOps: GitHub, GitLab, Jira, Linear, Trello");
    println!("    - AI: OpenAI (deep), Anthropic, Mistral, Pinecone, Replicate, ElevenLabs");
    println!("    - Notifications: Slack, Discord, Telegram, Twilio, SendGrid");
    println!("  Make CLI usage:");
    println!("    make login --org my-team");
    println!("    make scenario list --status active");
    println!("    make scenario run --scenario-id ABC123 --input @input.json");
    println!("    make scenario export --scenario-id ABC123 --output scenario.json");
    println!("    make template browse --app shopify --category sync");
    println!("    make custom-app create --name 'My API' --base-url https://api.example.com");
    println!("    make ai generate --prompt 'sync new Shopify orders to Slack'");
    println!("  Customers (~500K+ users):");
    println!("    - SMB + marketing agencies + RevOps teams sweet spot");
    println!("    - Heavy in Europe (Czech + UK + DACH region)");
    println!("    - International users from 175+ countries");
    println!("    - Enterprise expansion: Heineken, Spotify, Razer, Adidas (some teams)");
    println!("    - Celonis enterprise customers cross-sold (Coca-Cola, BMW, Vodafone, Lufthansa)");
    println!("  Critique: SMB sweet spot competes hard with Zapier on price + simplicity");
    println!("           enterprise governance lighter than Workato/MuleSoft");
    println!("           Celonis ownership = some uncertainty about long-term standalone roadmap");
    println!("           AI features competitive but not leading (vs Zapier's MCP positioning)");
    println!("           connector count (~1,500) less than Zapier (7,000)");
    println!("           Microsoft Power Automate E5 bundle threatens prosumer base");
    println!("           operation-based pricing can balloon for high-volume use cases");
    println!("  Differentiator: bubble-style visual canvas (UX differentiator — more powerful than Zapier's linear UI) + advanced logic primitives (routers, iterators, aggregators) + Celonis ownership (process-mining synergy) + ~1,500 apps + 500K+ users + lower price than Zapier for high-volume + European-founded (Brno, Czech Republic) — the visual automation platform that power users choose when Zapier's linear flows aren't expressive enough but enterprise iPaaS is overkill");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "make".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_make(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_make};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/make"), "make");
        assert_eq!(basename(r"C:\bin\make.exe"), "make.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("make.exe"), "make");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_make(&["--help".to_string()], "make"), 0);
        assert_eq!(run_make(&["-h".to_string()], "make"), 0);
        let _ = run_make(&["--version".to_string()], "make");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_make(&[], "make");
    }
}
