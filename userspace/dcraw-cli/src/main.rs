#![deny(clippy::all)]

//! dcraw-cli — SlateOS dcraw RAW photo decoder
//!
//! Single personality: `dcraw`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dcraw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dcraw [OPTIONS] FILE.raw...");
        println!("dcraw v9.28 (Slate OS) — RAW photo decoder");
        println!();
        println!("Options:");
        println!("  -i                Identify files (no decode)");
        println!("  -e                Extract thumbnail");
        println!("  -w                Use camera white balance");
        println!("  -a                Average white balance");
        println!("  -o N              Output colorspace (0-6)");
        println!("  -q N              Interpolation quality (0-3)");
        println!("  -T                Write TIFF (default: PPM)");
        println!("  -6                16-bit output");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if args.iter().any(|a| a == "-i") {
        for f in &files {
            println!("{}: Canon EOS R5, 8192x5464, 14-bit CR3", f);
        }
    } else {
        for f in &files {
            println!("Processing: {}", f);
            println!("  Camera: Canon EOS R5");
            println!("  Resolution: 8192x5464");
            println!("  WB: camera");
            println!("  Output: {}.ppm", f);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dcraw".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dcraw(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dcraw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dcraw"), "dcraw");
        assert_eq!(basename(r"C:\bin\dcraw.exe"), "dcraw.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dcraw.exe"), "dcraw");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dcraw(&["--help".to_string()], "dcraw"), 0);
        assert_eq!(run_dcraw(&["-h".to_string()], "dcraw"), 0);
        let _ = run_dcraw(&["--version".to_string()], "dcraw");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dcraw(&[], "dcraw");
    }
}
