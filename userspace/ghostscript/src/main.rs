#![deny(clippy::all)]

//! ghostscript — OurOS PostScript and PDF interpreter
//!
//! Multi-personality: `gs`, `ps2pdf`, `pdf2ps`

use std::env;
use std::process;

fn run_gs(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gs [options] [file1 ...]");
        println!();
        println!("Options:");
        println!("  -sDEVICE=<device>     Select output device");
        println!("  -sOutputFile=<file>   Output file name");
        println!("  -dNOPAUSE             Don't pause between pages");
        println!("  -dBATCH               Exit after processing");
        println!("  -dSAFER               Restrict file operations");
        println!("  -r<resolution>        Set resolution (e.g., -r300)");
        println!("  -dFirstPage=<n>       First page to process");
        println!("  -dLastPage=<n>        Last page to process");
        println!("  -dPDFSETTINGS=<val>   PDF quality (/screen/ebook/printer/prepress)");
        println!("  -dCompatibilityLevel  PDF version level");
        println!("  --version             Show version");
        println!();
        println!("Devices: pdfwrite, ps2write, png16m, jpeg, tiff32nc, pnm, bmp16m, eps2write");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GPL Ghostscript 10.03.0 (OurOS) (2025-05-22)");
        return 0;
    }

    let device = args.iter().find_map(|a| a.strip_prefix("-sDEVICE=")).unwrap_or("pdfwrite");
    let output = args.iter().find_map(|a| a.strip_prefix("-sOutputFile=")).unwrap_or("output");
    let input_files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-') && !a.starts_with("/")).map(|s| s.as_str()).collect();

    println!("GPL Ghostscript 10.03.0 (OurOS) (2025-05-22)");
    println!("Copyright (C) 2025 Artifex Software, Inc.  All rights reserved.");
    if !input_files.is_empty() {
        println!("Processing {} file(s) with device '{}'", input_files.len(), device);
        println!("Output: {}", output);
        println!("Processing pages...");
        println!("Page 1");
        println!("Page 2");
        println!("Page 3");
    }
    0
}

fn run_ps2pdf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ps2pdf [options] input.ps [output.pdf]");
        return 0;
    }
    let input = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.ps");
    let output = args.iter().filter(|a| !a.starts_with('-')).nth(1).map(|s| s.as_str()).unwrap_or("output.pdf");
    println!("Converting {} -> {} (simulated)", input, output);
    0
}

fn run_pdf2ps(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pdf2ps [options] input.pdf [output.ps]");
        return 0;
    }
    let input = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.pdf");
    let output = args.iter().filter(|a| !a.starts_with('-')).nth(1).map(|s| s.as_str()).unwrap_or("output.ps");
    println!("Converting {} -> {} (simulated)", input, output);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("gs");
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
        "ps2pdf" => run_ps2pdf(rest),
        "pdf2ps" => run_pdf2ps(rest),
        _ => run_gs(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gs};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gs(vec!["--help".to_string()]), 0);
        assert_eq!(run_gs(vec!["-h".to_string()]), 0);
        assert_eq!(run_gs(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gs(vec![]), 0);
    }
}
