#![deny(clippy::all)]
//! yugabyte-cli — personality CLI for YugabyteDB, the distributed SQL
//! database from ex-Facebook engineers, with native Postgres + Cassandra
//! wire compatibility.
//!
//! YugaByte (later Yugabyte) was founded 2016 by Kannan Muthukkaruppan,
//! Karthik Ranganathan, and Mikhail Bautin. All three came from Facebook
//! where they worked on the HBase + Apache Cassandra forks that ran the
//! Messages and Operational stores. Raised $188M Series C in Oct 2021 at
//! a $1.3B valuation. YugabyteDB is Apache 2.0 — a strong contrast to
//! CockroachDB's licence trajectory. Architecturally: DocDB storage layer
//! (RocksDB) with Raft, then two query layers — YSQL (PostgreSQL fork
//! with the upstream PG executor) and YCQL (Cassandra-compatible).

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — YugabyteDB distributed SQL personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Facebook lineage, OSS posture");
    println!("    architecture  DocDB + Raft + tablets + query layers");
    println!("    ysql          Postgres-fork SQL layer");
    println!("    ycql          Cassandra-compatible CQL layer");
    println!("    license       Apache 2.0 forever pledge");
    println!("    managed       YugabyteDB Managed cloud service");
    println!("    pricing       OSS + Managed bands");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("yugabyte-cli 0.1.0 (DocDB-on-Raft personality build)"); }

fn run_about() {
    println!("Yugabyte, Inc.");
    println!("  Founded:    2016, Sunnyvale, California.");
    println!("  Founders:   Kannan Muthukkaruppan (CEO, ex-Facebook,");
    println!("              co-built FB Messages on HBase),");
    println!("              Karthik Ranganathan (CTO), Mikhail Bautin.");
    println!("  Heritage:   All three on Facebook's HBase + Cassandra teams.");
    println!("  Funding:    ~$291M total. $188M Series C Oct 2021 at $1.3B.");
    println!("  OSS:        License switched to permissive Apache 2.0 in 2019.");
}

fn run_architecture() {
    println!("Architecture:");
    println!("  DocDB        the document-oriented storage substrate.");
    println!("               RocksDB per node, MVCC, encoded as documents.");
    println!("  Tablets      DocDB shards, each replicated by Raft (3-5x).");
    println!("  Master       cluster-wide metadata and tablet placement.");
    println!("  TServer      per-node process that owns local tablets.");
    println!("  Transactions distributed across tablets via 2-PC with the");
    println!("               transaction status table.");
    println!("  Time         hybrid logical clocks.");
}

fn run_ysql() {
    println!("YSQL — Postgres-compatible SQL layer.");
    println!("  Reuses the upstream PostgreSQL executor and parser code");
    println!("  with the storage layer swapped to DocDB.");
    println!("  Result: very high upstream-feature parity (foreign keys,");
    println!("  partial indexes, GIN, GIST, materialised views, extensions).");
    println!("  Tracks recent Postgres major versions on a delay.");
}

fn run_ycql() {
    println!("YCQL — Cassandra-compatible CQL layer.");
    println!("  Wire-compatible with Apache Cassandra and Datastax drivers.");
    println!("  Adds strongly-consistent transactions on top of CQL operations,");
    println!("  unlike Cassandra's eventual default.");
    println!("  Targets customers migrating off Cassandra who want stronger");
    println!("  guarantees without changing client libraries.");
}

fn run_license() {
    println!("License posture:");
    println!("  Apache 2.0 since 2019.");
    println!("  Yugabyte publicly committed not to follow MongoDB/Cockroach");
    println!("  into source-available restrictive licences.");
    println!("  Enterprise features (xCluster, encryption-at-rest with KMS,");
    println!("  some managed-cloud features) are still Apache-licensed,");
    println!("  with paid support being the monetisation path for OSS users.");
}

fn run_managed() {
    println!("YugabyteDB Aeon (formerly Managed).");
    println!("  Fully managed cloud service on AWS, GCP, Azure.");
    println!("  Single-region, multi-region, multi-zone deployments.");
    println!("  Read Replicas, xCluster async/sync replication across regions.");
    println!("  Self-serve cluster creation in <10 minutes.");
    println!("  Private cluster (VPC peering) for enterprise tenants.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  YugabyteDB         Apache 2.0, free forever, self-host.");
    println!("  Managed Sandbox    free single-node cluster for development.");
    println!("  Managed Dedicated  per-VCPU-hour + storage GB.");
    println!("  Enterprise         custom contracts, BYO-cloud, dedicated SE.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  General Motors       connected-vehicle data");
    println!("  Mastercard           payments backbone");
    println!("  Wells Fargo          banking core modernisation");
    println!("  Kroger               retail data platform");
    println!("  Fidelity             investment data");
    println!("  Justuno              e-commerce personalisation");
    println!("  Beijing-based fintech and telco customers (APAC presence)");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "yugabyte-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "architecture" => run_architecture(),
        "ysql" => run_ysql(),
        "ycql" => run_ycql(),
        "license" => run_license(),
        "managed" => run_managed(),
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
        run_architecture();
        run_ysql();
        run_ycql();
        run_license();
        run_managed();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("yugabyte-cli");
        print_version();
    }
}
