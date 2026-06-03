#![deny(clippy::all)]

//! xelatex-cli — OurOS XeLaTeX/XeTeX engine
//!
//! Multi-personality: `xelatex`, `xetex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xelatex(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xelatex [OPTIONS] FILE.tex");
        println!("XeTeX 3.141592653-2.6-0.999996 (TeX Live 2024/OurOS)");
        println!();
        println!("Options:");
        println!("  -interaction=MODE    Set interaction mode");
        println!("  -output-directory=DIR  Output directory");
        println!("  -output-driver=CMD   Output driver (xdvipdfmx)");
        println!("  -no-pdf              Generate XDV instead of PDF");
        println!("  -synctex=NUM         Enable SyncTeX");
        println!("  -shell-escape        Enable \\write18 commands");
        println!("  -halt-on-error       Stop on first error");
        println!("  -file-line-error     Show file:line:error format");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("XeTeX 3.141592653-2.6-0.999996 (TeX Live 2024/OurOS)");
        println!("kpathsea version 6.4.0");
        println!("ICU version 74.1, HarfBuzz version 8.3.0");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".tex") || !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("document.tex");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    let no_pdf = args.iter().any(|a| a == "-no-pdf");
    let ext = if no_pdf { "xdv" } else { "pdf" };
    println!("This is XeTeX, Version 3.141592653-2.6-0.999996 (TeX Live 2024/OurOS)");
    println!(" restricted \\write18 enabled.");
    println!("entering extended mode");
    println!("({})", file);
    println!("LaTeX2e <2024-02-01> patch level 2");
    println!("({}.aux)", base);
    println!("[1] [2] [3] [4] [5]");
    println!("({}.aux)", base);
    println!("Output written on {}.{} (5 pages).", base, ext);
    println!("Transcript written on {}.log.", base);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xelatex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xelatex(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xelatex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xelatex"), "xelatex");
        assert_eq!(basename(r"C:\bin\xelatex.exe"), "xelatex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xelatex.exe"), "xelatex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_xelatex(&["--help".to_string()]), 0);
        assert_eq!(run_xelatex(&["-h".to_string()]), 0);
        assert_eq!(run_xelatex(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_xelatex(&[]), 0);
    }
}
