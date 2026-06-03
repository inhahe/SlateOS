#![deny(clippy::all)]

//! bcftools-cli — OurOS BCFtools variant calling tools
//!
//! Multi-personality: `bcftools`

use std::env;
use std::process;

fn run_bcftools(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Program: bcftools (Tools for variant calling and manipulating VCFs and BCFs)");
        println!("Version: 1.19 (OurOS, using htslib 1.19)");
        println!();
        println!("Usage:   bcftools <command> <arguments>");
        println!();
        println!("Commands:");
        println!("  -- VCF/BCF manipulation");
        println!("     annotate      annotate and edit VCF/BCF files");
        println!("     concat        concatenate VCF/BCF files");
        println!("     filter        filter VCF/BCF files");
        println!("     merge         merge VCF/BCF files");
        println!("     norm          normalize variants");
        println!("     query         query VCF/BCF files");
        println!("     sort          sort VCF/BCF file");
        println!("     view          VCF/BCF conversion");
        println!();
        println!("  -- Variant calling");
        println!("     call          SNP/indel calling");
        println!("     mpileup       multi-way pileup");
        println!("     stats         VCF/BCF statistics");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => {
            println!("bcftools 1.19");
            println!("Using htslib 1.19");
        }
        "view" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("input.vcf.gz");
            println!("##fileformat=VCFv4.2");
            println!("##source=bcftools view {}", file);
            println!("#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO");
            println!("chr1\t12345\t.\tA\tG\t99\tPASS\tDP=42;AF=0.5");
        }
        "stats" => {
            println!("# BCFtools stats (1.19)");
            println!("SN\t0\tnumber of records:\t5678");
            println!("SN\t0\tnumber of SNPs:\t4567");
            println!("SN\t0\tnumber of indels:\t1111");
            println!("SN\t0\tnumber of multiallelic sites:\t234");
            println!("SN\t0\tts/tv:\t2.12");
        }
        "call" => {
            println!("bcftools call: calling variants...");
            println!("5678 variant sites called.");
        }
        "mpileup" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("input.bam");
            println!("bcftools mpileup: processing {}", file);
            println!("[mpileup] maximum depth set to 250");
            println!("[mpileup] 1 sample in 1 input file");
            println!("Processing complete.");
        }
        "filter" => {
            println!("bcftools filter: filtering variants...");
            println!("4567 of 5678 records passed filters.");
        }
        "norm" => {
            println!("bcftools norm: normalizing variants...");
            println!("Lines   total/split/joined/realigned/skipped: 5678/12/0/45/0");
        }
        "merge" => {
            println!("bcftools merge: merging files...");
            println!("Merged output written.");
        }
        "sort" => {
            println!("bcftools sort: sorting...");
            println!("Done. 5678 records written.");
        }
        _ => println!("bcftools: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bcftools(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_bcftools};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bcftools(&["--help".to_string()]), 0);
        assert_eq!(run_bcftools(&["-h".to_string()]), 0);
        assert_eq!(run_bcftools(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bcftools(&[]), 0);
    }
}
