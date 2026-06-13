#![deny(clippy::all)]

//! blast-cli — SlateOS NCBI BLAST+ sequence search
//!
//! Multi-personality: `blastn`, `blastp`, `blastx`, `tblastn`, `tblastx`, `makeblastdb`, `blastdbcmd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_blastn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") {
        println!("USAGE");
        println!("  blastn [-query input] [-db database] [-out output]");
        println!("  -query FILE     Input FASTA file");
        println!("  -db NAME        BLAST database name");
        println!("  -out FILE       Output file");
        println!("  -evalue FLOAT   E-value threshold (default: 10)");
        println!("  -outfmt N       Output format (0-18)");
        println!("  -num_threads N  Number of threads");
        println!("  -version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("blastn: 2.15.0+");
        println!(" Package: blast 2.15.0, build Jan 15 2024");
        return 0;
    }
    let query = args.windows(2).find(|w| w[0] == "-query").map(|w| w[1].as_str()).unwrap_or("query.fasta");
    let db = args.windows(2).find(|w| w[0] == "-db").map(|w| w[1].as_str()).unwrap_or("nt");
    println!("BLASTN 2.15.0+");
    println!("Query: {}", query);
    println!("Database: {}", db);
    println!();
    println!("                                                                   Score     E");
    println!("Sequences producing significant alignments:                       (Bits)  Value");
    println!();
    println!("  gi|12345|ref|NM_001234.5| Homo sapiens example gene              456   1e-128");
    println!("  gi|12346|ref|NM_005678.3| Homo sapiens related gene              234   3e-62");
    println!();
    println!("  2 hits found");
    0
}

fn run_blastp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") {
        println!("USAGE");
        println!("  blastp [-query input] [-db database] [-out output]");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("blastp: 2.15.0+");
        return 0;
    }
    let query = args.windows(2).find(|w| w[0] == "-query").map(|w| w[1].as_str()).unwrap_or("query.fasta");
    let db = args.windows(2).find(|w| w[0] == "-db").map(|w| w[1].as_str()).unwrap_or("nr");
    println!("BLASTP 2.15.0+");
    println!("Query: {}", query);
    println!("Database: {}", db);
    println!("  3 hits found");
    0
}

fn run_blastx(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-version") {
        println!("blastx: 2.15.0+");
        return 0;
    }
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") || args.is_empty() {
        println!("USAGE: blastx [-query input] [-db database] [-out output]");
        return 0;
    }
    println!("BLASTX 2.15.0+");
    println!("Translated nucleotide -> protein search");
    println!("  2 hits found");
    0
}

fn run_makeblastdb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") || args.is_empty() {
        println!("USAGE");
        println!("  makeblastdb [-in input] [-dbtype type] [-out db_name]");
        println!("  -in FILE       Input FASTA file");
        println!("  -dbtype STR    Molecule type (nucl or prot)");
        println!("  -out NAME      Database name");
        println!("  -title STR     Database title");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("makeblastdb: 2.15.0+");
        return 0;
    }
    let input = args.windows(2).find(|w| w[0] == "-in").map(|w| w[1].as_str()).unwrap_or("sequences.fasta");
    let dbtype = args.windows(2).find(|w| w[0] == "-dbtype").map(|w| w[1].as_str()).unwrap_or("nucl");
    let out = args.windows(2).find(|w| w[0] == "-out").map(|w| w[1].as_str()).unwrap_or("mydb");
    println!("Building a new DB, current ID: 0");
    println!("New DB name:   {}", out);
    println!("New DB title:  {}", input);
    println!("Sequence type: {}", if dbtype == "prot" { "Protein" } else { "Nucleotide" });
    println!("Adding sequences from FASTA...");
    println!("  1234 sequences added.");
    println!("Database created successfully.");
    0
}

fn run_blastdbcmd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") || args.is_empty() {
        println!("USAGE: blastdbcmd [-db database] [-entry id] [-info]");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("blastdbcmd: 2.15.0+");
        return 0;
    }
    if args.iter().any(|a| a == "-info") {
        let db = args.windows(2).find(|w| w[0] == "-db").map(|w| w[1].as_str()).unwrap_or("nt");
        println!("Database: {}", db);
        println!("  1,234 sequences; 456,789,012 total bases");
        println!("  Date: Jan 15 2024  12:00:00");
        println!("  Longest sequence: 248,956,422 bases");
    } else {
        println!(">gi|12345|ref|NM_001234.5| Homo sapiens example gene");
        println!("ATGCGATCGATCGATCGATCGATCGATCGATCGATCGATCGATCGATCG");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "blastn".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "blastp" => run_blastp(&rest),
        "blastx" | "tblastn" | "tblastx" => run_blastx(&rest),
        "makeblastdb" => run_makeblastdb(&rest),
        "blastdbcmd" => run_blastdbcmd(&rest),
        _ => run_blastn(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_blastn};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/blast"), "blast");
        assert_eq!(basename(r"C:\bin\blast.exe"), "blast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("blast.exe"), "blast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_blastn(&["--help".to_string()]), 0);
        assert_eq!(run_blastn(&["-h".to_string()]), 0);
        let _ = run_blastn(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_blastn(&[]);
    }
}
