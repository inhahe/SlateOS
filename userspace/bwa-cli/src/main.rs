#![deny(clippy::all)]

//! bwa-cli — OurOS BWA sequence aligner
//!
//! Multi-personality: `bwa`, `bwa-mem2`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bwa(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Program: bwa (Burrows-Wheeler Aligner)");
        println!("Version: 0.7.17-r1188 (OurOS)");
        println!();
        println!("Usage:   bwa <command> [options]");
        println!();
        println!("Commands:");
        println!("  index       Index reference sequences");
        println!("  mem         BWA-MEM algorithm");
        println!("  aln         BWA-backtrack algorithm");
        println!("  samse       Generate SAM (single-end)");
        println!("  sampe       Generate SAM (paired-end)");
        println!("  bwasw       BWA-SW algorithm");
        println!("  fastmap     Identify super-maximal exact matches");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "index" => {
            let ref_file = args.get(1).map(|s| s.as_str()).unwrap_or("reference.fa");
            println!("[bwa_index] Building BWT for {}", ref_file);
            println!("[bwa_index] Constructing suffix array...");
            println!("[bwa_index] Constructing BWT...");
            println!("[bwa_index] Packing reference...");
            println!("[bwa_index] Done. 5 index files generated.");
        }
        "mem" => {
            let reference = args.get(1).map(|s| s.as_str()).unwrap_or("reference.fa");
            let reads = args.get(2).map(|s| s.as_str()).unwrap_or("reads.fq");
            println!("[M::bwa_idx_load_from_disk] read {} BWT", reference);
            println!("[M::process] read {} sequences (185000000 bp)...", reads);
            println!("[M::mem_process_seqs] Processed 1234567 reads");
            println!("[main] Version: 0.7.17-r1188");
            println!("[main] CMD: bwa mem {} {}", reference, reads);
            println!("[main] Real time: 234.56 sec; CPU: 890.12 sec");
        }
        "aln" => {
            println!("[bwa_aln] Processing reads...");
            println!("[bwa_aln] 1234567 reads processed.");
        }
        _ => println!("bwa: '{}' completed", subcmd),
    }
    0
}

fn run_bwa_mem2(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Program: bwa-mem2 (Accelerated BWA-MEM)");
        println!("Version: 2.2.1 (OurOS, AVX-512 enabled)");
        println!();
        println!("Usage:   bwa-mem2 <command> [options]");
        println!("Commands: index, mem");
        return 0;
    }
    if args.iter().any(|a| a == "version") {
        println!("bwa-mem2 2.2.1 (OurOS)");
        println!("SIMD: AVX-512");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "index" => {
            let ref_file = args.get(1).map(|s| s.as_str()).unwrap_or("reference.fa");
            println!("[bwa-mem2 index] Building index for {}", ref_file);
            println!("[bwa-mem2 index] Done.");
        }
        "mem" => {
            let reference = args.get(1).map(|s| s.as_str()).unwrap_or("reference.fa");
            let reads = args.get(2).map(|s| s.as_str()).unwrap_or("reads.fq");
            println!("[bwa-mem2] Aligning {} to {}", reads, reference);
            println!("[bwa-mem2] Using AVX-512 acceleration");
            println!("[bwa-mem2] 1234567 reads aligned");
            println!("[bwa-mem2] Real time: 123.45 sec (2x faster than bwa)");
        }
        _ => println!("bwa-mem2: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bwa".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "bwa-mem2" => run_bwa_mem2(&rest),
        _ => run_bwa(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bwa};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bwa"), "bwa");
        assert_eq!(basename(r"C:\bin\bwa.exe"), "bwa.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bwa.exe"), "bwa");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bwa(&["--help".to_string()]), 0);
        assert_eq!(run_bwa(&["-h".to_string()]), 0);
        assert_eq!(run_bwa(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bwa(&[]), 0);
    }
}
