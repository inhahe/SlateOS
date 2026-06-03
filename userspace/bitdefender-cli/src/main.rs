#![deny(clippy::all)]

//! bitdefender-cli — OurOS Bitdefender Total Security
//!
//! Single personality: `bitdefender`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bitdefender [OPTIONS]");
        println!("Bitdefender Total Security 27.0 (OurOS) — Multi-platform security");
        println!();
        println!("Options:");
        println!("  --scan TYPE            quick/full/custom/contextual");
        println!("  --safepay              Safepay sandboxed browser");
        println!("  --vpn                  Bitdefender VPN (200MB/day free, premium upgrade)");
        println!("  --ransomware-remediation  Ransomware Remediation (rollback)");
        println!("  --gravityzone          Bitdefender GravityZone (business endpoint)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bitdefender Total Security 27.0.43.221 (OurOS)"); return 0; }
    println!("Bitdefender Total Security 27.0.43.221 (OurOS)");
    println!("  Editions: Antivirus Plus, Internet Security, Total Security, Premium Security");
    println!("  Mac: Bitdefender Antivirus for Mac");
    println!("  Mobile: Bitdefender Mobile Security (Android/iOS)");
    println!("  Family Pack: Family Pack (15 devices, family plan)");
    println!("  Business: GravityZone Small Office, Business Security, Advanced/Ultra");
    println!("  Engines: Photon (adapts to system), B-HAVE behavioral, ML, sandbox analyzer");
    println!("  Features: AV, firewall, anti-tracker, webcam/mic protection, anti-theft,");
    println!("            Safepay, file shredder, vulnerability assessment, parental");
    println!("  Threat intel: 500M+ endpoints in GPN (Global Protective Network)");
    println!("  Consistently top-ranked by AV-Comparatives, AV-TEST, SE Labs");
    println!("  License: annual subscription (1/3/5/10 devices) + family plans");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bitdefender".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bd(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bitdefender"), "bitdefender");
        assert_eq!(basename(r"C:\bin\bitdefender.exe"), "bitdefender.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bitdefender.exe"), "bitdefender");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_bd(&["--help".to_string()], "bitdefender"), 0);
        assert_eq!(run_bd(&["-h".to_string()], "bitdefender"), 0);
        assert_eq!(run_bd(&["--version".to_string()], "bitdefender"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_bd(&[], "bitdefender"), 0);
    }
}
