//! uname — print system information.
//!
//! # Where the values come from, and why not from here
//!
//! The previous version carried its own copies of the answers:
//!
//! ```ignore
//! const SYSNAME: &str = "Slate OS";
//! const RELEASE: &str = "0.1.0";
//! ```
//!
//! Every one of them disagreed with the kernel. At the time of this rewrite
//! **four** different components each held a private answer to "what is this
//! system called", and no two matched:
//!
//! | Source | Said |
//! |---|---|
//! | `sys_uname` and `/proc/sys/kernel/ostype` (kernel) | `Linux` |
//! | `posix`'s `uname()` (`posix/src/utsname.rs`) | `CustomOS` |
//! | `/sys/kernel/ostype` (kernel sysfs) | `MintOS` |
//! | this command | `Slate OS` |
//!
//! That is what a compiled-in copy of somebody else's value always decays
//! into. So this version does not have one: it *reads*
//! `/proc/sys/kernel/ostype`, `.../osrelease`, `.../version` and
//! `.../hostname` — the files the kernel itself serves — and can therefore
//! only agree with `uname(2)`. The constants below are fallbacks for a system
//! whose `/proc` is not mounted, and are set to the kernel's own values so
//! that even the fallback path cannot introduce a fifth answer.
//!
//! # Why `uname -s` says `Linux`
//!
//! This is settled policy, not a choice made here. `kernel/src/syscall/linux.rs`
//! → `sys_uname` states it: *"sysname / release are Linux-ABI-only surfaces …
//! the ONLY callers of uname(2) are Linux binaries that expect Linux values.
//! Reporting `Linux` / `6.6.x` is therefore the faithful answer for this ABI,
//! not a lie about what we are — it tells a Linux program exactly which Linux
//! personality it is talking to."* It also has to be: the release string must
//! satisfy glibc's startup version gate, which prints "FATAL: kernel too old"
//! and refuses to run when the leading `MAJOR.MINOR` is below its build-time
//! minimum. See `roadmap-detailed.md` §72, "Version-surface policy".
//!
//! The system's own name is what `-o`/`--operating-system` is for — exactly
//! the distinction GNU draws when its `uname -s` says `Linux` and its
//! `uname -o` says `GNU/Linux`.
//!
//! Note also that `Slate OS` could never have been right for `-s` whatever the
//! name: it contains a space, and `uname -a` is routinely split on whitespace.
//! One field that is two words silently shifts every field after it.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::process;

use coreutils::errmsg::strerror;
use coreutils::quote::{os_bytes, quote};

/// Kernel name, matching `sys_uname`'s `sysname`.
const DEFAULT_KERNEL_NAME: &[u8] = b"Linux";

/// Kernel release, matching `sys_uname`. The leading `6.6.0` is what glibc's
/// startup gate parses; the suffix identifies the build.
const DEFAULT_KERNEL_RELEASE: &[u8] = b"6.6.0-slateos";

/// Kernel version, matching `sys_uname`.
const DEFAULT_KERNEL_VERSION: &[u8] = b"#1 SMP";

/// Machine hardware name. The only target we build.
const MACHINE: &[u8] = b"x86_64";

/// The operating system as distinct from the kernel ABI it presents.
///
/// One token, deliberately: `-o` is compared and split by scripts just as `-s`
/// is, so the display name `Slate OS` (correct in a window title) is wrong
/// here for the same reason it was wrong for `-s`.
const OPERATING_SYSTEM: &[u8] = b"SlateOS";

/// What GNU prints for a field it cannot determine, and what it omits from
/// `-a` on that basis.
const UNKNOWN: &[u8] = b"unknown";

const PROC_OSTYPE: &str = "/proc/sys/kernel/ostype";
const PROC_OSRELEASE: &str = "/proc/sys/kernel/osrelease";
const PROC_VERSION: &str = "/proc/sys/kernel/version";
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";
const ETC_HOSTNAME: &str = "/etc/hostname";

/// One piece of system information.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    KernelName,
    NodeName,
    KernelRelease,
    KernelVersion,
    Machine,
    Processor,
    HardwarePlatform,
    OperatingSystem,
}

/// The order `uname` prints fields in, which is fixed and independent of the
/// order the options were given: `uname -mrs` and `uname -srm` print the same
/// line. GNU behaves this way and scripts depend on it.
const PRINT_ORDER: [Field; 8] = [
    Field::KernelName,
    Field::NodeName,
    Field::KernelRelease,
    Field::KernelVersion,
    Field::Machine,
    Field::Processor,
    Field::HardwarePlatform,
    Field::OperatingSystem,
];

/// Which fields to print, and whether the selection came from `-a`.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Selection {
    fields: [bool; 8],
    /// `-a` omits `-p` and `-i` when they are unknown; naming them explicitly
    /// prints `unknown`. Tracking where the selection came from is the only
    /// way to tell those two cases apart.
    from_all: bool,
}

impl Selection {
    fn index(field: Field) -> usize {
        match field {
            Field::KernelName => 0,
            Field::NodeName => 1,
            Field::KernelRelease => 2,
            Field::KernelVersion => 3,
            Field::Machine => 4,
            Field::Processor => 5,
            Field::HardwarePlatform => 6,
            Field::OperatingSystem => 7,
        }
    }

    fn set(&mut self, field: Field) {
        if let Some(slot) = self.fields.get_mut(Self::index(field)) {
            *slot = true;
        }
    }

    fn has(self, field: Field) -> bool {
        self.fields
            .get(Self::index(field))
            .copied()
            .unwrap_or(false)
    }

    fn is_empty(self) -> bool {
        !self.fields.iter().any(|&f| f)
    }

    /// What `-a` selects. Assigning this over an existing selection loses
    /// nothing, because every field is already on; and `from_all` is meant to
    /// stick even when a field is *also* named explicitly. GNU decides whether
    /// to omit an unknown processor by testing "was `-a` given" and not "was
    /// `-p` given", so `uname -a -p` omits an unknown processor exactly as
    /// plain `uname -a` does.
    fn all() -> Self {
        Self {
            fields: [true; 8],
            from_all: true,
        }
    }
}

/// What the command line asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Request {
    Print(Selection),
    Help,
    Version,
}

// ============================================================================
// Command line
// ============================================================================

/// Parse the arguments after `argv[0]`.
fn parse_args(args: &[OsString]) -> Result<Request, String> {
    let mut selection = Selection::default();
    let mut end_of_options = false;

    for arg in args {
        let bytes = os_bytes(arg);

        if end_of_options || bytes.first() != Some(&b'-') || bytes.len() == 1 {
            return Err(format!(
                "extra operand {}\nTry 'uname --help' for more information.",
                quote(&bytes)
            ));
        }

        if bytes.starts_with(b"--") {
            if bytes.len() == 2 {
                end_of_options = true;
                continue;
            }
            let long = bytes.get(2..).unwrap_or_default();
            match long {
                b"help" => return Ok(Request::Help),
                b"version" => return Ok(Request::Version),
                b"all" => selection = Selection::all(),
                b"kernel-name" => selection.set(Field::KernelName),
                b"nodename" => selection.set(Field::NodeName),
                b"kernel-release" => selection.set(Field::KernelRelease),
                b"kernel-version" => selection.set(Field::KernelVersion),
                b"machine" => selection.set(Field::Machine),
                b"processor" => selection.set(Field::Processor),
                b"hardware-platform" => selection.set(Field::HardwarePlatform),
                b"operating-system" => selection.set(Field::OperatingSystem),
                _ => {
                    return Err(format!(
                        "unrecognized option {}\nTry 'uname --help' for more information.",
                        quote(&bytes)
                    ));
                }
            }
            continue;
        }

        for &c in bytes.get(1..).unwrap_or_default() {
            match c {
                b'a' => selection = Selection::all(),
                b's' => selection.set(Field::KernelName),
                b'n' => selection.set(Field::NodeName),
                b'r' => selection.set(Field::KernelRelease),
                b'v' => selection.set(Field::KernelVersion),
                b'm' => selection.set(Field::Machine),
                b'p' => selection.set(Field::Processor),
                b'i' => selection.set(Field::HardwarePlatform),
                b'o' => selection.set(Field::OperatingSystem),
                _ => {
                    return Err(format!(
                        "invalid option -- {}\nTry 'uname --help' for more information.",
                        quote(&[c])
                    ));
                }
            }
        }
    }

    Ok(Request::Print(selection))
}

// ============================================================================
// Values
// ============================================================================

/// The system's answers, read once.
#[derive(Clone, Debug, PartialEq, Eq)]
struct System {
    kernel_name: Vec<u8>,
    node_name: Vec<u8>,
    kernel_release: Vec<u8>,
    kernel_version: Vec<u8>,
    machine: Vec<u8>,
    processor: Vec<u8>,
    hardware_platform: Vec<u8>,
    operating_system: Vec<u8>,
}

/// Strip leading and trailing ASCII whitespace.
fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i.saturating_add(1));
    bytes.get(start..end).unwrap_or_default()
}

/// A file's trimmed contents, or `None` if it cannot be read or is blank.
fn read_trimmed(path: &str) -> Option<Vec<u8>> {
    let content = fs::read(path).ok()?;
    let trimmed = trim(&content);
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_vec())
    }
}

/// Read what the kernel reports, falling back to its own values.
fn read_system() -> System {
    System {
        kernel_name: read_trimmed(PROC_OSTYPE).unwrap_or_else(|| DEFAULT_KERNEL_NAME.to_vec()),
        // Same pair, same order, as the `hostname` command and `osh`'s
        // `$HOSTNAME`, so `uname -n` and `hostname` can never disagree.
        node_name: read_trimmed(PROC_HOSTNAME)
            .or_else(|| read_trimmed(ETC_HOSTNAME))
            .unwrap_or_else(|| UNKNOWN.to_vec()),
        kernel_release: read_trimmed(PROC_OSRELEASE)
            .unwrap_or_else(|| DEFAULT_KERNEL_RELEASE.to_vec()),
        kernel_version: read_trimmed(PROC_VERSION)
            .unwrap_or_else(|| DEFAULT_KERNEL_VERSION.to_vec()),
        machine: MACHINE.to_vec(),
        // GNU reports these as unknown on x86_64 Linux too; they exist for
        // compatibility with systems that distinguish them from `-m`.
        processor: UNKNOWN.to_vec(),
        hardware_platform: UNKNOWN.to_vec(),
        operating_system: OPERATING_SYSTEM.to_vec(),
    }
}

/// One field's value.
fn value_of(system: &System, field: Field) -> &[u8] {
    match field {
        Field::KernelName => &system.kernel_name,
        Field::NodeName => &system.node_name,
        Field::KernelRelease => &system.kernel_release,
        Field::KernelVersion => &system.kernel_version,
        Field::Machine => &system.machine,
        Field::Processor => &system.processor,
        Field::HardwarePlatform => &system.hardware_platform,
        Field::OperatingSystem => &system.operating_system,
    }
}

/// Build the output line.
///
/// With nothing selected this is `-s`, which is what plain `uname` means.
fn render(system: &System, selection: Selection) -> Vec<u8> {
    let mut effective = selection;
    if effective.is_empty() {
        effective.set(Field::KernelName);
    }

    let mut parts: Vec<&[u8]> = Vec::new();
    for &field in &PRINT_ORDER {
        if !effective.has(field) {
            continue;
        }
        let value = value_of(system, field);
        // `-a` omits an unknown processor or platform rather than printing
        // "unknown unknown" in the middle of the line; asking for them by
        // name still prints it, so a script probing `uname -p` gets an answer.
        if effective.from_all
            && matches!(field, Field::Processor | Field::HardwarePlatform)
            && value == UNKNOWN
        {
            continue;
        }
        parts.push(value);
    }
    parts.join(&b' ')
}

// ============================================================================
// Output
// ============================================================================

/// Write bytes and a newline, reporting a write failure instead of panicking.
///
/// `println!` panics when stdout cannot be written, so `uname -a | head -1`
/// could end in a panic message. A closed pipe is the one write error that
/// means success.
fn write_line(bytes: &[u8]) -> i32 {
    let mut out = io::stdout().lock();
    let result = out
        .write_all(bytes)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    match result {
        Ok(()) => 0,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("uname: write error: {}", strerror(&e));
            1
        }
    }
}

fn usage() -> &'static str {
    "Usage: uname [OPTION]...\n\
     Print certain system information.  With no OPTION, same as -s.\n\
     \n\
       -a, --all                  print all information, in the order below,\n\
                                    omitting -p and -i if they are unknown\n\
       -s, --kernel-name          print the kernel name\n\
       -n, --nodename             print the network node host name\n\
       -r, --kernel-release       print the kernel release\n\
       -v, --kernel-version       print the kernel version\n\
       -m, --machine              print the machine hardware name\n\
       -p, --processor            print the processor type\n\
       -i, --hardware-platform    print the hardware platform\n\
       -o, --operating-system     print the operating system\n\
           --help                 print this message\n\
           --version              print the version\n\
     \n\
     The kernel name, release, version and node name are read from\n\
     /proc/sys/kernel, so they always match uname(2)."
}

fn main() {
    // `args_os`, not `args`: `env::args()` unwraps `into_string()` and so
    // panics outright on an argument that is not UTF-8. An unusable option is
    // worth a diagnostic, not a crash.
    let args: Vec<OsString> = env::args_os().skip(1).collect();

    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("uname: {message}");
            process::exit(1);
        }
    };

    let status = match request {
        Request::Help => write_line(usage().as_bytes()),
        Request::Version => write_line(b"uname (SlateOS coreutils) 0.1.0"),
        Request::Print(selection) => write_line(&render(&read_system(), selection)),
    };
    process::exit(status);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn a(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn selection(items: &[&str]) -> Selection {
        match parse_args(&a(items)).unwrap() {
            Request::Print(s) => s,
            other => panic!("expected a Print, got {other:?}"),
        }
    }

    /// A system whose every field is distinguishable, so a test can tell
    /// which value landed in which column.
    fn sample() -> System {
        System {
            kernel_name: b"Linux".to_vec(),
            node_name: b"box1".to_vec(),
            kernel_release: b"6.6.0-slateos".to_vec(),
            kernel_version: b"#1 SMP".to_vec(),
            machine: b"x86_64".to_vec(),
            processor: b"unknown".to_vec(),
            hardware_platform: b"unknown".to_vec(),
            operating_system: b"SlateOS".to_vec(),
        }
    }

    fn line(items: &[&str]) -> String {
        String::from_utf8(render(&sample(), selection(items))).unwrap()
    }

    // ---------------- the missing options ----------------

    #[test]
    fn nodename_exists() {
        // `uname -n` is the portable way to ask for the host name and the old
        // version answered "unknown option: -n".
        assert_eq!(line(&["-n"]), "box1");
        assert_eq!(line(&["--nodename"]), "box1");
    }

    #[test]
    fn kernel_version_exists() {
        // POSIX requires -v, and -a is defined to include it.
        assert_eq!(line(&["-v"]), "#1 SMP");
    }

    #[test]
    fn every_posix_option_is_present() {
        for opt in ["-s", "-n", "-r", "-v", "-m", "-a"] {
            assert!(
                parse_args(&a(&[opt])).is_ok(),
                "POSIX requires {opt} and it was refused"
            );
        }
    }

    #[test]
    fn the_gnu_extensions_are_present_too() {
        for opt in ["-p", "-i", "-o"] {
            assert!(parse_args(&a(&[opt])).is_ok(), "{opt} was refused");
        }
    }

    #[test]
    fn help_and_version_are_options_not_errors() {
        assert_eq!(parse_args(&a(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&a(&["--version"])).unwrap(), Request::Version);
    }

    // ---------------- the values ----------------

    #[test]
    fn the_kernel_name_is_a_single_token() {
        // `Slate OS` had a space in it, and `uname -a` is routinely split on
        // whitespace: one field that is two words shifts every field after it.
        let name = read_system().kernel_name;
        assert!(
            !name.contains(&b' '),
            "kernel name {name:?} contains a space"
        );
    }

    #[test]
    fn no_field_of_a_full_line_contains_a_space_except_the_kernel_version() {
        // `#1 SMP` genuinely has one, exactly as on Linux, and is the reason
        // `uname -a` must be parsed by position rather than by splitting.
        let system = sample();
        for &field in &PRINT_ORDER {
            if field == Field::KernelVersion {
                continue;
            }
            let value = value_of(&system, field);
            assert!(
                !value.contains(&b' '),
                "{field:?} = {value:?} contains a space"
            );
        }
    }

    #[test]
    fn the_fallbacks_are_the_kernels_own_values() {
        // If these ever drift from `sys_uname`, a system without /proc gets a
        // different answer than one with it.
        assert_eq!(DEFAULT_KERNEL_NAME, b"Linux");
        assert_eq!(DEFAULT_KERNEL_RELEASE, b"6.6.0-slateos");
        assert_eq!(DEFAULT_KERNEL_VERSION, b"#1 SMP");
    }

    #[test]
    fn the_release_satisfies_glibcs_version_gate() {
        // glibc's __libc_start_main prints "FATAL: kernel too old" and refuses
        // to run if the leading MAJOR.MINOR is below its build-time minimum,
        // so the release must begin with a parseable integer triple.
        let release = String::from_utf8(DEFAULT_KERNEL_RELEASE.to_vec()).unwrap();
        let head: Vec<&str> = release
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .unwrap()
            .split('.')
            .collect();
        assert_eq!(head.len(), 3, "release {release} is not MAJOR.MINOR.PATCH");
        let major: u32 = head.first().unwrap().parse().unwrap();
        let minor: u32 = head.get(1).unwrap().parse().unwrap();
        assert!((major, minor) >= (6, 6), "release {release} is below 6.6");
    }

    // ---------------- rendering ----------------

    #[test]
    fn no_options_is_the_kernel_name() {
        assert_eq!(line(&[]), "Linux");
        assert_eq!(line(&["-s"]), "Linux");
    }

    #[test]
    fn the_field_order_is_fixed_regardless_of_option_order() {
        assert_eq!(line(&["-mrs"]), line(&["-srm"]));
        assert_eq!(line(&["-mrs"]), "Linux 6.6.0-slateos x86_64");
    }

    #[test]
    fn all_prints_every_field_in_order() {
        // -p and -i are unknown here, so -a omits them, as GNU does.
        assert_eq!(
            line(&["-a"]),
            "Linux box1 6.6.0-slateos #1 SMP x86_64 SlateOS"
        );
    }

    #[test]
    fn all_includes_processor_and_platform_when_they_are_known() {
        let mut system = sample();
        system.processor = b"amd64".to_vec();
        system.hardware_platform = b"pc".to_vec();
        let out = String::from_utf8(render(&system, selection(&["-a"]))).unwrap();
        assert_eq!(
            out,
            "Linux box1 6.6.0-slateos #1 SMP x86_64 amd64 pc SlateOS"
        );
    }

    #[test]
    fn naming_an_unknown_field_explicitly_still_prints_it() {
        // A script doing `test "$(uname -p)" = unknown` needs an answer, so
        // the omission is a property of -a and not of the field.
        assert_eq!(line(&["-p"]), "unknown");
        assert_eq!(line(&["-i"]), "unknown");
    }

    #[test]
    fn all_still_omits_an_unknown_processor_even_when_it_is_also_named() {
        // GNU tests "was -a given", not "was -p given": once -a appears, an
        // unknown processor is omitted however else it was asked for. Getting
        // this backwards would make `uname -a -p` print a stray "unknown" in
        // the middle of the line and shift every field after it.
        assert_eq!(line(&["-a", "-p"]), line(&["-a"]));
        assert_eq!(line(&["-p", "-a"]), line(&["-a"]));
        assert_eq!(line(&["-ap"]), line(&["-a"]));
    }

    #[test]
    fn a_field_named_twice_is_printed_once() {
        assert_eq!(line(&["-s", "-s"]), "Linux");
        assert_eq!(line(&["-ss"]), "Linux");
    }

    #[test]
    fn all_absorbs_the_other_options_whichever_side_they_are_on() {
        assert_eq!(line(&["-a", "-s"]), line(&["-a"]));
        assert_eq!(line(&["-s", "-a"]), line(&["-a"]));
    }

    #[test]
    fn the_operating_system_is_distinct_from_the_kernel_name() {
        // Exactly GNU's distinction: `uname -s` is Linux, `uname -o` is
        // GNU/Linux. Ours is the ABI we present versus the system we are.
        assert_ne!(line(&["-s"]), line(&["-o"]));
        assert_eq!(line(&["-o"]), "SlateOS");
    }

    // ---------------- errors ----------------

    #[test]
    fn an_unknown_option_is_refused() {
        assert!(
            parse_args(&a(&["-x"]))
                .unwrap_err()
                .contains("invalid option")
        );
        assert!(
            parse_args(&a(&["--frobnicate"]))
                .unwrap_err()
                .contains("unrecognized option")
        );
    }

    #[test]
    fn an_operand_is_refused() {
        let err = parse_args(&a(&["foo"])).unwrap_err();
        assert!(err.contains("extra operand"), "{err}");
    }

    #[test]
    fn a_bundle_with_a_bad_letter_is_refused() {
        assert!(
            parse_args(&a(&["-sx"]))
                .unwrap_err()
                .contains("invalid option")
        );
    }

    // ---------------- trimming ----------------

    #[test]
    fn values_read_from_proc_lose_their_trailing_newline() {
        // The kernel writes "Linux\n"; a `uname -s` that kept the newline
        // would break every `[ "$(uname -s)" = Linux ]` in the tree.
        assert_eq!(trim(b"Linux\n"), b"Linux");
        assert_eq!(trim(b"  6.6.0-slateos \r\n"), b"6.6.0-slateos");
        assert_eq!(trim(b""), b"");
    }
}
