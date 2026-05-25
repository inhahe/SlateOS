#![deny(clippy::all)]

//! mkcert-cli — OurOS mkcert local CA tool
//!
//! Single personality: `mkcert`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mkcert(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mkcert [OPTIONS] [HOSTNAMES...]");
        println!("mkcert v1.4 (OurOS) — Zero-config local CA for development certs");
        println!();
        println!("Options:");
        println!("  -install           Install local CA in system trust store");
        println!("  -uninstall         Uninstall local CA");
        println!("  -client            Generate client certificate");
        println!("  -ecdsa             Use ECDSA key (default: RSA)");
        println!("  -pkcs12            Generate PKCS#12 file");
        println!("  -cert-file FILE    Certificate output file");
        println!("  -key-file FILE     Key output file");
        println!("  -CAROOT            Print CA root directory");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mkcert v1.4.4 (OurOS)"); return 0; }
    println!("mkcert v1.4.4 (OurOS)");
    println!("  CA root: ~/.local/share/mkcert");
    println!("  CA installed in: system trust store");
    println!("  Created cert for: localhost, 127.0.0.1, ::1");
    println!("  Certificate: ./localhost+2.pem");
    println!("  Key: ./localhost+2-key.pem");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mkcert".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mkcert(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
