#![deny(clippy::all)]
//! noloco-cli — personality CLI for Noloco, the no-code internal-tools
//! + client-portal platform.
//!
//! Founded 2020 in Dublin by Darragh Mc Kay (CEO, ex-Intercom + ex-Pointy
//! engineering) and Simon Kerr (CTO, ex-Intercom + ex-Pointy engineering).
//! Both founders previously built at Intercom and at Pointy (acquired by
//! Google in 2020), then started Noloco to address what they saw as the
//! gap between Airtable's data layer and a real business-facing portal +
//! internal tool. Picked up seed + Series A from Atlantic Bridge + Frontline.
//! Noloco's pitch is 'tool on top of your existing data' — sits over
//! Airtable, Google Sheets, Postgres, HubSpot, MySQL, with no obligation
//! to migrate data into the platform.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Noloco no-code internal-tools + client-portal personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Mc Kay + Kerr 2020 Dublin, ex-Intercom + ex-Pointy");
    println!("    builder       Drag-drop pages + tables + forms + record views");
    println!("    datasources   Airtable + Sheets + Postgres + MySQL + HubSpot + Noloco DB");
    println!("    portals       Client portals + member portals + role-based access");
    println!("    workflows     Automations + triggers + conditional logic + webhooks");
    println!("    ai            Noloco AI — agents + automations + content suggestions");
    println!("    pricing       Starter + Pro + Business + Enterprise tiers");
    println!("    customers     SMB ops teams + agencies + Airtable + HubSpot stack");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("noloco-cli 0.1.0 (no-code-portal-and-internal-tools personality build)"); }

fn run_about() {
    println!("Noloco Limited.");
    println!("  Founded:    2020, Dublin, Ireland.");
    println!("  Founders:   Darragh Mc Kay (CEO; ex-Intercom + ex-Pointy engineering) +");
    println!("              Simon Kerr (CTO; ex-Intercom + ex-Pointy engineering).");
    println!("              Both previously built engineering at Pointy (Google acq. 2020).");
    println!("  Backers:    Atlantic Bridge, Frontline Ventures, several Irish + UK angels");
    println!("              with operator backgrounds.");
    println!("  Funding:    ~$5M seed + Series A; lean Dublin-based engineering team.");
    println!("  Position:   no-code internal tools + client portals on top of your");
    println!("              existing data — no data migration required.");
}

fn run_builder() {
    println!("Visual builder.");
    println!("  Page builder with drag-drop layout: tables, forms, charts, record details,");
    println!("  kanban boards, calendars, maps, lists, comments, file uploads, signatures.");
    println!("  Per-record action buttons that trigger workflows or open custom forms.");
    println!("  Conditional visibility on every block based on the logged-in user + record.");
    println!("  Page-level + block-level permissions wired to user roles.");
    println!("  Themes + branding + custom domain + email customisation.");
    println!("  No raw code-on-canvas — closer to Softr's block-assembly than Bubble's pixel UI.");
}

fn run_datasources() {
    println!("Data sources.");
    println!("  Airtable: original + primary integration, full read + write semantics.");
    println!("  Google Sheets: spreadsheet-backed projects without Airtable.");
    println!("  Postgres + MySQL: existing relational databases connected directly.");
    println!("  HubSpot: CRM-backed portals + internal tools over contacts / deals / tickets.");
    println!("  Xano + SmartSuite: alternative backends for users not on the big two.");
    println!("  Noloco Tables: built-in tables when there is no existing data source.");
    println!("  Multi-source apps: blend Airtable + Postgres + HubSpot in one project.");
}

fn run_portals() {
    println!("Client + member portals.");
    println!("  Authentication: email + Google + Microsoft + SAML SSO + magic-link.");
    println!("  Roles + groups: granular per-table + per-field + per-record permissions.");
    println!("  Row-level filters: 'show records where assigned_user = current_user'.");
    println!("  Stripe-backed paid memberships + billing portals.");
    println!("  Embedded mode: serve the portal inside a host app via iframe.");
    println!("  Common shapes: client onboarding portals, partner portals, course delivery,");
    println!("  property-management tenant portals, agency project portals.");
}

fn run_workflows() {
    println!("Workflows + automations.");
    println!("  Trigger types: record created / updated / deleted, scheduled cron, webhook.");
    println!("  Actions: update record, send email, send Slack, HTTP request, run JS, AI step.");
    println!("  Conditional branches + loops + delays in the same chain.");
    println!("  Per-workflow run history with success / failure / retry visibility.");
    println!("  Native HubSpot + Stripe + Google Calendar action steps.");
    println!("  Built-in webhook receivers for incoming integrations.");
}

fn run_ai() {
    println!("Noloco AI.");
    println!("  AI agents: scoped assistants that act over your Noloco data + workflows.");
    println!("  AI action steps inside workflows: classify, summarise, extract, draft.");
    println!("  AI content suggestions during page + form building (copy + field config).");
    println!("  Customer-facing chat agents grounded in portal data via RAG.");
    println!("  All built on OpenAI + Anthropic underneath, exposed to non-technical builders.");
    println!("  The standard 2023-2024 low-code AI bolt-on, scoped to portal use cases.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Starter:    ~$59/month for small portals + light internal tools.");
    println!("  Pro:        ~$149/month, more users + roles + integrations.");
    println!("  Business:   ~$299/month, advanced permissions + audit + SSO.");
    println!("  Enterprise: custom — SAML SSO, dedicated success, SLA, data-residency.");
    println!("  Pricing is per-workspace + per-end-user — same shape as Softr + Glide,");
    println!("  not the per-builder-seat shape of Bubble + Retool.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: operations teams + agencies + small businesses 5-200 employees.");
    println!("  Industries: agencies (client portals), real estate, property management,");
    println!("  professional services, education, recruitment, marketing services.");
    println!("  Geographic: heavy EU + UK + Ireland + North America; growing APAC.");
    println!("  Common stacks: Airtable + HubSpot + Google Workspace + Stripe.");
    println!("  Common origin: 'we run on Airtable + need a real client-facing UI for it'.");
    println!("  Differentiation vs Softr: stronger relational data model + workflow engine.");
    println!("  Differentiation vs Retool: non-technical-builder-first instead of dev-first.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "noloco-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "builder" => run_builder(),
        "datasources" => run_datasources(),
        "portals" => run_portals(),
        "workflows" => run_workflows(),
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
        run_builder();
        run_datasources();
        run_portals();
        run_workflows();
        run_ai();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("noloco-cli");
        print_version();
    }
}
