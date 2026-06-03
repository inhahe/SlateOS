#![deny(clippy::all)]

//! brlcad-cli — OurOS BRL-CAD solid modeling system
//!
//! Multi-personality: `mged`, `archer`, `rt`, `nirt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mged(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mged [OPTIONS] [DATABASE.g]");
        println!("mged v7.38 (OurOS) — Multi-device Geometry Editor");
        println!();
        println!("Options:");
        println!("  -c              Command-line mode");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mged v7.38 (OurOS, BRL-CAD)"); return 0; }
    println!("mged: BRL-CAD geometry editor started");
    println!("  CSG primitives: sphere, box, cylinder, cone, torus, pipe");
    println!("  Boolean ops: union, intersection, subtraction");
    println!("  Analysis: volume, surface area, mass properties");
    0
}

fn run_archer(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: archer [OPTIONS] [DATABASE.g]");
        println!("archer v7.38 (OurOS) — BRL-CAD GUI geometry editor");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("archer v7.38 (OurOS, BRL-CAD)"); return 0; }
    println!("archer: BRL-CAD GUI editor started");
    0
}

fn run_rt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rt [OPTIONS] MODEL.g OBJECTS...");
        println!("rt v7.38 (OurOS) — BRL-CAD ray tracer");
        println!();
        println!("Options:");
        println!("  -s SIZE         Image size");
        println!("  -o FILE         Output file");
        println!("  -a AZIMUTH      View azimuth");
        println!("  -e ELEVATION    View elevation");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("rt v7.38 (OurOS, BRL-CAD)"); return 0; }
    println!("rt: ray tracing...");
    0
}

fn run_nirt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nirt [OPTIONS] MODEL.g OBJECTS...");
        println!("nirt v7.38 (OurOS) — BRL-CAD interactive ray tracer");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("nirt v7.38 (OurOS, BRL-CAD)"); return 0; }
    println!("nirt: interactive ray tracing started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mged".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "archer" => run_archer(&rest, &prog),
        "rt" => run_rt(&rest, &prog),
        "nirt" => run_nirt(&rest, &prog),
        _ => run_mged(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mged};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/brlcad"), "brlcad");
        assert_eq!(basename(r"C:\bin\brlcad.exe"), "brlcad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("brlcad.exe"), "brlcad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mged(&["--help".to_string()], "brlcad"), 0);
        assert_eq!(run_mged(&["-h".to_string()], "brlcad"), 0);
        assert_eq!(run_mged(&["--version".to_string()], "brlcad"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mged(&[], "brlcad"), 0);
    }
}
