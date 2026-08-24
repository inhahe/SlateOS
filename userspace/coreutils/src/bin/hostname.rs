//! hostname — show or set the system host name.
//!
//! # Why this is file-based rather than `gethostname()`/`sethostname()`
//!
//! The previous version of this utility called the C functions
//! `gethostname()` and `sethostname()` from `posix`. Both of those store the
//! name in a `process_global!`, which expands to a `static mut` inside the
//! calling program's own address space (`posix/src/unistd.rs`, "Hostname
//! storage"). Nothing about them crosses a process boundary. So:
//!
//! * `hostname` with no arguments printed the *initial value of that static*,
//!   which is the literal string `localhost`, no matter what the machine was
//!   actually called.
//! * `hostname newname` wrote `newname` into a variable belonging to the
//!   `hostname` process, and then the `hostname` process exited. The system
//!   name was untouched and the exit status was 0.
//!
//! Neither direction worked, and neither said so. The rest of the tree keeps
//! the host name in two files — `/proc/sys/kernel/hostname` (live) and
//! `/etc/hostname` (persistent) — and every other program that cares reads
//! them: `dhcpcd` writes both when a lease supplies a name, `getty` shows the
//! name in its login banner, `osh` fills `$HOSTNAME` from exactly this pair in
//! this order, and `sysctl` maps `kernel.hostname` onto the first one. This
//! utility now uses the same two files, so it agrees with all of them.
//!
//! The C functions are a separate defect and are tracked separately; see
//! `known-issues.md` → `B-POSIX-HOSTNAME-IS-PROCESS-LOCAL`.
//!
//! # Why there is no `#[cfg]` in this file
//!
//! Everything here is file I/O and byte manipulation, which compiles and runs
//! identically on the Windows development host — the paths simply do not exist
//! there, which the code already has to handle anyway, because they may not
//! exist on the real system either. The previous version put its entire
//! working body inside `#[cfg(target_os = "linux")]`, so `cargo test` on the
//! development host compiled *none* of it and the seven tests it did have all
//! tested one 4-line decoding helper. That is why nobody noticed that the
//! program did nothing.

use coreutils::diag;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::process;

use coreutils::errmsg::strerror;
use coreutils::quote::{os_bytes, quote, quote_os};

/// Live kernel host name. Written by `sysctl kernel.hostname` and by us.
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";

/// Persistent host name, read at boot. Written by us and by `dhcpcd`.
const ETC_HOSTNAME: &str = "/etc/hostname";

/// Resolver configuration, the source of the domain part for `-f` and `-d`.
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// Interface address table, the source for `-i` and `-I`.
const PROC_IF_INET: &str = "/proc/net/if_inet";

/// Per-interface network directory, the fallback source for `-i` and `-I`.
const SYS_NET_DIR: &str = "/sys/class/net";

/// Maximum total host name length, RFC 1123 §2.1.
const MAX_HOSTNAME_LEN: usize = 253;

/// Maximum length of one dot-separated label, RFC 1123 §2.1.
const MAX_LABEL_LEN: usize = 63;

/// Which piece of information a display invocation asks for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Query {
    /// The name as stored.
    Full,
    /// Up to the first dot.
    Short,
    /// The fully qualified name.
    Fqdn,
    /// Everything after the first dot of the fully qualified name.
    Domain,
    /// Addresses belonging to this host.
    Ip,
    /// Addresses on every interface.
    AllIp,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Action {
    /// Print something.
    Show(Query),
    /// Set the name to a literal.
    Set { name: OsString, boot: bool },
    /// Set the name to the first meaningful line of a file.
    SetFromFile { path: OsString, boot: bool },
    /// Print the usage message.
    Help,
    /// Print the version banner.
    Version,
}

/// Bytes back to an `OsString` without going through UTF-8.
///
/// Exact on `cfg(unix)`, which includes our own target — `x86_64-slateos.json`
/// declares `"target-family": ["unix"]`. On the Windows development host the
/// round trip is lossy, because `OsString` there is UTF-16 and there is no
/// byte constructor. That only affects host test runs, and no test relies on a
/// non-UTF-8 byte surviving the trip.
#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(b).to_os_string()
}

#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

// ============================================================================
// Command line
// ============================================================================

/// Parse the arguments after `argv[0]`.
///
/// Short options bundle (`-sf` is `-s -f`), and for the display options the
/// last one wins — GNU's `hostname` assigns to one variable in its `getopt`
/// loop and acts on it afterwards, so that is the behaviour scripts see.
/// `-h` and `-V` are answered where they appear, as they are in GNU.
///
/// `--` ends the options, which the standalone `userspace/hostname` cannot do:
/// without it there is no way to set a name beginning with a hyphen, and more
/// importantly no way to be *sure* an argument from a variable is treated as a
/// name rather than as an option.
fn parse_args(args: &[OsString]) -> Result<Action, String> {
    let mut query: Option<Query> = None;
    let mut boot = false;
    let mut file: Option<OsString> = None;
    let mut name: Option<OsString> = None;
    let mut end_of_options = false;

    let mut i = 0;
    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        let bytes = os_bytes(arg);

        // An operand: the name to set. `-` alone is an operand too, matching
        // every other utility; only a hyphen with something after it is an
        // option.
        if end_of_options || bytes.first() != Some(&b'-') || bytes.len() == 1 {
            if let Some(first) = &name {
                return Err(format!(
                    "too many arguments: already given {}, then {}",
                    quote_os(first),
                    quote_os(arg)
                ));
            }
            name = Some(arg.clone());
            i = i.saturating_add(1);
            continue;
        }

        if bytes.starts_with(b"--") {
            if bytes.len() == 2 {
                end_of_options = true;
                i = i.saturating_add(1);
                continue;
            }
            let long = bytes.get(2..).unwrap_or_default();
            match parse_long(long, args, i, &mut query, &mut boot, &mut file)? {
                Long::Answered(action) => return Ok(action),
                Long::Consumed(next) => i = next,
            }
            continue;
        }

        let body = bytes.get(1..).unwrap_or_default();
        match parse_shorts(body, args, i, &mut query, &mut boot, &mut file)? {
            Long::Answered(action) => return Ok(action),
            Long::Consumed(next) => i = next,
        }
    }

    resolve(query, boot, file, name)
}

/// The outcome of reading one option: either the whole command line is already
/// answered, or parsing continues at the returned index.
enum Long {
    Answered(Action),
    Consumed(usize),
}

/// Read one `--long` option.
fn parse_long(
    long: &[u8],
    args: &[OsString],
    i: usize,
    query: &mut Option<Query>,
    boot: &mut bool,
    file: &mut Option<OsString>,
) -> Result<Long, String> {
    // `--file=PATH` carries its argument; every other long option does not.
    if let Some(path) = long.strip_prefix(b"file=") {
        *file = Some(os_from_bytes(path));
        return Ok(Long::Consumed(i.saturating_add(1)));
    }

    match long {
        b"help" => return Ok(Long::Answered(Action::Help)),
        b"version" => return Ok(Long::Answered(Action::Version)),
        b"short" => *query = Some(Query::Short),
        b"fqdn" | b"long" => *query = Some(Query::Fqdn),
        b"domain" => *query = Some(Query::Domain),
        b"ip-address" => *query = Some(Query::Ip),
        b"all-ip-addresses" => *query = Some(Query::AllIp),
        b"boot" => *boot = true,
        b"file" => {
            let next = i.saturating_add(1);
            let Some(path) = args.get(next) else {
                return Err("option '--file' requires an argument".to_string());
            };
            *file = Some(path.clone());
            return Ok(Long::Consumed(next.saturating_add(1)));
        }
        _ => {
            let mut whole = b"--".to_vec();
            whole.extend_from_slice(long);
            return Err(format!(
                "unrecognized option {}\nTry 'hostname --help' for more information.",
                quote(&whole)
            ));
        }
    }
    Ok(Long::Consumed(i.saturating_add(1)))
}

/// Read a bundle of short options, e.g. the `sf` of `-sf`.
fn parse_shorts(
    body: &[u8],
    args: &[OsString],
    i: usize,
    query: &mut Option<Query>,
    boot: &mut bool,
    file: &mut Option<OsString>,
) -> Result<Long, String> {
    let mut j = 0;
    while j < body.len() {
        let Some(&c) = body.get(j) else { break };
        match c {
            b'h' => return Ok(Long::Answered(Action::Help)),
            b'V' => return Ok(Long::Answered(Action::Version)),
            b's' => *query = Some(Query::Short),
            b'f' => *query = Some(Query::Fqdn),
            b'd' => *query = Some(Query::Domain),
            b'i' => *query = Some(Query::Ip),
            b'I' => *query = Some(Query::AllIp),
            b'b' => *boot = true,
            b'F' => {
                // `-Fpath` carries the rest of the bundle; a bare `-F` takes
                // the next argument.
                let rest = body.get(j.saturating_add(1)..).unwrap_or_default();
                if rest.is_empty() {
                    let next = i.saturating_add(1);
                    let Some(path) = args.get(next) else {
                        return Err("option requires an argument -- 'F'".to_string());
                    };
                    *file = Some(path.clone());
                    return Ok(Long::Consumed(next.saturating_add(1)));
                }
                *file = Some(os_from_bytes(rest));
                return Ok(Long::Consumed(i.saturating_add(1)));
            }
            _ => {
                return Err(format!(
                    "invalid option -- {}\nTry 'hostname --help' for more information.",
                    quote(&[c])
                ));
            }
        }
        j = j.saturating_add(1);
    }
    Ok(Long::Consumed(i.saturating_add(1)))
}

/// Turn the accumulated flags into one action, rejecting the combinations that
/// contradict each other rather than silently preferring one.
fn resolve(
    query: Option<Query>,
    boot: bool,
    file: Option<OsString>,
    name: Option<OsString>,
) -> Result<Action, String> {
    if query.is_some() && (file.is_some() || name.is_some()) {
        return Err("cannot display and set the host name in the same command".to_string());
    }
    if file.is_some() && name.is_some() {
        return Err("cannot give both a host name and --file".to_string());
    }
    if let Some(path) = file {
        return Ok(Action::SetFromFile { path, boot });
    }
    if let Some(name) = name {
        return Ok(Action::Set { name, boot });
    }
    if boot {
        return Err("--boot requires a host name or --file".to_string());
    }
    Ok(Action::Show(query.unwrap_or(Query::Full)))
}

// ============================================================================
// Validation
// ============================================================================

/// Check a host name against RFC 952 as amended by RFC 1123 §2.1.
///
/// This works on bytes rather than on a `str`, which is what makes the utility
/// safe on an argument that is not valid UTF-8: such an argument contains a
/// byte ≥ 0x80, that byte is not alphanumeric ASCII, and so the *same* rule
/// that rejects `host name` with a space also rejects it — with a message,
/// rather than with the panic `env::args()` would have produced.
fn validate_hostname(name: &[u8]) -> Result<(), String> {
    if name.is_empty() {
        return Err("host name must not be empty".to_string());
    }
    if name.len() > MAX_HOSTNAME_LEN {
        return Err(format!(
            "host name is {} bytes, the maximum is {MAX_HOSTNAME_LEN}",
            name.len()
        ));
    }

    for label in name.split(|&b| b == b'.') {
        if label.is_empty() {
            return Err(format!(
                "host name {} has an empty label (a leading, trailing or doubled dot)",
                quote(name)
            ));
        }
        if label.len() > MAX_LABEL_LEN {
            return Err(format!(
                "label {} is {} bytes, the maximum is {MAX_LABEL_LEN}",
                quote(label),
                label.len()
            ));
        }
        if label.first() == Some(&b'-') || label.last() == Some(&b'-') {
            return Err(format!(
                "label {} must not start or end with a hyphen",
                quote(label)
            ));
        }
        for &b in label {
            if !b.is_ascii_alphanumeric() && b != b'-' {
                return Err(format!(
                    "invalid byte {} in label {} (only letters, digits and hyphens are allowed)",
                    quote(&[b]),
                    quote(label)
                ));
            }
        }
    }
    Ok(())
}

// ============================================================================
// Pure name arithmetic
// ============================================================================

/// Everything before the first dot.
fn short_of(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == b'.') {
        Some(i) => name.get(..i).unwrap_or(name),
        None => name,
    }
}

/// The fully qualified name: the stored name if it already has a dot,
/// otherwise the stored name joined to the resolver's domain.
fn fqdn_of(name: &[u8], domain: Option<&[u8]>) -> Vec<u8> {
    if name.contains(&b'.') {
        return name.to_vec();
    }
    match domain {
        Some(d) if !d.is_empty() => {
            let mut out = name.to_vec();
            out.push(b'.');
            out.extend_from_slice(d);
            out
        }
        _ => name.to_vec(),
    }
}

/// Everything after the first dot of the fully qualified name, or nothing.
fn domain_of(name: &[u8], domain: Option<&[u8]>) -> Vec<u8> {
    let fqdn = fqdn_of(name, domain);
    match fqdn.iter().position(|&b| b == b'.') {
        Some(i) => fqdn.get(i.saturating_add(1)..).unwrap_or_default().to_vec(),
        None => Vec::new(),
    }
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

/// The domain configured in `resolv.conf` content.
///
/// A `domain` directive wins over `search`; with only `search`, its first
/// entry is used. This is the resolver's own precedence.
fn parse_resolv_conf(content: &[u8]) -> Option<Vec<u8>> {
    let mut from_search: Option<Vec<u8>> = None;

    for line in content.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line.first() == Some(&b'#') || line.first() == Some(&b';') {
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"domain") {
            let rest = trim(rest);
            if !rest.is_empty() {
                return Some(rest.to_vec());
            }
        }
        if from_search.is_none()
            && let Some(rest) = line.strip_prefix(b"search")
            && let Some(first) = trim(rest).split(|b| b.is_ascii_whitespace()).next()
            && !first.is_empty()
        {
            from_search = Some(first.to_vec());
        }
    }
    from_search
}

/// Addresses from `/proc/net/if_inet`, skipping loopback.
///
/// Each line is `<interface> <address> <netmask> <flags>`.
fn parse_proc_if_inet(content: &[u8]) -> Vec<Vec<u8>> {
    let mut ips = Vec::new();
    for line in content.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let mut fields = line
            .split(|b| b.is_ascii_whitespace())
            .filter(|f| !f.is_empty());
        let _interface = fields.next();
        if let Some(addr) = fields.next()
            && !is_loopback(addr)
        {
            ips.push(addr.to_vec());
        }
    }
    ips
}

/// Whether an address is a loopback address, which `-i`/`-I` omit.
fn is_loopback(addr: &[u8]) -> bool {
    addr == b"::1" || addr.starts_with(b"127.")
}

/// The first line of a file that is neither blank nor a comment.
fn pick_first_meaningful_line(content: &[u8]) -> Option<Vec<u8>> {
    for line in content.split(|&b| b == b'\n') {
        let line = trim(line);
        if !line.is_empty() && line.first() != Some(&b'#') {
            return Some(line.to_vec());
        }
    }
    None
}

// ============================================================================
// File access
// ============================================================================

/// Read a file and trim it, or `None` if it cannot be read or is blank.
fn read_trimmed(path: &str) -> Option<Vec<u8>> {
    let content = fs::read(path).ok()?;
    let trimmed = trim(&content);
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_vec())
    }
}

/// The current host name: the live value if there is one, else the persistent
/// one. This is the same pair, in the same order, that `osh` uses to fill
/// `$HOSTNAME`, so the two never disagree.
fn read_hostname() -> Result<Vec<u8>, String> {
    if let Some(name) = read_trimmed(PROC_HOSTNAME) {
        return Ok(name);
    }
    if let Some(name) = read_trimmed(ETC_HOSTNAME) {
        return Ok(name);
    }
    Err(format!(
        "cannot determine the host name: neither {PROC_HOSTNAME} nor {ETC_HOSTNAME} could be read"
    ))
}

/// The resolver's domain, if one is configured.
fn read_domain() -> Option<Vec<u8>> {
    let content = fs::read(RESOLV_CONF).ok()?;
    parse_resolv_conf(&content)
}

/// Addresses on every interface: the address table if it has any, otherwise a
/// scan of the per-interface directories.
fn read_addresses() -> Vec<Vec<u8>> {
    if let Ok(content) = fs::read(PROC_IF_INET) {
        let ips = parse_proc_if_inet(&content);
        if !ips.is_empty() {
            return ips;
        }
    }
    scan_interface_dir()
}

/// Read `<SYS_NET_DIR>/*/address`, skipping loopback and all-zero addresses.
fn scan_interface_dir() -> Vec<Vec<u8>> {
    let mut addrs = Vec::new();
    let Ok(entries) = fs::read_dir(Path::new(SYS_NET_DIR)) else {
        return addrs;
    };
    for entry in entries.flatten() {
        if entry.file_name() == OsStr::new("lo") {
            continue;
        }
        let Ok(content) = fs::read(entry.path().join("address")) else {
            continue;
        };
        let addr = trim(&content);
        if !addr.is_empty() && addr != b"00:00:00:00:00:00" && !is_loopback(addr) {
            addrs.push(addr.to_vec());
        }
    }
    addrs
}

/// Write the name to the live parameter and to the persistent file.
///
/// The persistent file is replaced by writing a sibling and renaming over it,
/// so a crash part-way through leaves the old name rather than half of the new
/// one — a truncated `/etc/hostname` is read at boot as a *different* valid
/// name, which is worse than not having been changed.
fn set_hostname(name: &[u8]) -> Result<(), String> {
    validate_hostname(name)?;

    if Path::new(PROC_HOSTNAME).exists() {
        fs::write(PROC_HOSTNAME, name).map_err(|e| write_error(PROC_HOSTNAME, &e))?;
    }

    let temp = Path::new(ETC_HOSTNAME)
        .parent()
        .unwrap_or_else(|| Path::new("/etc"))
        .join(".hostname.tmp");

    let mut content = name.to_vec();
    content.push(b'\n');

    fs::write(&temp, &content).map_err(|e| write_error(&temp.to_string_lossy(), &e))?;

    fs::rename(&temp, ETC_HOSTNAME).map_err(|e| {
        // Best effort: the rename already failed, so the temporary file is
        // litter either way, and a failure to remove it must not replace the
        // real diagnostic with a less useful one.
        drop(fs::remove_file(&temp));
        format!(
            "cannot replace {ETC_HOSTNAME}: {}{}",
            strerror(&e),
            root_hint(&e)
        )
    })?;

    Ok(())
}

/// A write failure, naming the file and saying what actually went wrong
/// instead of "failed to set hostname".
fn write_error(path: &str, e: &io::Error) -> String {
    format!("cannot write {path}: {}{}", strerror(e), root_hint(e))
}

/// The one hint worth adding: a permission failure here almost always means
/// the command was not run as root, and saying so saves a support round trip.
fn root_hint(e: &io::Error) -> &'static str {
    if e.kind() == ErrorKind::PermissionDenied {
        " (setting the host name requires root)"
    } else {
        ""
    }
}

// ============================================================================
// Output
// ============================================================================

/// Write bytes and a newline to standard output, reporting a write failure.
///
/// `println!` panics when standard output cannot be written, which turns
/// `hostname | head -1` into a panic message. A closed pipe is the one write
/// error that means success.
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
            diag!("hostname: write error: {}", strerror(&e));
            1
        }
    }
}

/// The usage message.
fn usage() -> &'static str {
    "Usage: hostname [OPTION]... [NAME]\n\
     Show or set the system host name.\n\
     \n\
     With no argument, print the host name.  With NAME, set it.\n\
     \n\
       -s, --short              print up to the first dot\n\
       -f, --fqdn, --long       print the fully qualified name\n\
       -d, --domain             print the domain part of the qualified name\n\
       -i, --ip-address         print the addresses for this host\n\
       -I, --all-ip-addresses   print the addresses on every interface\n\
       -F, --file FILE          read the name to set from FILE\n\
       -b, --boot               only set the name if none is set yet\n\
       -h, --help               print this message\n\
       -V, --version            print the version\n\
     \n\
     The name is read from /proc/sys/kernel/hostname, falling back to\n\
     /etc/hostname; setting it writes both."
}

// ============================================================================
// Driver
// ============================================================================

/// Carry out a display request.
fn show(query: &Query) -> Result<i32, String> {
    match *query {
        Query::Ip | Query::AllIp => {
            let addrs = read_addresses();
            if addrs.is_empty() {
                return Err("no addresses found".to_string());
            }
            let joined = addrs.join(&b' ');
            Ok(write_line(&joined))
        }
        Query::Full => Ok(write_line(&read_hostname()?)),
        Query::Short => Ok(write_line(short_of(&read_hostname()?))),
        Query::Fqdn => {
            let name = read_hostname()?;
            Ok(write_line(&fqdn_of(&name, read_domain().as_deref())))
        }
        Query::Domain => {
            let name = read_hostname()?;
            Ok(write_line(&domain_of(&name, read_domain().as_deref())))
        }
    }
}

/// Carry out a set request, honouring `--boot`.
fn set(name: &[u8], boot: bool) -> Result<i32, String> {
    if boot && read_hostname().is_ok() {
        // `--boot` means "only if nothing has set one yet", so an existing
        // name is the expected case and not an error.
        return Ok(0);
    }
    set_hostname(name)?;
    Ok(0)
}

fn run(action: &Action) -> Result<i32, String> {
    match *action {
        Action::Help => Ok(write_line(usage().as_bytes())),
        Action::Version => Ok(write_line(b"hostname (SlateOS coreutils) 0.1.0")),
        Action::Show(ref query) => show(query),
        Action::Set { ref name, boot } => set(&os_bytes(name), boot),
        Action::SetFromFile { ref path, boot } => {
            let content = fs::read(path)
                .map_err(|e| format!("cannot read {}: {}", quote_os(path), strerror(&e)))?;
            let name = pick_first_meaningful_line(&content).ok_or_else(|| {
                format!(
                    "no host name in {}: the file is empty or entirely comments",
                    quote_os(path)
                )
            })?;
            set(&name, boot)
        }
    }
}

fn main() {
    // `args_os`, not `args`: `env::args()` unwraps `into_string()` and so
    // panics on an argument that is not UTF-8. A host name that is not UTF-8
    // is invalid, but the right answer to it is a diagnostic, not a crash.
    let args: Vec<OsString> = env::args_os().skip(1).collect();

    let action = match parse_args(&args) {
        Ok(action) => action,
        Err(message) => {
            diag!("hostname: {message}");
            process::exit(1);
        }
    };

    match run(&action) {
        Ok(status) => process::exit(status),
        Err(message) => {
            diag!("hostname: {message}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn a(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn parsed(items: &[&str]) -> Action {
        parse_args(&a(items)).unwrap()
    }

    // ---------------- the bug that started this ----------------

    #[test]
    fn an_option_is_not_a_new_host_name() {
        // The whole bug in one assertion. The previous version had no option
        // parsing at all: anything that was not absent was a name to set, so
        // `hostname --help` renamed the machine to `--help`.
        assert_eq!(parsed(&["--help"]), Action::Help);
        assert_eq!(parsed(&["-h"]), Action::Help);
        assert_eq!(parsed(&["-V"]), Action::Version);
        assert_eq!(parsed(&["-s"]), Action::Show(Query::Short));
    }

    #[test]
    fn a_name_after_a_double_dash_is_a_name_even_if_it_looks_like_an_option() {
        assert_eq!(
            parsed(&["--", "-s"]),
            Action::Set {
                name: OsString::from("-s"),
                boot: false
            }
        );
    }

    #[test]
    fn an_unknown_option_is_refused_rather_than_installed_as_the_host_name() {
        let err = parse_args(&a(&["--frobnicate"])).unwrap_err();
        assert!(err.contains("unrecognized option"), "{err}");
        let err = parse_args(&a(&["-Z"])).unwrap_err();
        assert!(err.contains("invalid option"), "{err}");
    }

    // ---------------- parsing ----------------

    #[test]
    fn no_arguments_shows_the_full_name() {
        assert_eq!(parsed(&[]), Action::Show(Query::Full));
    }

    #[test]
    fn every_display_option_has_a_long_and_a_short_spelling() {
        assert_eq!(parsed(&["-s"]), parsed(&["--short"]));
        assert_eq!(parsed(&["-f"]), parsed(&["--fqdn"]));
        assert_eq!(parsed(&["--long"]), parsed(&["--fqdn"]));
        assert_eq!(parsed(&["-d"]), parsed(&["--domain"]));
        assert_eq!(parsed(&["-i"]), parsed(&["--ip-address"]));
        assert_eq!(parsed(&["-I"]), parsed(&["--all-ip-addresses"]));
    }

    #[test]
    fn short_options_bundle_and_the_last_display_option_wins() {
        // GNU's getopt loop assigns to one variable, so `-sf` is `-f`.
        assert_eq!(parsed(&["-sf"]), Action::Show(Query::Fqdn));
        assert_eq!(parsed(&["-fs"]), Action::Show(Query::Short));
    }

    #[test]
    fn a_bare_name_is_a_set() {
        assert_eq!(
            parsed(&["box1"]),
            Action::Set {
                name: OsString::from("box1"),
                boot: false
            }
        );
    }

    #[test]
    fn boot_is_a_modifier_not_an_action() {
        assert_eq!(
            parsed(&["-b", "box1"]),
            Action::Set {
                name: OsString::from("box1"),
                boot: true
            }
        );
        assert!(
            parse_args(&a(&["-b"]))
                .unwrap_err()
                .contains("requires a host name")
        );
    }

    #[test]
    fn file_takes_its_argument_attached_or_separate() {
        let expect = Action::SetFromFile {
            path: OsString::from("/etc/myname"),
            boot: false,
        };
        assert_eq!(parsed(&["-F", "/etc/myname"]), expect);
        assert_eq!(parsed(&["-F/etc/myname"]), expect);
        assert_eq!(parsed(&["--file", "/etc/myname"]), expect);
        assert_eq!(parsed(&["--file=/etc/myname"]), expect);
    }

    #[test]
    fn file_without_an_argument_is_an_error() {
        assert!(
            parse_args(&a(&["-F"]))
                .unwrap_err()
                .contains("requires an argument")
        );
        assert!(
            parse_args(&a(&["--file"]))
                .unwrap_err()
                .contains("requires an argument")
        );
    }

    #[test]
    fn displaying_and_setting_at_once_is_refused() {
        // Rather than silently doing one of them.
        let err = parse_args(&a(&["-s", "box1"])).unwrap_err();
        assert!(err.contains("cannot display and set"), "{err}");
    }

    #[test]
    fn two_names_are_refused_rather_than_the_second_ignored() {
        let err = parse_args(&a(&["box1", "box2"])).unwrap_err();
        assert!(err.contains("too many arguments"), "{err}");
    }

    #[test]
    fn a_name_and_a_file_are_refused() {
        let err = parse_args(&a(&["-F", "/etc/myname", "box1"])).unwrap_err();
        assert!(err.contains("both a host name and --file"), "{err}");
    }

    #[test]
    fn a_lone_hyphen_is_an_operand() {
        assert_eq!(
            parsed(&["-"]),
            Action::Set {
                name: OsString::from("-"),
                boot: false
            }
        );
    }

    // ---------------- validation ----------------

    #[test]
    fn valid_names_are_accepted() {
        assert!(validate_hostname(b"box1").is_ok());
        assert!(validate_hostname(b"host.example.org").is_ok());
        assert!(validate_hostname(b"a-b-c.example").is_ok());
        assert!(validate_hostname(b"1").is_ok());
    }

    #[test]
    fn an_empty_name_is_refused() {
        // The previous version passed this straight to `sethostname(_, 0)`.
        assert!(validate_hostname(b"").is_err());
    }

    #[test]
    fn a_name_with_a_space_or_a_newline_is_refused() {
        // A newline in /etc/hostname makes the second line look like a name.
        assert!(validate_hostname(b"my host").is_err());
        assert!(validate_hostname(b"a\nb").is_err());
    }

    #[test]
    fn a_non_utf8_byte_is_refused_by_the_same_rule_as_a_space() {
        // This is the point of validating bytes: no separate UTF-8 check is
        // needed, and no panic is possible.
        let err = validate_hostname(&[b'a', 0x80, b'b']).unwrap_err();
        assert!(err.contains("invalid byte"), "{err}");
    }

    #[test]
    fn empty_labels_are_refused() {
        assert!(validate_hostname(b".box").is_err());
        assert!(validate_hostname(b"box.").is_err());
        assert!(validate_hostname(b"a..b").is_err());
    }

    #[test]
    fn a_label_may_not_begin_or_end_with_a_hyphen() {
        assert!(validate_hostname(b"-box").is_err());
        assert!(validate_hostname(b"box-").is_err());
        assert!(validate_hostname(b"a.-b.c").is_err());
        assert!(validate_hostname(b"a-b").is_ok());
    }

    #[test]
    fn the_length_limits_are_the_rfc_ones() {
        let label = vec![b'a'; MAX_LABEL_LEN];
        assert!(validate_hostname(&label).is_ok());
        let too_long_label = vec![b'a'; MAX_LABEL_LEN + 1];
        assert!(validate_hostname(&too_long_label).is_err());

        // Exactly 253 bytes made of whole legal labels: fill with 63-byte
        // labels and let the last one take whatever room is left, so the name
        // never ends on a separator.
        let mut labels: Vec<Vec<u8>> = Vec::new();
        let mut total = 0;
        while total < MAX_HOSTNAME_LEN {
            let sep = usize::from(!labels.is_empty());
            let take = (MAX_HOSTNAME_LEN - total - sep).min(MAX_LABEL_LEN);
            labels.push(vec![b'a'; take]);
            total += sep + take;
        }
        let mut name = labels.join(&b'.');
        assert_eq!(name.len(), MAX_HOSTNAME_LEN);
        assert!(
            validate_hostname(&name).is_ok(),
            "{:?}",
            validate_hostname(&name)
        );

        // One byte over is refused — and it is the total, not the label, that
        // is over, since the last label still has room.
        name.push(b'a');
        assert!(validate_hostname(&name).is_err());
    }

    // ---------------- name arithmetic ----------------

    #[test]
    fn short_is_everything_before_the_first_dot() {
        assert_eq!(short_of(b"host.example.org"), b"host");
        assert_eq!(short_of(b"host"), b"host");
    }

    #[test]
    fn fqdn_uses_the_resolver_domain_only_when_the_name_has_none() {
        assert_eq!(fqdn_of(b"host", Some(b"example.org")), b"host.example.org");
        assert_eq!(fqdn_of(b"host.a.b", Some(b"example.org")), b"host.a.b");
        assert_eq!(fqdn_of(b"host", None), b"host");
        assert_eq!(fqdn_of(b"host", Some(b"")), b"host");
    }

    #[test]
    fn domain_is_everything_after_the_first_dot() {
        assert_eq!(domain_of(b"host", Some(b"example.org")), b"example.org");
        assert_eq!(domain_of(b"host.a.b", None), b"a.b");
        assert_eq!(domain_of(b"host", None), b"");
    }

    // ---------------- resolv.conf ----------------

    #[test]
    fn domain_directive_wins_over_search() {
        let content = b"search first.org second.org\ndomain real.org\n";
        assert_eq!(parse_resolv_conf(content).unwrap(), b"real.org");
    }

    #[test]
    fn search_supplies_its_first_entry() {
        let content = b"search first.org second.org\n";
        assert_eq!(parse_resolv_conf(content).unwrap(), b"first.org");
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let content = b"# domain commented.org\n\n; domain also.org\nsearch real.org\n";
        assert_eq!(parse_resolv_conf(content).unwrap(), b"real.org");
    }

    #[test]
    fn no_domain_configured_is_none() {
        assert_eq!(parse_resolv_conf(b"nameserver 10.0.0.1\n"), None);
        assert_eq!(parse_resolv_conf(b""), None);
    }

    // ---------------- interface addresses ----------------

    #[test]
    fn addresses_come_from_the_second_column_and_skip_loopback() {
        let content = b"lo 127.0.0.1 255.0.0.0 up\neth0 10.0.2.15 255.255.255.0 up\n";
        assert_eq!(parse_proc_if_inet(content), vec![b"10.0.2.15".to_vec()]);
    }

    #[test]
    fn ipv6_loopback_is_skipped_too() {
        let content = b"lo ::1 128 up\neth0 fe80::1 64 up\n";
        assert_eq!(parse_proc_if_inet(content), vec![b"fe80::1".to_vec()]);
    }

    #[test]
    fn blank_and_comment_lines_in_the_address_table_are_skipped() {
        let content = b"\n# iface addr mask flags\neth0 10.0.2.15 255.255.255.0 up\n";
        assert_eq!(parse_proc_if_inet(content), vec![b"10.0.2.15".to_vec()]);
    }

    // ---------------- -F file contents ----------------

    #[test]
    fn the_first_meaningful_line_skips_comments_and_blanks() {
        let content = b"# the name of this machine\n\n  box1  \nbox2\n";
        assert_eq!(pick_first_meaningful_line(content).unwrap(), b"box1");
    }

    #[test]
    fn an_empty_or_all_comment_file_has_no_name() {
        assert_eq!(pick_first_meaningful_line(b""), None);
        assert_eq!(pick_first_meaningful_line(b"# nothing\n#here\n"), None);
    }

    // ---------------- trimming ----------------

    #[test]
    fn trim_removes_ascii_whitespace_from_both_ends() {
        assert_eq!(trim(b"  box1 \r\n"), b"box1");
        assert_eq!(trim(b"box1"), b"box1");
        assert_eq!(trim(b"   "), b"");
        assert_eq!(trim(b""), b"");
    }

    #[test]
    fn trim_keeps_interior_bytes_including_invalid_ones() {
        assert_eq!(trim(&[b' ', 0x80, b'a', b' ']), &[0x80, b'a']);
    }
}
