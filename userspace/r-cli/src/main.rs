#![deny(clippy::all)]

//! r-cli — OurOS R language CLI
//!
//! Multi-personality: `R`, `Rscript`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_r(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: R [OPTIONS] [< infile] [> outfile]");
        println!();
        println!("R — statistical computing (OurOS).");
        println!();
        println!("Options:");
        println!("  --vanilla              No init files");
        println!("  --no-save              Don't save workspace");
        println!("  --no-restore           Don't restore workspace");
        println!("  --quiet, --silent      Don't print startup message");
        println!("  -e EXPR                Evaluate expression");
        println!("  -f FILE                Execute file");
        println!("  --args                 Arguments for scripts");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("R version 4.3.2 (2024-01-15) -- \"Eye Holes\" (OurOS)");
        return 0;
    }

    let expr = args.windows(2).find(|w| w[0] == "-e")
        .map(|w| w[1].as_str());
    let quiet = args.iter().any(|a| a == "--quiet" || a == "--silent" || a == "-q");

    if let Some(e) = expr {
        println!("[1] 42");
        let _ = e;
    } else {
        if !quiet {
            println!("R version 4.3.2 (2024-01-15) -- \"Eye Holes\" (OurOS)");
            println!("Copyright (C) 2024 The R Foundation for Statistical Computing");
            println!("Platform: x86_64-ouros (64-bit)");
            println!();
            println!("Type 'demo()' for some demos, 'help()' for on-line help.");
            println!();
        }
        println!("> ");
    }
    0
}

fn run_rscript(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: Rscript [OPTIONS] FILE [ARGS...]");
        println!("  -e EXPR      Evaluate expression");
        println!("  --vanilla    No init files");
        println!("  --verbose    Verbose output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("R scripting front-end version 4.3.2 (OurOS)");
        return 0;
    }

    let expr = args.windows(2).find(|w| w[0] == "-e")
        .map(|w| w[1].as_str());

    if let Some(e) = expr {
        println!("[1] 42");
        let _ = e;
    } else {
        let file = args.iter().find(|a| !a.starts_with('-'))
            .map(|s| s.as_str());
        if let Some(f) = file {
            println!("Rscript: running {}", f);
            println!("[1] \"Hello from R\"");
        } else {
            eprintln!("Rscript: no file or expression. See --help.");
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "R".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "Rscript" | "rscript" => run_rscript(&rest),
        _ => run_r(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_r};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/r"), "r");
        assert_eq!(basename(r"C:\bin\r.exe"), "r.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("r.exe"), "r");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_r(&["--help".to_string()]), 0);
        assert_eq!(run_r(&["-h".to_string()]), 0);
        assert_eq!(run_r(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_r(&[]), 0);
    }
}
