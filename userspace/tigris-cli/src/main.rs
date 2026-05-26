#![deny(clippy::all)]
//! tigris-cli — personality CLI for Tigris Data, the S3-compatible globally
//! distributed object storage built on FoundationDB.
//!
//! Founded 2022 by Ovais Tariq, Yevgeniy Firsov, and Himank Chaudhary in
//! Sunnyvale, CA. Tariq previously ran the storage org at Uber and has a
//! long career in MySQL/Postgres open-source operations. The company
//! initially shipped a Tigris database product (a document/realtime database
//! on top of FoundationDB) but in late 2024 sunset the database surface
//! to focus exclusively on Tigris Object Storage — an S3-compatible
//! globally distributed object store with no egress fees, designed to
//! sit behind Fly.io apps and Cloudflare Workers as a regionally aware
//! alternative to S3 + CloudFront.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Tigris Data object storage personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Uber lineage, sunset of DB product");
    println!("    storage       S3-compatible global object store");
    println!("    foundationdb  Underlying metadata via FoundationDB");
    println!("    flyio         Strategic Fly.io partnership + bundling");
    println!("    egress        No-egress-fee positioning vs S3");
    println!("    regions       Tigris global PoPs + automatic data placement");
    println!("    pricing       Per-GB-month storage, no egress");
    println!("    customers     Fly.io apps, dataset hosting");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("tigris-cli 0.1.0 (post-DB-sunset object-storage build)"); }

fn run_about() {
    println!("Tigris Data, Inc.");
    println!("  Founded:    2022, Sunnyvale, California.");
    println!("  Founders:   Ovais Tariq (CEO), Yevgeniy Firsov, Himank Chaudhary.");
    println!("  Heritage:   Tariq ran storage engineering at Uber; prior career");
    println!("              in MySQL/Percona consulting and operations.");
    println!("  Original:   Tigris DB — a document + realtime database on top");
    println!("              of FoundationDB, sunset in 2024.");
    println!("  Pivot:      Focus on Tigris Object Storage, the S3-compatible");
    println!("              global object store.");
    println!("  Partner:    Tight bundling with Fly.io (default storage backend).");
}

fn run_storage() {
    println!("Tigris Object Storage.");
    println!("  S3-compatible API: standard PUT/GET/LIST/DELETE/multipart.");
    println!("  No egress fees — a deliberate differentiator vs S3.");
    println!("  Automatic regional placement: objects propagate to PoPs near");
    println!("  the readers that access them, not just the originator.");
    println!("  Strong-consistency for object writes; eventual for cross-region");
    println!("  metadata propagation.");
}

fn run_foundationdb() {
    println!("FoundationDB underneath.");
    println!("  All bucket and object metadata lives in a global FoundationDB");
    println!("  cluster, exploiting FDB's strict-serializable transactions.");
    println!("  Object payloads land on regional storage nodes (similar to");
    println!("  Ceph + zone-aware placement).");
    println!("  FoundationDB choice inherits from Apple/Snowflake-era hardening.");
}

fn run_flyio() {
    println!("Fly.io partnership.");
    println!("  Tigris is the default storage backend for Fly.io machines.");
    println!("  Provisioned with `fly storage create`.");
    println!("  Same-region access between Fly apps and Tigris buckets is");
    println!("  near-zero latency.");
    println!("  Joint go-to-market: Tigris bills via Fly for small users,");
    println!("  direct for larger contracts.");
}

fn run_egress() {
    println!("No-egress positioning.");
    println!("  AWS S3 + CloudFront charges egress per GB ($0.05-$0.09 / GB).");
    println!("  Backblaze B2, Cloudflare R2, and Tigris all pitch zero-egress");
    println!("  as the primary cost advantage for outbound-heavy workloads");
    println!("  (CDN origins, dataset distribution, AI model weights, video).");
    println!("  Tigris's twist: globally distributed by default, not just");
    println!("  per-region with optional replication.");
}

fn run_regions() {
    println!("Regions / PoPs.");
    println!("  ~15 PoPs across North America, Europe, Asia, Australia,");
    println!("  South America (lineup expanding).");
    println!("  Data placement is automatic by default — a hot read pattern");
    println!("  from Frankfurt will see the object cached/co-located there.");
    println!("  Explicit region pinning available for compliance / data");
    println!("  residency requirements.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Storage          ~$0.02 / GB-month (varies by region/tier).");
    println!("  Egress           $0.");
    println!("  Operations       per-million-call price for PUT/GET/LIST.");
    println!("  Cheaper than S3 once egress + CDN charges are factored in.");
}

fn run_customers() {
    println!("Selected customers + use cases:");
    println!("  Fly.io tenants    default object storage for Fly machines.");
    println!("  AI dataset hosts  model weights + training data distribution.");
    println!("  CDN-origin users  global origin without paying CloudFront tax.");
    println!("  Video platforms   media files served close to viewers.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "tigris-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "storage" => run_storage(),
        "foundationdb" => run_foundationdb(),
        "flyio" => run_flyio(),
        "egress" => run_egress(),
        "regions" => run_regions(),
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
        run_storage();
        run_foundationdb();
        run_flyio();
        run_egress();
        run_regions();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("tigris-cli");
        print_version();
    }
}
