#![deny(clippy::all)]

//! fscrypt — SlateOS filesystem-level encryption
//!
//! Multi-personality binary for managing filesystem encryption policies.
//! Detected via argv[0]:
//!
//! - `fscrypt` (default) — filesystem encryption management
//! - `fscryptctl` — low-level fscrypt control

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _FSCRYPT_CONF: &str = "/.fscrypt";
const _FSCRYPT_METADATA: &str = ".fscrypt";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct EncryptionPolicy {
    _id: String,
    descriptor: String,
    contents_mode: EncryptionMode,
    filenames_mode: EncryptionMode,
    _flags: u32,
    _protector_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EncryptionMode {
    Aes256Xts,
    Aes256Cts,
    _Aes128Cbc,
    _Adiantum,
}

impl std::fmt::Display for EncryptionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aes256Xts => write!(f, "AES-256-XTS"),
            Self::Aes256Cts => write!(f, "AES-256-CTS"),
            Self::_Aes128Cbc => write!(f, "AES-128-CBC"),
            Self::_Adiantum => write!(f, "Adiantum"),
        }
    }
}

#[derive(Clone, Debug)]
struct Protector {
    _id: String,
    name: String,
    protector_type: ProtectorType,
    _linked_policies: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ProtectorType {
    Login,
    CustomPassphrase,
    _RawKey,
}

impl std::fmt::Display for ProtectorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Login => write!(f, "login protector"),
            Self::CustomPassphrase => write!(f, "custom passphrase"),
            Self::_RawKey => write!(f, "raw key"),
        }
    }
}

#[derive(Clone, Debug)]
struct MountpointStatus {
    path: String,
    _filesystem: String,
    encryption_supported: bool,
    _max_key_size: u32,
    _has_metadata: bool,
    policies: Vec<EncryptionPolicy>,
    protectors: Vec<Protector>,
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_mountpoints() -> Vec<MountpointStatus> {
    vec![
        MountpointStatus {
            path: "/".to_string(),
            _filesystem: "ext4".to_string(),
            encryption_supported: true,
            _max_key_size: 64,
            _has_metadata: true,
            policies: vec![
                EncryptionPolicy {
                    _id: "policy1".to_string(),
                    descriptor: "ab12cd34ef56".to_string(),
                    contents_mode: EncryptionMode::Aes256Xts,
                    filenames_mode: EncryptionMode::Aes256Cts,
                    _flags: 0x0C,
                    _protector_ids: vec!["prot1".to_string()],
                },
            ],
            protectors: vec![
                Protector {
                    _id: "prot1".to_string(),
                    name: "user login".to_string(),
                    protector_type: ProtectorType::Login,
                    _linked_policies: vec!["policy1".to_string()],
                },
                Protector {
                    _id: "prot2".to_string(),
                    name: "backup passphrase".to_string(),
                    protector_type: ProtectorType::CustomPassphrase,
                    _linked_policies: vec![],
                },
            ],
        },
        MountpointStatus {
            path: "/home".to_string(),
            _filesystem: "ext4".to_string(),
            encryption_supported: true,
            _max_key_size: 64,
            _has_metadata: true,
            policies: vec![],
            protectors: vec![],
        },
    ]
}

// ── fscrypt personality ───────────────────────────────────────────────

fn run_fscrypt(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "status".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: fscrypt <command> [args]");
            println!();
            println!("Filesystem encryption management.");
            println!();
            println!("Commands:");
            println!("  setup [MOUNTPOINT]     Set up fscrypt on a filesystem");
            println!("  encrypt DIR            Encrypt a directory");
            println!("  unlock DIR             Unlock an encrypted directory");
            println!("  lock DIR               Lock an encrypted directory");
            println!("  purge MOUNTPOINT       Purge all unlocked keys");
            println!("  status [PATH]          Show encryption status (default)");
            println!("  metadata                Manage metadata");
            println!("  --version              Show version");
            0
        }
        "--version" | "-V" => { println!("fscrypt 0.1.0 (Slate OS)"); 0 }
        "status" => fscrypt_status(&cmd_args),
        "setup" => fscrypt_setup(&cmd_args),
        "encrypt" => fscrypt_encrypt(&cmd_args),
        "unlock" => fscrypt_unlock(&cmd_args),
        "lock" => fscrypt_lock(&cmd_args),
        "purge" => fscrypt_purge(&cmd_args),
        "metadata" => fscrypt_metadata(&cmd_args),
        other => { eprintln!("fscrypt: unknown command '{}'", other); 1 }
    }
}

fn fscrypt_status(args: &[String]) -> i32 {
    let path = args.first().map(|s| s.as_str());
    let mounts = read_mountpoints();

    if let Some(p) = path {
        // Status of specific path
        match mounts.iter().find(|m| m.path == p) {
            Some(m) => {
                println!("{} filesystem \"{}\":", if m.encryption_supported { "Encrypted" } else { "Unencrypted" }, m.path);
                println!("  Filesystem type: {}", m._filesystem);
                println!("  Encryption: {}", if m.encryption_supported { "supported" } else { "not supported" });
                println!("  Policies: {}", m.policies.len());
                println!("  Protectors: {}", m.protectors.len());
                println!();

                for pol in &m.policies {
                    println!("  Policy {}:", pol.descriptor);
                    println!("    Contents: {}", pol.contents_mode);
                    println!("    Filenames: {}", pol.filenames_mode);
                }
                for prot in &m.protectors {
                    println!("  Protector \"{}\" ({}):", prot.name, prot.protector_type);
                }
            }
            None => {
                println!("{}: not an fscrypt-enabled mountpoint", p);
            }
        }
    } else {
        // Overview
        println!("fscrypt status:");
        println!();
        for m in &mounts {
            let status = if m.encryption_supported { "yes" } else { "no" };
            println!("  {} ({}): encryption={}, policies={}, protectors={}",
                m.path, m._filesystem, status, m.policies.len(), m.protectors.len());
        }
    }
    0
}

fn fscrypt_setup(args: &[String]) -> i32 {
    let mountpoint = args.first().map(|s| s.as_str()).unwrap_or("/");
    println!("fscrypt setup: initializing {} for encryption", mountpoint);
    println!("  Creating {}/.fscrypt directory", mountpoint);
    println!("  Writing metadata files");
    println!("fscrypt: setup complete for {}", mountpoint);
    0
}

fn fscrypt_encrypt(args: &[String]) -> i32 {
    let dir = match args.first() {
        Some(d) => d.as_str(),
        None => { eprintln!("fscrypt: encrypt requires a directory"); return 1; }
    };

    println!("fscrypt: encrypting {}", dir);
    println!("  Contents encryption: AES-256-XTS");
    println!("  Filenames encryption: AES-256-CTS");
    println!("  Select a protector:");
    println!("    1 - Login protector (user login)");
    println!("    2 - Custom passphrase");
    println!("  Using protector 1 (simulated)");
    println!();
    println!("fscrypt: {} is now encrypted", dir);
    0
}

fn fscrypt_unlock(args: &[String]) -> i32 {
    let dir = match args.first() {
        Some(d) => d.as_str(),
        None => { eprintln!("fscrypt: unlock requires a directory"); return 1; }
    };

    println!("fscrypt: unlocking {}", dir);
    println!("  Using login protector (simulated)");
    println!("fscrypt: {} is now unlocked", dir);
    0
}

fn fscrypt_lock(args: &[String]) -> i32 {
    let dir = match args.first() {
        Some(d) => d.as_str(),
        None => { eprintln!("fscrypt: lock requires a directory"); return 1; }
    };

    println!("fscrypt: locking {}", dir);
    println!("fscrypt: {} is now locked", dir);
    0
}

fn fscrypt_purge(args: &[String]) -> i32 {
    let mountpoint = args.first().map(|s| s.as_str()).unwrap_or("/");
    println!("fscrypt: purging unlocked keys on {}", mountpoint);
    println!("  WARNING: all locked directories will become inaccessible");
    println!("fscrypt: keys purged (simulated)");
    0
}

fn fscrypt_metadata(args: &[String]) -> i32 {
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("dump");

    match subcmd {
        "dump" => {
            let mounts = read_mountpoints();
            for m in &mounts {
                println!("Mountpoint: {}", m.path);
                println!("  Policies:");
                for p in &m.policies {
                    println!("    {} (contents={}, filenames={})",
                        p.descriptor, p.contents_mode, p.filenames_mode);
                }
                println!("  Protectors:");
                for p in &m.protectors {
                    println!("    {} ({})", p.name, p.protector_type);
                }
            }
            0
        }
        "create" => { println!("fscrypt metadata: creating metadata (simulated)"); 0 }
        "destroy" => { println!("fscrypt metadata: destroying metadata (simulated)"); 0 }
        other => { eprintln!("fscrypt metadata: unknown subcommand '{}'", other); 1 }
    }
}

// ── fscryptctl personality ────────────────────────────────────────────

fn run_fscryptctl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: fscryptctl <command>");
            println!();
            println!("Low-level fscrypt control.");
            println!();
            println!("Commands:");
            println!("  get_policy DIR         Get encryption policy of directory");
            println!("  set_policy DIR KEY     Set encryption policy");
            println!("  get_key KEY_DESC       Get key status");
            println!("  add_key                Add key to keyring");
            println!("  remove_key KEY_DESC    Remove key from keyring");
            0
        }
        "--version" | "-V" => { println!("fscryptctl 0.1.0 (Slate OS)"); 0 }
        "get_policy" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Encryption policy for {}:", dir);
            println!("  Version: 2");
            println!("  Contents: AES-256-XTS");
            println!("  Filenames: AES-256-CTS");
            println!("  Flags: 0x0c (direct key, IV_INO_LBLK_64)");
            println!("  Key descriptor: ab12cd34ef56");
            0
        }
        "get_key" => {
            let desc = args.get(1).map(|s| s.as_str()).unwrap_or("ab12cd34ef56");
            println!("Key {}: present (unlocked)", desc);
            0
        }
        "add_key" => {
            println!("fscryptctl: key added to session keyring (simulated)");
            println!("Key descriptor: ab12cd34ef56");
            0
        }
        "remove_key" => {
            let desc = args.get(1).map(|s| s.as_str()).unwrap_or("ab12cd34ef56");
            println!("fscryptctl: key {} removed (simulated)", desc);
            0
        }
        other => { eprintln!("fscryptctl: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("fscrypt");
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
        "fscryptctl" => run_fscryptctl(rest),
        _ => run_fscrypt(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mountpoints() {
        let mounts = read_mountpoints();
        assert_eq!(mounts.len(), 2);
        assert!(mounts[0].encryption_supported);
    }

    #[test]
    fn test_policies() {
        let mounts = read_mountpoints();
        let root = &mounts[0];
        assert_eq!(root.policies.len(), 1);
        assert_eq!(root.policies[0].contents_mode, EncryptionMode::Aes256Xts);
    }

    #[test]
    fn test_protectors() {
        let mounts = read_mountpoints();
        let root = &mounts[0];
        assert_eq!(root.protectors.len(), 2);
        assert_eq!(root.protectors[0].protector_type, ProtectorType::Login);
    }

    #[test]
    fn test_encryption_mode_display() {
        assert_eq!(format!("{}", EncryptionMode::Aes256Xts), "AES-256-XTS");
        assert_eq!(format!("{}", EncryptionMode::Aes256Cts), "AES-256-CTS");
        assert_eq!(format!("{}", EncryptionMode::_Adiantum), "Adiantum");
    }

    #[test]
    fn test_protector_type_display() {
        assert_eq!(format!("{}", ProtectorType::Login), "login protector");
        assert_eq!(format!("{}", ProtectorType::CustomPassphrase), "custom passphrase");
    }
}
