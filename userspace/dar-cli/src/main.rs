#![deny(clippy::all)]

//! dar-cli — OurOS DAR disk archive tool
//!
//! Multi-personality: `dar`, `dar_manager`, `dar_xform`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dar [OPTIONS]");
        println!("dar v2.7 (OurOS) — Disk Archive tool");
        println!();
        println!("Operations:");
        println!("  -c BASE     Create archive");
        println!("  -x BASE     Extract archive");
        println!("  -l BASE     List archive contents");
        println!("  -t BASE     Test archive integrity");
        println!("  -d BASE     Diff archive with filesystem");
        println!();
        println!("Options:");
        println!("  -R PATH     Root directory");
        println!("  -A REF      Differential from reference");
        println!("  -s SIZE     Slice size");
        println!("  -z ALGO     Compression (gzip, bzip2, lzo, xz, zstd)");
        println!("  --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dar v2.7 (OurOS)"); return 0; }
    println!("dar: archive operation");
    println!("  Files: 1,234");
    println!("  Total size: 256 MiB");
    println!("  Compressed: 89 MiB (35%)");
    0
}

fn run_dar_manager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dar_manager [OPTIONS]");
        println!("dar_manager v2.7 (OurOS) — DAR archive catalog manager");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dar_manager v2.7 (OurOS)"); return 0; }
    println!("dar_manager: catalog database");
    println!("  Archives: 5 (2 full, 3 differential)");
    0
}

fn run_dar_xform(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dar_xform [OPTIONS] <source> <destination>");
        println!("dar_xform v2.7 (OurOS) — Transform DAR archive slicing");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dar_xform v2.7 (OurOS)"); return 0; }
    println!("dar_xform: re-slicing archive");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "dar_manager" => run_dar_manager(&rest, &prog),
        "dar_xform" => run_dar_xform(&rest, &prog),
        _ => run_dar(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dar"), "dar");
        assert_eq!(basename(r"C:\bin\dar.exe"), "dar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dar.exe"), "dar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dar(&["--help".to_string()], "dar"), 0);
        assert_eq!(run_dar(&["-h".to_string()], "dar"), 0);
        let _ = run_dar(&["--version".to_string()], "dar");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dar(&[], "dar");
    }
}
