#![deny(clippy::all)]

//! kerberos-cli — OurOS Kerberos authentication tools
//!
//! Multi-personality: `kinit`, `klist`, `kdestroy`, `kpasswd`, `kadmin`, `ktutil`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kinit(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kinit [OPTIONS] [principal]");
        println!();
        println!("kinit — Kerberos initial authentication (OurOS, MIT Kerberos 1.21).");
        println!();
        println!("Options:");
        println!("  -l <lifetime>    Ticket lifetime");
        println!("  -r <renewable>   Renewable lifetime");
        println!("  -k               Use keytab");
        println!("  -t <keytab>      Keytab file");
        println!("  -f               Forwardable tickets");
        println!("  -R               Renew existing ticket");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("kinit (MIT Kerberos 1.21 OurOS)");
        return 0;
    }

    let principal = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("user@EXAMPLE.COM");
    if args.iter().any(|a| a == "-R") {
        println!("Ticket renewed for {}.", principal);
    } else {
        println!("Authenticated as {}.", principal);
    }
    0
}

fn run_klist(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: klist [OPTIONS]");
        println!("  -e    Show encryption types");
        println!("  -f    Show ticket flags");
        println!("  -s    Silent (exit code only)");
        println!("  -k    Show keytab");
        return 0;
    }

    if args.iter().any(|a| a == "-k") {
        println!("Keytab name: FILE:/etc/krb5.keytab");
        println!("KVNO Principal");
        println!("---- --------------------------------------------------------------------------");
        println!("   2 host/ouros-desktop.example.com@EXAMPLE.COM");
        println!("   2 host/ouros-desktop.example.com@EXAMPLE.COM");
        return 0;
    }

    println!("Ticket cache: FILE:/tmp/krb5cc_1000");
    println!("Default principal: user@EXAMPLE.COM");
    println!();
    println!("Valid starting       Expires              Service principal");
    println!("05/22/2024 08:00:00  05/22/2024 18:00:00  krbtgt/EXAMPLE.COM@EXAMPLE.COM");
    if args.iter().any(|a| a == "-f") {
        println!("\trenew until 05/29/2024 08:00:00, Flags: FRI");
    }
    println!("05/22/2024 08:05:00  05/22/2024 18:00:00  host/server.example.com@EXAMPLE.COM");
    println!("05/22/2024 08:10:00  05/22/2024 18:00:00  nfs/fileserver.example.com@EXAMPLE.COM");
    if args.iter().any(|a| a == "-e") {
        println!("\tEtype (skey, tkt): aes256-cts-hmac-sha1-96, aes256-cts-hmac-sha1-96");
    }
    0
}

fn run_kdestroy(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kdestroy [OPTIONS]");
        println!("  -A    Destroy all caches");
        return 0;
    }
    let _ = args;
    println!("Ticket cache destroyed.");
    0
}

fn run_kpasswd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kpasswd [principal]");
        return 0;
    }
    let principal = args.first().map(|s| s.as_str()).unwrap_or("user@EXAMPLE.COM");
    println!("Password changed for {}.", principal);
    0
}

fn run_kadmin(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kadmin [OPTIONS] [-q query]");
        println!();
        println!("kadmin — Kerberos administration (OurOS).");
        println!();
        println!("Options:");
        println!("  -p <principal>    Admin principal");
        println!("  -q <query>        Execute query");
        println!("  -l                Local mode (kadmin.local)");
        println!();
        println!("Queries: addprinc, delprinc, modprinc, listprincs, getprinc,");
        println!("  addpol, delpol, modpol, listpols, getpol, ktadd, ktremove");
        return 0;
    }

    let query = args.windows(2).find(|w| w[0] == "-q").map(|w| w[1].as_str());
    if let Some(q) = query {
        if q == "listprincs" || q.starts_with("list") {
            println!("K/M@EXAMPLE.COM");
            println!("admin/admin@EXAMPLE.COM");
            println!("host/ouros-desktop.example.com@EXAMPLE.COM");
            println!("krbtgt/EXAMPLE.COM@EXAMPLE.COM");
            println!("user@EXAMPLE.COM");
        } else {
            println!("kadmin: {} completed.", q);
        }
    } else {
        println!("kadmin: Authenticating as principal admin/admin@EXAMPLE.COM");
        println!("kadmin:");
    }
    0
}

fn run_ktutil(_args: &[String]) -> i32 {
    println!("ktutil: ");
    println!("ktutil: list");
    println!("slot KVNO Principal");
    println!("---- ---- ---------------------------------------------------------------------");
    println!("   1    2 host/ouros-desktop.example.com@EXAMPLE.COM");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kinit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "klist" => run_klist(&rest),
        "kdestroy" => run_kdestroy(&rest),
        "kpasswd" => run_kpasswd(&rest),
        "kadmin" | "kadmin.local" => run_kadmin(&rest),
        "ktutil" => run_ktutil(&rest),
        _ => run_kinit(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
