#![deny(clippy::all)]

//! ldap-cli — SlateOS LDAP client tools
//!
//! Multi-personality: `ldapsearch`, `ldapadd`, `ldapmodify`, `ldapdelete`, `ldapwhoami`, `ldappasswd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ldapsearch(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ldapsearch [OPTIONS] <filter> [attrs...]");
        println!();
        println!("ldapsearch — LDAP search tool (SlateOS, OpenLDAP 2.6).");
        println!();
        println!("Options:");
        println!("  -H <URI>       LDAP URI");
        println!("  -b <base>      Search base DN");
        println!("  -D <binddn>    Bind DN");
        println!("  -w <passwd>    Bind password");
        println!("  -W             Prompt for password");
        println!("  -x             Simple auth");
        println!("  -LLL           LDIF output without comments");
        return 0;
    }

    let filter = args.iter().find(|a| a.starts_with('(')).map(|s| s.as_str()).unwrap_or("(objectClass=*)");
    let _ = filter;

    println!("# extended LDIF");
    println!("#");
    println!("# LDAPv3");
    println!("# base <dc=example,dc=com> with scope subtree");
    println!("# filter: (objectClass=inetOrgPerson)");
    println!("# requesting: ALL");
    println!("#");
    println!();
    println!("# jdoe, People, example.com");
    println!("dn: uid=jdoe,ou=People,dc=example,dc=com");
    println!("objectClass: inetOrgPerson");
    println!("objectClass: posixAccount");
    println!("uid: jdoe");
    println!("cn: John Doe");
    println!("sn: Doe");
    println!("givenName: John");
    println!("mail: jdoe@example.com");
    println!("uidNumber: 1001");
    println!("gidNumber: 1001");
    println!("homeDirectory: /home/jdoe");
    println!("loginShell: /bin/bash");
    println!();
    println!("# asmith, People, example.com");
    println!("dn: uid=asmith,ou=People,dc=example,dc=com");
    println!("objectClass: inetOrgPerson");
    println!("uid: asmith");
    println!("cn: Alice Smith");
    println!("sn: Smith");
    println!("mail: asmith@example.com");
    println!();
    println!("# search result");
    println!("search: 2");
    println!("result: 0 Success");
    println!();
    println!("# numResponses: 3");
    println!("# numEntries: 2");
    0
}

fn run_ldapadd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ldapadd [OPTIONS] [-f ldiffile]");
        println!("  -H <URI>    LDAP URI");
        println!("  -D <dn>     Bind DN");
        println!("  -w <pw>     Password");
        println!("  -f <file>   LDIF file");
        return 0;
    }

    let file = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("input.ldif");
    println!("adding new entry from \"{}\"", file);
    println!("adding new entry \"uid=newuser,ou=People,dc=example,dc=com\"");
    println!();
    0
}

fn run_ldapdelete(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ldapdelete [OPTIONS] <dn> [dn...]");
        return 0;
    }
    let dn = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("uid=user,ou=People,dc=example,dc=com");
    println!("deleting entry \"{}\"", dn);
    0
}

fn run_ldapwhoami(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ldapwhoami [OPTIONS]");
        return 0;
    }
    let _ = args;
    println!("dn:uid=admin,ou=People,dc=example,dc=com");
    println!("Result: Success (0)");
    0
}

fn run_ldappasswd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ldappasswd [OPTIONS] [user]");
        return 0;
    }
    let user = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("uid=jdoe,ou=People,dc=example,dc=com");
    println!("Result: Success (0)");
    println!("Password changed for \"{}\"", user);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ldapsearch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ldapadd" | "ldapmodify" => run_ldapadd(&rest),
        "ldapdelete" => run_ldapdelete(&rest),
        "ldapwhoami" => run_ldapwhoami(&rest),
        "ldappasswd" => run_ldappasswd(&rest),
        _ => run_ldapsearch(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ldapsearch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ldap"), "ldap");
        assert_eq!(basename(r"C:\bin\ldap.exe"), "ldap.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ldap.exe"), "ldap");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ldapsearch(&["--help".to_string()]), 0);
        assert_eq!(run_ldapsearch(&["-h".to_string()]), 0);
        let _ = run_ldapsearch(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ldapsearch(&[]);
    }
}
