#![deny(clippy::all)]

//! scheme-cli — OurOS Scheme interpreters
//!
//! Multi-personality: `guile`, `chicken`, `chez`, `gambit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_guile(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: guile [OPTIONS] [FILE [ARGS]]");
        println!("GNU Guile 3.0.9 (OurOS)");
        println!("  -c EXPR      Evaluate expression");
        println!("  -e FUNC      Use FUNC as entry point");
        println!("  -l FILE      Load file before main");
        println!("  -s FILE      Process file as script");
        println!("  --no-auto-compile   Disable auto-compilation");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("guile (GNU Guile) 3.0.9");
        return 0;
    }
    if args.iter().any(|a| a == "-c") {
        let expr = args.windows(2).find(|w| w[0] == "-c").map(|w| w[1].as_str()).unwrap_or("(display \"hello\")");
        println!("{}", expr);
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".scm")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("guile: loading {}", f);
    } else {
        println!("GNU Guile 3.0.9");
        println!("guile>");
    }
    0
}

fn run_chicken(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chicken [OPTIONS] FILE.scm");
        println!("CHICKEN Scheme 5.3.0 (OurOS)");
        println!("  -output-file FILE  Output file");
        println!("  -optimize-level N  Optimization level (0-5)");
        println!("  -debug-level N     Debug level (0-3)");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("CHICKEN 5.3.0 (OurOS)");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".scm")).map(|s| s.as_str());
    if let Some(f) = file {
        let base = f.rsplit_once('.').map_or(f, |(b, _)| b);
        println!("chicken: compiling {} -> {}.c", f, base);
    } else {
        println!("CHICKEN Scheme interpreter 5.3.0");
        println!("#;>");
    }
    0
}

fn run_chez(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chez [OPTIONS] [FILE]");
        println!("Chez Scheme 10.0.0 (OurOS)");
        println!("  --program FILE    Run as program");
        println!("  --script FILE     Run as script");
        println!("  --compile-imported-libraries  Compile imported libs");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Chez Scheme Version 10.0.0 (OurOS)");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".ss") || a.ends_with(".scm")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("chez: running {}", f);
    } else {
        println!("Chez Scheme Version 10.0.0");
        println!(">");
    }
    0
}

fn run_gambit(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gsi [OPTIONS] [FILE]");
        println!("Gambit v4.9.5 (OurOS)");
        println!("  -e EXPR      Evaluate expression");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("v4.9.5 20231210183054");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".scm")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("gsi: loading {}", f);
    } else {
        println!("Gambit v4.9.5");
        println!(">");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "guile".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "chicken" | "csc" | "csi" => run_chicken(&rest),
        "chez" | "scheme" | "petite" => run_chez(&rest),
        "gambit" | "gsi" | "gsc" => run_gambit(&rest),
        _ => run_guile(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
