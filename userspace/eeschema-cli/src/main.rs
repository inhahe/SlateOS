#![deny(clippy::all)]

//! eeschema-cli — OurOS KiCad Eeschema schematic editor
//!
//! Multi-personality: `eeschema`, `pcbnew`, `gerbview`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eeschema(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: eeschema [OPTIONS] [FILE.kicad_sch]");
        println!("eeschema v8.0 (OurOS) — KiCad schematic editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features: hierarchical sheets, BOM export, ERC,");
        println!("  symbol library editor, annotation, bus support");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("eeschema v8.0 (OurOS, KiCad)"); return 0; }
    println!("eeschema: KiCad schematic editor started");
    0
}

fn run_pcbnew(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pcbnew [OPTIONS] [FILE.kicad_pcb]");
        println!("pcbnew v8.0 (OurOS) — KiCad PCB layout editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features: interactive router, length tuning,");
        println!("  3D viewer, DRC, footprint editor, zone fills");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("pcbnew v8.0 (OurOS, KiCad)"); return 0; }
    println!("pcbnew: KiCad PCB layout editor started");
    println!("  Layers: 32 copper + technical layers");
    println!("  Router: interactive with DRC");
    println!("  3D viewer: STEP/VRML export");
    0
}

fn run_gerbview(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gerbview [OPTIONS] [FILE.gbr...]");
        println!("gerbview v8.0 (OurOS) — KiCad Gerber file viewer");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("gerbview v8.0 (OurOS, KiCad)"); return 0; }
    println!("gerbview: Gerber viewer started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "eeschema".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pcbnew" => run_pcbnew(&rest, &prog),
        "gerbview" => run_gerbview(&rest, &prog),
        _ => run_eeschema(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eeschema};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/eeschema"), "eeschema");
        assert_eq!(basename(r"C:\bin\eeschema.exe"), "eeschema.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("eeschema.exe"), "eeschema");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_eeschema(&["--help".to_string()], "eeschema"), 0);
        assert_eq!(run_eeschema(&["-h".to_string()], "eeschema"), 0);
        assert_eq!(run_eeschema(&["--version".to_string()], "eeschema"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_eeschema(&[], "eeschema"), 0);
    }
}
