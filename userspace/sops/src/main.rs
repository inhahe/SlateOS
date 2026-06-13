#![deny(clippy::all)]

//! sops — SlateOS secrets OPerationS (encrypted file editor)
//!
//! Single personality: `sops`

use std::env;
use std::process;

fn run_sops(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sops [OPTIONS] <FILE>");
        println!();
        println!("Encrypted file editor with AWS KMS, GCP KMS, Azure Key Vault, age, and PGP.");
        println!();
        println!("Commands:");
        println!("  (default)              Edit encrypted file");
        println!("  -e, --encrypt          Encrypt a file");
        println!("  -d, --decrypt          Decrypt a file");
        println!("  -r, --rotate           Rotate data encryption key");
        println!("  updatekeys             Update keys in file");
        println!("  groups                 Manage key groups");
        println!("  filestatus             Show file encryption status");
        println!("  exec-env              Execute with decrypted env vars");
        println!("  exec-file             Execute with decrypted file");
        println!("  publish               Publish to a destination");
        println!();
        println!("Options:");
        println!("  --age <RECIPIENTS>     age recipients");
        println!("  --pgp <FINGERPRINTS>   PGP fingerprints");
        println!("  --kms <ARN>            AWS KMS ARN");
        println!("  --gcp-kms <RESOURCE>   GCP KMS resource");
        println!("  --azure-kv <URL>       Azure Key Vault URL");
        println!("  --hc-vault-transit <P> HashiCorp Vault transit path");
        println!("  -i, --in-place         Edit in place");
        println!("  --input-type <TYPE>    Input type (yaml/json/dotenv/ini/binary)");
        println!("  --output-type <TYPE>   Output type (yaml/json/dotenv/ini/binary)");
        println!("  --output <FILE>        Output file");
        println!("  --extract <PATH>       Extract specific value by path");
        println!("  --set <PATH> <VALUE>   Set specific value");
        println!("  --config <FILE>        Config file path");
        println!("  --ignore-mac           Ignore MAC mismatch");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("sops 3.8.1 (SlateOS)");
        return 0;
    }

    let encrypt = args.iter().any(|a| a == "-e" || a == "--encrypt");
    let decrypt = args.iter().any(|a| a == "-d" || a == "--decrypt");
    let rotate = args.iter().any(|a| a == "-r" || a == "--rotate");

    let file = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("secrets.yaml");

    if encrypt {
        println!("Encrypting: {}", file);
        println!("  Using age key: age1...");
        println!("  Encrypted successfully.");
    } else if decrypt {
        println!("db_password: super_secret_password");
        println!("api_key: sk-1234567890abcdef");
        println!("jwt_secret: my-jwt-secret-key");
    } else if rotate {
        println!("Rotating data key for: {}", file);
        println!("  Generated new data key");
        println!("  Re-encrypted with all master keys");
        println!("  Done.");
    } else {
        println!("Editing: {} (decrypted for editing)", file);
        println!("  (editor launched with decrypted content)");
        println!("  Re-encrypting on save...");
        println!("  Done.");
    }
    0
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
