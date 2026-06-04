#![deny(clippy::all)]

//! zimbra-cli — OurOS Zimbra collaboration suite
//!
//! Multi-personality: `zmcontrol`, `zmcertmgr`, `zmhsm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zimbra(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "zmcertmgr" => {
                println!("zmcertmgr (OurOS) — Zimbra certificate manager");
                println!("  viewdeployedcrt    View deployed certificates");
                println!("  createcsr DOMAIN   Create CSR");
                println!("  deploycrt TYPE FILE  Deploy certificate");
                println!("  viewcsr            View pending CSR");
                println!("  verifycrt CERT     Verify certificate");
            }
            "zmhsm" => {
                println!("zmhsm (OurOS) — Zimbra hierarchical storage management");
                println!("  start              Start HSM session");
                println!("  status             Show HSM status");
                println!("  abort              Abort running session");
            }
            _ => {
                println!("zmcontrol (OurOS) — Zimbra service controller");
                println!("  start              Start all services");
                println!("  stop               Stop all services");
                println!("  restart            Restart all services");
                println!("  status             Show service status");
                println!("  maintenance on|off Toggle maintenance mode");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zimbra v10.0.5 (OurOS)"); return 0; }
    match prog {
        "zmcertmgr" => {
            println!("Zimbra Certificate Manager");
            println!("  Deployed certs: 3");
            println!("  Self-signed: mail.example.com (expires 2025-12-31)");
            println!("  LDAP: internal CA");
        }
        "zmhsm" => {
            println!("Zimbra HSM Status:");
            println!("  Primary volume: /opt/zimbra/store (80% used)");
            println!("  Secondary volume: /opt/zimbra/store2 (30% used)");
            println!("  Last session: completed 2 hours ago");
        }
        _ => {
            println!("Zimbra v10.0.5 (OurOS) Status:");
            println!("  antispam: Running");
            println!("  antivirus: Running");
            println!("  ldap: Running");
            println!("  mailbox: Running");
            println!("  mta: Running");
            println!("  proxy: Running");
            println!("  snmp: Running");
            println!("  spell: Running");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zmcontrol".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zimbra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zimbra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zimbra"), "zimbra");
        assert_eq!(basename(r"C:\bin\zimbra.exe"), "zimbra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zimbra.exe"), "zimbra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zimbra(&["--help".to_string()], "zimbra"), 0);
        assert_eq!(run_zimbra(&["-h".to_string()], "zimbra"), 0);
        let _ = run_zimbra(&["--version".to_string()], "zimbra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zimbra(&[], "zimbra");
    }
}
