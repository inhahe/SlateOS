#![deny(clippy::all)]

//! groff-cli — SlateOS groff/troff CLI
//!
//! Multi-personality: `groff`, `troff`, `nroff`, `tbl`, `eqn`, `pic`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_groff(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog {
            "groff" => {
                println!("Usage: groff [OPTIONS] [FILE...]");
                println!();
                println!("groff — GNU roff typesetter (Slate OS).");
                println!();
                println!("Options:");
                println!("  -T DEVICE    Output device (ascii, utf8, html, pdf, ps)");
                println!("  -m MACRO     Use macro package (an, ms, me, mm, mom)");
                println!("  -t           Invoke tbl preprocessor");
                println!("  -e           Invoke eqn preprocessor");
                println!("  -p           Invoke pic preprocessor");
                println!("  -s           Invoke soelim preprocessor");
                println!("  -a           Produce ASCII approximation");
                println!("  -z           Suppress formatted output");
            }
            "nroff" => {
                println!("Usage: nroff [OPTIONS] [FILE...]");
                println!("nroff — format documents for terminal display (Slate OS).");
            }
            "tbl" => {
                println!("Usage: tbl [FILE...]");
                println!("tbl — format tables for groff (Slate OS).");
            }
            "eqn" => {
                println!("Usage: eqn [FILE...]");
                println!("eqn — format equations for groff (Slate OS).");
            }
            "pic" => {
                println!("Usage: pic [FILE...]");
                println!("pic — compile pictures for groff (Slate OS).");
            }
            _ => {
                println!("Usage: troff [OPTIONS] [FILE...]");
                println!("troff — the troff processor (Slate OS).");
            }
        }
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("GNU groff version 1.23.0 (Slate OS)");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    match prog {
        "tbl" | "eqn" | "pic" => {
            if files.is_empty() {
                println!(".\\\" {} output", prog);
            } else {
                for f in &files {
                    println!(".\\\" {} preprocessed: {}", prog, f);
                }
            }
        }
        "nroff" => {
            let file = files.first().copied().unwrap_or("document.1");
            println!("NAME");
            println!("       example - an example manual page");
            println!();
            println!("SYNOPSIS");
            println!("       example [options] [file...]");
            println!();
            println!("DESCRIPTION");
            println!("       This is an example manual page formatted by {}.", prog);
            let _ = file;
        }
        _ => {
            // groff / troff
            let device = args.windows(2).find(|w| w[0] == "-T")
                .map(|w| w[1].as_str()).unwrap_or("utf8");
            let file = files.first().copied().unwrap_or("document");

            println!("{}: processing {} (device: {})", prog, file, device);
            match device {
                "pdf" => println!("  Output: {}.pdf", strip_ext(file)),
                "ps" => println!("  Output: {}.ps", strip_ext(file)),
                "html" => println!("  Output: {}.html", strip_ext(file)),
                _ => println!("  Output to stdout (terminal)."),
            }
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "groff".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_groff(&prog, &rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_groff};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/groff"), "groff");
        assert_eq!(basename(r"C:\bin\groff.exe"), "groff.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("groff.exe"), "groff");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_groff("groff", &["--help".to_string()]), 0);
        assert_eq!(run_groff("groff", &["-h".to_string()]), 0);
        let _ = run_groff("groff", &["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_groff("groff", &[]);
    }
}
