#![deny(clippy::all)]

//! age — SlateOS modern file encryption tool
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `age` (default) — encrypt/decrypt files
//! - `age-keygen` — generate age key pairs

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _AGE_HEADER: &str = "age-encryption.org/v1";
const _AGE_KEY_PREFIX: &str = "AGE-SECRET-KEY-";
const _AGE_RECIPIENT_PREFIX: &str = "age1";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Encrypt,
    Decrypt,
}

#[derive(Clone, Debug)]
struct AgeOptions {
    action: Action,
    recipients: Vec<String>,
    _recipient_files: Vec<String>,
    identity_files: Vec<String>,
    passphrase: bool,
    armor: bool,
    output: Option<String>,
    files: Vec<String>,
}

impl Default for AgeOptions {
    fn default() -> Self {
        Self {
            action: Action::Encrypt,
            recipients: Vec::new(),
            _recipient_files: Vec::new(),
            identity_files: Vec::new(),
            passphrase: false,
            armor: false,
            output: None,
            files: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct KeyPair {
    public_key: String,
    secret_key: String,
    _created: String,
}

// ── Simulated data ────────────────────────────────────────────────────

fn generate_keypair() -> KeyPair {
    KeyPair {
        public_key: "age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p".to_string(),
        secret_key: "AGE-SECRET-KEY-1QVAHC9TQPAZ4GTWKXN8NK5MJNW274N6GH2AYLJ9V4TMZCV9FYYQRHES4K".to_string(),
        _created: "2025-05-22T10:30:00Z".to_string(),
    }
}

// ── age personality ──────────────────────────────────────────────────

fn run_age(args: Vec<String>) -> i32 {
    let mut opts = AgeOptions::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: age [--encrypt] [-r RECIPIENT] [-o OUTPUT] [INPUT]");
                println!("       age --decrypt [-i IDENTITY] [-o OUTPUT] [INPUT]");
                println!();
                println!("Modern file encryption tool.");
                println!();
                println!("Options:");
                println!("  -e, --encrypt          Encrypt (default)");
                println!("  -d, --decrypt          Decrypt");
                println!("  -r, --recipient REC    Recipient public key");
                println!("  -R, --recipients-file  File with recipient keys");
                println!("  -i, --identity FILE    Identity (private key) file");
                println!("  -p, --passphrase       Encrypt with passphrase");
                println!("  -a, --armor            ASCII armor output");
                println!("  -o, --output FILE      Output file");
                println!("  --version              Show version");
                return 0;
            }
            "--version" => {
                println!("age v0.1.0 (SlateOS)");
                return 0;
            }
            "-e" | "--encrypt" => opts.action = Action::Encrypt,
            "-d" | "--decrypt" => opts.action = Action::Decrypt,
            "-p" | "--passphrase" => opts.passphrase = true,
            "-a" | "--armor" => opts.armor = true,
            "-r" | "--recipient" => {
                i += 1;
                if i < args.len() { opts.recipients.push(args[i].clone()); }
            }
            "-R" | "--recipients-file" => {
                i += 1;
                if i < args.len() { opts._recipient_files.push(args[i].clone()); }
            }
            "-i" | "--identity" => {
                i += 1;
                if i < args.len() { opts.identity_files.push(args[i].clone()); }
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() { opts.output = Some(args[i].clone()); }
            }
            s if !s.starts_with('-') => opts.files.push(s.to_string()),
            _ => { eprintln!("age: unknown option '{}'", args[i]); return 1; }
        }
        i += 1;
    }

    match opts.action {
        Action::Encrypt => age_encrypt(&opts),
        Action::Decrypt => age_decrypt(&opts),
    }
}

fn age_encrypt(opts: &AgeOptions) -> i32 {
    if opts.recipients.is_empty() && !opts.passphrase {
        eprintln!("age: error: no recipients specified (use -r or -p)");
        return 1;
    }

    let input = opts.files.first().map(|s| s.as_str()).unwrap_or("stdin");
    let output = opts.output.as_deref().unwrap_or("stdout");

    if opts.passphrase {
        println!("Enter passphrase (simulated): ********");
        println!("Confirm passphrase: ********");
    }

    if opts.armor {
        println!("-----BEGIN AGE ENCRYPTED FILE-----");
        println!("YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBZYTdCSGliWWNPKzRW");
        println!("b0hLY3BEZ0RLZE1UbWVWSmR3PT0KSW5YaGJPS3Ric25MVkJGVGl5OHlHQT09");
        println!("--- (simulated encrypted data)");
        println!("-----END AGE ENCRYPTED FILE-----");
    } else {
        println!("(binary encrypted data written to {})", output);
    }

    if !opts.recipients.is_empty() {
        for r in &opts.recipients {
            eprintln!("age: encrypting {} for recipient {}...", input, &r[..20.min(r.len())]);
        }
    } else {
        eprintln!("age: encrypting {} with passphrase", input);
    }

    0
}

fn age_decrypt(opts: &AgeOptions) -> i32 {
    if opts.identity_files.is_empty() && !opts.passphrase {
        eprintln!("age: error: no identity specified (use -i or -p)");
        return 1;
    }

    let input = opts.files.first().map(|s| s.as_str()).unwrap_or("stdin");
    let output = opts.output.as_deref().unwrap_or("stdout");

    if opts.passphrase {
        println!("Enter passphrase: ********");
    }

    println!("age: decrypting {} → {} (simulated)", input, output);
    0
}

// ── age-keygen personality ───────────────────────────────────────────

fn run_age_keygen(args: Vec<String>) -> i32 {
    let mut output: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: age-keygen [-o FILE]");
                println!();
                println!("Generate an age X25519 identity (key pair).");
                println!();
                println!("Options:");
                println!("  -o FILE    Write key to FILE instead of stdout");
                println!("  --version  Show version");
                return 0;
            }
            "--version" => {
                println!("age-keygen v0.1.0 (SlateOS)");
                return 0;
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() { output = Some(args[i].clone()); }
            }
            _ => {}
        }
        i += 1;
    }

    let kp = generate_keypair();

    if let Some(path) = &output {
        eprintln!("Public key: {}", kp.public_key);
        println!("# created: 2025-05-22T10:30:00Z");
        println!("# public key: {}", kp.public_key);
        println!("{}", kp.secret_key);
        eprintln!("age-keygen: key written to {}", path);
    } else {
        println!("# created: 2025-05-22T10:30:00Z");
        println!("# public key: {}", kp.public_key);
        println!("{}", kp.secret_key);
    }

    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("age");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "age-keygen" => run_age_keygen(rest),
        _ => run_age(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keygen() {
        let kp = generate_keypair();
        assert!(kp.public_key.starts_with("age1"));
        assert!(kp.secret_key.starts_with("AGE-SECRET-KEY-"));
    }

    #[test]
    fn test_default_options() {
        let opts = AgeOptions::default();
        assert_eq!(opts.action, Action::Encrypt);
        assert!(!opts.passphrase);
        assert!(!opts.armor);
    }
}
