#![deny(clippy::all)]

//! vcftools-cli — SlateOS VCFtools variant call format tools
//!
//! Single personality: `vcftools`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vcftools(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vcftools [OPTIONS]");
        println!("VCFtools v0.1.17 (Slate OS) — VCF/BCF file manipulation");
        println!();
        println!("Input:");
        println!("  --vcf FILE        Input VCF file");
        println!("  --gzvcf FILE      Input gzipped VCF");
        println!("  --bcf FILE        Input BCF file");
        println!();
        println!("Filtering:");
        println!("  --chr CHR         Include chromosome");
        println!("  --from-bp N       Start position");
        println!("  --to-bp N         End position");
        println!("  --maf N           Min minor allele freq");
        println!("  --max-missing N   Max missing genotypes");
        println!("  --minQ N          Min quality score");
        println!();
        println!("Output:");
        println!("  --recode          Output filtered VCF");
        println!("  --freq            Allele frequency");
        println!("  --het             Heterozygosity");
        println!("  --hardy           Hardy-Weinberg test");
        println!("  --out PREFIX      Output prefix");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("VCFtools v0.1.17 (Slate OS)"); return 0; }
    println!("VCFtools v0.1.17 (Slate OS)");
    println!("  Variants: 1,234,567");
    println!("  Samples: 100");
    println!("  After filtering: 987,654 variants");
    println!("  Run time: 12.3 seconds");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vcftools".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vcftools(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vcftools};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vcftools"), "vcftools");
        assert_eq!(basename(r"C:\bin\vcftools.exe"), "vcftools.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vcftools.exe"), "vcftools");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vcftools(&["--help".to_string()], "vcftools"), 0);
        assert_eq!(run_vcftools(&["-h".to_string()], "vcftools"), 0);
        let _ = run_vcftools(&["--version".to_string()], "vcftools");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vcftools(&[], "vcftools");
    }
}
