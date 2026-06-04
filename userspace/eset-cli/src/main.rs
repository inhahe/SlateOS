#![deny(clippy::all)]

//! eset-cli — OurOS ESET HOME / NOD32 security
//!
//! Single personality: `eset`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eset(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: eset [OPTIONS]");
        println!("ESET HOME Security Premium 17.2 / NOD32 (OurOS)");
        println!();
        println!("Options:");
        println!("  --scan TYPE            smart/full/custom/computer/removable");
        println!("  --sysrescue            ESET SysRescue Live (bootable AV)");
        println!("  --sysinspector         SysInspector diagnostic tool");
        println!("  --protect              ESET PROTECT (business endpoint)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ESET HOME Security Premium 17.2.7.0 (OurOS)"); return 0; }
    println!("ESET HOME Security Premium 17.2.7.0 (OurOS)");
    println!("  Origin: Slovakia, founded 1992; NOD32 antivirus engine since 1998");
    println!("  Consumer line (rebranded 2023 from 'ESET Smart Security'):");
    println!("    HOME Security Essential / Premium / Ultimate");
    println!("  Business: ESET PROTECT (Entry/Advanced/Complete/Elite/Enterprise/MDR)");
    println!("  Engine: ThreatSense (signatures + heuristics + ML + cloud LiveGrid)");
    println!("  Strengths: low resource usage, false-positive rate, gamer mode");
    println!("  Features: AV, anti-phishing, ransomware shield, exploit blocker,");
    println!("            UEFI scanner, network attack protection, secure browser, VPN");
    println!("  Mobile: ESET Mobile Security for Android, ESET Cybersecurity for Mac");
    println!("  License: annual subscription (Essential/Premium/Ultimate by features)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "eset".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eset(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eset};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/eset"), "eset");
        assert_eq!(basename(r"C:\bin\eset.exe"), "eset.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("eset.exe"), "eset");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_eset(&["--help".to_string()], "eset"), 0);
        assert_eq!(run_eset(&["-h".to_string()], "eset"), 0);
        let _ = run_eset(&["--version".to_string()], "eset");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_eset(&[], "eset");
    }
}
