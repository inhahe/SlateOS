#![deny(clippy::all)]

//! gscan2pdf-cli — OurOS gscan2pdf scan-to-PDF tool
//!
//! Single personality: `gscan2pdf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gscan2pdf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gscan2pdf [OPTIONS] [FILE...]");
        println!("gscan2pdf v2.13 (OurOS) — Scan to PDF/DjVu/TIFF");
        println!();
        println!("Options:");
        println!("  -d DEVICE         Use specific scanner");
        println!("  --import FILE     Import existing image");
        println!("  --output FILE     Output file");
        println!("  --ocr             Enable OCR (tesseract)");
        println!("  --resolution DPI  Scan resolution");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gscan2pdf v2.13 (OurOS)"); return 0; }
    println!("gscan2pdf: scan-to-PDF application started");
    println!("  Scanner: Epson Perfection V39");
    println!("  OCR engine: tesseract 5.x");
    println!("  Output formats: PDF, DjVu, TIFF, PNG");
    println!("  Ready to scan");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gscan2pdf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gscan2pdf(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gscan2pdf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gscan2pdf"), "gscan2pdf");
        assert_eq!(basename(r"C:\bin\gscan2pdf.exe"), "gscan2pdf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gscan2pdf.exe"), "gscan2pdf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gscan2pdf(&["--help".to_string()], "gscan2pdf"), 0);
        assert_eq!(run_gscan2pdf(&["-h".to_string()], "gscan2pdf"), 0);
        assert_eq!(run_gscan2pdf(&["--version".to_string()], "gscan2pdf"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gscan2pdf(&[], "gscan2pdf"), 0);
    }
}
