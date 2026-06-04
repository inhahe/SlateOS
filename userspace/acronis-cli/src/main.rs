#![deny(clippy::all)]

//! acronis-cli — OurOS Acronis Cyber Protect backup/security
//!
//! Single personality: `acronis`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_acr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: acronis [OPTIONS]");
        println!("Acronis Cyber Protect Home Office 2024 / Cyber Protect 16 (OurOS)");
        println!();
        println!("Options:");
        println!("  --backup TYPE          full/incremental/differential/image");
        println!("  --restore IMAGE        Restore from backup");
        println!("  --clone DISK           Disk cloning (sector-level)");
        println!("  --boot-media           Create bootable rescue media (USB/ISO)");
        println!("  --cyber-protect        Anti-ransomware + AV + backup integrated");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Acronis Cyber Protect Home Office 2024 41478 (OurOS)"); return 0; }
    println!("Acronis Cyber Protect Home Office 2024 build 41478 (OurOS)");
    println!("  Vendor: Acronis International GmbH (Schaffhausen, Switzerland; founded 2003 Singapore)");
    println!("  Founders: Serguei Beloussov (Belarusian-Singaporean entrepreneur)");
    println!("  Originally: Acronis True Image (disk imaging) since 2003 — pioneer in space");
    println!("  Renamed: 'Cyber Protect Home Office' (2021) → consumer line");
    println!("  Business: Cyber Protect (Cloud) — backup + AV + EDR + DLP integrated platform");
    println!("  Features: full disk image, file-level backup, dual protection (local + cloud),");
    println!("            ransomware rollback, active anti-malware (ML-based), clone,");
    println!("            blockchain notarization (Acronis Notary), eSign");
    println!("  Plans: Standard $49.99/yr (1 PC), Advanced $89.99 (5 PCs, 500GB cloud),");
    println!("        Premium $124.99 (5 PCs, 1TB cloud + notary)");
    println!("  MSP/Enterprise: Cyber Protect Cloud — multi-tenant, per-workload pricing");
    println!("  Differentiator: only consumer backup with integrated anti-malware engine");
    println!("  Cyber Protection Operations Centers: 24/7 SOC monitoring for enterprise");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "acronis".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_acr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_acr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/acronis"), "acronis");
        assert_eq!(basename(r"C:\bin\acronis.exe"), "acronis.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("acronis.exe"), "acronis");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_acr(&["--help".to_string()], "acronis"), 0);
        assert_eq!(run_acr(&["-h".to_string()], "acronis"), 0);
        let _ = run_acr(&["--version".to_string()], "acronis");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_acr(&[], "acronis");
    }
}
