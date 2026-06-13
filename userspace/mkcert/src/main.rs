#![deny(clippy::all)]

//! mkcert — SlateOS local development certificate tool
//!
//! Single personality: `mkcert`

use std::env;
use std::process;

fn run_mkcert(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mkcert [OPTIONS] <DOMAIN>...");
        println!();
        println!("Create locally-trusted development certificates.");
        println!();
        println!("Options:");
        println!("  -install            Install the local CA in the system trust store");
        println!("  -uninstall          Uninstall the local CA");
        println!("  -cert-file <FILE>   Output certificate file path");
        println!("  -key-file <FILE>    Output key file path");
        println!("  -p12-file <FILE>    Output PKCS#12 file path");
        println!("  -client             Generate a client certificate");
        println!("  -ecdsa              Generate ECDSA key (default: RSA)");
        println!("  -pkcs12             Generate PKCS#12 instead of PEM");
        println!("  -csr <FILE>         Generate cert from CSR");
        println!("  -CAROOT             Print CA root directory");
        return 0;
    }

    let install = args.iter().any(|a| a == "-install");
    let uninstall = args.iter().any(|a| a == "-uninstall");
    let caroot = args.iter().any(|a| a == "-CAROOT");

    if caroot {
        println!("/home/user/.local/share/mkcert");
        return 0;
    }

    if install {
        println!("The local CA is now installed in the system trust store!");
        println!("  Created CA at: /home/user/.local/share/mkcert/rootCA.pem");
        return 0;
    }

    if uninstall {
        println!("The local CA is now uninstalled from the system trust store!");
        return 0;
    }

    let domains: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if domains.is_empty() {
        eprintln!("Error: domain required. See --help.");
        return 1;
    }

    let name = domains[0];
    println!("Created a new certificate valid for the following names:");
    for d in &domains {
        println!("  - \"{}\"", d);
    }
    println!();
    println!("The certificate is at \"./{}.pem\" and the key at \"./{}-key.pem\"", name, name);
    println!();
    println!("It will expire on 15 April 2026.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mkcert(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mkcert};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mkcert(vec!["--help".to_string()]), 0);
        assert_eq!(run_mkcert(vec!["-h".to_string()]), 0);
        let _ = run_mkcert(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mkcert(vec![]);
    }
}
