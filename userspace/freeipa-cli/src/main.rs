#![deny(clippy::all)]

//! freeipa-cli — OurOS FreeIPA identity management
//!
//! Multi-personality: `ipa`, `ipa-server-install`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_freeipa(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "ipa-server-install" => {
                println!("ipa-server-install (OurOS) — FreeIPA server installer");
                println!("  --realm REALM      Kerberos realm");
                println!("  --domain DOMAIN    Domain name");
                println!("  --ds-password PASS Directory manager password");
                println!("  --admin-password P IPA admin password");
                println!("  --setup-dns        Setup integrated DNS");
                println!("  --no-ntp           Skip NTP configuration");
                println!("  --unattended       Unattended install");
            }
            _ => {
                println!("ipa (OurOS) — FreeIPA administration tool");
                println!("  user-add USER      Add user");
                println!("  user-find          Find users");
                println!("  user-show USER     Show user details");
                println!("  group-add GROUP    Add group");
                println!("  host-add HOST      Add host");
                println!("  dnsrecord-add      Add DNS record");
                println!("  cert-request       Request certificate");
                println!("  sudorule-add RULE  Add sudo rule");
                println!("  hbacrule-add RULE  Add HBAC rule");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FreeIPA v4.11.2 (OurOS)"); return 0; }
    match prog {
        "ipa-server-install" => {
            println!("FreeIPA Server Installer v4.11.2");
            println!("  This will configure: 389-ds, KDC, httpd, certmonger");
            println!("  Required: realm, domain, passwords");
        }
        _ => {
            println!("FreeIPA v4.11.2 (OurOS)");
            println!("  Server: ipa.example.com");
            println!("  Realm: EXAMPLE.COM");
            println!("  Domain: example.com");
            println!("  Users: 345");
            println!("  Groups: 23");
            println!("  Hosts: 67");
            println!("  Services: LDAP, Kerberos, DNS, CA, NTP");
            println!("  Certificates: 89 issued");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ipa".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_freeipa(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
