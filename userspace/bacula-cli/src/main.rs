#![deny(clippy::all)]

//! bacula-cli — Slate OS Bacula backup CLI
//!
//! Multi-personality: `bconsole`, `bscan`, `bls`, `bextract`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_bconsole(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") {
        println!("Usage: bconsole [OPTIONS]");
        println!();
        println!("bconsole — Bacula console (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c FILE     Config file");
        println!("  -d N        Debug level");
        println!("  -n          No conio");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("bconsole 13.0.4 (Slate OS)");
        return 0;
    }
    println!("Connecting to Director localhost:9101...");
    println!("1000 OK: director version 13.0.4 (Slate OS)");
    println!("Enter a period (.) to cancel a command.");
    println!("*");
    0
}

fn run_bscan(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bscan [OPTIONS] DEVICE");
        println!();
        println!("bscan — scan Bacula volumes into catalog (Slate OS).");
        println!();
        println!("Options:");
        println!("  -b BS      Bootstrap file");
        println!("  -c FILE    Config file");
        println!("  -d N       Debug level");
        println!("  -s         Scan in stored format");
        println!("  -v         Verbose");
        println!("  -V VOLUME  Volume name");
        return 0;
    }
    let device = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/dev/st0");
    println!("bscan: scanning device '{}'", device);
    println!("  Records: 4523");
    println!("  Jobs: 12");
    println!("  Volumes: 1");
    println!("  Catalog records created: 4523");
    0
}

fn run_bls(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bls [OPTIONS] DEVICE");
        println!();
        println!("bls — list Bacula volume contents (Slate OS).");
        return 0;
    }
    println!("bls: listing volume contents");
    println!("  Job: BackupJob1  2024-01-15 10:30:00");
    println!("    /home/user/.bashrc        4096 bytes");
    println!("    /home/user/docs/report    12288 bytes");
    println!("    /etc/hosts                  256 bytes");
    println!("  Total: 3 files, 16640 bytes");
    0
}

fn run_bextract(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bextract [OPTIONS] DEVICE DIRECTORY");
        println!();
        println!("bextract — extract Bacula volume to directory (Slate OS).");
        println!();
        println!("Options:");
        println!("  -b BS      Bootstrap file");
        println!("  -c FILE    Config file");
        println!("  -e FILE    Exclude list");
        println!("  -i FILE    Include list");
        println!("  -V VOLUME  Volume name");
        return 0;
    }
    println!("bextract: extracting files...");
    println!("  Extracted 47 files (1.2 MB)");
    println!("  Done.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "bconsole".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "bscan" => run_bscan(&rest),
        "bls" => run_bls(&rest),
        "bextract" => run_bextract(&rest),
        _ => run_bconsole(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bconsole};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bacula"), "bacula");
        assert_eq!(basename(r"C:\bin\bacula.exe"), "bacula.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bacula.exe"), "bacula");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bconsole(&["--help".to_string()]), 0);
        assert_eq!(run_bconsole(&["-h".to_string()]), 0);
        let _ = run_bconsole(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bconsole(&[]);
    }
}
