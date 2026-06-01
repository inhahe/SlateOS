#![deny(clippy::all)]
//! retool-cli — personality CLI for Retool, the internal-tools low-code
//! platform.
//!
//! Founded 2017 in San Francisco by David Hsu (CEO; ex-Cambridge maths
//! student) and Anand Goyal. The defining commercial pitch: every
//! company has dozens of "internal tools" — customer-support look-up
//! screens, refund admin panels, ops dashboards, ad-hoc data-editing UIs —
//! that engineering teams hate building. Retool gives them a drag-drop
//! component canvas + first-class query editor against any database +
//! API, so a six-person eng team can ship the internal CRUD app a finance
//! ops manager needs in an afternoon instead of a sprint. Reached
//! $3.2B valuation Dec 2022 on a $45M Series C extension at the tail end
//! of the SaaS funding boom; one of the canonical YC W17 alumni
//! success stories.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Retool internal-tools low-code personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         David Hsu + Anand Goyal 2017 SF YC W17");
    println!("    canvas        Drag-drop component canvas + state + JS expressions");
    println!("    queries       SQL + REST + GraphQL queries against any datasource");
    println!("    workflows     Retool Workflows: cron + event-driven server-side jobs");
    println!("    db            Retool Database: hosted Postgres for prototypes");
    println!("    selfhost      Self-hosted Retool for regulated industries");
    println!("    pricing       Per-user-per-month tiered + Workflows usage-based");
    println!("    customers     Selected enterprise + scale-up customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("retool-cli 0.1.0 (internal-tools personality build)"); }

fn run_about() {
    println!("Retool, Inc.");
    println!("  Founded:    2017, San Francisco (Y Combinator W17).");
    println!("  Founders:   David Hsu (CEO; Cambridge maths) + Anand Goyal.");
    println!("  Backers:    Sequoia (lead), Greenoaks, Brad Buss, Patrick + John Collison,");
    println!("              several solo CEO + founder angels.");
    println!("  Funding:    $45M Series C extension Dec 2022 at $3.2B valuation;");
    println!("              ~$141M total disclosed raised.");
    println!("  Scale:      10,000+ customer companies, with deep adoption inside many.");
    println!("  Single-app metric: many large customers run dozens to hundreds of");
    println!("              independent Retool apps from one workspace.");
}

fn run_canvas() {
    println!("Component canvas.");
    println!("  Drag-drop component library: tables, charts, forms, modals, file uploads,");
    println!("  multi-step wizards, kanban boards, calendars, maps, custom React widgets.");
    println!("  Component state is reactive: a table row selection automatically updates");
    println!("  every downstream component bound to that selection.");
    println!("  Per-component {{ }} JS expressions: bind any component property to a JS");
    println!("  literal that re-evaluates whenever its dependencies change.");
    println!("  Custom components via React + iframe for use cases the built-ins don't cover.");
    println!("  Mobile companion app for the same Retool tools on iOS / Android.");
}

fn run_queries() {
    println!("Queries + datasources.");
    println!("  First-class connectors for Postgres, MySQL, MS SQL, Snowflake, BigQuery,");
    println!("  Redshift, MongoDB, DynamoDB, REST + GraphQL APIs, Salesforce, Stripe,");
    println!("  GitHub, Google Sheets, Airtable, Hasura, Firebase, S3, OpenAI, more.");
    println!("  Queries: written as SQL or as request specifications, with parameter");
    println!("  binding to component state ({{ tableSelectedRow.id }} etc.).");
    println!("  Run on the customer's edge via Retool Agent for self-hosted data sources");
    println!("  not directly reachable from Retool Cloud.");
    println!("  Transformers: post-query JS to reshape the response before binding.");
}

fn run_workflows() {
    println!("Retool Workflows.");
    println!("  Server-side cron + event-driven job platform launched 2022.");
    println!("  Visual workflow builder: triggers (schedule, webhook, on-event) →");
    println!("  blocks (query, transform, branch, loop, OpenAI, function).");
    println!("  Same query engine + datasource catalog as the canvas product.");
    println!("  Use cases: nightly billing reconciliation, ETL pipelines too small to");
    println!("  justify Airflow, AI-batch enrichment jobs, webhook integration glue.");
    println!("  Distinct from Zapier / Make positioning: Workflows is for engineers");
    println!("  who want to write SQL + JS, not point-and-click marketers.");
}

fn run_db() {
    println!("Retool Database.");
    println!("  Hosted Postgres database inside the Retool workspace, launched 2023.");
    println!("  Removes the chicken-and-egg problem of 'I want to build a Retool app");
    println!("  but I don't have a backend yet'.");
    println!("  Schema editor + sample-data import + permission management in-product.");
    println!("  Use case: prototype a tool against a Retool DB, migrate to real Postgres");
    println!("  once the tool proves its value.");
    println!("  Pairs naturally with Retool AI + Workflows to build small full-stack apps.");
}

fn run_selfhost() {
    println!("Self-hosted Retool.");
    println!("  Customer-hosted deployment option since the early days — Docker /");
    println!("  Kubernetes manifests delivered to enterprise customers.");
    println!("  Use cases: HIPAA-regulated healthcare, financial-services data-residency,");
    println!("  government, customers with strict network egress + auditing requirements.");
    println!("  Same product as Retool Cloud; just runs on customer infra.");
    println!("  SSO + audit logs + role-based permissions surface identically.");
    println!("  Major sales-cycle accelerator for enterprise; uncommon among low-code peers.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:       up to 5 users, basic features.");
    println!("  Team:       ~$10 per user per month, full app builder + integrations.");
    println!("  Business:   ~$50 per user per month, advanced permissions + audit logs.");
    println!("  Enterprise: custom pricing — self-hosted, SAML SSO, SLAs, premium support.");
    println!("  Retool Workflows: separate usage-based pricing tied to runs + run-time.");
    println!("  Retool AI: separate usage-based pricing tied to model + token volume.");
    println!("  Distinguishes 'end user' (~lower-cost viewer-style seats) vs 'builder'");
    println!("  seats (higher), more like Tableau's traditional licensing split.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 50-50,000 employee tech-forward companies needing dozens to");
    println!("  hundreds of internal-only CRUD admin / ops / support tools.");
    println!("  Industries: SaaS, marketplaces, fintech, e-commerce ops, AI startups,");
    println!("  large-tech ops + finance organisations.");
    println!("  Frequently named customers: Brex (heavy adopter), DoorDash, Coinbase,");
    println!("  Plaid, Lyft, Stripe (selective), Amazon (selective), Mercury, Ramp,");
    println!("  Notion (selective), Pinterest, Peloton, NBC.");
    println!("  Replaces: hand-built Rails / Django admin scaffolds, Forest Admin, custom");
    println!("  React internal-tool apps that nobody at the company really wants to maintain.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "retool-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "canvas" => run_canvas(),
        "queries" => run_queries(),
        "workflows" => run_workflows(),
        "db" => run_db(),
        "selfhost" => run_selfhost(),
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
        run_canvas();
        run_queries();
        run_workflows();
        run_db();
        run_selfhost();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("retool-cli");
        print_version();
    }
}
