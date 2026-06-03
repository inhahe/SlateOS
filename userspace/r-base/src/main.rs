#![deny(clippy::all)]

//! r-base — OurOS R statistical computing
//!
//! Multi-personality: `R`, `Rscript`

use std::env;
use std::process;

fn run_r(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: R [options] [< infile] [> outfile]");
        println!("   or: R CMD command [arguments]");
        println!();
        println!("Options:");
        println!("  --save           Save workspace on exit");
        println!("  --no-save        Don't save workspace");
        println!("  --restore        Restore saved workspace");
        println!("  --no-restore     Don't restore workspace");
        println!("  --vanilla        Combine --no-save --no-restore --no-site-file --no-init-file --no-environ");
        println!("  -e expr          Execute expression");
        println!("  -f file          Execute file");
        println!("  --quiet          Don't print startup message");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("R version 4.4.1 (2025-05-22) -- \"OurOS\"");
        println!("Platform: x86_64-ouros (64-bit)");
        return 0;
    }

    let exec_expr = args.iter().position(|a| a == "-e")
        .and_then(|i| args.get(i + 1));
    if let Some(expr) = exec_expr {
        println!("> {}", expr);
        println!("[1] (result simulated)");
        return 0;
    }

    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q");
    if !quiet {
        println!("R version 4.4.1 (2025-05-22) -- \"OurOS\"");
        println!("Copyright (C) 2025 The R Foundation for Statistical Computing");
        println!("Platform: x86_64-ouros (64-bit)");
        println!();
        println!("Type 'demo()' for some demos, 'help()' for on-line help.");
        println!("Type 'q()' to quit R.");
    }
    println!("> (interactive mode — simulated)");
    0
}

fn run_rscript(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: Rscript [options] file [args]");
        println!("   or: Rscript [options] -e expr [-e expr2 ...] [args]");
        println!();
        println!("Options:");
        println!("  --default-packages=list  Default packages to load");
        println!("  --vanilla                Combine --no-save etc.");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("R scripting front-end version 4.4.1 (2025-05-22) (OurOS)");
        return 0;
    }

    let exec_expr = args.iter().position(|a| a == "-e")
        .and_then(|i| args.get(i + 1));
    if let Some(expr) = exec_expr {
        println!("[1] ({})", expr);
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(f) = file {
        println!("(running script: {})", f);
    } else {
        eprintln!("Rscript: no script specified");
        return 1;
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("R");
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
        "Rscript" => run_rscript(rest),
        _ => run_r(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_r};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_r(vec!["--help".to_string()]), 0);
        assert_eq!(run_r(vec!["-h".to_string()]), 0);
        assert_eq!(run_r(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_r(vec![]), 0);
    }
}
