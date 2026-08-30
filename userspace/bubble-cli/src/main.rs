#![deny(clippy::all)]
//! bubble-cli — personality CLI for Bubble, the consumer-app no-code
//! pioneer.
//!
//! Founded 2012 in New York by Emmanuel Straschnov and Joshua Haas as the
//! original general-purpose no-code platform for building real consumer +
//! marketplace web apps — not just internal tools, not just dashboards, but
//! actual public-facing products with users, payments, complex workflows.
//! Bootstrapped + profitable for the first seven years before raising
//! $100M Series A from Insight Partners in 2021 at the no-code-hype peak.
//! Famously the platform used by many non-technical founders to build
//! pre-funding MVPs of seven-figure-ARR businesses (Comet's CRM, Plato,
//! Latitude, multiple Y Combinator companies built initial Bubble versions).

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Bubble consumer no-code app builder personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Straschnov + Haas 2012 NYC; bootstrapped 7yrs to $100M Series A");
    println!("    editor        Visual editor: page elements + workflows + data");
    println!("    workflows     Workflow engine: triggers + actions + custom states");
    println!("    data          Built-in database + privacy-rule engine");
    println!("    plugins       Plugin marketplace + custom-plugin SDK");
    println!("    apis          Backend workflows + API connector + webhooks");
    println!("    pricing       Workload-unit-based pricing model");
    println!("    customers     Non-technical founder + agency customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("bubble-cli 0.1.0 (consumer-no-code-pioneer personality build)"); }

fn run_about() {
    println!("Bubble Group, Inc.");
    println!("  Founded:    2012, New York.");
    println!("  Founders:   Emmanuel Straschnov (CEO) + Joshua Haas (CTO).");
    println!("  Backers:    Insight Partners (lead $100M Series A 2021), CRV.");
    println!("  Funding:    $100M Series A Jul 2021 — notable for being raised after 7+ years");
    println!("              of bootstrapped profitability rather than the usual seed → A path.");
    println!("  Scale:      ~3M+ users built Bubble apps; ~1M+ apps created on the platform.");
    println!("  Position:   the canonical 'real apps not just dashboards' no-code platform.");
    println!("              Distinct from Webflow (sites), Glide (mobile-from-sheets),");
    println!("              Adalo (mobile-first), Retool (internal-tools-only).");
}

fn run_editor() {
    println!("Visual editor.");
    println!("  Drag-drop page-element canvas with responsive engine (post-2022 rebuild).");
    println!("  Elements: text, buttons, inputs, images, repeating groups (= for-each lists),");
    println!("  groups (containers), reusable elements (= components), maps, video, custom HTML.");
    println!("  Property panel per element with conditional-formatting rules + dynamic data");
    println!("  bindings from URL params, current user, database queries.");
    println!("  Mobile + tablet breakpoint editing in the same canvas.");
    println!("  Preview-as-different-user mode for testing role-specific UIs.");
}

fn run_workflows() {
    println!("Workflow engine.");
    println!("  Bubble's defining computation model: every interaction (button click, input");
    println!("  change, page-load, API webhook, scheduled run) triggers a workflow.");
    println!("  Each workflow is a sequence of actions with conditional 'only when' steps.");
    println!("  Actions: navigate page, create + modify + delete data records, send email,");
    println!("  Stripe charge, schedule a future workflow, call external API, custom plugin.");
    println!("  Custom states: per-element + per-page in-memory variables for UI state.");
    println!("  Async + parallel patterns supported via backend workflows + recursive scheduling.");
}

fn run_data() {
    println!("Built-in database.");
    println!("  Hosted database per Bubble app; you define types + fields like tables + columns.");
    println!("  Field types: text, number, date, boolean, geographic-address, image, file,");
    println!("  custom-type-reference, list-of-thing-references, options-set (enum).");
    println!("  Privacy rules: per-type row-level access controls evaluated server-side —");
    println!("  e.g. 'this Order is only visible when Current User is its buyer'.");
    println!("  Built on Postgres under the hood, hidden behind Bubble's abstraction.");
    println!("  Dedicated-instance plans for larger customers + dataset-size guarantees.");
}

fn run_plugins() {
    println!("Plugin marketplace + custom plugins.");
    println!("  Marketplace: thousands of community + first-party plugins — Stripe Connect,");
    println!("  Mapbox, Algolia search, Auth0, custom UI components, third-party APIs.");
    println!("  Custom-plugin SDK: write plugins in JavaScript + (server-side) Bubble's");
    println!("  hosted-functions runtime; expose as actions + elements + datasources.");
    println!("  Major revenue stream for plugin developers — there's an entire ecosystem of");
    println!("  agencies + indie developers building paid plugins for the Bubble market.");
    println!("  Plugins are how Bubble extends to capabilities outside its native primitives.");
}

fn run_apis() {
    println!("APIs + backend workflows.");
    println!("  API Connector: configure REST + GraphQL external APIs as datasources +");
    println!("  workflow actions.");
    println!("  API workflows: expose Bubble workflows as HTTP endpoints for external");
    println!("  callers (mobile apps, Zapier, webhooks-from-other-services).");
    println!("  Scheduled workflows: cron-like scheduling for recurring jobs.");
    println!("  Recursive workflows: pattern for batching long-running data processing on");
    println!("  Bubble's server-side runtime.");
    println!("  Most large Bubble apps end up being half-frontend + half-backend-workflows.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Modern: workload-unit (WU) based — every workflow action, page-load,");
    println!("  database read, API call consumes WUs.");
    println!("  Plans:");
    println!("    Free:       limited learning + prototype use.");
    println!("    Starter:    ~$29/month, 175K WU/month.");
    println!("    Growth:     ~$119/month, 250K WU/month + custom domain + multi-environment.");
    println!("    Team:       ~$349/month, multiple seats + 500K WU/month.");
    println!("    Enterprise: custom, dedicated infra + premium support.");
    println!("  WU pricing was a controversial 2023 change from a previous capacity-unit");
    println!("  model — some heavier apps experienced cost spikes, generating a lot of");
    println!("  community discussion.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: non-technical founders + technical founders looking for a fast MVP,");
    println!("  agencies that build Bubble apps for client SMBs, small product teams.");
    println!("  Industries: marketplaces, social platforms, SaaS prototypes, internal");
    println!("  business apps, hyperlocal community apps, vertical-niche CRMs.");
    println!("  Famously: many YC-batch companies built Bubble MVPs that later got rebuilt");
    println!("  in code post-funding. Some chose to scale on Bubble indefinitely.");
    println!("  Agencies: Airdev, Bree Brouwer, Goodspeed, many more — Bubble's certified");
    println!("  agency program is a significant ecosystem of its own.");
    println!("  Anti-segment: enterprise IT, mobile-app-first products, high-QPS public apps.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "bubble-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "editor" => run_editor(),
        "workflows" => run_workflows(),
        "data" => run_data(),
        "plugins" => run_plugins(),
        "apis" => run_apis(),
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
        run_editor();
        run_workflows();
        run_data();
        run_plugins();
        run_apis();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("bubble-cli");
        print_version();
    }
}
