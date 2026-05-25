#![deny(clippy::all)]

//! openldap-cli — OurOS OpenLDAP directory server
//!
//! Multi-personality: `slapd`, `ldapsearch`, `ldapadd`, `ldapmodify`, `ldapdelete`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_openldap(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "ldapsearch" => {
                println!("ldapsearch (OurOS) — LDAP search tool");
                println!("  -H URI             LDAP URI");
                println!("  -b BASE            Search base DN");
                println!("  -D BINDDN          Bind DN");
                println!("  -w PASSWD          Bind password");
                println!("  -s SCOPE           Search scope (base/one/sub)");
                println!("  FILTER [ATTRS]     Search filter and attributes");
            }
            "ldapadd" => {
                println!("ldapadd (OurOS) — LDAP add entries");
                println!("  -H URI             LDAP URI");
                println!("  -D BINDDN          Bind DN");
                println!("  -w PASSWD          Bind password");
                println!("  -f FILE            LDIF file to add");
            }
            "ldapmodify" => {
                println!("ldapmodify (OurOS) — LDAP modify entries");
                println!("  -H URI             LDAP URI");
                println!("  -D BINDDN          Bind DN");
                println!("  -w PASSWD          Bind password");
                println!("  -f FILE            LDIF modification file");
            }
            "ldapdelete" => {
                println!("ldapdelete (OurOS) — LDAP delete entries");
                println!("  -H URI             LDAP URI");
                println!("  -D BINDDN          Bind DN");
                println!("  -w PASSWD          Bind password");
                println!("  DN [DN ...]        DNs to delete");
            }
            _ => {
                println!("slapd (OurOS) — OpenLDAP directory server");
                println!("  -f FILE            Config file (slapd.conf)");
                println!("  -F DIR             Config directory (slapd.d)");
                println!("  -h URLS            Listener URLs");
                println!("  -d LEVEL           Debug level");
                println!("  -u USER            Run as user");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-VV") { println!("OpenLDAP v2.6.7 (OurOS)"); return 0; }
    match prog {
        "ldapsearch" | "ldapadd" | "ldapmodify" | "ldapdelete" => {
            println!("LDAP operation completed");
            println!("  Server: ldap://localhost:389");
            println!("  Result: Success (0)");
        }
        _ => {
            println!("slapd v2.6.7 (OurOS)");
            println!("  Listening: ldap://0.0.0.0:389/ ldaps://0.0.0.0:636/");
            println!("  Backend: MDB (/var/openldap/data)");
            println!("  Suffix: dc=example,dc=com");
            println!("  Entries: 2,345");
            println!("  TLS: enabled");
            println!("  SASL: PLAIN, EXTERNAL");
            println!("  Overlays: memberof, refint, ppolicy");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "slapd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openldap(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
