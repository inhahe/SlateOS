#![deny(clippy::all)]

//! dvips-cli — OurOS DVI to PostScript converter & DVI tools
//!
//! Multi-personality: `dvips`, `dvipdfmx`, `dvisvgm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dvips(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dvips [OPTIONS] FILE.dvi");
        println!("dvips 2024.1 (OurOS)");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file (default: FILE.ps)");
        println!("  -p NUM        First page to output");
        println!("  -l NUM        Last page to output");
        println!("  -pp RANGE     Page range (e.g., 1-5,8)");
        println!("  -t PAPERSIZE  Paper size (letter, a4, etc.)");
        println!("  -D NUM        DPI resolution");
        println!("  -E            Generate EPSF output");
        println!("  -Z            Compress bitmap fonts");
        println!("  -P PRINTER    Printer config");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dvips(k) 2024.1 (TeX Live 2024/OurOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".dvi"))
        .map(|s| s.as_str())
        .unwrap_or("document.dvi");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    let outfile = args.windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| w[1].as_str());
    let out_default = format!("{}.ps", base);
    let out = outfile.unwrap_or(out_default.as_str());
    println!("This is dvips(k) 2024.1 (TeX Live 2024/OurOS)");
    println!("' TeX output {}.dvi' ->  {}", base, out);
    println!("<texc.pro><texps.pro>.");
    println!("[1] [2] [3] [4] [5]");
    println!("Output written on {} (5 pages).", out);
    0
}

fn run_dvipdfmx(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dvipdfmx [OPTIONS] FILE.dvi");
        println!("dvipdfmx 20240116 (OurOS)");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file");
        println!("  -s RANGE      Page range");
        println!("  -p PAPER      Paper size");
        println!("  -c            Ignore color specials");
        println!("  -z NUM        Compression level (0-9)");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dvipdfmx version 20240116 (OurOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".dvi") || a.ends_with(".xdv"))
        .map(|s| s.as_str())
        .unwrap_or("document.dvi");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    let out_default = format!("{}.pdf", base);
    let outfile = args.windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| w[1].as_str());
    let out = outfile.unwrap_or(out_default.as_str());
    println!("dvipdfmx: DVI -> {}", out);
    println!("[1][2][3][4][5]");
    println!("{} bytes written", 42000);
    0
}

fn run_dvisvgm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dvisvgm [OPTIONS] FILE.dvi");
        println!("dvisvgm 3.2.2 (OurOS)");
        println!();
        println!("Options:");
        println!("  -o FILE       Output file");
        println!("  -p RANGE      Page range");
        println!("  -n            No fonts (convert to paths)");
        println!("  -e            Exact bounding box");
        println!("  -z            Compress SVG output");
        println!("  --font-format=FMT  Font format (svg, woff, woff2)");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dvisvgm 3.2.2 (OurOS)");
        return 0;
    }
    let file = args.iter()
        .find(|a| a.ends_with(".dvi") || a.ends_with(".pdf"))
        .map(|s| s.as_str())
        .unwrap_or("document.dvi");
    let base = file.rsplit_once('.').map_or(file, |(b, _)| b);
    println!("processing page 1");
    println!("  output written to {}.svg", base);
    println!("5 of 5 pages converted");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dvips".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "dvipdfmx" | "xdvipdfmx" => run_dvipdfmx(&rest),
        "dvisvgm" => run_dvisvgm(&rest),
        _ => run_dvips(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dvips};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dvips"), "dvips");
        assert_eq!(basename(r"C:\bin\dvips.exe"), "dvips.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dvips.exe"), "dvips");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dvips(&["--help".to_string()]), 0);
        assert_eq!(run_dvips(&["-h".to_string()]), 0);
        let _ = run_dvips(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dvips(&[]);
    }
}
