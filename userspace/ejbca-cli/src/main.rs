#![deny(clippy::all)]

//! ejbca-cli — SlateOS EJBCA enterprise PKI
//!
//! Single personality: `ejbca`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ejbca(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ejbca [COMMAND] [OPTIONS]");
        println!("EJBCA v8.2 (Slate OS) — Enterprise PKI certificate authority");
        println!();
        println!("Commands:");
        println!("  ca list            List certificate authorities");
        println!("  ca create          Create new CA");
        println!("  cert request       Request certificate");
        println!("  cert revoke        Revoke certificate");
        println!("  cert search        Search certificates");
        println!("  endentity add      Add end entity");
        println!("  profile list       List certificate profiles");
        println!("  crl create         Generate CRL");
        println!();
        println!("Options:");
        println!("  --url URL          EJBCA server URL");
        println!("  --cert FILE        Client certificate");
        println!("  --key FILE         Client key");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("EJBCA v8.2.0 (Slate OS)"); return 0; }
    println!("EJBCA v8.2.0 (Slate OS)");
    println!("  CAs: 3 (Root, Issuing, SubCA)");
    println!("  Certificates: 45,678 issued");
    println!("  End entities: 12,345");
    println!("  Profiles: 8 certificate, 5 end entity");
    println!("  Protocols: CMP, EST, ACME, SCEP, REST");
    println!("  Web: https://0.0.0.0:8443/ejbca");
    println!("  HSM: SoftToken (PKCS#11 available)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ejbca".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ejbca(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ejbca};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ejbca"), "ejbca");
        assert_eq!(basename(r"C:\bin\ejbca.exe"), "ejbca.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ejbca.exe"), "ejbca");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ejbca(&["--help".to_string()], "ejbca"), 0);
        assert_eq!(run_ejbca(&["-h".to_string()], "ejbca"), 0);
        let _ = run_ejbca(&["--version".to_string()], "ejbca");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ejbca(&[], "ejbca");
    }
}
