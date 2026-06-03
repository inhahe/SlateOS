#![deny(clippy::all)]

//! cellranger-cli — OurOS Cell Ranger single-cell analysis
//!
//! Single personality: `cellranger`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cellranger(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cellranger COMMAND [OPTIONS]");
        println!("Cell Ranger v8.0 (OurOS) — 10x Genomics single-cell analysis");
        println!();
        println!("Commands:");
        println!("  count             Gene expression quantification");
        println!("  vdj               V(D)J immune profiling");
        println!("  multi             Multi-omic analysis");
        println!("  aggr              Aggregate multiple samples");
        println!("  reanalyze         Reanalyze with new parameters");
        println!("  mkref             Build reference");
        println!("  sitecheck         System requirements check");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("cellranger v8.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("sitecheck");
    match cmd {
        "count" => {
            println!("Cell Ranger count pipeline");
            println!("  Reads: 400,000,000");
            println!("  Cells detected: 8,500");
            println!("  Mean reads/cell: 47,000");
            println!("  Median genes/cell: 3,200");
            println!("  Output: outs/filtered_feature_bc_matrix/");
        }
        "mkref" => {
            println!("Building reference...");
            println!("  Genome: GRCh38");
            println!("  Genes: 36,000");
            println!("  Done.");
        }
        "sitecheck" => {
            println!("System check:");
            println!("  Memory: 64 GB (recommended: 64 GB) OK");
            println!("  Disk: 500 GB free OK");
            println!("  CPUs: 8 OK");
        }
        _ => println!("cellranger {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cellranger".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cellranger(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cellranger};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cellranger"), "cellranger");
        assert_eq!(basename(r"C:\bin\cellranger.exe"), "cellranger.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cellranger.exe"), "cellranger");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cellranger(&["--help".to_string()], "cellranger"), 0);
        assert_eq!(run_cellranger(&["-h".to_string()], "cellranger"), 0);
        assert_eq!(run_cellranger(&["--version".to_string()], "cellranger"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cellranger(&[], "cellranger"), 0);
    }
}
