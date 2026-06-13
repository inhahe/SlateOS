#![deny(clippy::all)]

//! nessus-cli — SlateOS Tenable Nessus vulnerability scanner
//!
//! Single personality: `nessus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nessus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nessus [OPTIONS] [SUBCMD]");
        println!("Tenable Nessus 10.8 (SlateOS) — Vulnerability scanner");
        println!();
        println!("Options:");
        println!("  --scan-new TEMPLATE    Create new scan");
        println!("  --scan-launch ID       Launch scan");
        println!("  --report-export ID     Export scan report");
        println!("  --edition ED           Essentials/Professional/Expert/Manager");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tenable Nessus 10.8.2 (SlateOS)"); return 0; }
    println!("Tenable Nessus 10.8.2 (SlateOS)");
    println!("  Editions: Essentials (free, 16 IPs), Professional, Expert, Manager");
    println!("  Plugins: 200,000+ vulnerability checks updated daily");
    println!("  Coverage: OS, network devices, web apps, databases, IoT, OT/ICS");
    println!("  Compliance: CIS, DISA STIG, PCI DSS, HIPAA, SCAP, custom audit files");
    println!("  Cloud: Tenable Vulnerability Management (Tenable.io), Tenable Security Center");
    println!("  Scan types: basic, advanced, web app, credentialed, agent, malware");
    println!("  API: REST API for automation, ServiceNow/JIRA/Splunk/SIEM integration");
    println!("  Tenable One: unified exposure management platform (asset/vuln/cloud)");
    println!("  License: Free (Essentials), per-asset (Pro/Expert), enterprise (TVM)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nessus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nessus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nessus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nessus"), "nessus");
        assert_eq!(basename(r"C:\bin\nessus.exe"), "nessus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nessus.exe"), "nessus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nessus(&["--help".to_string()], "nessus"), 0);
        assert_eq!(run_nessus(&["-h".to_string()], "nessus"), 0);
        let _ = run_nessus(&["--version".to_string()], "nessus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nessus(&[], "nessus");
    }
}
