#![deny(clippy::all)]
//! glide-cli — personality CLI for Glide, the no-code platform
//! for building mobile apps + business apps from a spreadsheet.
//!
//! Founded 2018 in San Francisco by David Siegel (CEO, ex-Khan Academy
//! engineering manager) and Antonio García Aprea (CTO, ex-Xamarin/
//! Microsoft mobile tooling). Glide's original 2019 product was a
//! breakout viral hit: paste a Google Sheets URL and get a polished
//! mobile-shaped app instantly. Picked up Series A from Benchmark
//! 2020 led by Eric Vishria, then Series B 2021. Glide has since
//! pivoted toward business apps + AI features ('Glide AI') alongside
//! the original mobile-first product, broadening from consumer-style
//! mobile to internal-tool + workflow territory.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Glide no-code mobile-first app-from-spreadsheet personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Siegel + García Aprea 2018 SF; Benchmark Series A 2020");
    println!("    mobile        Mobile-shaped apps as the original viral product");
    println!("    business      Glide Apps + Pages — business + internal app pivot");
    println!("    sheets        Google Sheets + Glide Tables + Airtable + SQL data sources");
    println!("    ai            Glide AI — generative columns + agent-style workflows");
    println!("    pricing       Free + Maker + Team + Business + Enterprise tiers");
    println!("    customers     Operators + ops teams + small business + field workers");
    println!("    history       2018 origin -> 2019 viral -> Benchmark -> business pivot");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("glide-cli 0.1.0 (no-code-mobile-app-from-spreadsheet personality build)"); }

fn run_about() {
    println!("Glide (Glide Apps, Inc.).");
    println!("  Founded:    2018, San Francisco, California.");
    println!("  Founders:   David Siegel (CEO; ex-Khan Academy engineering manager) +");
    println!("              Antonio García Aprea (CTO; ex-Xamarin / Microsoft).");
    println!("  Backers:    Benchmark (Series A lead, Eric Vishria board seat), First Round,");
    println!("              Y Combinator, prominent operator + designer angels.");
    println!("  Funding:    ~$28M Series B 2021; ~$48M total raised.");
    println!("  Position:   no-code mobile apps from spreadsheet data — fastest");
    println!("              time-to-mobile-app in the no-code stack.");
    println!("  Reach:      hundreds of thousands of apps built; viral demo-driven growth.");
}

fn run_mobile() {
    println!("Mobile-shaped apps (the original viral product).");
    println!("  Paste a Google Sheets URL + Glide auto-generates a mobile-shaped app");
    println!("  with sensible defaults: list + detail + edit screens + tab navigation.");
    println!("  Install as a PWA on iOS + Android home screens — no app-store review.");
    println!("  Native-feeling UI components: tile lists, swipe actions, image galleries,");
    println!("  date / time pickers, signature pads, barcode scanners, map pins, video.");
    println!("  The 'paste-sheet-get-app' demo was the viral mechanic from 2019 onward.");
}

fn run_business() {
    println!("Business + internal-app pivot (Glide Apps + Pages).");
    println!("  Glide Pages: desktop-shaped layouts in addition to mobile-first apps.");
    println!("  Workflows: triggered actions, multi-step server-side workflows, scheduling.");
    println!("  Computed columns + relations + lookups — light spreadsheet-style data model.");
    println!("  Per-row + per-user access controls + roles + tenant isolation.");
    println!("  Use cases: field-service apps, inventory + asset tracking, customer-facing");
    println!("  portals for small businesses, sales-team mobile tools, dispatcher dashboards.");
    println!("  Positioning has shifted from 'mobile from sheets' to 'business apps from data'.");
}

fn run_sheets() {
    println!("Data sources.");
    println!("  Google Sheets: the original integration — still the most common source.");
    println!("  Glide Tables: Glide's own built-in tables for users without spreadsheets.");
    println!("  Airtable: full bidirectional integration.");
    println!("  SQL: Postgres + MySQL + MS SQL for customers with existing databases.");
    println!("  BigQuery: read-side analytics-style integration.");
    println!("  Excel: Microsoft 365 Excel files as data sources.");
    println!("  API calls: outbound HTTP integrations in workflows for the rest.");
}

fn run_ai() {
    println!("Glide AI.");
    println!("  Generative computed columns: 'classify this support ticket', 'summarise',");
    println!("  'translate', 'extract entities', 'rewrite' — applied at row level.");
    println!("  Image + document understanding columns powered by vision models.");
    println!("  Workflow steps: prompt-an-LLM as a first-class step in workflow chains.");
    println!("  Agent-style features for guided + assisted app building inside the editor.");
    println!("  Built on top of OpenAI + Anthropic + other LLM providers under the hood.");
    println!("  Like all 2023-2024 low-code platforms, Glide bolted on AI as a top-tier feature.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:       limited rows + users + Glide branding.");
    println!("  Maker:      ~$25/month for personal + hobby + small-team apps.");
    println!("  Team:       ~$60-100/month for small businesses + ops teams.");
    println!("  Business:   ~$249-400+/month with private apps, advanced data, SSO.");
    println!("  Enterprise: custom — dedicated success, SLAs, advanced security + compliance.");
    println!("  Pricing is per-app + per-end-user, not per-builder seat — same shape as Softr.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: small business owners + ops teams + frontline-worker software.");
    println!("  Industries: field service + maintenance, retail ops, restaurant chains,");
    println!("  contracting + trades, school districts, NGOs + non-profits, real-estate teams.");
    println!("  Geographic: heavy US + LATAM + EU; growing APAC; particularly strong in markets");
    println!("  with high smartphone-but-low-laptop penetration where mobile-first is natural.");
    println!("  Common origin: 'we run on a spreadsheet + our field team needs a phone app'.");
    println!("  Anti-segment: developers (go to Bubble or code) + heavy enterprises (Power Apps).");
}

fn run_history() {
    println!("History (compressed).");
    println!("  2018:  founded by Siegel + García Aprea in San Francisco.");
    println!("  2019:  paste-Google-Sheets-get-an-app demo goes viral on Twitter + HN.");
    println!("         Y Combinator, then early Benchmark interest builds.");
    println!("  2020:  Series A led by Benchmark with Eric Vishria taking the board seat.");
    println!("  2021:  Series B in the late-stage no-code funding wave.");
    println!("  2022:  product pivot toward business apps + Glide Pages launch.");
    println!("  2023:  Glide AI launches — generative columns + workflows.");
    println!("  2024+: positioning consolidates around 'business apps for non-developers'.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "glide-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "mobile" => run_mobile(),
        "business" => run_business(),
        "sheets" => run_sheets(),
        "ai" => run_ai(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
        "history" => run_history(),
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
        run_mobile();
        run_business();
        run_sheets();
        run_ai();
        run_pricing();
        run_customers();
        run_history();
    }

    #[test]
    fn help_and_version() {
        print_help("glide-cli");
        print_version();
    }
}
