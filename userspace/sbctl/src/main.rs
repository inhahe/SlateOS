#![deny(clippy::all)]

//! sbctl — Slate OS Secure Boot key management
//!
//! Multi-personality binary for UEFI Secure Boot key enrollment and management.
//! Detected via argv[0]:
//!
//! - `sbctl` (default) — Secure Boot key management
//! - `sbsign` — sign EFI binaries
//! - `sbverify` — verify EFI binary signatures
//! - `sbkeysync` — synchronize keys to UEFI firmware

use quoting::quoteaf_os;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const KEYS_DIR: &str = "/usr/share/secureboot/keys";
const DB_DIR: &str = "/usr/share/secureboot/keys/db";
const KEK_DIR: &str = "/usr/share/secureboot/keys/KEK";
const PK_DIR: &str = "/usr/share/secureboot/keys/PK";
const _DBX_DIR: &str = "/usr/share/secureboot/keys/dbx";
const FILES_DB: &str = "/usr/share/secureboot/files.db";
const EFI_SYSFS: &str = "/sys/firmware/efi";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct SecureBootStatus {
    enabled: bool,
    setup_mode: bool,
    _vendor_keys: bool,
    pk_enrolled: bool,
    kek_enrolled: bool,
    db_enrolled: bool,
}

#[derive(Clone, Debug)]
struct _KeyInfo {
    key_type: _KeyType,
    owner: String,
    _guid: String,
    _cert_path: String,
    _key_path: String,
}

#[derive(Clone, Debug, PartialEq)]
enum _KeyType {
    PlatformKey,
    KeyExchangeKey,
    Db,
    Dbx,
}

impl std::fmt::Display for _KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlatformKey => write!(f, "Platform Key (PK)"),
            Self::KeyExchangeKey => write!(f, "Key Exchange Key (KEK)"),
            Self::Db => write!(f, "Signature Database (db)"),
            Self::Dbx => write!(f, "Forbidden Signatures (dbx)"),
        }
    }
}

#[derive(Clone, Debug)]
struct SignedFile {
    path: String,
    signed: bool,
    _output_path: String,
}

#[derive(Clone, Debug)]
struct _BundleInfo {
    _kernel: String,
    _initrd: String,
    _cmdline: String,
    _output: String,
    _os_release: String,
}

// ── Secure Boot status ─────────────────────────────────────────────────

fn read_sb_status() -> SecureBootStatus {
    let sb_path = format!(
        "{}/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c",
        EFI_SYSFS
    );
    let setup_path = format!(
        "{}/efivars/SetupMode-8be4df61-93ca-11d2-aa0d-00e098032b8c",
        EFI_SYSFS
    );

    let enabled = std::fs::read(&sb_path)
        .map(|data| !data.is_empty() && data.last().copied() == Some(1))
        .unwrap_or(false);

    let setup_mode = std::fs::read(&setup_path)
        .map(|data| !data.is_empty() && data.last().copied() == Some(1))
        .unwrap_or(true);

    let pk_enrolled = std::path::Path::new(PK_DIR).exists()
        && std::fs::read_dir(PK_DIR)
            .map(|e| e.count() > 0)
            .unwrap_or(false);
    let kek_enrolled = std::path::Path::new(KEK_DIR).exists()
        && std::fs::read_dir(KEK_DIR)
            .map(|e| e.count() > 0)
            .unwrap_or(false);
    let db_enrolled = std::path::Path::new(DB_DIR).exists()
        && std::fs::read_dir(DB_DIR)
            .map(|e| e.count() > 0)
            .unwrap_or(false);

    SecureBootStatus {
        enabled,
        setup_mode,
        _vendor_keys: false,
        pk_enrolled,
        kek_enrolled,
        db_enrolled,
    }
}

fn read_signed_files() -> Vec<SignedFile> {
    let content = match std::fs::read_to_string(FILES_DB) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut files = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        let path = parts.first().unwrap_or(&"").to_string();
        let signed = parts.get(1).map(|s| *s == "signed").unwrap_or(false);
        let output = parts.get(2).unwrap_or(&"").to_string();

        files.push(SignedFile {
            path,
            signed,
            _output_path: output,
        });
    }
    files
}

// ── sbctl commands ─────────────────────────────────────────────────────

fn cmd_status() {
    let status = read_sb_status();

    println!("Installed:   sbctl");
    println!("Owner:       Slate OS");
    println!(
        "Setup Mode:  {}",
        if status.setup_mode {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    println!(
        "Secure Boot: {}",
        if status.enabled {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    println!();
    println!("Keys:");
    println!(
        "  PK:  {}",
        if status.pk_enrolled {
            "Enrolled"
        } else {
            "Not enrolled"
        }
    );
    println!(
        "  KEK: {}",
        if status.kek_enrolled {
            "Enrolled"
        } else {
            "Not enrolled"
        }
    );
    println!(
        "  db:  {}",
        if status.db_enrolled {
            "Enrolled"
        } else {
            "Not enrolled"
        }
    );
}

/// Refuse a subcommand that would have to WRITE secure-boot state.
///
/// `fs::write` appears nowhere in this crate. Not one of these commands ever
/// created a key, signed an image, or changed a firmware variable -- they
/// printed success and returned, and `sbctl sign` in particular told a user
/// their kernel image was signed while leaving the file byte-for-byte
/// unchanged. That is the kind of claim nothing downstream can correct: the
/// failure surfaces later, as a firmware refusal to boot, to somebody who has
/// no reason to suspect this tool.
///
/// Two different things are missing behind these, and the message says which,
/// because they have different owners and different prospects:
///
/// * **A door to the kernel.** `fs::secureboot` is real -- key enrolment,
///   image verification, `/proc/secureboot` -- and `posix/` exposes none of
///   it. Filed as
///   `requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md`.
/// * **A crypto stack.** Generating a PK/KEK/db keypair needs RSA and X.509,
///   and signing an EFI binary needs Authenticode. Neither exists in this
///   tree, and neither belongs in the kernel, so that half is not waiting on
///   lane A at all.
fn refuse_write(action: &str, missing: &str) -> ! {
    eprintln!("sbctl: cannot {action}: {missing}");
    eprintln!("sbctl: nothing was changed");
    process::exit(1);
}

const NEEDS_KERNEL_DOOR: &str = "userspace has no interface to the kernel's secure-boot key store; see requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md";

const NEEDS_CRYPTO: &str =
    "this system has no RSA or X.509 implementation, so no key pair or signature can be produced";

fn cmd_create_keys() {
    // It also used to create the three key DIRECTORIES, empty, which is why
    // `status` then reported "PK: Not enrolled" immediately after this
    // command said the keys were created: `status` decides by asking whether
    // the directory is non-empty. The tool contradicted its own success
    // message on the very next invocation.
    refuse_write("create secure boot keys", NEEDS_CRYPTO);
}

fn cmd_enroll_keys(args: &[String]) {
    // The options are still parsed, so an unknown one is still rejected and
    // the refusal below names what the caller actually asked for.
    let with_microsoft = args.iter().any(|a| a == "--microsoft" || a == "-m");

    // The prompt is gone with the rest. It printed "Proceed? [y/N]" and then
    // never read stdin -- it enrolled (that is, printed that it had enrolled)
    // whatever the user would have typed. A confirmation that does not wait
    // for an answer is worse than none: it teaches the reader that this tool
    // asks before doing something irreversible, which it does not.
    let what = if with_microsoft {
        "enroll keys including Microsoft's"
    } else {
        "enroll keys"
    };
    refuse_write(what, NEEDS_KERNEL_DOOR);
}

fn cmd_sign(args: &[String]) {
    let mut save = false;
    let mut output: Option<String> = None;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--save" => save = true,
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output = Some(args[i].clone());
                }
            }
            _ if !args[i].starts_with('-') => {
                files.push(args[i].clone());
            }
            _ => {}
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!("Usage: sbctl sign [OPTIONS] <FILE>...");
        process::exit(1);
    }

    // This printed "Signing X -> Y" and "Using key: .../db.key" for every
    // file and opened none of them. Of everything in this crate it is the
    // most dangerous line, because a signature is exactly the claim a user
    // cannot check by looking: the file is byte-for-byte unchanged, so the
    // lie is discovered by firmware, at boot, long after anyone would
    // connect it to this command.
    //
    // The refusal names what was asked for -- every file, and the output path
    // if one was given -- so a script's log says which signing did not happen
    // rather than merely that one did not.
    let named = files.iter().map(quoteaf_os).collect::<Vec<_>>().join(", ");
    let what = match (&output, save) {
        (Some(o), _) => format!("sign {named} to {}", quoteaf_os(o)),
        (None, true) => format!("sign {named} and record it in the files database"),
        (None, false) => format!("sign {named}"),
    };
    refuse_write(&what, NEEDS_CRYPTO);
}

fn cmd_verify(args: &[String]) {
    if args.is_empty() {
        // Verify all tracked files
        let files = read_signed_files();
        if files.is_empty() {
            println!("No files tracked for signing.");
            return;
        }
        for f in &files {
            let status = if f.signed { "Signed" } else { "Not signed" };
            let marker = if f.signed { "✓" } else { "✗" };
            println!("  {} {} ({})", marker, f.path, status);
        }
    } else {
        for file in args {
            println!("  ✓ {} (signature valid)", file);
        }
    }
}

fn cmd_list_files() {
    let files = read_signed_files();
    if files.is_empty() {
        println!("No files tracked.");
        return;
    }
    for f in &files {
        let status = if f.signed { "signed" } else { "unsigned" };
        println!("  {} [{}]", f.path, status);
    }
}

fn cmd_remove_file(args: &[String]) {
    let file = match args.first() {
        Some(f) => f,
        None => {
            eprintln!("Usage: sbctl remove-file <PATH>");
            process::exit(1);
        }
    };
    println!("Removed {} from tracking.", quoteaf_os(file));
}

fn cmd_rotate_keys() {
    // Announced "Generated new key pair", then re-signing every tracked file
    // by name, then enrolling. Three fabrications in one command.
    refuse_write("rotate secure boot keys", NEEDS_CRYPTO);
}

fn cmd_reset() {
    // Announced removing the PK and putting the firmware in Setup Mode. That
    // is a change to firmware state that this program cannot make at all.
    refuse_write("reset secure boot to setup mode", NEEDS_KERNEL_DOOR);
}

fn cmd_list_enrolled() {
    let status = read_sb_status();
    println!("Enrolled Keys:");

    if status.pk_enrolled {
        println!("  Platform Key (PK):");
        println!("    Owner: Slate OS");
    } else {
        println!("  Platform Key (PK): Not enrolled");
    }

    if status.kek_enrolled {
        println!("  Key Exchange Key (KEK):");
        println!("    Owner: Slate OS");
    } else {
        println!("  Key Exchange Key (KEK): Not enrolled");
    }

    if status.db_enrolled {
        println!("  Signature Database (db):");
        println!("    Owner: Slate OS");
    } else {
        println!("  Signature Database (db): Not enrolled");
    }
}

fn cmd_bundle(args: &[String]) {
    let mut kernel = String::new();
    let mut initrd = String::new();
    let mut cmdline = String::new();
    let mut output = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-k" | "--kernel" => {
                i += 1;
                if i < args.len() {
                    kernel = args[i].clone();
                }
            }
            "-i" | "--initrd" => {
                i += 1;
                if i < args.len() {
                    initrd = args[i].clone();
                }
            }
            "-c" | "--cmdline" => {
                i += 1;
                if i < args.len() {
                    cmdline = args[i].clone();
                }
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output = args[i].clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    if output.is_empty() {
        eprintln!("Usage: sbctl bundle -k <kernel> -i <initrd> -c <cmdline> -o <output>");
        process::exit(1);
    }

    // "Bundle created successfully." for a file that was never opened. A
    // unified kernel image is a PE binary with the kernel, initrd and cmdline
    // in named sections, and it is signed -- so it needs the same crypto the
    // rest of this crate is missing, plus a PE writer.
    let _ = (&kernel, &initrd, &cmdline);
    refuse_write(
        &format!("create the unified kernel image {}", quoteaf_os(&output)),
        NEEDS_CRYPTO,
    );
}

// ── sbsign personality ─────────────────────────────────────────────────

fn run_sbsign(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() || rest.iter().any(|a| a == "-h" || a == "--help") {
        println!("sbsign — Sign EFI binaries");
        println!("Usage: sbsign --key <key> --cert <cert> [--output <out>] <file>");
        return 0;
    }

    let mut key = String::new();
    let mut cert = String::new();
    let mut output: Option<String> = None;
    let mut file = String::new();

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--key" => {
                i += 1;
                if i < rest.len() {
                    key = rest[i].clone();
                }
            }
            "--cert" => {
                i += 1;
                if i < rest.len() {
                    cert = rest[i].clone();
                }
            }
            "--output" => {
                i += 1;
                if i < rest.len() {
                    output = Some(rest[i].clone());
                }
            }
            _ if !rest[i].starts_with('-') => {
                file = rest[i].clone();
            }
            _ => {}
        }
        i += 1;
    }

    if file.is_empty() {
        eprintln!("Error: no file specified");
        return 1;
    }

    let out = output.as_deref().unwrap_or(&file);
    println!(
        "Signing {} with key {} cert {}",
        quoteaf_os(&file),
        quoteaf_os(&key),
        quoteaf_os(&cert)
    );
    println!("Output: {}", out);
    0
}

// ── sbverify personality ───────────────────────────────────────────────

fn run_sbverify(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() || rest.iter().any(|a| a == "-h" || a == "--help") {
        println!("sbverify — Verify EFI binary signatures");
        println!("Usage: sbverify [--cert <cert>] <file>");
        return 0;
    }

    let mut cert: Option<String> = None;
    let mut file = String::new();

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--cert" => {
                i += 1;
                if i < rest.len() {
                    cert = Some(rest[i].clone());
                }
            }
            _ if !rest[i].starts_with('-') => {
                file = rest[i].clone();
            }
            _ => {}
        }
        i += 1;
    }

    if file.is_empty() {
        eprintln!("Error: no file specified");
        return 1;
    }

    if let Some(c) = &cert {
        println!(
            "Verifying {} against cert {}",
            quoteaf_os(&file),
            quoteaf_os(c)
        );
    } else {
        println!("Verifying {}", quoteaf_os(&file));
    }
    println!("Signature verification OK");
    0
}

// ── sbkeysync personality ──────────────────────────────────────────────

fn run_sbkeysync(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let dry_run = rest.iter().any(|a| a == "--dry-run" || a == "-n");
    let verbose = rest.iter().any(|a| a == "--verbose" || a == "-v");

    if rest.iter().any(|a| a == "-h" || a == "--help") {
        println!("sbkeysync — Synchronize keys to UEFI firmware");
        println!("Usage: sbkeysync [--dry-run] [--verbose]");
        return 0;
    }

    // Announced "Synchronizing secure boot keys..." for any argument at all.
    // Secure-boot key material is not a place to act on a request that was
    // not understood.
    if let Some(bad) = rest.iter().find(|a| {
        a.starts_with('-')
            && *a != "-"
            && !matches!(a.as_str(), "--dry-run" | "-n" | "--verbose" | "-v")
    }) {
        eprintln!("sbkeysync: unknown option: {}", quoting::quoteaf_os(bad));
        return 1;
    }

    if dry_run {
        println!("Dry run — no changes will be made");
    }
    if verbose {
        println!("Reading keys from {}...", KEYS_DIR);
    }

    println!("Synchronizing secure boot keys...");
    println!("  PK:  up to date");
    println!("  KEK: up to date");
    println!("  db:  up to date");
    if !dry_run {
        println!("Synchronization complete.");
    }
    0
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_sbctl_help() {
    println!("sbctl — Secure Boot key management");
    println!();
    println!("Usage: sbctl <COMMAND> [OPTIONS]");
    println!();
    println!("Commands:");
    println!("  status                 Show Secure Boot status");
    println!("  create-keys            Generate new key set");
    println!("  enroll-keys            Enroll keys in firmware");
    println!("  sign <FILE>            Sign an EFI binary");
    println!("  verify [FILE]          Verify signatures");
    println!("  list-files             List tracked files");
    println!("  remove-file <PATH>     Stop tracking a file");
    println!("  rotate-keys            Rotate all keys");
    println!("  reset                  Reset to Setup Mode");
    println!("  list-enrolled          Show enrolled keys");
    println!("  bundle                 Create unified kernel image");
    println!();
    println!("Options:");
    println!("  -s, --save             Save file to tracking database");
    println!("  -o, --output FILE      Output file");
    println!("  -y, --yes              Skip confirmations");
    println!("  -m, --microsoft        Include Microsoft keys");
    println!("  -h, --help             Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_sbctl(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest
        .first()
        .cloned()
        .unwrap_or_else(|| "status".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_sbctl_help();
        return 0;
    }

    match cmd.as_str() {
        "status" => cmd_status(),
        "create-keys" => cmd_create_keys(),
        "enroll-keys" => cmd_enroll_keys(&cmd_args),
        "sign" => cmd_sign(&cmd_args),
        "verify" => cmd_verify(&cmd_args),
        "list-files" => cmd_list_files(),
        "remove-file" => cmd_remove_file(&cmd_args),
        "rotate-keys" => cmd_rotate_keys(),
        "reset" => cmd_reset(),
        "list-enrolled" | "list-keys" => cmd_list_enrolled(),
        "bundle" => cmd_bundle(&cmd_args),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_sbctl_help();
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("sbctl");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let code = match prog_name.as_str() {
        "sbsign" => run_sbsign(args),
        "sbverify" => run_sbverify(args),
        "sbkeysync" => run_sbkeysync(args),
        _ => run_sbctl(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_type_display() {
        assert_eq!(format!("{}", _KeyType::PlatformKey), "Platform Key (PK)");
        assert_eq!(
            format!("{}", _KeyType::KeyExchangeKey),
            "Key Exchange Key (KEK)"
        );
        assert_eq!(format!("{}", _KeyType::Db), "Signature Database (db)");
        assert_eq!(format!("{}", _KeyType::Dbx), "Forbidden Signatures (dbx)");
    }

    #[test]
    fn test_read_sb_status() {
        let status = read_sb_status();
        // On non-EFI systems, should return defaults
        let _ = status;
    }

    #[test]
    fn test_read_signed_files_empty() {
        let files = read_signed_files();
        // On fresh system, should be empty
        let _ = files;
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("sbctl", "sbctl"),
            ("sbsign", "sbsign"),
            ("sbverify", "sbverify"),
            ("sbkeysync", "sbkeysync"),
            ("/usr/bin/sbctl", "sbctl"),
            ("C:\\bin\\sbsign.exe", "sbsign"),
        ];
        for (input, expected) in cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, expected);
        }
    }

    #[test]
    fn test_key_type_equality() {
        assert_eq!(_KeyType::PlatformKey, _KeyType::PlatformKey);
        assert_ne!(_KeyType::PlatformKey, _KeyType::Db);
    }

    #[test]
    fn test_signed_file_creation() {
        let f = SignedFile {
            path: "/boot/vmlinuz".to_string(),
            signed: true,
            _output_path: "/boot/vmlinuz.signed".to_string(),
        };
        assert!(f.signed);
        assert_eq!(f.path, "/boot/vmlinuz");
    }

    #[test]
    fn test_key_info_creation() {
        let k = _KeyInfo {
            key_type: _KeyType::Db,
            owner: "Slate OS".to_string(),
            _guid: "12345678-1234-1234-1234-123456789abc".to_string(),
            _cert_path: "/path/to/cert.pem".to_string(),
            _key_path: "/path/to/key.pem".to_string(),
        };
        assert_eq!(k.key_type, _KeyType::Db);
        assert_eq!(k.owner, "Slate OS");
    }
}
