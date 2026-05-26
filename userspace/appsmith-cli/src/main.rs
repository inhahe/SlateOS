#![deny(clippy::all)]
//! appsmith-cli — personality CLI for Appsmith, the open-source
//! self-hosted internal-tool builder.
//!
//! Founded 2019 in Bengaluru, India by Abhishek Nayak, Arpit Mohan, and
//! Nikhil Nandagopal as a direct open-source answer to Retool: same
//! drag-drop component canvas + query-against-any-datasource model, but
//! Apache 2.0 licensed and self-hostable from day one. Picked up a Series B
//! in 2022 led by Insight Partners ($41M); free + self-hosted has been the
//! defining go-to-market — most adoption is by individual engineers
//! deploying Appsmith on company infra without going through procurement,
//! followed later by upsell to Appsmith Cloud or Business tier for
//! permissions + SSO.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Appsmith open-source self-hosted internal-tools personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Nayak + Mohan + Nandagopal 2019 Bengaluru; Apache 2.0");
    println!("    canvas        Drag-drop widget canvas + JS expressions");
    println!("    queries       Datasources + SQL + REST + GraphQL + Mongo + S3");
    println!("    selfhost      Docker / Helm / Kubernetes self-host first")  ;
    println!("    workflows     Server-side workflows + AI-agent recent additions");
    println!("    git           Git-versioned app sources");
    println!("    pricing       Free OSS + Business + Enterprise tiers");
    println!("    customers     Bottom-up developer-adoption customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("appsmith-cli 0.1.0 (open-source-internal-tools personality build)"); }

fn run_about() {
    println!("Appsmith, Inc.");
    println!("  Founded:    2019, Bengaluru, India.");
    println!("  Founders:   Abhishek Nayak (CEO), Arpit Mohan, Nikhil Nandagopal.");
    println!("  Licence:    Apache 2.0 (core open-source).");
    println!("  Backers:    Insight Partners (Series B lead), Canaan, Accel, Bessemer,");
    println!("              Khosla Ventures.");
    println!("  Funding:    $41M Series B Aug 2022; ~$51M total raised.");
    println!("  Scale:      hundreds of thousands of self-hosted instances + GitHub stars,");
    println!("              tens of thousands of paying companies.");
    println!("  Position:   open-source first answer to Retool with strong developer DX.");
}

fn run_canvas() {
    println!("Component canvas.");
    println!("  Drag-drop widget library: tables, charts, forms, inputs, modals, tabs,");
    println!("  iFrames, file pickers, calendars, kanban, maps, audio + video, list views.");
    println!("  Reactive {{ }} JS expressions bind any widget property to live data,");
    println!("  re-evaluating when dependencies change.");
    println!("  Custom widgets via JS for cases the built-ins don't cover.");
    println!("  Responsive layout engine + multi-page apps + reusable Modal components.");
    println!("  Dark mode + branded-theming + per-page customisation.");
}

fn run_queries() {
    println!("Queries + datasources.");
    println!("  First-class connectors: Postgres, MySQL, MS SQL, Oracle, Mongo, Redis,");
    println!("  DynamoDB, BigQuery, Snowflake, Redshift, S3, ElasticSearch, REST + GraphQL,");
    println!("  SOAP, Firebase, Airtable, Google Sheets, Twilio, Sendgrid, OpenAI, more.");
    println!("  Query bindings to widget state for dynamic parameters.");
    println!("  Transformations: post-query JS to reshape the response.");
    println!("  Datasource sharing across apps in the same workspace.");
    println!("  Query caching + scheduled-refresh strategies.");
}

fn run_selfhost() {
    println!("Self-hosted-first delivery.");
    println!("  Single Docker container, full stack in one image — quickstart for solo devs.");
    println!("  Docker Compose for multi-service installs.");
    println!("  Helm chart for production Kubernetes deployments with replicaSets + ingress.");
    println!("  Air-gapped install path for regulated industries.");
    println!("  Embedded vs external database options (Mongo for metadata, Redis for cache).");
    println!("  Most growth is bottom-up: an engineer self-hosts, builds tools, the team");
    println!("  adopts them, eventually upgrades to Business for SSO + audit logs.");
}

fn run_workflows() {
    println!("Workflows + AI agents (newer additions).");
    println!("  Appsmith Workflows: server-side cron + event-driven jobs (cron / webhook /");
    println!("  scheduled) on the same query + transform engine.");
    println!("  Appsmith AI: integrated chat + agent + RAG widget for building LLM apps");
    println!("  against the same datasources.");
    println!("  Use cases: AI-assisted internal support, semantic search over internal data,");
    println!("  agent loops that read + write back to the company's actual systems via");
    println!("  Appsmith's existing query connectors.");
    println!("  This is the standard 2023-2024 pivot trajectory for low-code platforms.");
}

fn run_git() {
    println!("Git-versioned apps.");
    println!("  Connect any Appsmith app to a Git remote (GitHub, GitLab, Bitbucket).");
    println!("  Branch + commit + push + pull from the Appsmith editor.");
    println!("  Merge conflicts resolved via Git's normal tooling outside Appsmith.");
    println!("  Code-review flow on app changes via standard Git PR workflow.");
    println!("  Multi-environment promotion: dev → staging → prod via Git branches.");
    println!("  Appsmith was an early mover on first-class Git integration vs.");
    println!("  competitors' bespoke versioning systems.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free / Community: open source, self-host, unlimited users + apps.");
    println!("  Business:     ~$15 per user per month, SSO + audit logs + advanced");
    println!("                permissions + premium support.");
    println!("  Enterprise:   custom pricing — air-gapped, SAML SSO, dedicated success.");
    println!("  Appsmith Cloud: managed-hosting alternative on Free + Business tiers.");
    println!("  Free-tier feature parity is large by design — Appsmith deliberately keeps");
    println!("  the OSS edition fully capable, monetising on org-scale features only.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: engineering-led teams 5-1,000 employees who want to self-host.");
    println!("  Industries: SaaS, fintech, e-commerce ops, regional banks (regulated EU/IN),");
    println!("  government + public sector, large enterprises with strict data-residency.");
    println!("  Geographic: heavy India + EU + LATAM presence; growing US enterprise.");
    println!("  Frequently named: GSK, Tata, several Indian government departments,");
    println!("  Lyft (selective), large European banks under NDA, regional health systems.");
    println!("  Typical journey: engineer self-hosts on a spare VM → builds 3 admin tools →");
    println!("  team consolidates around them → company upgrades to Business for SSO.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "appsmith-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "canvas" => run_canvas(),
        "queries" => run_queries(),
        "selfhost" => run_selfhost(),
        "workflows" => run_workflows(),
        "git" => run_git(),
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
        run_selfhost();
        run_workflows();
        run_git();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("appsmith-cli");
        print_version();
    }
}
