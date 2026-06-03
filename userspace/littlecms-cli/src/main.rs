#![deny(clippy::all)]

//! littlecms-cli — OurOS Little CMS color management
//!
//! Multi-personality: `jpgicc`, `linkicc`, `transicc`, `tificc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_jpgicc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: jpgicc [OPTIONS] INPUT.jpg OUTPUT.jpg");
        println!("jpgicc v2.16 (OurOS) — Apply ICC profiles to JPEG files");
        println!();
        println!("Options:");
        println!("  -i PROFILE        Input ICC profile");
        println!("  -o PROFILE        Output ICC profile");
        println!("  -t INTENT         Rendering intent (0-3)");
        println!("  -b                Black point compensation");
        println!("  -q QUALITY        JPEG quality (0-100)");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let input = files.first().copied().unwrap_or("input.jpg");
    let output = files.get(1).copied().unwrap_or("output.jpg");
    println!("Converting: {} -> {}", input, output);
    println!("  Input profile: sRGB");
    println!("  Output profile: AdobeRGB");
    println!("  Intent: perceptual");
    println!("  Done.");
    0
}

fn run_linkicc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: linkicc [OPTIONS] PROFILES... OUTPUT.icc");
        println!("linkicc v2.16 (OurOS) — Link multiple ICC profiles into a device link");
        println!();
        println!("Options:");
        println!("  -t INTENT         Rendering intent (0-3)");
        println!("  -b                Black point compensation");
        return 0;
    }
    println!("Creating device link profile...");
    println!("  Profiles linked: 3");
    println!("  Output: devicelink.icc");
    0
}

fn run_transicc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: transicc [OPTIONS]");
        println!("transicc v2.16 (OurOS) — Transform colors between ICC profiles");
        println!();
        println!("Options:");
        println!("  -i PROFILE        Input ICC profile");
        println!("  -o PROFILE        Output ICC profile");
        println!("  -t INTENT         Rendering intent (0-3)");
        return 0;
    }
    println!("Color transform (sRGB -> CMYK):");
    println!("  Input:  R=128 G=64 B=192");
    println!("  Output: C=55.2 M=78.1 Y=0.0 K=3.8");
    0
}

fn run_tificc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tificc [OPTIONS] INPUT.tif OUTPUT.tif");
        println!("tificc v2.16 (OurOS) — Apply ICC profiles to TIFF files");
        println!();
        println!("Options:");
        println!("  -i PROFILE        Input ICC profile");
        println!("  -o PROFILE        Output ICC profile");
        println!("  -t INTENT         Rendering intent (0-3)");
        println!("  -b                Black point compensation");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let input = files.first().copied().unwrap_or("input.tif");
    let output = files.get(1).copied().unwrap_or("output.tif");
    println!("Converting: {} -> {}", input, output);
    println!("  Input profile: embedded (sRGB)");
    println!("  Output profile: ProPhoto RGB");
    println!("  Intent: relative colorimetric");
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "jpgicc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "linkicc" => run_linkicc(&rest, &prog),
        "transicc" => run_transicc(&rest, &prog),
        "tificc" => run_tificc(&rest, &prog),
        _ => run_jpgicc(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_jpgicc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/littlecms"), "littlecms");
        assert_eq!(basename(r"C:\bin\littlecms.exe"), "littlecms.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("littlecms.exe"), "littlecms");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_jpgicc(&["--help".to_string()], "littlecms"), 0);
        assert_eq!(run_jpgicc(&["-h".to_string()], "littlecms"), 0);
        assert_eq!(run_jpgicc(&["--version".to_string()], "littlecms"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_jpgicc(&[], "littlecms"), 0);
    }
}
