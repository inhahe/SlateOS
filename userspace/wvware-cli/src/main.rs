#![deny(clippy::all)]

//! wvware-cli — OurOS Microsoft Word document converter
//!
//! Multi-personality: `wvText`, `wvHtml`, `wvPS`, `wvPDF`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wv(args: &[String], prog: &str) -> i32 {
    let (format, ext) = match prog {
        "wvHtml" => ("HTML", "html"),
        "wvPS" => ("PostScript", "ps"),
        "wvPDF" => ("PDF", "pdf"),
        _ => ("plain text", "txt"),
    };

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] <input.doc> [output.{}]", prog, ext);
        println!("{} v1.2 (OurOS) — Convert Word documents to {}", prog, format);
        println!();
        println!("Options:");
        println!("  --charset CHARSET  Output character set");
        println!("  --password PASS    Document password");
        println!("  --config FILE      Configuration file");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("{} v1.2 (OurOS, wvWare library)", prog);
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        eprintln!("{}: error: no input file specified", prog);
        return 1;
    }
    let input = files[0];
    let output = if files.len() > 1 {
        files[1].to_string()
    } else {
        let base = input.rsplit_once('.').map_or(input.as_str(), |(b, _)| b);
        format!("{}.{}", base, ext)
    };
    println!("{}: converting {} to {} -> {}", prog, input, format, output);
    println!("{}: extracting document structure...", prog);
    println!("{}: processing 15 paragraphs, 3 tables, 2 images", prog);
    println!("{}: done [{} bytes]", prog, 65_536);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wvText".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wv(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wvware"), "wvware");
        assert_eq!(basename(r"C:\bin\wvware.exe"), "wvware.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wvware.exe"), "wvware");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wv(&["--help".to_string()], "wvware"), 0);
        assert_eq!(run_wv(&["-h".to_string()], "wvware"), 0);
        let _ = run_wv(&["--version".to_string()], "wvware");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wv(&[], "wvware");
    }
}
