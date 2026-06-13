#![deny(clippy::all)]

//! sops-cli — Slate OS SOPS (Secrets OPerationS) CLI
//!
//! Single personality: `sops`

use std::env;
use std::process;

fn run_sops(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sops <COMMAND> [OPTIONS] [FILE]");
        println!();
        println!("SOPS: Secrets OPerationS — encrypted file editor (Slate OS).");
        println!();
        println!("Commands:");
        println!("  encrypt      Encrypt a file");
        println!("  decrypt      Decrypt a file");
        println!("  edit         Edit an encrypted file");
        println!("  rotate       Rotate data keys");
        println!("  updatekeys   Update keys");
        println!("  groups       Manage key groups");
        println!("  filestatus   Show encryption status");
        println!("  publish      Publish file to store");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sops 3.8.1 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "encrypt" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("secrets.yaml");
            let in_place = args.iter().any(|a| a == "-i" || a == "--in-place");
            println!("Encrypting {}...", file);
            println!("  Using key: age1abc123def456...");
            if in_place {
                println!("  File encrypted in-place");
            } else {
                println!("  db_password: ENC[AES256_GCM,data:abc123def456,iv:...]");
                println!("  api_key: ENC[AES256_GCM,data:ghi789jkl012,iv:...]");
                println!("  sops:");
                println!("    lastmodified: \"2024-01-15T14:00:00Z\"");
                println!("    mac: ENC[AES256_GCM,data:mno345pqr678,iv:...]");
            }
            0
        }
        "decrypt" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("secrets.enc.yaml");
            let extract = args.windows(2).find(|w| w[0] == "--extract")
                .map(|w| w[1].as_str());
            println!("Decrypting {}...", file);
            if let Some(path) = extract {
                println!("  {} = \"decrypted-value\"", path);
            } else {
                println!("  db_password: s3cur3-p4ssw0rd!");
                println!("  api_key: sk-abc123def456ghi789");
                println!("  redis_url: redis://localhost:6379/0");
            }
            0
        }
        "edit" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("secrets.enc.yaml");
            println!("Decrypting {} for editing...", file);
            println!("  Opening in $EDITOR...");
            println!("  File saved and re-encrypted.");
            0
        }
        "rotate" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("secrets.enc.yaml");
            println!("Rotating data key for {}...", file);
            println!("  Data key rotated successfully");
            println!("  All values re-encrypted with new data key");
            0
        }
        "updatekeys" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("secrets.enc.yaml");
            println!("Updating keys for {}...", file);
            println!("  Added key: age1newkey...");
            println!("  Removed key: age1oldkey...");
            println!("  Keys updated successfully");
            0
        }
        "filestatus" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("secrets.enc.yaml");
            println!("File: {}", file);
            println!("  Encrypted: true");
            println!("  Last modified: 2024-01-15T14:00:00Z");
            println!("  Key groups:");
            println!("    Group 0:");
            println!("      age: age1abc123def456...");
            println!("      pgp: ABC123DEF456...");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: sops <command>. See --help.");
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
    let code = run_sops(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sops};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sops(vec!["--help".to_string()]), 0);
        assert_eq!(run_sops(vec!["-h".to_string()]), 0);
        let _ = run_sops(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sops(vec![]);
    }
}
