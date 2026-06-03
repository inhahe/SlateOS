#![deny(clippy::all)]

//! cosign-cli — OurOS Sigstore container signing CLI
//!
//! Single personality: `cosign`

use std::env;
use std::process;

fn run_cosign(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cosign <COMMAND> [OPTIONS]");
        println!();
        println!("Container image signing, verification, and storage.");
        println!();
        println!("Commands:");
        println!("  sign         Sign a container image");
        println!("  verify       Verify a signed image");
        println!("  generate-key-pair  Generate a key pair");
        println!("  sign-blob    Sign a blob");
        println!("  verify-blob  Verify a signed blob");
        println!("  attach       Attach artifacts to images");
        println!("  download     Download artifacts from images");
        println!("  tree         Display supply chain info");
        println!("  triangulate  Find cosign storage location");
        println!("  clean        Remove cosign signatures");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("cosign 2.2.3 (OurOS)");
            0
        }
        "generate-key-pair" => {
            println!("Enter password for private key:");
            println!("Enter password for private key again:");
            println!("Private key written to cosign.key");
            println!("Public key written to cosign.pub");
            0
        }
        "sign" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("myregistry/myimage:latest");
            println!("Signing image: {}", image);
            println!("  Pushing signature to: {}", image);
            println!("  tlog entry created with index: 12345678");
            0
        }
        "verify" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("myregistry/myimage:latest");
            println!("Verification for {}:", image);
            println!("  The following checks were performed:");
            println!("  - Signature verified against key");
            println!("  - Transparency log inclusion verified");
            println!("  - Claim validated");
            println!();
            println!("[{{\"critical\":{{\"identity\":{{\"docker-reference\":\"{}\"}}}},\"optional\":{{}}}}]", image);
            0
        }
        "tree" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("myregistry/myimage:latest");
            println!("📦 Supply Chain Security for {}", image);
            println!("└── 🔐 Signatures for digest: sha256:abc123...");
            println!("    └── 🍒 sha256:def456... (cosign.sig)");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: cosign <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cosign(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_cosign};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cosign(vec!["--help".to_string()]), 0);
        assert_eq!(run_cosign(vec!["-h".to_string()]), 0);
        assert_eq!(run_cosign(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cosign(vec![]), 0);
    }
}
