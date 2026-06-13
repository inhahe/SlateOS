#![deny(clippy::all)]

//! poppler — Slate OS PDF rendering library utilities
//!
//! Multi-personality: `pdftotext`, `pdfinfo`, `pdftohtml`, `pdftoppm`, `pdfimages`, `pdfseparate`, `pdfunite`

use std::env;
use std::process;

fn run_pdfinfo(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdfinfo [options] <PDF-file>");
        println!("  -enc <encoding>   Text encoding");
        println!("  -f <int>          First page");
        println!("  -l <int>          Last page");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("pdfinfo version 24.05.0 (Slate OS)");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("document.pdf");
    let _ = file;
    println!("Title:          Example Document");
    println!("Subject:        ");
    println!("Keywords:       ");
    println!("Author:         Slate OS User");
    println!("Creator:        Slate OS Writer");
    println!("Producer:       LibreOffice 7.6");
    println!("CreationDate:   Wed May 22 10:00:00 2025 UTC");
    println!("ModDate:        Wed May 22 10:00:00 2025 UTC");
    println!("Tagged:         yes");
    println!("UserProperties: no");
    println!("Suspects:       no");
    println!("Form:           none");
    println!("JavaScript:     no");
    println!("Pages:          42");
    println!("Encrypted:      no");
    println!("Page size:      595.276 x 841.89 pts (A4)");
    println!("Page rot:       0");
    println!("File size:      1234567 bytes");
    println!("Optimized:      yes");
    println!("PDF version:    1.7");
    0
}

fn run_pdftotext(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdftotext [options] <PDF-file> [<text-file>]");
        println!("  -f <int>          First page");
        println!("  -l <int>          Last page");
        println!("  -layout           Maintain original layout");
        println!("  -raw              Raw ordering");
        println!("  -enc <encoding>   Text encoding");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("document.pdf");
    let _ = file;
    println!("This is the extracted text content from the PDF document.");
    println!("It contains multiple paragraphs and sections.");
    println!();
    println!("Section 1: Introduction");
    println!("Lorem ipsum dolor sit amet, consectetur adipiscing elit.");
    0
}

fn run_pdftoppm(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdftoppm [options] <PDF-file> <PPM-root>");
        println!("  -f <int>      First page");
        println!("  -l <int>      Last page");
        println!("  -r <int>      Resolution (default: 150)");
        println!("  -png          Output PNG format");
        println!("  -jpeg         Output JPEG format");
        println!("  -tiff         Output TIFF format");
        return 0;
    }
    let root = args.iter().filter(|a| !a.starts_with('-')).nth(1).map(|s| s.as_str()).unwrap_or("page");
    println!("Converting pages to images...");
    println!("  {}-1.png", root);
    println!("  {}-2.png", root);
    println!("  {}-3.png", root);
    0
}

fn run_pdfimages(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdfimages [options] <PDF-file> <image-root>");
        println!("  -f <int>   First page");
        println!("  -l <int>   Last page");
        println!("  -j         Write JPEG images");
        println!("  -list      List images");
        return 0;
    }
    if args.iter().any(|a| a == "-list") {
        println!("page   num  type   width  height  color  comp  bpc  enc   interp  object ID  x-ppi  y-ppi  size   ratio");
        println!("-----------------------------------------------------------------------------------------------------------");
        println!("   1     0  image    800    600    rgb     3    8   jpeg   yes     10 0       150    150    128K   26.8%");
        println!("   3     1  image   1024    768    rgb     3    8   png    no      25 0       300    300    512K   21.7%");
        return 0;
    }
    println!("Extracting images from PDF... 2 images extracted.");
    0
}

fn run_pdfseparate(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdfseparate [options] <PDF-file> <PDF-file-pattern>");
        println!("  -f <int>   First page");
        println!("  -l <int>   Last page");
        return 0;
    }
    println!("Separating pages...");
    println!("  page-1.pdf");
    println!("  page-2.pdf");
    println!("  page-3.pdf");
    0
}

fn run_pdfunite(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdfunite <PDF-file-1> ... <PDF-file-n> <output-PDF>");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if files.len() >= 2 {
        let output = files.last().unwrap_or(&"output.pdf");
        println!("Merging {} files into {}", files.len() - 1, output);
    } else {
        println!("pdfunite: at least 2 input files and 1 output file required");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("pdfinfo");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "pdftotext" => run_pdftotext(rest),
        "pdftoppm" => run_pdftoppm(rest),
        "pdftohtml" => { println!("(HTML conversion — simulated)"); 0 }
        "pdfimages" => run_pdfimages(rest),
        "pdfseparate" => run_pdfseparate(rest),
        "pdfunite" => run_pdfunite(rest),
        _ => run_pdfinfo(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pdfinfo};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pdfinfo(vec!["--help".to_string()]), 0);
        assert_eq!(run_pdfinfo(vec!["-h".to_string()]), 0);
        let _ = run_pdfinfo(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pdfinfo(vec![]);
    }
}
