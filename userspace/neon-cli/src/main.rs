#![deny(clippy::all)]
//! neon-cli — personality CLI for Neon, the serverless Postgres database
//! with separated storage and compute, acquired by Databricks in 2025.
//!
//! Founded 2021 by Nikita Shamgunov (ex-SingleStore/MemSQL CEO), Stas
//! Kelvich, and Heikki Linnakangas (long-time Postgres committer). Series
//! B $46M Jun 2023 led by Menlo Ventures. Acquired by Databricks May 2025
//! for ~$1B to anchor Databricks's transactional database story alongside
//! its analytical lakehouse. Distinctive architecture: WAL-stream-fed
//! page-server tier holds storage, ephemeral compute pods attach for
//! query, which enables instant 'scale to zero' and instant branching.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Neon serverless Postgres personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Postgres lineage, Databricks deal");
    println!("    architecture  Separated storage + compute");
    println!("    branching     Copy-on-write branches in seconds");
    println!("    scaletozero   Compute autosuspend and cold-start");
    println!("    pageserver    Page server + safekeepers + compute");
    println!("    extensions    pgvector, PostGIS, etc.");
    println!("    databricks    The acquisition and the strategic fit");
    println!("    pricing       Free tier, autoscaling pricing model");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("neon-cli 0.1.0 (Databricks-era serverless Postgres build)"); }

fn run_about() {
    println!("Neon (Neon Inc., now part of Databricks)");
    println!("  Founded:    2021.");
    println!("  Founders:   Nikita Shamgunov (CEO, ex-SingleStore/MemSQL CEO),");
    println!("              Stas Kelvich, Heikki Linnakangas.");
    println!("  Linnakangas: long-time Postgres core committer (PostgreSQL");
    println!("               TOAST, multi-xact, WAL improvements).");
    println!("  Funding:    Series B $46M Jun 2023 led by Menlo Ventures.");
    println!("  Acquired:   By Databricks, May 14 2025, for ~$1B.");
    println!("  Source:     github.com/neondatabase/neon (Apache 2.0).");
}

fn run_architecture() {
    println!("Architecture — separated storage and compute.");
    println!("  Page Server   stores Postgres pages addressed by LSN.");
    println!("                Persists to object storage (S3) with a hot tier.");
    println!("  Safekeepers   accept WAL from compute, replicate by Paxos.");
    println!("  Compute       a stock Postgres process running in a pod that");
    println!("                reads pages from the Page Server and writes WAL");
    println!("                to the Safekeepers. Stateless.");
    println!("  Net effect:   you can spin up a fresh Postgres against any");
    println!("                historical LSN in seconds.");
}

fn run_branching() {
    println!("Branching — copy-on-write database branches.");
    println!("  A branch is a new LSN root pointing at the same pages.");
    println!("  No data is copied; only diverging pages allocate new storage.");
    println!("  Spin up a dev branch from prod in <2 seconds.");
    println!("  Branches per-PR is a common workflow (Vercel preview + Neon");
    println!("  branch -> isolated DB per PR).");
}

fn run_scaletozero() {
    println!("Scale to zero — compute autosuspend.");
    println!("  After N minutes idle the compute pod is shut down.");
    println!("  On the next connection a fresh compute boots and re-attaches");
    println!("  to the Page Server.");
    println!("  Cold start target: <1 second on warm regions.");
    println!("  Storage continues to be billed; compute is not.");
    println!("  Makes 'thousands of small dev DBs' economically viable.");
}

fn run_pageserver() {
    println!("Page Server + Safekeepers + Compute — the three Neon tiers.");
    println!("  All open source under Apache 2.0.");
    println!("  Page Server: Rust, designed for object-storage-backed page");
    println!("               retrieval and snapshot management.");
    println!("  Safekeepers: durable WAL ring via Paxos quorum.");
    println!("  Compute: vanilla Postgres patched only to plug in the");
    println!("           remote storage SMGR.");
}

fn run_extensions() {
    println!("Postgres extensions supported:");
    println!("  pgvector            embeddings + ANN indexes for RAG / LLMs.");
    println!("  PostGIS             geospatial.");
    println!("  pg_partman          partition automation.");
    println!("  hypopg              hypothetical indexes.");
    println!("  citext, hstore, ltree, pgcrypto.");
    println!("  Time-series via timescaledb extension (community version).");
    println!("Neon plays heavily on the pgvector angle for AI/RAG workloads.");
}

fn run_databricks() {
    println!("Databricks acquisition — May 14 2025, ~$1B.");
    println!("  Rationale: Databricks's analytical lakehouse has had no");
    println!("  serious transactional companion. Neon fills the 'OLTP next");
    println!("  to your lakehouse' slot.");
    println!("  Strategic positioning: AI agents that read/write Postgres");
    println!("  against the same Unity Catalog governance perimeter.");
    println!("  Neon brand and team continue under Databricks ownership.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free            3 projects, 0.5 GB storage, auto-suspend compute.");
    println!("  Launch          per-CU (Compute Units) + storage + branches.");
    println!("  Scale           higher quotas, IP allow lists.");
    println!("  Business        SOC 2, MFA, support SLAs.");
    println!("  Compute is billed per second when the pod is awake;");
    println!("  storage is billed per GB-month.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "neon-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "architecture" => run_architecture(),
        "branching" => run_branching(),
        "scaletozero" => run_scaletozero(),
        "pageserver" => run_pageserver(),
        "extensions" => run_extensions(),
        "databricks" => run_databricks(),
        "pricing" => run_pricing(),
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
        run_architecture();
        run_branching();
        run_scaletozero();
        run_pageserver();
        run_extensions();
        run_databricks();
        run_pricing();
    }

    #[test]
    fn help_and_version() {
        print_help("neon-cli");
        print_version();
    }
}
