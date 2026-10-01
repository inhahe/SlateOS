//! Tests for `sbctl`: the parse of the kernel's key store, the firmware's
//! variables read from a directory of fixtures, and the text each command
//! prints.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests unwrap and index what they built"
)]

use super::*;

/// `/proc/secureboot` as `kernel/src/fs/procfs.rs`'s `gen_secureboot` writes
/// it: `  {:<4} {:<5} {:<28} {}` per key.
const STORE: &str = "=== Secure Boot ===
key_count: 3
record_count: 0
Keys:
  1    PK    OS Vendor PK                 SHA256:aabb
  2    KEK   OS Vendor KEK                SHA256:ccdd
  3    db    Kernel Signing Key           SHA256:eeff
total_verified: 4
total_rejected: 1
ops: 9
";

fn key(id: &str, kind: &str, subject: &str, fingerprint: &str) -> Key {
    Key {
        id: id.into(),
        kind: kind.into(),
        subject: subject.into(),
        fingerprint: fingerprint.into(),
    }
}

#[test]
fn the_kernels_table_is_read_with_its_subjects_whole() {
    assert_eq!(
        parse_keys(STORE),
        vec![
            key("1", "PK", "OS Vendor PK", "SHA256:aabb"),
            key("2", "KEK", "OS Vendor KEK", "SHA256:ccdd"),
            key("3", "db", "Kernel Signing Key", "SHA256:eeff"),
        ]
    );
}

#[test]
fn only_the_keys_table_holds_keys() {
    // Indented lines outside the table, and a row too short to have a
    // subject, are not keys.
    let text =
        "  9 PK stray SHA256:00\nKeys:\n  1 PK SHA256:only\nops: 1\n  2 db after SHA256:11\n";
    assert!(parse_keys(text).is_empty());
    assert!(parse_keys("").is_empty());
    assert!(parse_keys("Keys:\n").is_empty());
}

#[test]
fn a_long_subject_that_overruns_its_column_still_parses() {
    let text = "Keys:\n  12   dbx   A subject longer than twenty-eight columns SHA256:ff\n";
    assert_eq!(
        parse_keys(text),
        vec![key(
            "12",
            "dbx",
            "A subject longer than twenty-eight columns",
            "SHA256:ff"
        )]
    );
}

/// A directory of firmware variables: attributes, then the value.
fn efivars(vars: &[(&str, &[u8])]) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sbctl-efivars-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, value) in vars {
        let mut data = vec![0x06, 0, 0, 0];
        data.extend_from_slice(value);
        std::fs::write(dir.join(format!("{name}-{EFI_GLOBAL}")), data).unwrap();
    }
    dir
}

#[test]
fn the_firmwares_flags_are_read_from_its_variables() {
    let dir = efivars(&[("SecureBoot", &[1]), ("SetupMode", &[0])]);
    assert_eq!(
        read_firmware(dir.to_str().unwrap()),
        Firmware::Known {
            secure_boot: true,
            setup_mode: false
        }
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_or_malformed_variable_is_unreadable_not_guessed() {
    let dir = efivars(&[("SecureBoot", &[1])]);
    assert_eq!(read_firmware(dir.to_str().unwrap()), Firmware::Unreadable);
    std::fs::remove_dir_all(&dir).unwrap();

    let dir = efivars(&[("SecureBoot", &[1, 0]), ("SetupMode", &[0])]);
    assert_eq!(read_firmware(dir.to_str().unwrap()), Firmware::Unreadable);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_variables_directory_is_not_visible() {
    let dir = std::env::temp_dir().join("sbctl-no-such-efivars-dir");
    assert_eq!(read_firmware(dir.to_str().unwrap()), Firmware::NotVisible);
}

#[test]
fn status_says_what_it_read_and_what_it_could_not() {
    let keys = parse_keys(STORE);
    assert_eq!(
        status_text(
            Firmware::Known {
                secure_boot: true,
                setup_mode: false
            },
            Some(&keys)
        ),
        "Secure Boot: Enabled\nSetup Mode:  Disabled\nKernel keys: 3, 1 PK, 1 KEK, 1 db, 0 dbx\n"
    );
    let unknown = status_text(Firmware::NotVisible, None);
    assert!(unknown.starts_with(
        "Secure Boot: unknown (the firmware's variables are not visible at /sys/firmware/efi/efivars)\n"
    ));
    assert!(unknown.ends_with("Setup Mode:  unknown\nKernel keys: unknown\n"));
    // Nothing is reported as enabled when it could not be read.
    assert!(!unknown.contains("Enabled"));
    assert!(!status_text(Firmware::Unreadable, Some(&[])).contains("Enabled"));
}

#[test]
fn list_enrolled_prints_the_kernels_keys() {
    let keys = parse_keys(STORE);
    assert_eq!(
        list_text(&keys),
        "ID    TYPE  SUBJECT                       FINGERPRINT\n\
         1     PK    OS Vendor PK                  SHA256:aabb\n\
         2     KEK   OS Vendor KEK                 SHA256:ccdd\n\
         3     db    Kernel Signing Key            SHA256:eeff\n"
    );
    assert_eq!(
        list_text(&[]),
        "No keys are enrolled in the kernel's key store.\n"
    );
}

#[test]
fn the_help_offers_no_deleted_command() {
    for gone in [
        "create-keys",
        "sign",
        "verify",
        "list-files",
        "remove-file",
        "rotate-keys",
        "bundle",
        "--save",
        "--output",
    ] {
        assert!(!HELP.contains(gone), "{gone} is still in the help");
    }
}
