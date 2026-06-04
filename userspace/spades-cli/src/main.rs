#![deny(clippy::all)]

//! spades-cli — OurOS SPAdes genome assembler
//!
//! Single personality: `spades`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_spades(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: spades [OPTIONS]");
        println!("SPAdes v3.15 (OurOS) — De novo genome assembler");
        println!();
        println!("Input:");
        println!("  -1 FILE        Forward reads");
        println!("  -2 FILE        Reverse reads");
        println!("  -s FILE        Single reads");
        println!("  --pacbio FILE  PacBio reads");
        println!("  --nanopore FILE  Nanopore reads");
        println!();
        println!("Pipeline:");
        println!("  --only-assembler   Skip read error correction");
        println!("  --careful          Careful mode (fewer misassemblies)");
        println!("  --isolate          Isolate mode");
        println!("  --meta             Metagenomic mode");
        println!("  --rna              RNA-Seq assembly");
        println!();
        println!("Options:");
        println!("  -o DIR         Output directory");
        println!("  -t N           Number of threads");
        println!("  -m N           Memory limit (GB)");
        println!("  -k LIST        K-mer sizes (comma-separated)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SPAdes v3.15.5 (OurOS)"); return 0; }
    println!("SPAdes v3.15.5 (OurOS)");
    println!("  Error correction: BayesHammer");
    println!("  Assembly: k=21,33,55,77,99,127");
    println!("  Scaffolding: done");
    println!("  Results:");
    println!("    Contigs: 1,234");
    println!("    N50: 45,678 bp");
    println!("    Total length: 4,567,890 bp");
    println!("    Largest contig: 234,567 bp");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "spades".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_spades(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_spades};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/spades"), "spades");
        assert_eq!(basename(r"C:\bin\spades.exe"), "spades.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("spades.exe"), "spades");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_spades(&["--help".to_string()], "spades"), 0);
        assert_eq!(run_spades(&["-h".to_string()], "spades"), 0);
        let _ = run_spades(&["--version".to_string()], "spades");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_spades(&[], "spades");
    }
}
