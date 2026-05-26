#![deny(clippy::all)]
//! tooljet-cli — personality CLI for ToolJet, the open-source
//! MIT-licensed low-code platform for building internal tools.
//!
//! Founded 2021 in Bengaluru by Navaneeth Pandiyaraj (CEO) after years
//! building internal tools at consultancies; positioned as a more
//! permissively-licensed (MIT vs Appsmith's Apache 2.0 vs Budibase's GPL)
//! alternative to Retool. Y Combinator W22. Picked up modest seed +
//! Series A funding from Ghost VC + Nexus Venture Partners; engineering
//! mostly remote out of India. ToolJet's pitch is full GitOps-style
//! workflow + multi-page apps + a wide connector library, with the
//! lightest licence in the open-source low-code space.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — ToolJet open-source MIT-licensed low-code personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Navaneeth Pandiyaraj 2021 Bengaluru YC W22; MIT");
    println!("    builder       Drag-drop component canvas + multi-page apps");
    println!("    datasources   40+ first-class connectors + REST + GraphQL");
    println!("    workflows     Server-side workflows + cron + webhook triggers");
    println!("    selfhost      Docker + Helm + Kubernetes + Heroku one-click");
    println!("    licence       MIT licence — most permissive in open-source low-code");
    println!("    pricing       Free OSS + Cloud + Enterprise");
    println!("    customers     Engineering-led SMB + startup + EU public sector");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("tooljet-cli 0.1.0 (open-source-MIT-low-code personality build)"); }

fn run_about() {
    println!("ToolJet, Inc.");
    println!("  Founded:    2021, Bengaluru, India.");
    println!("  Founder:    Navaneeth Pandiyaraj (CEO).");
    println!("  Cohort:     Y Combinator W22.");
    println!("  Licence:    MIT (most permissive in the open-source low-code segment).");
    println!("  Backers:    Ghost VC, Nexus Venture Partners, YC, several angels.");
    println!("  Funding:    seed + Series A; modest by category standards.");
    println!("  Team:       distributed remote, primary engineering hub India.");
    println!("  Position:   permissive-licence Retool alternative — corporate-friendly OSS.");
}

fn run_builder() {
    println!("Visual builder.");
    println!("  Drag-drop canvas: tables, charts, forms, kanban, calendar, file pickers,");
    println!("  modal, drawer, tabs, list views, maps, rich-text, custom HTML/JS components.");
    println!("  Multi-page apps + reusable component groups + layout breakpoints.");
    println!("  Per-property {{ }} JS expressions with dependency-tracking reactivity.");
    println!("  Run-as-role preview + per-component permission overrides.");
    println!("  Themes, branding, dark mode, custom CSS hook for advanced theming.");
}

fn run_datasources() {
    println!("Data sources.");
    println!("  40+ connectors: Postgres, MySQL, MS SQL, Oracle, Mongo, Redis, BigQuery,");
    println!("  Snowflake, Redshift, S3, DynamoDB, ElasticSearch, Cassandra, ClickHouse,");
    println!("  REST, GraphQL, SOAP, gRPC, Stripe, Twilio, SendGrid, Slack, Notion, Airtable,");
    println!("  Google Sheets, Google Calendar, OpenAI, Anthropic, custom plugins via SDK.");
    println!("  Query transformer: post-query JS to reshape responses.");
    println!("  Cached queries + scheduled refresh + run-on-page-load options.");
    println!("  Plugin SDK lets customers build proprietary connectors.");
}

fn run_workflows() {
    println!("Workflows.");
    println!("  Server-side workflow engine alongside the app builder.");
    println!("  Visual node-graph editor: trigger -> action -> conditional -> action.");
    println!("  Triggers: cron, webhook, app event, manual run.");
    println!("  Actions: query, REST, send email, Slack, run JS, AI prompt, branch, loop.");
    println!("  Run history + retry semantics + per-run logs for debugging.");
    println!("  Closer in style to n8n than to Appsmith Workflows.");
}

fn run_selfhost() {
    println!("Self-hosting.");
    println!("  Single Docker container quickstart for solo developers.");
    println!("  Docker Compose for production multi-service installs.");
    println!("  Official Helm chart for Kubernetes deployments + ingress + replicas.");
    println!("  One-click Heroku / Render / DigitalOcean App Platform buttons.");
    println!("  Air-gapped install path supported.");
    println!("  Embedded Postgres + Redis by default; external DB option for HA.");
}

fn run_licence() {
    println!("Licence positioning.");
    println!("  MIT licence — the most permissive in the open-source low-code stack.");
    println!("    Appsmith:  Apache 2.0  (also corporate-friendly, patent grant)");
    println!("    Budibase:  GPL-3.0     (copyleft — commercial-use scrutiny needed)");
    println!("    ToolJet:   MIT         (corporate-friendly, no patent grant)");
    println!("  No CLA required to contribute back upstream.");
    println!("  This positions ToolJet as the safest pick for risk-averse legal teams.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free / Community: open source, self-host, unlimited users + apps.");
    println!("  Cloud Free:   managed-hosting free tier with usage caps.");
    println!("  Cloud Team:   ~$10 per user per month, branded + custom domain.");
    println!("  Business:     advanced permissions + audit logs + SSO.");
    println!("  Enterprise:   custom — dedicated success, SLA, air-gapped support.");
    println!("  Like peers, free-tier feature parity is large; monetisation on org scale.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: engineering-led startups + SMBs 5-300 employees.");
    println!("  Industries: SaaS startups, fintech, e-commerce ops, EU public sector,");
    println!("  university IT, NGOs and non-profits attracted by the MIT licence.");
    println!("  Geographic: heavy India + EU + LATAM; growing US presence.");
    println!("  Typical journey: solo engineer self-hosts on a VM, builds 2-3 ops tools,");
    println!("  team adopts them, eventually upgrades to Cloud or Enterprise for SSO.");
    println!("  Frequent positioning: 'pick ToolJet if your legal team flagged the GPL'.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "tooljet-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "builder" => run_builder(),
        "datasources" => run_datasources(),
        "workflows" => run_workflows(),
        "selfhost" => run_selfhost(),
        "licence" | "license" => run_licence(),
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
        run_builder();
        run_datasources();
        run_workflows();
        run_selfhost();
        run_licence();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("tooljet-cli");
        print_version();
    }
}
