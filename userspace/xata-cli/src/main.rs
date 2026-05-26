#![deny(clippy::all)]
//! xata-cli — personality CLI for Xata, the serverless data platform that
//! combined Postgres + Elasticsearch + file storage behind one TypeScript SDK,
//! then pivoted in 2025 to "branchable Postgres".
//!
//! Founded 2019 by Monica Sarbu and Tudor Golubenco. Sarbu was the creator
//! of Filebeat at Elastic and grew the Elastic Beats team. Xata raised a
//! $30M Series A in 2022 led by Index Ventures + Redpoint. The original
//! Xata product abstracted away the database choice entirely with a JSON
//! API on top of Postgres + Elasticsearch. In Mar 2025 Xata announced
//! the sunset of the original platform and a pivot to a Postgres-platform
//! product with database branching (acquired the pgroll OSS team).

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Xata serverless data platform personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about       Sarbu + Golubenco, Elastic lineage");
    println!("    original    The original 'PG + ES behind a JSON SDK' product");
    println!("    pgroll      pgroll zero-downtime schema migration tool");
    println!("    pivot       2025 pivot to branchable Postgres");
    println!("    branches    Postgres branches via copy-on-write");
    println!("    sdk         TypeScript SDK + code generation");
    println!("    pricing     Free tier + paid bands");
    println!("    customers   Selected named accounts");
    println!("    help        Show this help");
    println!("    version     Show version");
}

fn print_version() { println!("xata-cli 0.1.0 (post-pivot branchable Postgres build)"); }

fn run_about() {
    println!("Xata, Inc.");
    println!("  Founded:    2019.");
    println!("  Founders:   Monica Sarbu (CEO), Tudor Golubenco (CTO).");
    println!("  Heritage:   Sarbu created Filebeat at Elastic, grew Beats team.");
    println!("              Golubenco contributed to Elastic Stack.");
    println!("  Funding:    Series A ~$30M Sep 2022 led by Index Ventures.");
    println!("  HQ:         Remote-first; Sarbu based in Amsterdam.");
}

fn run_original() {
    println!("Original Xata product (2021-2025).");
    println!("  TypeScript-first JSON API hiding Postgres + Elasticsearch.");
    println!("  Free-text search via the ES backend behind every column.");
    println!("  File attachments stored alongside rows (signed URL access).");
    println!("  Branches for schema iteration via internal Postgres replicas.");
    println!("  Zero-downtime migrations via pgroll engine.");
    println!("  Pitch: 'a single SDK, no schema-migration pain, search free'.");
}

fn run_pgroll() {
    println!("pgroll — Xata's zero-downtime schema migration OSS tool.");
    println!("  Apache 2.0, github.com/xataio/pgroll.");
    println!("  Implements expand-then-contract migrations on Postgres.");
    println!("  Old and new schemas are both queryable during cutover.");
    println!("  YAML-described migrations; supports add column, drop column,");
    println!("  rename, change type, all without locking the table.");
    println!("  Adopted standalone by users who don't use Xata's hosted DB.");
}

fn run_pivot() {
    println!("March 2025 pivot.");
    println!("  Xata announced the original product would be sunset and");
    println!("  the team would refocus on a 'Postgres developer platform'");
    println!("  centred on branching, observability, and pgroll migrations.");
    println!("  Existing customers given a defined migration window.");
    println!("  Rationale: the all-in-one abstraction undersold against");
    println!("             dedicated Postgres-only competitors (Neon, Supabase).");
}

fn run_branches() {
    println!("Branches — the post-pivot focus.");
    println!("  Postgres database branches with copy-on-write storage.");
    println!("  Branch per pull request, isolated schema + data.");
    println!("  Branches connect to the same workflow CI uses.");
    println!("  Production stays untouched while devs iterate.");
}

fn run_sdk() {
    println!("Xata TypeScript SDK.");
    println!("  Code generation from your schema to typed query helpers.");
    println!("  Friendly to Vercel, Netlify, Cloudflare Workers.");
    println!("  HTTP-based (no driver) so deploys to edge runtimes work.");
    println!("  Now also exposes raw Postgres connections for ORM users.");
}

fn run_pricing() {
    println!("Pricing model (post-pivot bands):");
    println!("  Free            generous developer free tier.");
    println!("  Pro             per-DB + per-storage-GB.");
    println!("  Business        team features, audit, dedicated support.");
    println!("  Enterprise      BYO-cloud / VPC peering options.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Various Jamstack startups (Netlify/Vercel-era era)");
    println!("  Sourcegraph (early reference, internal tooling)");
    println!("  Pgroll OSS users including teams at large enterprises");
    println!("  Indie SaaS preferring 'managed Postgres + branches' workflow");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "xata-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "original" => run_original(),
        "pgroll" => run_pgroll(),
        "pivot" => run_pivot(),
        "branches" => run_branches(),
        "sdk" => run_sdk(),
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
        run_original();
        run_pgroll();
        run_pivot();
        run_branches();
        run_sdk();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("xata-cli");
        print_version();
    }
}
