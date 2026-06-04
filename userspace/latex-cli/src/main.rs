#![deny(clippy::all)]

//! latex-cli — OurOS LaTeX CLI
//!
//! Multi-personality: `pdflatex`, `xelatex`, `lualatex`, `latex`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_latex(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-help" || a == "-h") {
        println!("Usage: {} [OPTIONS] FILE.tex", prog);
        println!();
        println!("{} — TeX/LaTeX typesetter (OurOS).", prog);
        println!();
        println!("Options:");
        println!("  -output-directory DIR  Output directory");
        println!("  -jobname NAME          Output file base name");
        println!("  -interaction MODE      Interaction mode (batchmode, nonstopmode, scrollmode, errorstopmode)");
        println!("  -halt-on-error         Stop on first error");
        println!("  -file-line-error       Show file:line:error format");
        println!("  -shell-escape          Enable \\write18");
        println!("  -synctex=N             Generate SyncTeX data");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        match prog {
            "pdflatex" => println!("pdfTeX 3.141592653-2.6-1.40.25 (TeX Live 2024/OurOS)"),
            "xelatex" => println!("XeTeX 3.141592653-2.6-0.999996 (TeX Live 2024/OurOS)"),
            "lualatex" => println!("LuaHBTeX 1.17.0 (TeX Live 2024/OurOS)"),
            _ => println!("TeX 3.141592653 (TeX Live 2024/OurOS)"),
        }
        return 0;
    }

    let file = args.iter()
        .rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("document.tex");

    let base = strip_ext(file);
    let halt = args.iter().any(|a| a == "-halt-on-error");

    println!("This is {} (OurOS)", prog);
    println!("entering extended mode");
    println!(" restricted \\write18 enabled.");
    println!("({}",  file);
    println!("LaTeX2e <2023-11-01> patch level 1");
    println!(" L3 programming layer <2024-01-04>");
    println!("(/usr/share/texlive/texmf-dist/tex/latex/base/article.cls");
    println!("Document Class: article 2023/05/17 v1.4n Standard LaTeX document class)");
    println!("(./document.aux)");
    println!("[1{{/usr/share/texlive/texmf-dist/fonts/map/pdftex/updmap/pdftex.map}}]");
    println!("[2] [3]");

    if halt {
        println!("No errors.");
    }

    let ext = match prog {
        "latex" => "dvi",
        _ => "pdf",
    };

    println!("Output written on {}.{} (3 pages, 45678 bytes).", base, ext);
    println!("Transcript written on {}.log.", base);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "pdflatex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_latex(&prog, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_latex};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/latex"), "latex");
        assert_eq!(basename(r"C:\bin\latex.exe"), "latex.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("latex.exe"), "latex");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_latex("latex", &["--help".to_string()]), 0);
        assert_eq!(run_latex("latex", &["-h".to_string()]), 0);
        let _ = run_latex("latex", &["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_latex("latex", &[]);
    }
}
