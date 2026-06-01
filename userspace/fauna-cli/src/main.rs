#![deny(clippy::all)]
//! fauna-cli — personality CLI for Fauna, the document-relational database
//! built by ex-Twitter infrastructure engineers around the Calvin protocol.
//!
//! Founded 2012 by Evan Weaver and Matt Freels (ex-Twitter, where they
//! worked on FlockDB / large-scale storage). Headquartered in SF. Raised
//! ~$57M Series C in 2021. Notable for the Fauna Query Language (FQL),
//! the Calvin-style deterministic distributed transaction protocol, and
//! a globally distributed strict-serializable consistency story. In March
//! 2025 the company announced the sunset of the Fauna Cloud service, with
//! shutdown completed later that year — a high-profile end for one of the
//! "modern transactional database" wave.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Fauna document-relational database personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about       Founders, Twitter origins, sunset");
    println!("    model       Document-relational + relations + indexes");
    println!("    fql         The Fauna Query Language");
    println!("    calvin      The Calvin deterministic transaction protocol");
    println!("    isolation   Strict serializable, no leader bottleneck");
    println!("    serverless  HTTPS-only, no connection pool");
    println!("    sunset      The March 2025 shutdown announcement");
    println!("    customers   Selected named accounts");
    println!("    help        Show this help");
    println!("    version     Show version");
}

fn print_version() { println!("fauna-cli 0.1.0 (Calvin-protocol personality build)"); }

fn run_about() {
    println!("Fauna, Inc.");
    println!("  Founded:    2012, San Francisco.");
    println!("  Founders:   Evan Weaver, Matt Freels.");
    println!("  Heritage:   Both ex-Twitter infrastructure. Weaver was a");
    println!("              prolific OSS contributor (Ruby + storage scene).");
    println!("  Funding:    ~$57M Series C 2021.");
    println!("  Sunset:     March 2025 announcement of Fauna Cloud shutdown,");
    println!("              giving customers a 90-day migration window.");
    println!("  Lesson:     'Right database, wrong distribution model' — an");
    println!("              expensive proof that strict serializability alone");
    println!("              does not win the developer market against MySQL/PG.");
}

fn run_model() {
    println!("Data model: document-relational.");
    println!("  Collections of documents (think MongoDB).");
    println!("  Documents may reference other documents (think SQL foreign keys).");
    println!("  Indexes are first-class objects with their own permissions.");
    println!("  Schemas may be enforced or left flexible.");
    println!("  Built-in user/permission/role objects.");
}

fn run_fql() {
    println!("FQL — Fauna Query Language.");
    println!("  Functional, composable, no separate ORM needed.");
    println!("  Originally a Lisp-like S-expression form, modernised in");
    println!("  v10 (2023) to a TypeScript-ish syntax with familiar dot-method");
    println!("  chaining (e.g. Collection('users').firstWhere(.email == 'x')).");
    println!("  Pure functional: no side effects outside the explicit Update/");
    println!("  Create/Delete combinators.");
}

fn run_calvin() {
    println!("Calvin — the distributed transaction protocol.");
    println!("  Daniel Abadi's 2012 paper at Yale: pre-order transactions");
    println!("  through a global log, execute deterministically.");
    println!("  No 2PC, no leader bottleneck per shard.");
    println!("  Fauna's adaptation supports geographically distributed regions");
    println!("  with strict-serializable cross-region transactions.");
    println!("  Cost: every transaction includes a global ordering step,");
    println!("  bounding write latency by inter-region round-trip.");
}

fn run_isolation() {
    println!("Isolation level: strict serializable.");
    println!("  All transactions appear to execute in a total order that");
    println!("  respects real-time ordering.");
    println!("  No phantoms, no read skew, no lost updates by construction.");
    println!("  Trade-off vs PG's read-committed default: stronger correctness,");
    println!("  but higher per-transaction latency in multi-region deployments.");
}

fn run_serverless() {
    println!("Serverless access model.");
    println!("  HTTPS-only protocol. No persistent connections, no pool.");
    println!("  Works natively from Lambda, Cloudflare Workers, Vercel, etc.");
    println!("  Authentication via short-lived signed tokens, ABAC role model.");
    println!("  Eliminated the 'serverless + Postgres = connection storm' pain,");
    println!("  but at the cost of locking customers to FQL.");
}

fn run_sunset() {
    println!("Sunset (March 2025).");
    println!("  Announcement: Fauna Cloud will be wound down.");
    println!("  Customers given a multi-month migration window.");
    println!("  Open-sourced core technology under a permissive licence.");
    println!("  Reasons cited: GTM headwinds + concentration of the modern");
    println!("                 DB market into a few hyperscaler-aligned bets.");
    println!("  Notable as a case study in 'too novel to land': great paper,");
    println!("  great engineering, lukewarm developer adoption.");
}

fn run_customers() {
    println!("Selected customers (historical):");
    println!("  Nextdoor       neighbourhood data");
    println!("  Drizly         alcohol marketplace");
    println!("  Cardano        Daedalus-related infra");
    println!("  Hannaford      grocery rewards");
    println!("  Various Jamstack-era startups using FaunaDB + Netlify/Vercel.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fauna-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "model" => run_model(),
        "fql" => run_fql(),
        "calvin" => run_calvin(),
        "isolation" => run_isolation(),
        "serverless" => run_serverless(),
        "sunset" => run_sunset(),
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
        run_model();
        run_fql();
        run_calvin();
        run_isolation();
        run_serverless();
        run_sunset();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("fauna-cli");
        print_version();
    }
}
