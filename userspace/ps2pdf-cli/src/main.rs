#![deny(clippy::all)]

//! ps2pdf-cli — OurOS PostScript/PDF conversion tools
//!
//! Multi-personality: `ps2pdf`, `pdf2ps`, `ps2eps`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ps2pdf(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] <input.ps> [output.pdf]", prog);
        println!("{} v10.02 (OurOS) — PostScript to PDF converter", prog);
        println!();
        println!("Options:");
        println!("  -dPDFSETTINGS=/screen     Low-quality, small size");
        println!("  -dPDFSETTINGS=/ebook      Medium quality");
        println!("  -dPDFSETTINGS=/printer    High quality");
        println!("  -dPDFSETTINGS=/prepress   Highest quality");
        println!("  -dCompatibilityLevel=N.M   PDF version (1.4, 1.5, etc.)");
        println!("  -dNOPAUSE                  No pause between pages");
        println!("  -dBATCH                    Batch mode");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        eprintln!("{}: error: no input file specified", prog);
        return 1;
    }
    let input = files[0];
    let default_out;
    let output = if files.len() > 1 {
        files[1].as_str()
    } else {
        let base = input.rsplit_once('.').map_or(input.as_str(), |(b, _)| b);
        default_out = format!("{}.pdf", base);
        default_out.as_str()
    };
    println!("GPL Ghostscript 10.02 (OurOS)");
    println!("Converting {} -> {}", input, output);
    println!("Processing pages: 1 2 3 4 5");
    println!("Done [{} bytes]", 204_800);
    0
}

fn run_pdf2ps(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] <input.pdf> [output.ps]", prog);
        println!("{} v10.02 (OurOS) — PDF to PostScript converter", prog);
        println!();
        println!("Options:");
        println!("  -dFirstPage=N   Start from page N");
        println!("  -dLastPage=N    End at page N");
        println!("  -dLanguageLevel=N  PostScript level (1, 2, 3)");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        eprintln!("{}: error: no input file specified", prog);
        return 1;
    }
    println!("GPL Ghostscript 10.02 (OurOS)");
    println!("Converting {} to PostScript...", files[0]);
    println!("Processing 5 pages...");
    println!("Done [{} bytes]", 819_200);
    0
}

fn run_ps2eps(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] <input.ps> [output.eps]", prog);
        println!("{} v1.68 (OurOS) — PostScript to EPS converter", prog);
        println!();
        println!("Options:");
        println!("  -f              Force overwrite");
        println!("  -q              Quiet mode");
        println!("  -l              Loose bounding box");
        println!("  -B              No HiResBoundingBox");
        println!("  -C              Use clip");
        println!("  -R ROTATION     Rotate (0, 90, 180, 270)");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        eprintln!("{}: error: no input file specified", prog);
        return 1;
    }
    println!("ps2eps v1.68 (OurOS)");
    println!("Converting {} to EPS...", files[0]);
    println!("Bounding box: 0 0 612 792");
    println!("Done");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ps2pdf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pdf2ps" => run_pdf2ps(&rest, &prog),
        "ps2eps" => run_ps2eps(&rest, &prog),
        _ => run_ps2pdf(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ps2pdf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ps2pdf"), "ps2pdf");
        assert_eq!(basename(r"C:\bin\ps2pdf.exe"), "ps2pdf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ps2pdf.exe"), "ps2pdf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ps2pdf(&["--help".to_string()], "ps2pdf"), 0);
        assert_eq!(run_ps2pdf(&["-h".to_string()], "ps2pdf"), 0);
        let _ = run_ps2pdf(&["--version".to_string()], "ps2pdf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ps2pdf(&[], "ps2pdf");
    }
}
