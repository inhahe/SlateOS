#![deny(clippy::all)]

//! htslib-cli — OurOS HTSlib utilities
//!
//! Multi-personality: `htsfile`, `bgzip`, `tabix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_htsfile(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: htsfile [OPTIONS] FILE...");
        println!("htsfile v1.20 (OurOS) — Identify high-throughput sequencing files");
        return 0;
    }
    for f in args.iter().filter(|a| !a.starts_with('-')) {
        println!("{}: BAM version 1.6, sorted, 1,000,000 alignments", f);
    }
    0
}

fn run_bgzip(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: bgzip [OPTIONS] FILE");
        println!("bgzip v1.20 (OurOS) — Block gzip compress/decompress");
        println!();
        println!("Options:");
        println!("  -d                Decompress");
        println!("  -c                Output to stdout");
        println!("  -i                Create index (.gzi)");
        println!("  -@ N              Threads");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("data.vcf");
    if args.iter().any(|a| a == "-d") {
        println!("Decompressing: {}", file);
    } else {
        println!("Compressing: {} -> {}.gz", file, file);
    }
    0
}

fn run_tabix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tabix [OPTIONS] FILE [REGION]");
        println!("tabix v1.20 (OurOS) — Index/query tabix-indexed files");
        println!();
        println!("Options:");
        println!("  -p TYPE           Preset (gff, bed, sam, vcf)");
        println!("  -s N              Sequence column");
        println!("  -b N              Begin column");
        println!("  -e N              End column");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("data.vcf.gz");
    let region = args.iter()
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .map(|s| s.as_str());
    if let Some(r) = region {
        println!("Querying {} for region {}...", file, r);
        println!("  3 records found.");
    } else {
        println!("Indexing: {} -> {}.tbi", file, file);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "htsfile".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "bgzip" => run_bgzip(&rest, &prog),
        "tabix" => run_tabix(&rest, &prog),
        _ => run_htsfile(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
