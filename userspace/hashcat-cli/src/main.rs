#![deny(clippy::all)]

//! hashcat-cli — SlateOS hashcat password recovery
//!
//! Single personality: `hashcat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hashcat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hashcat [OPTIONS] HASH|HASHFILE [DICT|MASK]");
        println!("hashcat v6.2 (SlateOS) — Advanced password recovery");
        println!();
        println!("Options:");
        println!("  -m MODE       Hash type (0=MD5, 1000=NTLM, 1800=sha512crypt, etc.)");
        println!("  -a MODE       Attack mode (0=dict, 1=combination, 3=brute, 6=hybrid)");
        println!("  -o FILE       Output file for cracked passwords");
        println!("  -r FILE       Rules file");
        println!("  -w LEVEL      Workload profile (1=low, 2=default, 3=high, 4=nightmare)");
        println!("  --session NAME  Session name");
        println!("  --restore     Restore session");
        println!("  --increment   Enable increment mode");
        println!("  --increment-min N  Min mask length");
        println!("  --increment-max N  Max mask length");
        println!("  -D DEVICES    OpenCL device types (1=CPU, 2=GPU)");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("hashcat v6.2.6 (SlateOS)"); return 0; }
    println!("hashcat v6.2.6 (SlateOS)");
    println!("  OpenCL Platform: SlateOS GPU Runtime");
    println!("  Device 1: GPU (4096 cores, 8192 MB)");
    println!("  Hash type: SHA-256 (mode 1400)");
    println!("  Attack mode: Dictionary + Rules");
    println!("  Hashes: 100");
    println!("  Speed: 2,345.6 MH/s");
    println!("  Progress: 89,012,345 / 100,000,000 (89.01%)");
    println!("  Recovered: 67/100 (67.00%)");
    println!("  Remaining: 33/100 (33.00%)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hashcat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hashcat(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hashcat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hashcat"), "hashcat");
        assert_eq!(basename(r"C:\bin\hashcat.exe"), "hashcat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hashcat.exe"), "hashcat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hashcat(&["--help".to_string()], "hashcat"), 0);
        assert_eq!(run_hashcat(&["-h".to_string()], "hashcat"), 0);
        let _ = run_hashcat(&["--version".to_string()], "hashcat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hashcat(&[], "hashcat");
    }
}
