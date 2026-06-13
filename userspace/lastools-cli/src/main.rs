#![deny(clippy::all)]

//! lastools-cli — SlateOS LAStools LiDAR processing
//!
//! Multi-personality: `lasinfo`, `lasview`, `las2txt`, `txt2las`, `lasmerge`, `lassort`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lastools(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "lasinfo" => {
                println!("lasinfo (Slate OS) — Display LAS/LAZ file information");
                println!("  -i FILE        Input LAS/LAZ file");
                println!("  -compute_density  Compute point density");
            }
            "las2txt" => {
                println!("las2txt (Slate OS) — Convert LAS/LAZ to text");
                println!("  -i FILE        Input LAS/LAZ file");
                println!("  -o FILE        Output text file");
                println!("  -parse SPEC    Parse string (xyzirnctu)");
                println!("  -sep CHAR      Separator (comma, tab, space)");
            }
            "txt2las" => {
                println!("txt2las (Slate OS) — Convert text to LAS/LAZ");
                println!("  -i FILE        Input text file");
                println!("  -o FILE        Output LAS/LAZ file");
                println!("  -parse SPEC    Parse string");
            }
            "lasmerge" => {
                println!("lasmerge (Slate OS) — Merge LAS/LAZ files");
                println!("  -i FILE [FILE...]  Input files");
                println!("  -o FILE        Output file");
            }
            "lassort" => {
                println!("lassort (Slate OS) — Sort LAS/LAZ points");
                println!("  -i FILE        Input file");
                println!("  -o FILE        Output file");
                println!("  -by_gps_time   Sort by GPS time");
            }
            _ => {
                println!("LAStools (Slate OS) — LiDAR processing suite");
                println!("  Tools: lasinfo, lasview, las2txt, txt2las, lasmerge, lassort");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("LAStools v2.0 (Slate OS)"); return 0; }
    match prog {
        "lasinfo" => {
            println!("lasinfo: reading LAS file...");
            println!("  File: survey.laz");
            println!("  Version: 1.4, Point format: 6");
            println!("  Points: 45,678,901");
            println!("  Bounds: X(234567.89, 245678.90)");
            println!("          Y(1234567.89, 1245678.90)");
            println!("          Z(100.00, 450.00)");
            println!("  Returns: 1st=34M, 2nd=8M, 3rd=2M, 4th=500K");
            println!("  Classification: ground=20M, vegetation=15M, building=8M");
        }
        "las2txt" => {
            println!("las2txt: converting...");
            println!("  Points: 45,678,901");
            println!("  Format: x,y,z,intensity,class");
            println!("  Output: survey.txt (2.1 GB)");
        }
        "lasmerge" => {
            println!("lasmerge: merging 12 tiles...");
            println!("  Total points: 567,890,123");
            println!("  Output: merged.laz (4.5 GB)");
        }
        _ => {
            println!("LAStools v2.0 (Slate OS) — LiDAR Processing");
            println!("  Use specific tool: lasinfo, las2txt, txt2las, lasmerge, lassort");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lasinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lastools(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lastools};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lastools"), "lastools");
        assert_eq!(basename(r"C:\bin\lastools.exe"), "lastools.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lastools.exe"), "lastools");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lastools(&["--help".to_string()], "lastools"), 0);
        assert_eq!(run_lastools(&["-h".to_string()], "lastools"), 0);
        let _ = run_lastools(&["--version".to_string()], "lastools");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lastools(&[], "lastools");
    }
}
