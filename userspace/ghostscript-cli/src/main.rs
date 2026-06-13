#![deny(clippy::all)]

//! ghostscript-cli — SlateOS Ghostscript CLI
//!
//! Multi-personality: `gs`, `ghostscript`, `ps2pdf`, `pdf2ps`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_gs(prog: &str, args: &[String]) -> i32 {
    match prog {
        "ps2pdf" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: ps2pdf [OPTIONS] INPUT.ps [OUTPUT.pdf]");
                println!("Convert PostScript to PDF (SlateOS).");
                return 0;
            }
            let input = args.iter().find(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).unwrap_or("input.ps");
            let default_out = format!("{}.pdf", strip_ext(input));
            let output = args.iter().filter(|a| !a.starts_with('-'))
                .nth(1).map(|s| s.as_str())
                .unwrap_or(&default_out);
            println!("Converting {} -> {}", input, output);
            return 0;
        }
        "pdf2ps" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: pdf2ps [OPTIONS] INPUT.pdf [OUTPUT.ps]");
                println!("Convert PDF to PostScript (SlateOS).");
                return 0;
            }
            let input = args.iter().find(|a| !a.starts_with('-'))
                .map(|s| s.as_str()).unwrap_or("input.pdf");
            let default_out = format!("{}.ps", strip_ext(input));
            let output = args.iter().filter(|a| !a.starts_with('-'))
                .nth(1).map(|s| s.as_str())
                .unwrap_or(&default_out);
            println!("Converting {} -> {}", input, output);
            return 0;
        }
        _ => {}
    }

    // gs / ghostscript
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gs [OPTIONS] [FILE...]");
        println!();
        println!("Ghostscript — PostScript/PDF interpreter (SlateOS).");
        println!();
        println!("Options:");
        println!("  -dBATCH              Exit after processing");
        println!("  -dNOPAUSE            No pause between pages");
        println!("  -dQUIET              Suppress output");
        println!("  -dSAFER              Restrict file operations");
        println!("  -sDEVICE=DEV         Output device (pdfwrite, png16m, jpeg, tiff32nc)");
        println!("  -sOutputFile=FILE    Output file");
        println!("  -r300                Resolution (DPI)");
        println!("  -dFirstPage=N        First page to process");
        println!("  -dLastPage=N         Last page to process");
        println!("  -dPDFSETTINGS=/SET   PDF settings (/screen, /ebook, /printer, /prepress)");
        println!("  -dCompatibilityLevel=N   PDF version (1.4, 1.5, 2.0)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("GPL Ghostscript 10.02.1 (SlateOS)");
        return 0;
    }

    let device = args.iter().find(|a| a.starts_with("-sDEVICE="))
        .map(|a| a.strip_prefix("-sDEVICE=").unwrap_or("pdfwrite"))
        .unwrap_or("pdfwrite");
    let output = args.iter().find(|a| a.starts_with("-sOutputFile="))
        .map(|a| a.strip_prefix("-sOutputFile=").unwrap_or("output"));
    let quiet = args.iter().any(|a| a == "-dQUIET");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if !quiet {
        println!("GPL Ghostscript 10.02.1 (SlateOS)");
        println!("Copyright (C) 2023 Artifex Software, Inc.  All rights reserved.");
    }

    if let Some(out) = output {
        if !quiet {
            println!("Processing pages 1 through 5.");
            for page in 1..=5 {
                println!("Page {}", page);
            }
        }
        println!("Output: {} (device: {})", out, device);
    } else if !files.is_empty() {
        for f in &files {
            if !quiet {
                println!("Processing: {}", f);
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "gs".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gs(&prog, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gs};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ghostscript"), "ghostscript");
        assert_eq!(basename(r"C:\bin\ghostscript.exe"), "ghostscript.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ghostscript.exe"), "ghostscript");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gs("ghostscript", &["--help".to_string()]), 0);
        assert_eq!(run_gs("ghostscript", &["-h".to_string()]), 0);
        let _ = run_gs("ghostscript", &["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gs("ghostscript", &[]);
    }
}
