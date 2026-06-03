#![deny(clippy::all)]

//! responder-cli — OurOS Responder LLMNR/NBT-NS/MDNS poisoner
//!
//! Single personality: `responder`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_responder(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: responder [OPTIONS] -I INTERFACE");
        println!("Responder v3.1 (OurOS) — LLMNR/NBT-NS/MDNS poisoner");
        println!();
        println!("Options:");
        println!("  -I IFACE       Network interface");
        println!("  -A             Analyze mode (no poisoning)");
        println!("  -f             Fingerprint hosts");
        println!("  -w             Start WPAD rogue proxy");
        println!("  -F             Force WPAD authentication");
        println!("  -P             Force proxy authentication for all");
        println!("  -v             Verbose mode");
        println!("  --lm           Force LM downgrade");
        println!("  --disable-ess  Disable ESS downgrade");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Responder v3.1.4 (OurOS)"); return 0; }
    println!("Responder v3.1.4 (OurOS)");
    println!("  Interface: eth0 (192.168.1.50)");
    println!("  Servers:");
    println!("    HTTP:  ON  | SMB:    ON  | LDAP:  ON");
    println!("    SQL:   ON  | FTP:    ON  | DNS:   ON");
    println!("    WPAD:  ON  | Kerberos: ON");
    println!("  Poisoning: LLMNR, NBT-NS, MDNS");
    println!();
    println!("  [LLMNR] Poisoned: WPAD from 192.168.1.101");
    println!("  [SMB] NTLMv2 hash captured: DOMAIN\\jsmith");
    println!("  [HTTP] NTLMv2 hash captured: DOMAIN\\admin");
    println!("  Hashes saved: Responder-Session.log");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "responder".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_responder(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_responder};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/responder"), "responder");
        assert_eq!(basename(r"C:\bin\responder.exe"), "responder.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("responder.exe"), "responder");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_responder(&["--help".to_string()], "responder"), 0);
        assert_eq!(run_responder(&["-h".to_string()], "responder"), 0);
        assert_eq!(run_responder(&["--version".to_string()], "responder"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_responder(&[], "responder"), 0);
    }
}
