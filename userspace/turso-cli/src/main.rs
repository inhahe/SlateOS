#![deny(clippy::all)]
//! turso-cli — personality CLI for Turso, the edge-replicated SQLite
//! database built on the libSQL fork.
//!
//! Turso is operated by ChiselStrike Inc., founded 2022 by Glauber Costa
//! and Pekka Enberg (both ex-ScyllaDB, both deep in low-level systems
//! work; Costa has worked on KVM/Xen at Red Hat). The product began as
//! 'ChiselStore' edge SQLite-on-Raft, evolved through 'ChiselStrike' (a
//! TypeScript ORM), and pivoted hard to 'Turso' centred on the libSQL
//! fork of SQLite. libSQL adds: native server mode, network replication,
//! WAL-streaming protocol, and a permissive license re-base. Turso's
//! pitch is 'one database per user, deployed at the edge' — millions of
//! tiny databases, each replicated to PoPs near the user.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Turso edge SQLite personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, ChiselStrike pivot to Turso");
    println!("    libsql        The libSQL fork of SQLite");
    println!("    edge          Edge replicas in 30+ regions");
    println!("    embedded      Embedded replicas pattern");
    println!("    perdb         Database-per-user product pattern");
    println!("    vector        libSQL native vector + ANN");
    println!("    pricing       Generous free tier + per-DB bands");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("turso-cli 0.1.0 (libSQL-edge personality build)"); }

fn run_about() {
    println!("Turso (ChiselStrike, Inc.)");
    println!("  Founded:    2022, Lisbon / remote.");
    println!("  Founders:   Glauber Costa (CEO), Pekka Enberg (CTO).");
    println!("              Both ex-ScyllaDB, deep systems backgrounds.");
    println!("              Costa: ex-Red Hat KVM/Xen + ScyllaDB.");
    println!("              Enberg: long-time Linux kernel hacker + ScyllaDB.");
    println!("  Pivot:      ChiselStrike (TS ORM) -> Turso (libSQL edge DB).");
    println!("  Funding:    ~$24M Series A 2024.");
    println!("  Pitch:      'Database per user' at the edge, on libSQL.");
}

fn run_libsql() {
    println!("libSQL — the fork of SQLite that Turso steers.");
    println!("  Apache 2.0 fork that adds:");
    println!("    - Server mode over a network protocol (HTTP + WebSocket).");
    println!("    - Embedded replicas (local SQLite that syncs to a remote).");
    println!("    - WAL streaming for incremental replication.");
    println!("    - Native vector columns and ANN indexes.");
    println!("    - User-defined functions via Wasm.");
    println!("  Stays close to upstream SQLite where compatible.");
    println!("  Source: github.com/tursodatabase/libsql");
}

fn run_edge() {
    println!("Edge replicas.");
    println!("  Each Turso database has a primary region (writes) and any");
    println!("  number of read replicas in other regions.");
    println!("  30+ available regions across continents.");
    println!("  Reads served locally on the nearest replica.");
    println!("  Writes routed to the primary; ack typically 50-150ms");
    println!("  intercontinentally, single-digit-ms intra-region.");
}

fn run_embedded() {
    println!("Embedded replicas — the killer pattern.");
    println!("  libSQL client embeds a local SQLite file in the application.");
    println!("  That local file is a sync'd replica of the remote DB.");
    println!("  Reads hit local SQLite — sub-millisecond, no network.");
    println!("  Writes proxy to the remote primary.");
    println!("  Result: SQLite-fast reads for a server-side application,");
    println!("  with sync to a remote authority for durability.");
}

fn run_perdb() {
    println!("Database-per-user — the pricing-enabled pattern.");
    println!("  Turso prices per database cheaply enough that creating");
    println!("  one DB per end user is viable.");
    println!("  Each user gets isolated storage, independent schema,");
    println!("  independent backups, no cross-tenant query risk.");
    println!("  Particularly popular for AI agent apps (memory per user)");
    println!("  and for B2B SaaS that want hard tenant isolation.");
}

fn run_vector() {
    println!("Vector + ANN built into libSQL.");
    println!("  Native F32_BLOB and F16_BLOB column types.");
    println!("  Vector index built on libsql_vector_idx.");
    println!("  Approximate nearest-neighbour queries via SQL functions.");
    println!("  No separate vector database service needed for typical");
    println!("  embedding sizes; the same DB stores app data + vectors.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free          ~9 GB storage, 500 databases, 1B row reads/mo.");
    println!("  Scaler        per-extra-DB and per-storage-GB.");
    println!("  Pro           larger quotas + advanced features.");
    println!("  Enterprise    custom, dedicated infra, BYO-cloud option.");
    println!("Free tier is generous on purpose — the product is built");
    println!("for hobbyists and small-MAU SaaS to graduate from.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Val.town          Cloudflare-Worker code hosting platform");
    println!("  Various AI agent startups using Turso as agent memory");
    println!("  Hobbyist + indie SaaS communities (HN-prominent)");
    println!("  Mobile-app makers using embedded replicas pattern");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "turso-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "libsql" => run_libsql(),
        "edge" => run_edge(),
        "embedded" => run_embedded(),
        "perdb" => run_perdb(),
        "vector" => run_vector(),
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
        run_libsql();
        run_edge();
        run_embedded();
        run_perdb();
        run_vector();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("turso-cli");
        print_version();
    }
}
