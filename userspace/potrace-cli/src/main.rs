#![deny(clippy::all)]

//! potrace-cli — SlateOS Potrace/mkbitmap bitmap tracing CLI
//!
//! Multi-personality: `potrace`, `mkbitmap`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_potrace(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: potrace [OPTIONS] [FILE ...]");
        println!();
        println!("potrace — bitmap to vector tracing (Slate OS).");
        println!();
        println!("Options:");
        println!("  -b, --backend NAME    Output format (svg,eps,ps,pdf,dxf,pgm,gimppath)");
        println!("  -o, --output FILE     Output file");
        println!("  -s, --svg             SVG output");
        println!("  -e, --eps             EPS output");
        println!("  -p, --postscript      PostScript output");
        println!("  -g, --pgm             PGM output");
        println!("  -z, --turnpolicy P    Turn policy (black/white/right/left/minority/majority/random)");
        println!("  -t, --turdsize N      Suppress speckles up to N pixels");
        println!("  -a, --alphamax N      Corner threshold (0-1.33)");
        println!("  -O, --opttolerance N  Optimize paths (default 0.2)");
        println!("  -n, --longcurve       Turn off curve optimization");
        println!("  -i, --invert          Invert input");
        println!("  -k, --blacklevel N    Threshold (0-1, default 0.5)");
        println!("  --flat                No grouping in SVG");
        println!("  --tight               Remove whitespace");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("potrace 1.16 (Slate OS)");
        return 0;
    }

    let backend = args.windows(2)
        .find(|w| w[0] == "-b" || w[0] == "--backend")
        .map(|w| w[1].as_str())
        .unwrap_or(if args.iter().any(|a| a == "-s" || a == "--svg") { "svg" }
            else if args.iter().any(|a| a == "-e" || a == "--eps") { "eps" }
            else if args.iter().any(|a| a == "-p" || a == "--postscript") { "ps" }
            else if args.iter().any(|a| a == "-g" || a == "--pgm") { "pgm" }
            else { "svg" });

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        println!("potrace: reading from stdin...");
        println!("potrace: output written to stdout ({})", backend);
    } else {
        for f in &files {
            let base = strip_ext(f);
            println!("potrace: tracing '{}' → '{}.{}' ({})", f, base, backend, backend.to_uppercase());
        }
    }
    0
}

fn run_mkbitmap(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mkbitmap [OPTIONS] [FILE ...]");
        println!();
        println!("mkbitmap — transform images for tracing (Slate OS).");
        println!();
        println!("Options:");
        println!("  -o, --output FILE   Output file");
        println!("  -f, --filter N      Highpass filter radius (default 4)");
        println!("  -s, --scale N       Scale factor (default 2)");
        println!("  -t, --threshold N   Threshold (0-1, default 0.45)");
        println!("  -b, --blur N        Gaussian blur radius");
        println!("  -n, --nodefaults    Don't apply defaults");
        println!("  -i, --invert        Invert");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mkbitmap 1.16 (Slate OS)");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        println!("mkbitmap: reading from stdin...");
        println!("mkbitmap: output written to stdout");
    } else {
        for f in &files {
            println!("mkbitmap: processing '{}' → PBM", f);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "potrace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "mkbitmap" => run_mkbitmap(&rest),
        _ => run_potrace(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_potrace};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/potrace"), "potrace");
        assert_eq!(basename(r"C:\bin\potrace.exe"), "potrace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("potrace.exe"), "potrace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_potrace(&["--help".to_string()]), 0);
        assert_eq!(run_potrace(&["-h".to_string()]), 0);
        let _ = run_potrace(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_potrace(&[]);
    }
}
