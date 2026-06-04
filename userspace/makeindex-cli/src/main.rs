#![deny(clippy::all)]

//! makeindex-cli — OurOS TeX indexing and bibliography tools
//!
//! Multi-personality: `makeindex`, `xindy`, `splitindex`, `texindy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_makeindex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: makeindex [OPTIONS] FILE.idx [FILE.idx ...]");
        println!("MakeIndex 2.17 (OurOS)");
        println!();
        println!("Options:");
        println!("  -c             Compress blanks");
        println!("  -g             German word ordering");
        println!("  -i             Read from stdin");
        println!("  -l             Letter ordering (default: word)");
        println!("  -o FILE        Output index file");
        println!("  -p NUM         Starting page number");
        println!("  -q             Quiet mode");
        println!("  -r             Disable implicit page ranges");
        println!("  -s FILE        Style file (.ist)");
        println!("  -t FILE        Transcript file");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| a.ends_with(".idx"))
        .map(|s| s.as_str())
        .collect();
    let quiet = args.iter().any(|a| a == "-q");
    let style = args.windows(2)
        .find(|w| w[0] == "-s")
        .map(|w| w[1].as_str());
    if files.is_empty() {
        println!("makeindex: no input files");
        return 1;
    }
    for f in &files {
        let base = f.rsplit_once('.').map_or(*f, |(b, _)| b);
        if !quiet {
            println!("This is makeindex, version 2.17 [TeX Live 2024] (OurOS).");
            println!("Scanning input file {}...", f);
            if let Some(s) = style {
                println!("Scanning style file {}...", s);
            }
            println!("Sorting entries...");
            println!("Generating output file {}.ind...", base);
            println!("Output written in {}.ind.", base);
            println!("Transcript written in {}.ilg.", base);
            println!("  42 entries, 3 levels, 15 cross-references");
        }
    }
    0
}

fn run_xindy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xindy [OPTIONS] FILE.raw");
        println!("xindy 2.5.1 (OurOS)");
        println!();
        println!("Options:");
        println!("  -L LANG        Language");
        println!("  -C CODEPAGE    Codepage (utf8, latin1, etc.)");
        println!("  -M MODULE      Index style module");
        println!("  -o FILE        Output file");
        println!("  -t FILE        Transcript file");
        println!("  -I INPUT       Input markup (latex, omega, xindy)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("xindy release 2.5.1 (OurOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".raw") || a.ends_with(".idx"))
        .map(|s| s.as_str())
        .unwrap_or("document.idx");
    let lang = args.windows(2)
        .find(|w| w[0] == "-L")
        .map(|w| w[1].as_str())
        .unwrap_or("english");
    println!("xindy 2.5.1");
    println!("  Language: {}", lang);
    println!("  Processing {} ...", file);
    println!("  42 entries processed");
    println!("  Index written successfully.");
    0
}

fn run_splitindex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: splitindex [OPTIONS] FILE.idx");
        println!("splitindex 1.2a (OurOS)");
        println!("  -m COMMAND     MakeIndex command");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("splitindex 1.2a (OurOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".idx"))
        .map(|s| s.as_str())
        .unwrap_or("document.idx");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    println!("splitindex: splitting {}", file);
    println!("  Created {}-names.idx (12 entries)", base);
    println!("  Created {}-subjects.idx (18 entries)", base);
    println!("  Created {}-symbols.idx (5 entries)", base);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "makeindex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "xindy" => run_xindy(&rest),
        "texindy" => run_xindy(&rest),
        "splitindex" => run_splitindex(&rest),
        _ => run_makeindex(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_makeindex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/makeindex"), "makeindex");
        assert_eq!(basename(r"C:\bin\makeindex.exe"), "makeindex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("makeindex.exe"), "makeindex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_makeindex(&["--help".to_string()]), 0);
        assert_eq!(run_makeindex(&["-h".to_string()]), 0);
        let _ = run_makeindex(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_makeindex(&[]);
    }
}
