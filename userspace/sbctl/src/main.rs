//! sbctl -- Secure Boot: what the firmware and the kernel hold, and the
//! key-management commands this system can honestly offer.
//!
//! # What it reads
//!
//! * **The kernel's key store**, `/proc/secureboot`: every key SlateOS's
//!   `fs::secureboot` holds -- its id, its type (`PK`, `KEK`, `db`, `dbx`), its
//!   subject and its fingerprint. On SlateOS that store, not a firmware
//!   variable, is what a key being *enrolled* means. What `sbctl` lists is
//!   what the kernel publishes, no more.
//! * **The firmware's own state**, where its variables are visible
//!   (`/sys/firmware/efi/efivars`, as Linux publishes them): whether Secure
//!   Boot is on and whether the firmware is in Setup Mode. Where they are not,
//!   `sbctl` says so. It used to report "Setup Mode: Enabled" on a machine
//!   with no EFI at all, and to call a key "Enrolled" when a directory of key
//!   files under `/usr/share/secureboot` was not empty -- which says nothing
//!   about what the firmware or the kernel will accept.
//!
//! # What it does not do, and why
//!
//! `design-decisions.md` §1049 (the operator's answer to B-Q17), applying
//! §1006: a command that does not work is deleted, not kept as a stub.
//!
//! * **Deleted, needing cryptography this project has not planned** (RSA,
//!   X.509, Authenticode): `create-keys`, `sign`, `rotate-keys` and `bundle`.
//!   With `sign` went the files database it kept -- `verify`, `list-files`
//!   and `remove-file` -- and the `sbsign`, `sbverify` and `sbkeysync`
//!   personalities, which printed "Signature verification OK" and
//!   "Synchronization complete." without reading a byte.
//! * **Refusing until the kernel's door lands:** `enroll-keys` and `reset`
//!   need a way for a program to write the kernel's key store, which lane A
//!   is adding (`requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md`,
//!   design-decisions §978 on lane A's side).

use quoting::{os_bytes, quoteaf};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, warnx};

#[cfg(test)]
mod tests;

/// The kernel's key store, as `fs::secureboot` publishes it.
const PROC_SECUREBOOT: &str = "/proc/secureboot";

/// Where Linux publishes the firmware's variables.
const EFIVARS: &str = "/sys/firmware/efi/efivars";

/// The EFI global variable GUID, under which `SecureBoot` and `SetupMode`
/// live (UEFI 2.x, section 3.3).
const EFI_GLOBAL: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// Why `enroll-keys` and `reset` cannot act yet.
const NEEDS_KERNEL_DOOR: &str = "userspace has no interface that writes the kernel's secure-boot key store yet; see requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md";

/// A fatal error, already printed: the status to exit with.
struct Fatal(u8);

/// One key from `/proc/secureboot`'s `Keys:` table.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    id: String,
    kind: String,
    subject: String,
    fingerprint: String,
}

/// Parse `/proc/secureboot`'s `Keys:` table, whose rows are
/// `  ID TYPE SUBJECT FINGERPRINT`: the id, the type and the fingerprint are
/// single words, and the subject is what lies between them.
fn parse_keys(text: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut in_keys = false;
    for line in text.lines() {
        if line == "Keys:" {
            in_keys = true;
            continue;
        }
        if !(in_keys && line.starts_with("  ")) {
            in_keys = false;
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        if let [id, kind, middle @ .., fingerprint] = words.as_slice()
            && !middle.is_empty()
        {
            keys.push(Key {
                id: (*id).to_string(),
                kind: (*kind).to_string(),
                subject: middle.join(" "),
                fingerprint: (*fingerprint).to_string(),
            });
        }
    }
    keys
}

/// A one-byte boolean firmware variable under the global GUID: the byte after
/// the variable's four attribute bytes. `None` when it cannot be read or is
/// not one byte.
fn efi_flag(dir: &str, name: &str) -> Option<bool> {
    let data = std::fs::read(format!("{dir}/{name}-{EFI_GLOBAL}")).ok()?;
    match data.get(4..)? {
        [b] => Some(*b == 1),
        _ => None,
    }
}

/// The firmware's state, or why it cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Firmware {
    /// `SecureBoot` and `SetupMode`, both read.
    Known { secure_boot: bool, setup_mode: bool },
    /// The variables directory is not there: not booted through UEFI, or a
    /// system that does not publish the variables.
    NotVisible,
    /// The directory is there and one of the two variables could not be read.
    Unreadable,
}

fn read_firmware(dir: &str) -> Firmware {
    if !std::path::Path::new(dir).is_dir() {
        return Firmware::NotVisible;
    }
    match (efi_flag(dir, "SecureBoot"), efi_flag(dir, "SetupMode")) {
        (Some(secure_boot), Some(setup_mode)) => Firmware::Known {
            secure_boot,
            setup_mode,
        },
        _ => Firmware::Unreadable,
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "Enabled" } else { "Disabled" }
}

/// `sbctl status`'s text, given the firmware's state and the kernel's keys
/// (`None` when the store could not be read).
fn status_text(firmware: Firmware, keys: Option<&[Key]>) -> String {
    let mut t = String::new();
    match firmware {
        Firmware::Known {
            secure_boot,
            setup_mode,
        } => {
            t.push_str(&format!("Secure Boot: {}\n", on_off(secure_boot)));
            t.push_str(&format!("Setup Mode:  {}\n", on_off(setup_mode)));
        }
        Firmware::NotVisible => {
            t.push_str(&format!(
                "Secure Boot: unknown (the firmware's variables are not visible at {EFIVARS})\n"
            ));
            t.push_str("Setup Mode:  unknown\n");
        }
        Firmware::Unreadable => {
            t.push_str(&format!(
                "Secure Boot: unknown (SecureBoot or SetupMode under {EFIVARS} could not be read)\n"
            ));
            t.push_str("Setup Mode:  unknown\n");
        }
    }
    match keys {
        Some(keys) => {
            t.push_str(&format!("Kernel keys: {}", keys.len()));
            for kind in ["PK", "KEK", "db", "dbx"] {
                let n = keys.iter().filter(|k| k.kind == kind).count();
                t.push_str(&format!(", {n} {kind}"));
            }
            t.push('\n');
        }
        None => t.push_str("Kernel keys: unknown\n"),
    }
    t
}

/// `sbctl list-enrolled`'s text.
fn list_text(keys: &[Key]) -> String {
    if keys.is_empty() {
        return String::from("No keys are enrolled in the kernel's key store.\n");
    }
    let mut t = String::from("ID    TYPE  SUBJECT                       FINGERPRINT\n");
    for k in keys {
        t.push_str(&format!(
            "{:<5} {:<5} {:<29} {}\n",
            k.id, k.kind, k.subject, k.fingerprint
        ));
    }
    t
}

/// The kernel's keys; on failure, the failure reported.
fn read_keys(short: &[u8]) -> Option<Vec<Key>> {
    match std::fs::read_to_string(PROC_SECUREBOOT) {
        Ok(text) => Some(parse_keys(&text)),
        Err(e) => {
            ulclosestream::warn(
                short,
                &format!("cannot read the kernel's key store {PROC_SECUREBOOT}"),
                &e,
            );
            None
        }
    }
}

/// A command that must write the kernel's key store, refused.
fn refuse(short: &[u8], action: &str) -> Fatal {
    warnx(short, &format!("cannot {action}: {NEEDS_KERNEL_DOOR}"));
    warnx(short, "nothing was changed");
    Fatal(1)
}

/// The help.
const HELP: &str = "\
sbctl -- Secure Boot status, and the keys the kernel holds

Usage: sbctl [COMMAND]

Commands:
  status                 Secure Boot state, and how many keys the kernel holds (the default)
  list-enrolled          List the keys the kernel holds
  enroll-keys [-m]       Enroll keys -- not yet: the kernel has no door for it
  reset                  Return to Setup Mode -- not yet: as enroll-keys

Options:
  -m, --microsoft        With enroll-keys: include Microsoft's keys
  -h, --help             Show this help
";

/// `program_invocation_short_name`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

fn run(argv: &[OsString], short: &[u8], out: &mut Stdout) -> Result<u8, Fatal> {
    let words: Vec<Vec<u8>> = argv
        .iter()
        .skip(1)
        .map(|a| os_bytes(a).into_owned())
        .collect();
    let (cmd, rest): (&[u8], &[Vec<u8>]) = match words.split_first() {
        Some((c, r)) => (c.as_slice(), r),
        None => (b"status", &[]),
    };
    match cmd {
        b"-h" | b"--help" => {
            out.write(HELP.as_bytes());
            Ok(0)
        }
        b"status" => {
            let keys = read_keys(short);
            out.write(status_text(read_firmware(EFIVARS), keys.as_deref()).as_bytes());
            Ok(if keys.is_some() { 0 } else { 1 })
        }
        b"list-enrolled" | b"list-keys" => {
            let keys = read_keys(short).ok_or(Fatal(1))?;
            out.write(list_text(&keys).as_bytes());
            Ok(0)
        }
        b"enroll-keys" => {
            if let Some(bad) = rest
                .iter()
                .find(|a| !matches!(a.as_slice(), b"-m" | b"--microsoft"))
            {
                warnx(
                    short,
                    &format!("enroll-keys: unknown argument {}", quoteaf(bad)),
                );
                return Err(Fatal(1));
            }
            Err(refuse(
                short,
                if rest.is_empty() {
                    "enroll keys"
                } else {
                    "enroll keys including Microsoft's"
                },
            ))
        }
        b"reset" => Err(refuse(short, "reset secure boot to setup mode")),
        other => {
            warnx(
                short,
                &format!("unknown command {}; see sbctl --help", quoteaf(other)),
            );
            Err(Fatal(1))
        }
    }
}

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("sbctl"), OsString::as_os_str),
    );
    let mut out = Stdout::new(1);
    let status = match run(&argv, &short, &mut out) {
        Ok(s) | Err(Fatal(s)) => s,
    };
    ExitCode::from(out.close(status, &short))
}
