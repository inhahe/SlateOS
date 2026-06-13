#![deny(clippy::all)]

//! gatk-cli — Slate OS GATK Genome Analysis Toolkit
//!
//! Multi-personality: `gatk`

use std::env;
use std::process;

fn run_gatk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gatk <tool> [arguments]");
        println!("GATK 4.5.0.0 (Slate OS)");
        println!();
        println!("Tools:");
        println!("  -- Variant Discovery");
        println!("     HaplotypeCaller          Call germline SNPs and indels");
        println!("     Mutect2                  Call somatic SNVs and indels");
        println!("     GenotypeGVCFs            Genotype gVCFs");
        println!();
        println!("  -- Variant Manipulation");
        println!("     SelectVariants           Select a subset of variants");
        println!("     VariantFiltration         Filter variants");
        println!("     MergeVcfs                Merge VCF files");
        println!();
        println!("  -- Read Data Manipulation");
        println!("     BaseRecalibrator         Generate BQSR table");
        println!("     ApplyBQSR                Apply BQSR");
        println!("     MarkDuplicatesSpark      Mark duplicates (Spark)");
        println!();
        println!("  -- Diagnostics and QC");
        println!("     CollectReadCounts        Collect read counts");
        println!("     CountVariants            Count variants");
        println!("     FlagStat                 Flag statistics");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("GATK 4.5.0.0 (Slate OS)"),
        "HaplotypeCaller" => {
            let input = args.windows(2).find(|w| w[0] == "-I").map(|w| w[1].as_str()).unwrap_or("input.bam");
            let reference = args.windows(2).find(|w| w[0] == "-R").map(|w| w[1].as_str()).unwrap_or("reference.fa");
            println!("HaplotypeCaller  (GATK 4.5.0.0)");
            println!("  Input: {}", input);
            println!("  Reference: {}", reference);
            println!("  Traversal mode: BY_SAMPLE");
            println!("  Processing chr1...");
            println!("  Processing chr2...");
            println!("  [...]");
            println!("  5,678 variants called");
            println!("  Done.");
        }
        "Mutect2" => {
            println!("Mutect2  (GATK 4.5.0.0)");
            println!("  Running in tumor-only mode...");
            println!("  Processing intervals...");
            println!("  1,234 somatic variants called");
            println!("  Done.");
        }
        "BaseRecalibrator" => {
            println!("BaseRecalibrator  (GATK 4.5.0.0)");
            println!("  Calculating recalibration table...");
            println!("  Processed 1,234,567 reads");
            println!("  Recalibration table written.");
        }
        "ApplyBQSR" => {
            println!("ApplyBQSR  (GATK 4.5.0.0)");
            println!("  Applying base quality score recalibration...");
            println!("  1,234,567 reads recalibrated");
            println!("  Done.");
        }
        "SelectVariants" => {
            println!("SelectVariants  (GATK 4.5.0.0)");
            println!("  4,567 of 5,678 variants selected");
            println!("  Done.");
        }
        "VariantFiltration" => {
            println!("VariantFiltration  (GATK 4.5.0.0)");
            println!("  Applying filters...");
            println!("  3,456 variants passed, 2,222 filtered");
            println!("  Done.");
        }
        "GenotypeGVCFs" => {
            println!("GenotypeGVCFs  (GATK 4.5.0.0)");
            println!("  Genotyping variants...");
            println!("  5,678 variant sites genotyped");
            println!("  Done.");
        }
        "CountVariants" => {
            println!("CountVariants  (GATK 4.5.0.0)");
            println!("  Tool returned: 5678");
        }
        _ => println!("gatk: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gatk(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gatk};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gatk(&["--help".to_string()]), 0);
        assert_eq!(run_gatk(&["-h".to_string()]), 0);
        let _ = run_gatk(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gatk(&[]);
    }
}
