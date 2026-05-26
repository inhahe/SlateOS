#![deny(clippy::all)]
//! cockroachdb-cli — personality CLI for CockroachDB, the distributed SQL
//! database built by ex-Google engineers around the Spanner papers.
//!
//! Founded 2015 by Spencer Kimball, Peter Mattis, and Ben Darnell. All
//! three worked on Google's storage stack; Kimball and Mattis previously
//! co-founded the image-hosting startup acquired by Google. Cockroach
//! Labs raised a $278M Series F in Dec 2021 at a $5B valuation. Built on
//! a sharded RocksDB (now Pebble) storage layer with Raft consensus per
//! range, MVCC, and a Postgres-compatible SQL layer. Switched the core
//! license from Apache 2.0 to BSL/CCL in 2019 and to a fully proprietary
//! Enterprise-only model in Aug 2024.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — CockroachDB distributed SQL personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Spanner inspiration");
    println!("    architecture  Ranges, Raft, MVCC, Pebble storage");
    println!("    sql           Postgres wire compatibility");
    println!("    geo           Multi-region survival goals + locality");
    println!("    serverless    Cockroach Cloud Serverless");
    println!("    license       Apache -> BSL -> Enterprise-only history");
    println!("    pricing       Self-host vs Standard vs Advanced");
    println!("    customers     DoorDash, Netflix, Comcast, JPMC, etc.");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("cockroachdb-cli 0.1.0 (Spanner-lineage personality build)"); }

fn run_about() {
    println!("Cockroach Labs, Inc.");
    println!("  Founded:    2015, New York City.");
    println!("  Founders:   Spencer Kimball (CEO), Peter Mattis (CTO),");
    println!("              Ben Darnell. All ex-Google infrastructure.");
    println!("  Kimball + Mattis previously co-founded Viewfinder (image");
    println!("              hosting), acquired by Square in 2013.");
    println!("  Funding:    ~$633M total raised.");
    println!("              $278M Series F Dec 2021 at $5B valuation.");
    println!("  Pitch:      'Make data easy' — distributed SQL with Postgres");
    println!("              wire compat, survives region failure transparently.");
}

fn run_architecture() {
    println!("Architecture:");
    println!("  Storage    Pebble (a Cockroach-built LevelDB descendant in Go).");
    println!("  Range      a 512MB-ish key span replicated by Raft (default 3x).");
    println!("  MVCC       multi-version timestamp-ordered values.");
    println!("  Distributed SQL  CockroachDB executor plans across ranges.");
    println!("  Time       hybrid logical clocks (HLC) with NTP bound.");
    println!("  Transactions strict serializable, no 2PC bottleneck thanks to");
    println!("               HLC-coordinated parallel commit.");
}

fn run_sql() {
    println!("SQL surface:");
    println!("  Postgres-compatible wire protocol (libpq, pgwire).");
    println!("  Most ANSI SQL features: joins, CTEs, window functions, JSON,");
    println!("  ARRAYs, GIN-like inverted indexes, computed columns.");
    println!("  Online schema changes without table locks (asynchronous DDL).");
    println!("  Built-in PRIMARY KEY UUID with random distribution to avoid");
    println!("  range hot-spotting under monotonic insert workloads.");
}

fn run_geo() {
    println!("Multi-region — the Cockroach moat.");
    println!("  Survival goals: REGION, ZONE, no-survive.");
    println!("  Per-table or per-row locality: REGIONAL BY ROW pins each row");
    println!("    to a home region for low local read/write latency.");
    println!("  GLOBAL tables for reference data readable everywhere at low");
    println!("    latency, with writes paying global consensus cost.");
    println!("  Follower reads for stale-bounded local reads.");
    println!("  Transparent region failover within the survival goal.");
}

fn run_serverless() {
    println!("Cockroach Cloud Serverless.");
    println!("  Multi-tenant cluster where compute scales by Request Units.");
    println!("  Storage charged separately per GB.");
    println!("  Pause-to-zero on idle; cold start ~100s of ms.");
    println!("  Branded competition for Neon/PlanetScale at the small end.");
    println!("  Underneath: dedicated VMs share a single Cockroach cluster");
    println!("    with per-tenant key-space isolation.");
}

fn run_license() {
    println!("Licence history:");
    println!("  2015-2019    Apache 2.0 core.");
    println!("  2019         Switched core to Business Source Licence (BSL),");
    println!("                converts to Apache 2.0 after 3 years; enterprise");
    println!("                features under Cockroach Community Licence (CCL).");
    println!("  Aug 2024     Eliminated the free self-hosted tier; the core");
    println!("                product became Enterprise-only with no source");
    println!("                released. CockroachDB 24.3+ ships as proprietary.");
    println!("  Pre-24.3 BSL/Apache versions remain available historically.");
}

fn run_pricing() {
    println!("Pricing model (post-2024 licence change):");
    println!("  Self-Hosted Enterprise Free  small-scale free with telemetry,");
    println!("                                limited features.");
    println!("  Standard                      per-VCPU-month managed cloud.");
    println!("  Advanced                      dedicated cluster, custom SLAs,");
    println!("                                BYO-cloud (AWS PrivateLink etc.).");
    println!("  Cloud Serverless              per-RU + per-GB storage.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  DoorDash       global order ledger");
    println!("  Netflix        global services backend");
    println!("  Comcast        identity store");
    println!("  JP Morgan Chase regulated financial workloads");
    println!("  Bose           connected device data");
    println!("  Hard Rock      gaming wallet");
    println!("  T-Mobile       subscriber data");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "cockroachdb-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "architecture" => run_architecture(),
        "sql" => run_sql(),
        "geo" => run_geo(),
        "serverless" => run_serverless(),
        "license" => run_license(),
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
        run_sql();
        run_geo();
        run_serverless();
        run_license();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("cockroachdb-cli");
        print_version();
    }
}
