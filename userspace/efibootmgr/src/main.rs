//! Slate OS EFI boot manager.
//!
//! Multi-personality binary providing:
//! - **efibootmgr** — report the machine's UEFI boot configuration
//! - **efivar** — list the EFI variables that exist
//!
//! Both read `/sys/firmware/efi/efivars/`.
//!
//! # This reports the boot configuration; it cannot change it
//!
//! `-c/--create`, `-B/--delete-bootnum`, `-a/--active`, `-A/--inactive`,
//! `-n/--bootnext`, `-o/--bootorder` and `-t/--timeout` used to be accepted
//! here. Every one of them edited an in-memory `Vec`, printed it and exited 0:
//! there is no `fs::write`, no `File::create` and no `OpenOptions` anywhere in
//! this crate, and there never was. `efibootmgr -c -L Slate` printed
//! `efibootmgr: created Boot0003` having written nothing at all, and the next
//! run showed no such entry.
//!
//! They are gone under design-decisions.md §1006 — a command that does not
//! work is deleted, not kept as a stub that refuses — the same ruling that
//! removed `acl`'s `setfacl` for printing "removing all ACL entries from
//! <path>" with zero writes in 793 lines. Reinstating them needs efivarfs
//! writes, which need the immutable attribute cleared first
//! (`FS_IOC_SETFLAGS`); `posix` does not expose that today. See
//! `TD-B-EFIBOOTMGR-MODIFIES-NOTHING`.
//!
//! # Absent is not the same as could-not-be-read
//!
//! Every variable this reads can be missing, unreadable or malformed, and the
//! three lead to different conclusions about a machine that will not boot. A
//! reader that folds them together — which `read_efi_var(..).unwrap_or_default()`
//! did — reports a machine with no boot configuration, which is a specific and
//! alarming claim, when the truth may be that it could not look.

#![deny(clippy::all)]

use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::process;

use quoting::quoteaf_os;

const VERSION: &str = "0.1.0";
const EFIVARS_DIR: &str = "/sys/firmware/efi/efivars";
const EFI_GLOBAL_GUID: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// `LOAD_OPTION_ACTIVE` — the entry is eligible to be booted.
const LOAD_OPTION_ACTIVE: u32 = 1;

/// Options that claimed to change the boot configuration and wrote nothing.
///
/// Spelled once: the help text is generated from this list and the parser
/// refuses from this list, so the sentence and the behaviour cannot drift
/// apart. Re-adding one to the parser without the write support behind it
/// fails `every_removed_option_is_refused_with_the_reason`.
const REMOVED_OPTIONS: &[&str] = &[
    "-c",
    "--create",
    "-B",
    "--delete-bootnum",
    "-a",
    "--active",
    "-A",
    "--inactive",
    "-b",
    "--bootnum",
    "-L",
    "--label",
    "-l",
    "--loader",
    "-d",
    "--disk",
    "-p",
    "--part",
    "-n",
    "--bootnext",
    "-N",
    "--delete-bootnext",
    "-o",
    "--bootorder",
    "-O",
    "--delete-bootorder",
    "-t",
    "--timeout",
    "-T",
    "--delete-timeout",
    "-u",
    "--unicode",
];

// ============================================================================
// EFI variables
// ============================================================================

/// Why an EFI variable could not be produced.
///
/// The variants are kept apart deliberately. "This machine has no boot order"
/// and "this machine's boot order could not be read" are different facts, and
/// only one of them is about the machine.
#[derive(Debug)]
enum VarError {
    /// No such variable. For most of these that is a real answer.
    Absent,
    /// The variable is there and could not be read.
    Unreadable(String),
    /// Too short for what it must contain.
    TooShort { got: usize, want: usize },
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => write!(f, "is not set on this machine"),
            Self::Unreadable(e) => write!(f, "cannot be read: {e}"),
            Self::TooShort { got, want } => {
                write!(f, "is {got} bytes where at least {want} were needed")
            }
        }
    }
}

/// Little-endian `u16` from the first two bytes.
///
/// `None` when there are not two, rather than treating a missing byte as
/// zero: a `BootCurrent` of `0000` is a boot entry number, so inventing one
/// from a truncated variable names an entry the firmware never chose.
fn le_u16(b: &[u8]) -> Option<u16> {
    match (b.first(), b.get(1)) {
        (Some(&lo), Some(&hi)) => Some(u16::from_le_bytes([lo, hi])),
        _ => None,
    }
}

/// Drop the four attribute bytes every efivarfs file begins with.
///
/// The test is `>= 4`, not `> 4`. A variable whose *value* is empty is exactly
/// four bytes long and is a perfectly real variable — `BootOrder` with nothing
/// in it is that file — and the old `> 4` reported it as though it did not
/// exist.
fn strip_attributes(data: &[u8]) -> Result<&[u8], VarError> {
    data.get(4..).ok_or(VarError::TooShort {
        got: data.len(),
        want: 4,
    })
}

fn read_efi_var(name: &str) -> Result<Vec<u8>, VarError> {
    let path = format!("{EFIVARS_DIR}/{name}-{EFI_GLOBAL_GUID}");
    match fs::read(&path) {
        Ok(data) => strip_attributes(&data).map(<[u8]>::to_vec),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(VarError::Absent),
        Err(e) => Err(VarError::Unreadable(e.to_string())),
    }
}

/// Read a variable holding a single little-endian `u16` — `BootCurrent`,
/// `BootNext`, `Timeout`.
fn read_u16_var(name: &str) -> Result<u16, VarError> {
    let data = read_efi_var(name)?;
    le_u16(&data).ok_or(VarError::TooShort {
        got: data.len(),
        want: 2,
    })
}

/// `BootOrder` is a packed little-endian `u16` array.
///
/// Returns the numbers and whether a trailing odd byte was left over. That
/// byte is the first half of an entry number, so a truncated variable is a
/// boot order with a silently missing entry — worth saying rather than
/// dropping.
fn parse_boot_order(data: &[u8]) -> (Vec<u16>, bool) {
    let nums = data.chunks_exact(2).filter_map(le_u16).collect();
    (nums, !data.len().is_multiple_of(2))
}

/// The boot number a `Boot####-<guid>` variable name refers to.
///
/// `BootOrder`, `BootCurrent` and `BootNext` share the prefix and must not
/// match; `BootNext` in particular is four characters after the prefix, so the
/// length test alone is not enough and the hex parse is what rejects it.
fn boot_number_from_var_name(name: &str) -> Option<u16> {
    let rest = name.strip_prefix("Boot")?;
    let hex = rest.strip_suffix(&format!("-{EFI_GLOBAL_GUID}"))?;
    if hex.len() != 4 {
        return None;
    }
    u16::from_str_radix(hex, 16).ok()
}

// ============================================================================
// Boot entries
// ============================================================================

/// A boot entry as a `Boot####` variable records it.
#[derive(Debug)]
struct BootEntry {
    num: u16,
    active: bool,
    label: String,
    /// Bytes of EFI device path the variable declares after the label. The
    /// path is not decoded into `HD(1,GPT,…)/File(…)` form — see
    /// `print_boot_entries`.
    path_bytes: usize,
    /// Set when fewer bytes follow the label than `path_bytes` declares.
    path_truncated: bool,
}

/// Decode the UCS-2 description starting at `start`, returning it and the
/// offset of the byte after its terminator.
///
/// `char::decode_utf16` over the whole run rather than `char::from_u32` per
/// code unit: a label containing any character outside the basic plane arrives
/// as a surrogate *pair*, and `from_u32` rejects each half on its own, so the
/// old code dropped such characters silently and produced a shorter label that
/// looked deliberate. A code unit that is still not decodable becomes U+FFFD,
/// which is visible; this is a firmware display string, not a path.
fn decode_label(data: &[u8], start: usize) -> (String, usize) {
    let mut units: Vec<u16> = Vec::new();
    let mut offset = start;
    while let Some(u) = data.get(offset..offset.saturating_add(2)).and_then(le_u16) {
        offset = offset.saturating_add(2);
        if u == 0 {
            break;
        }
        units.push(u);
    }
    let label = char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    (label, offset)
}

/// Parse a `Boot####` variable: attributes (4 bytes), device-path length
/// (2 bytes), UCS-2 description, then the device path itself.
fn parse_boot_entry(num: u16, data: &[u8]) -> Result<BootEntry, VarError> {
    let short = VarError::TooShort {
        got: data.len(),
        want: 6,
    };
    let attrs: [u8; 4] = match data.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok()) {
        Some(a) => a,
        None => return Err(short),
    };
    let Some(path_bytes) = data.get(4..6).and_then(le_u16) else {
        return Err(short);
    };
    let path_bytes = usize::from(path_bytes);

    let (label, after) = decode_label(data, 6);
    let available = data.len().saturating_sub(after);

    Ok(BootEntry {
        num,
        active: u32::from_le_bytes(attrs) & LOAD_OPTION_ACTIVE != 0,
        label,
        path_bytes,
        path_truncated: path_bytes > available,
    })
}

fn read_boot_entry(num: u16) -> Result<BootEntry, VarError> {
    let data = read_efi_var(&format!("Boot{num:04X}"))?;
    parse_boot_entry(num, &data)
}

/// Everything we could learn about the machine's boot entries.
struct BootState {
    entries: Vec<BootEntry>,
    /// Entries we could not produce, worded for their context: a variable
    /// named by `BootOrder` that does not exist means something different
    /// from one that failed to parse.
    problems: Vec<String>,
    /// Set when the efivars directory itself could not be listed, in which
    /// case "there are no entries outside the boot order" is not known.
    scan_error: Option<String>,
}

/// Every `Boot####` variable: those named by `BootOrder` first, in that order,
/// then any the boot order does not mention.
fn read_boot_entries(order: &[u16]) -> BootState {
    let mut entries: Vec<BootEntry> = Vec::new();
    let mut problems: Vec<String> = Vec::new();

    for &num in order {
        match read_boot_entry(num) {
            Ok(e) => entries.push(e),
            // A dangling boot order is worth naming: the firmware skips the
            // number and boots something else, which is exactly the symptom
            // somebody would be running this to explain.
            Err(VarError::Absent) => problems.push(format!(
                "BootOrder names Boot{num:04X} and there is no such variable; \
                 the firmware will skip it"
            )),
            Err(e) => problems.push(format!("Boot{num:04X} {e}")),
        }
    }

    let scan_error = match fs::read_dir(EFIVARS_DIR) {
        Ok(dir) => {
            for entry in dir.flatten() {
                let name = entry.file_name();
                // A variable name that is not UTF-8 is not a `Boot####-<guid>`
                // name, so skipping it loses nothing. Converting it lossily to
                // find out would be inventing the name to test it.
                let Some(name) = name.to_str() else { continue };
                let Some(num) = boot_number_from_var_name(name) else {
                    continue;
                };
                if entries.iter().any(|e| e.num == num) {
                    continue;
                }
                match read_boot_entry(num) {
                    Ok(e) => entries.push(e),
                    Err(e) => problems.push(format!("Boot{num:04X} {e}")),
                }
            }
            None
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(format!(
            "EFI variables are not available on this system ({EFIVARS_DIR} does not exist)"
        )),
        Err(e) => Some(format!(
            "cannot list {EFIVARS_DIR} ({e}); any boot entry not named by BootOrder \
             is missing from the list below"
        )),
    };

    BootState {
        entries,
        problems,
        scan_error,
    }
}

// ============================================================================
// Output
// ============================================================================

/// What the header lines describe, each `None` only when the variable really
/// is not set — a variable that could not be read is reported to stderr by
/// [`note`] before it becomes `None`.
struct Header {
    current: Option<u16>,
    next: Option<u16>,
    timeout: Option<u16>,
    order: Vec<u16>,
}

fn print_boot_entries(
    out: &mut impl Write,
    entries: &[BootEntry],
    header: &Header,
    verbose: bool,
) -> io::Result<()> {
    // `BootCurrent` and `Timeout` used to be printed as the fixed strings
    // "BootCurrent: 0000" and "Timeout: 3 seconds" without either variable
    // being read. They are read now, and a machine that does not set one gets
    // no line rather than a plausible wrong one.
    if let Some(n) = header.current {
        writeln!(out, "BootCurrent: {n:04X}")?;
    }
    if let Some(n) = header.next {
        writeln!(out, "BootNext: {n:04X}")?;
    }
    if let Some(n) = header.timeout {
        writeln!(out, "Timeout: {n} seconds")?;
    }
    if !header.order.is_empty() {
        let order: Vec<String> = header.order.iter().map(|n| format!("{n:04X}")).collect();
        writeln!(out, "BootOrder: {}", order.join(","))?;
    }

    for entry in entries {
        let active = if entry.active { '*' } else { ' ' };
        write!(out, "Boot{:04X}{active} {}", entry.num, entry.label)?;
        // The device path is not decoded, so it is described rather than
        // rendered. Printing a placeholder in the column where upstream prints
        // `HD(1,GPT,…)/File(\EFI\…)` would read as a path nobody can act on.
        if verbose {
            write!(
                out,
                "\tdevice path: {} bytes, not decoded",
                entry.path_bytes
            )?;
            if entry.path_truncated {
                write!(out, " (TRUNCATED: fewer bytes follow the label than that)")?;
            }
        } else if entry.path_truncated {
            write!(out, "\t(device path truncated)")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

// ============================================================================
// CLI
// ============================================================================

/// Reduce a variable we wanted to an `Option`, reporting the difference.
///
/// `None` means "this machine does not set one", which is only an honest thing
/// to return after an unreadable variable has been printed as unreadable.
fn note(prog: &str, name: &str, v: Result<u16, VarError>) -> Option<u16> {
    match v {
        Ok(n) => Some(n),
        Err(VarError::Absent) => None,
        Err(e) => {
            eprintln!("{prog}: {name} {e}");
            None
        }
    }
}

enum Action {
    List { verbose: bool },
    Help,
    Version,
}

/// The message for an option that used to modify.
fn removed_message(arg: &str) -> String {
    format!(
        "{} claimed to change the boot configuration and never wrote anything — \
         it edited an in-memory copy, printed it and exited 0. It was removed rather \
         than left to look as though it worked. This efibootmgr reports the boot \
         configuration; it cannot change it.",
        quoteaf_os(arg)
    )
}

fn parse_args(args: &[String]) -> Result<Action, String> {
    let mut verbose = false;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "-v" | "--verbose" => verbose = true,
            // This arm used to be `_ => {}`. `efibootmgr -c -L Slate` fell
            // straight through it, printed a boot list and exited 0, which
            // reads as success — the same lie as the fabricated
            // "created Boot0003" with the sentence removed.
            other if REMOVED_OPTIONS.contains(&other) => return Err(removed_message(other)),
            other => {
                return Err(format!(
                    "unrecognized argument {}; try --help",
                    quoteaf_os(other)
                ));
            }
        }
    }
    Ok(Action::List { verbose })
}

fn efibootmgr_help() -> String {
    let removed: Vec<&str> = REMOVED_OPTIONS
        .iter()
        .filter(|o| o.starts_with("--"))
        .copied()
        .collect();
    format!(
        "Usage: efibootmgr [-v]\n\
         \n\
         Report the machine's UEFI boot configuration, read from\n\
         {EFIVARS_DIR}/.\n\
         \n\
         Options:\n\
         \x20 -v, --verbose   Also describe each entry's EFI device path\n\
         \x20 -h, --help      Show this help\n\
         \x20 -V, --version   Show version\n\
         \n\
         This build cannot change the boot configuration, and does not accept\n\
         options that say otherwise. These were removed because they wrote\n\
         nothing while reporting that they had:\n\
         \x20 {}\n",
        removed.join(", ")
    )
}

fn cmd_efibootmgr(args: &[String]) -> i32 {
    let verbose = match parse_args(args) {
        Ok(Action::Help) => {
            print!("{}", efibootmgr_help());
            return 0;
        }
        Ok(Action::Version) => {
            println!("efibootmgr {VERSION}");
            return 0;
        }
        Ok(Action::List { verbose }) => verbose,
        Err(msg) => {
            eprintln!("efibootmgr: {msg}");
            return 2;
        }
    };

    // An absent BootOrder is a real answer — a machine can have boot entries
    // and no order — but an unreadable one is not, and printing the entries
    // with no BootOrder line would say the machine has no boot order at all.
    let order = match read_efi_var("BootOrder") {
        Ok(data) => {
            let (nums, truncated) = parse_boot_order(&data);
            if truncated {
                eprintln!(
                    "efibootmgr: BootOrder ends in a half entry number; it is truncated \
                     and the order below is missing whatever entry that byte began"
                );
            }
            nums
        }
        Err(VarError::Absent) => Vec::new(),
        Err(e) => {
            eprintln!(
                "efibootmgr: BootOrder {e}; the entries below are in the order they were \
                 found, which is not the machine's boot order"
            );
            Vec::new()
        }
    };

    let state = read_boot_entries(&order);
    for p in &state.problems {
        eprintln!("efibootmgr: {p}");
    }
    if let Some(e) = &state.scan_error {
        eprintln!("efibootmgr: {e}");
    }

    // NO EFI MEANS NO BOOT ENTRIES, AND SAYING SO.
    //
    // This used to substitute two invented ones — "Slate OS" at
    // HD(1,GPT)/EFI/slateos/bootx64.efi and "UEFI Shell" at
    // HD(1,GPT)/EFI/Shell/Shell.efi — with a BootOrder of 0000,0001 to match,
    // and print them as the machine's boot configuration. Somebody debugging
    // why a machine will not boot would have read that and concluded the
    // entries were fine. A test asserted the labels, which is a test
    // certifying that the program fabricates.
    if state.entries.is_empty() {
        // When the scan failed we do not know that there are none, and the
        // reason was already printed above.
        if state.scan_error.is_none() {
            eprintln!("efibootmgr: no boot entries are defined in {EFIVARS_DIR}");
        }
        return 1;
    }

    let header = Header {
        current: note("efibootmgr", "BootCurrent", read_u16_var("BootCurrent")),
        next: note("efibootmgr", "BootNext", read_u16_var("BootNext")),
        timeout: note("efibootmgr", "Timeout", read_u16_var("Timeout")),
        order,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if let Err(e) = print_boot_entries(&mut out, &state.entries, &header, verbose) {
        eprintln!("efibootmgr: cannot write the boot configuration: {e}");
        return 1;
    }
    0
}

const EFIVAR_HELP: &str = "Usage: efivar [-l]\n\
     \n\
     List the EFI variables this system exposes.\n\
     \n\
     Options:\n\
     \x20 -l, --list      List EFI variables (the default)\n\
     \x20 -h, --help      Show this help\n\
     \x20 -V, --version   Show version\n";

fn list_variables() -> i32 {
    let dir = match fs::read_dir(EFIVARS_DIR) {
        Ok(d) => d,
        // This used to print "EFI variables are not supported on this system."
        // to STDOUT and exit 0, so a caller counting lines got one and a
        // caller checking the status got success.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!(
                "efivar: EFI variables are not available on this system \
                 ({EFIVARS_DIR} does not exist)"
            );
            return 1;
        }
        Err(e) => {
            eprintln!("efivar: cannot list {EFIVARS_DIR}: {e}");
            return 1;
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut status = 0;
    for entry in dir.flatten() {
        let name = entry.file_name();
        match name.to_str() {
            Some(s) => {
                if let Err(e) = writeln!(out, "{s}") {
                    eprintln!("efivar: cannot write: {e}");
                    return 1;
                }
            }
            // Printing it lossily would name a variable that does not exist,
            // and the caller would then fail to read it for no stated reason.
            None => {
                eprintln!("efivar: a variable name is not valid UTF-8 and was not listed");
                status = 1;
            }
        }
    }
    status
}

fn cmd_efivar(args: &[String]) -> i32 {
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{EFIVAR_HELP}");
                return 0;
            }
            "-V" | "--version" => {
                println!("efivar {VERSION}");
                return 0;
            }
            // Listing is all this does, so `-l` is accepted and is the default.
            "-l" | "--list" => {}
            other => {
                eprintln!(
                    "efivar: unrecognized argument {}; try --help",
                    quoteaf_os(other)
                );
                return 2;
            }
        }
    }
    list_variables()
}

/// The last path component of `argv[0]`, without a `.exe` suffix.
fn base_name(argv0: &str) -> &str {
    let mut last_sep = 0;
    for (i, &b) in argv0.as_bytes().iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i.saturating_add(1);
        }
    }
    let base = argv0.get(last_sep..).unwrap_or(argv0);
    base.strip_suffix(".exe").unwrap_or(base)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = base_name(args.first().map_or("efibootmgr", String::as_str)).to_string();
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "efivar" => cmd_efivar(&rest),
        _ => cmd_efibootmgr(&rest),
    };
    process::exit(code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn every_removed_option_is_refused_with_the_reason() {
        // The help names these and the parser refuses them, both from
        // REMOVED_OPTIONS. Re-adding one to the parser without efivarfs writes
        // behind it fails here.
        for opt in REMOVED_OPTIONS {
            let err = parse_args(&args(&[opt]))
                .err()
                .unwrap_or_else(|| panic!("{opt} is accepted again"));
            assert!(err.contains("never wrote anything"), "{opt}: {err}");
        }
    }

    #[test]
    fn an_unknown_argument_is_an_error_not_silence() {
        // The old parser's `_ => {}` printed a boot list and exited 0 for
        // anything it did not know, including every option above.
        assert!(parse_args(&args(&["-Z"])).is_err());
        assert!(parse_args(&args(&["Boot0001"])).is_err());
    }

    #[test]
    fn the_options_that_remain_are_accepted() {
        assert!(matches!(
            parse_args(&args(&["-v"])),
            Ok(Action::List { verbose: true })
        ));
        assert!(matches!(
            parse_args(&args(&[])),
            Ok(Action::List { verbose: false })
        ));
        assert!(matches!(parse_args(&args(&["--help"])), Ok(Action::Help)));
        assert!(matches!(parse_args(&args(&["-V"])), Ok(Action::Version)));
    }

    #[test]
    fn the_help_lists_what_was_removed() {
        let h = efibootmgr_help();
        assert!(h.contains("--create"), "{h}");
        assert!(h.contains("--bootorder"), "{h}");
        assert!(h.contains("cannot change the boot configuration"), "{h}");
    }

    #[test]
    fn an_empty_value_is_a_variable_not_a_missing_one() {
        // Four bytes is attributes and no value. `BootOrder` with nothing in
        // it is exactly this file, and the old `> 4` reported it as absent.
        assert_eq!(strip_attributes(&[1, 0, 0, 0]).ok(), Some(&[][..]));
        assert!(matches!(
            strip_attributes(&[1, 0, 0]),
            Err(VarError::TooShort { got: 3, want: 4 })
        ));
    }

    #[test]
    fn le_u16_reports_absence_rather_than_zero() {
        // A zero and a missing byte must not read the same: BootCurrent 0000
        // is a boot entry number.
        assert_eq!(le_u16(&[0, 0]), Some(0));
        assert_eq!(le_u16(&[7]), None);
        assert_eq!(le_u16(&[]), None);
        assert_eq!(le_u16(&[0x34, 0x12]), Some(0x1234));
    }

    #[test]
    fn boot_order_parses_little_endian_and_flags_a_half_entry() {
        assert_eq!(parse_boot_order(&[]), (vec![], false));
        assert_eq!(parse_boot_order(&[1, 0, 3, 0]), (vec![1, 3], false));
        // The trailing byte is the low half of an entry number.
        assert_eq!(parse_boot_order(&[1, 0, 3]), (vec![1], true));
    }

    #[test]
    fn boot_number_matches_entries_and_not_the_other_boot_variables() {
        let g = EFI_GLOBAL_GUID;
        assert_eq!(boot_number_from_var_name(&format!("Boot0003-{g}")), Some(3));
        assert_eq!(
            boot_number_from_var_name(&format!("BootFFFF-{g}")),
            Some(0xFFFF)
        );
        // `BootNext` is four characters after the prefix, so only the hex
        // parse rejects it.
        assert_eq!(boot_number_from_var_name(&format!("BootNext-{g}")), None);
        assert_eq!(boot_number_from_var_name(&format!("BootOrder-{g}")), None);
        assert_eq!(boot_number_from_var_name(&format!("BootCurrent-{g}")), None);
        assert_eq!(boot_number_from_var_name("Boot0003-not-the-guid"), None);
        assert_eq!(boot_number_from_var_name(&format!("Timeout-{g}")), None);
    }

    /// A `Boot####` value: attributes, device path length, UCS-2 label, path.
    fn boot_var(attrs: u32, label: &str, path: &[u8]) -> Vec<u8> {
        let mut v = attrs.to_le_bytes().to_vec();
        let path_len = u16::try_from(path.len()).unwrap_or(u16::MAX);
        v.extend_from_slice(&path_len.to_le_bytes());
        for u in label.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v.extend_from_slice(&[0, 0]);
        v.extend_from_slice(path);
        v
    }

    #[test]
    fn a_label_outside_the_basic_plane_survives() {
        // The old decoder called `char::from_u32` on each UCS-2 code unit, so
        // both halves of a surrogate pair were rejected and the character
        // vanished without trace.
        let data = boot_var(1, "Slate \u{1F680} OS", &[0xAA; 4]);
        let e = parse_boot_entry(3, &data).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(e.label, "Slate \u{1F680} OS");
        assert_eq!(e.num, 3);
        assert!(e.active);
        assert_eq!(e.path_bytes, 4);
        assert!(!e.path_truncated);
    }

    #[test]
    fn the_label_stops_at_its_terminator() {
        let data = boot_var(0, "Debian", &[0xBB; 2]);
        let e = parse_boot_entry(1, &data).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(e.label, "Debian");
        // Attributes without LOAD_OPTION_ACTIVE mean the firmware skips it.
        assert!(!e.active);
    }

    #[test]
    fn a_declared_path_that_is_not_there_is_reported() {
        // The length field is firmware-supplied and the variable can be
        // truncated; claiming 64 bytes of device path with none following is
        // the case that makes an entry unbootable.
        let mut data = boot_var(1, "Trunc", &[]);
        if let Some(b) = data.get_mut(4..6) {
            b.copy_from_slice(&64u16.to_le_bytes());
        }
        let e = parse_boot_entry(2, &data).unwrap_or_else(|e| panic!("{e}"));
        assert!(e.path_truncated);
    }

    #[test]
    fn a_variable_too_short_to_have_a_header_is_refused() {
        assert!(matches!(
            parse_boot_entry(0, &[1, 0, 0, 0, 5]),
            Err(VarError::TooShort { got: 5, want: 6 })
        ));
    }

    #[test]
    fn a_missing_variable_is_absent_not_unreadable() {
        assert!(matches!(
            read_efi_var("NoSuchVariableExistsHere"),
            Err(VarError::Absent)
        ));
    }

    #[test]
    fn the_header_omits_what_the_machine_does_not_set() {
        // The fixed "BootCurrent: 0000" and "Timeout: 3 seconds" were printed
        // whether or not either variable existed. A machine that sets neither
        // must get neither line.
        let entries = vec![BootEntry {
            num: 1,
            active: true,
            label: "Slate OS".to_string(),
            path_bytes: 76,
            path_truncated: false,
        }];
        let header = Header {
            current: None,
            next: None,
            timeout: None,
            order: vec![1],
        };
        let mut buf: Vec<u8> = Vec::new();
        print_boot_entries(&mut buf, &entries, &header, false).unwrap_or_else(|e| panic!("{e}"));
        let s = String::from_utf8(buf).unwrap_or_else(|e| panic!("{e}"));
        assert!(!s.contains("BootCurrent"), "{s}");
        assert!(!s.contains("Timeout"), "{s}");
        assert!(s.contains("BootOrder: 0001"), "{s}");
        assert!(s.contains("Boot0001* Slate OS"), "{s}");
        // Nothing that looks like a device path in the non-verbose listing.
        assert!(!s.contains("76"), "{s}");
    }

    #[test]
    fn verbose_describes_the_device_path_rather_than_inventing_one() {
        let entries = vec![BootEntry {
            num: 2,
            active: false,
            label: "UEFI Shell".to_string(),
            path_bytes: 76,
            path_truncated: false,
        }];
        let header = Header {
            current: Some(2),
            next: None,
            timeout: Some(5),
            order: vec![],
        };
        let mut buf: Vec<u8> = Vec::new();
        print_boot_entries(&mut buf, &entries, &header, true).unwrap_or_else(|e| panic!("{e}"));
        let s = String::from_utf8(buf).unwrap_or_else(|e| panic!("{e}"));
        assert!(s.contains("BootCurrent: 0002"), "{s}");
        assert!(s.contains("Timeout: 5 seconds"), "{s}");
        assert!(s.contains("76 bytes, not decoded"), "{s}");
        assert!(
            !s.contains("HD("),
            "must not render a path it cannot decode: {s}"
        );
        // No BootOrder line when the machine has no boot order.
        assert!(!s.contains("BootOrder"), "{s}");
    }

    #[test]
    fn base_name_strips_the_directory_and_the_exe_suffix() {
        assert_eq!(base_name("/usr/bin/efivar"), "efivar");
        assert_eq!(base_name("efibootmgr"), "efibootmgr");
        assert_eq!(base_name("target/debug/efivar.exe"), "efivar");
    }

    #[test]
    fn efivar_refuses_an_unknown_argument() {
        assert_eq!(cmd_efivar(&args(&["--frobnicate"])), 2);
        assert_eq!(cmd_efivar(&args(&["--help"])), 0);
    }
}
