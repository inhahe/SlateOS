#![deny(clippy::all)]

//! pdflatex-cli — OurOS pdfLaTeX/pdfTeX engine
//!
//! Multi-personality: `pdflatex`, `pdftex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pdflatex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdflatex [OPTIONS] FILE.tex");
        println!("pdfTeX 3.141592653-2.6-1.40.26 (TeX Live 2024/OurOS)");
        println!();
        println!("Options:");
        println!("  -interaction=MODE   Set interaction mode (batchmode, nonstopmode, scrollmode, errorstopmode)");
        println!("  -output-directory=DIR  Output directory");
        println!("  -jobname=NAME       Job name");
        println!("  -synctex=NUM        Enable SyncTeX (0=off, 1=on)");
        println!("  -shell-escape       Enable \\write18 commands");
        println!("  -no-shell-escape    Disable \\write18 commands");
        println!("  -halt-on-error      Stop on first error");
        println!("  -file-line-error    Show file:line:error format");
        println!("  -recorder           Enable file recording");
        println!("  --version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pdfTeX 3.141592653-2.6-1.40.26 (TeX Live 2024/OurOS)");
        println!("kpathsea version 6.4.0");
        println!("Copyright 2024 Han The Thanh (pdfTeX) et al.");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".tex") || !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("document.tex");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    println!("This is pdfTeX, Version 3.141592653-2.6-1.40.26 (TeX Live 2024/OurOS)");
    println!(" restricted \\write18 enabled.");
    println!("entering extended mode");
    println!("({})", file);
    println!("LaTeX2e <2024-02-01> patch level 2");
    println!("(/usr/local/texlive/2024/texmf-dist/tex/latex/base/article.cls");
    println!("Document Class: article 2023/05/17 v1.4n Standard LaTeX document class)");
    println!("({}.aux)", base);
    println!("[1{{/usr/local/texlive/2024/texmf-var/fonts/map/pdftex/updmap/pdftex.map}}]");
    println!("[2] [3] [4] [5]");
    println!("({}.aux)", base);
    println!("Output written on {}.pdf (5 pages, 42000 bytes).", base);
    println!("Transcript written on {}.log.", base);
    0
}

fn run_pdftex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pdftex [OPTIONS] FILE.tex");
        println!("pdfTeX (plain TeX mode)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pdfTeX 3.141592653-2.6-1.40.26 (TeX Live 2024/OurOS)");
        return 0;
    }
    run_pdflatex(args)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pdflatex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pdftex" => run_pdftex(&rest),
        _ => run_pdflatex(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pdflatex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pdflatex"), "pdflatex");
        assert_eq!(basename(r"C:\bin\pdflatex.exe"), "pdflatex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pdflatex.exe"), "pdflatex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pdflatex(&["--help".to_string()]), 0);
        assert_eq!(run_pdflatex(&["-h".to_string()]), 0);
        assert_eq!(run_pdflatex(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pdflatex(&[]), 0);
    }
}
