#![deny(clippy::all)]
//! budibase-cli — personality CLI for Budibase, the Northern Irish
//! open-source low-code platform.
//!
//! Founded 2019 in Belfast by Michael Drury (CEO, ex-Capita engineering)
//! and Joe Johnston (CTO). Open-source GPL-licensed; the defining
//! differentiator vs Appsmith + Tooljet is that Budibase ships its own
//! internal NoSQL database (BudibaseDB) on top of CouchDB, so customers
//! can build apps without first pointing at an external data source.
//! Picked up VC backing from Concentric + Frontline in 2022. Engineering
//! mostly remote from Belfast + the broader UK/IE. Distinct GPL stance
//! has positioned it as the philosophically-purest of the open-source
//! low-code stack.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Budibase open-source low-code platform personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Michael Drury + Joe Johnston 2019 Belfast; GPL");
    println!("    builder       Visual builder + screens + components");
    println!("    db            BudibaseDB built-in CouchDB-backed data layer");
    println!("    queries       External data sources + REST + SQL connectors");
    println!("    automations   Visual automation engine + cron + webhook");
    println!("    selfhost      Docker + Helm + Kubernetes self-host");
    println!("    pricing       Free OSS + Business + Enterprise tiers");
    println!("    customers     Belfast + EU SMB + IT-department customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("budibase-cli 0.1.0 (open-source-with-builtin-db personality build)"); }

fn run_about() {
    println!("Budibase Ltd.");
    println!("  Founded:    2019, Belfast, Northern Ireland.");
    println!("  Founders:   Michael Drury (CEO; ex-Capita engineering) + Joe Johnston (CTO).");
    println!("  Licence:    GPL-3.0 (community edition).");
    println!("  Backers:    Concentric, Frontline Ventures, several angels including");
    println!("              prominent open-source ecosystem investors.");
    println!("  Funding:    $7M Series A 2022; modest by category standards but matches");
    println!("              the lean Belfast-based engineering culture.");
    println!("  Team:       distributed remote across UK + Ireland + EU + broader.");
    println!("  Position:   open-source low-code with first-class built-in data layer.");
}

fn run_builder() {
    println!("Visual builder.");
    println!("  Drag-drop component canvas: tables, forms, charts, cards, dialogs, tabs,");
    println!("  rich text, file upload, repeater (= for-each), grouped containers.");
    println!("  Per-component property panel with conditional bindings + data sources.");
    println!("  Multi-screen apps with shared navigation + role-based screen visibility.");
    println!("  Themes + branding customisation per app.");
    println!("  In-app preview-as-role mode for testing different permission profiles.");
    println!("  Component bindings use {{ }} JS expressions, similar to peers.");
}

fn run_db() {
    println!("BudibaseDB (built-in data layer).");
    println!("  The defining vs-Appsmith-vs-Retool differentiator.");
    println!("  Apache CouchDB-backed schema engine: you define tables + columns + relationships");
    println!("  inside Budibase, the data lives in the Budibase instance.");
    println!("  Schema editor + CSV import + REST API auto-generated per table.");
    println!("  Foreign-key-like 'links between tables' for relational data modeling.");
    println!("  Use case: prototype a tool without first wrangling a Postgres database +");
    println!("  schema migrations — everything starts inside Budibase.");
    println!("  Customers can also point at external Postgres / MySQL / Mongo when ready.");
}

fn run_queries() {
    println!("External data sources.");
    println!("  Connectors: Postgres, MySQL, MS SQL, Oracle, Mongo, CouchDB, REST, GraphQL,");
    println!("  Airtable, Google Sheets, S3, ArangoDB, DynamoDB, Snowflake, Redis, Elasticsearch.");
    println!("  Tables in external sources can be used identically to BudibaseDB tables");
    println!("  in queries + bindings — Budibase abstracts the difference.");
    println!("  Query transformer step for post-processing in JS.");
    println!("  REST connector treats endpoints as 'queries' similar to other low-code peers.");
}

fn run_automations() {
    println!("Automation engine.");
    println!("  Visual automation builder: trigger + steps + branches.");
    println!("  Triggers: row created / updated / deleted, app event, webhook, cron.");
    println!("  Steps: send email (SMTP / SendGrid), Slack notification, query, branch,");
    println!("  loop, OpenAI prompt, REST call, delay, run-other-automation.");
    println!("  Server-side cron jobs orchestrated via the same engine.");
    println!("  AI step: built-in OpenAI / Anthropic / Azure OpenAI integration for");
    println!("  generative steps inside automations.");
    println!("  Similar grammar to Zapier / Make but with first-class app-data context.");
}

fn run_selfhost() {
    println!("Self-hosting.");
    println!("  Single Docker image quickstart.");
    println!("  Docker Compose for production-grade multi-service deployments.");
    println!("  Official Helm chart for Kubernetes installs with optional external CouchDB.");
    println!("  Air-gapped install supported for regulated environments.");
    println!("  Embedded Redis + CouchDB by default; external option for HA setups.");
    println!("  No artificial feature gating between self-hosted free + Cloud free tiers.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free / Community: open-source self-host, unlimited apps + users.");
    println!("  Cloud Free:   managed-hosting free tier with usage caps.");
    println!("  Pro / Premium: ~$50/month for managed Cloud teams, branded + custom domain.");
    println!("  Business:    ~$10 per user per month, SSO + audit logs + advanced perms.");
    println!("  Enterprise:  custom — air-gapped, dedicated success, SLA.");
    println!("  Pricing tends to be lower than Appsmith Business + materially lower than");
    println!("  Retool, matching the lean-startup positioning.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: 5-500 employee SMBs + IT-department use cases.");
    println!("  Industries: regional UK + EU SMBs, IT helpdesks at non-tech companies,");
    println!("  government + public sector (Northern Ireland origins help here),");
    println!("  manufacturing operations, finance + accounting back-office.");
    println!("  Geographic: heavy UK + Ireland + EU; growing US presence; APAC long-tail.");
    println!("  Common origin: IT or operations team wanting open-source self-host with");
    println!("  a turnkey database, often replacing Microsoft Access or spreadsheet ops.");
    println!("  Anti-segment: tech-forward US scale-ups (those tend to choose Retool).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "budibase-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "builder" => run_builder(),
        "db" => run_db(),
        "queries" => run_queries(),
        "automations" => run_automations(),
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
        run_builder();
        run_db();
        run_queries();
        run_automations();
        run_selfhost();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("budibase-cli");
        print_version();
    }
}
