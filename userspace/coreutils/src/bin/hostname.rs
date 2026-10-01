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
use coreutils::stdfd;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::process::ExitCode;

use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quote_os};

/// Live kernel host name. Written by `sysctl kernel.hostname` and by us.
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";

/// Persistent host name, read at boot. Written by us and by `dhcpcd`.
const ETC_HOSTNAME: &str = "/etc/hostname";

/// The hosts table, consulted for this machine's fully qualified name.
///
/// nsswitch puts `files` before `dns` on every normal system, so this is what
/// the resolver would look at first and what `hostname -f` reports.
const ETC_HOSTS: &str = "/etc/hosts";

/// Where the kernel keeps the NIS/YP domain name.
///
/// The sibling of `PROC_HOSTNAME`, and read the same way for the same reason:
/// `getdomainname(2)` and this file are two views of one value, and reading
/// the file keeps `hostname` free of a syscall it needs nowhere else.
const PROC_DOMAINNAME: &str = "/proc/sys/kernel/domainname";

/// Resolver configuration, the source of the domain part for `-f` and `-d`.
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// The kernel's network block, and the ONLY address source that exists on
/// SlateOS.
///
/// `/proc/net` is a file here, not a directory -- see `procfs.rs`'s
/// `ROOT_FILES` and `gen_net()` -- so nothing can live beneath it.
const PROC_NET: &str = "/proc/net";

/// Interface address table, the source for `-i` and `-I` on systems that have
/// one.
///
/// NOTHING IN THIS TREE PRODUCES IT. Two programs read it -- this one and
/// `userspace/ifconfig` -- and no kernel code writes it, which is why both
/// fell through to a fallback. Kept as a second choice rather than deleted
/// because it costs one `read` that fails, and removing it would leave
/// `ifconfig` the only reader of a path with no producer. Tracked in
/// `known-issues.md`.
const PROC_IF_INET: &str = "/proc/net/if_inet";

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
    /// `-y`, `--yp`, `--nis`: the NIS/YP domain name.
    ///
    /// A DIFFERENT name from [`Query::Domain`], which is the DNS domain taken
    /// from the fully qualified host name. This one is the kernel's
    /// `domainname`, set by `setdomainname(2)` and unrelated to DNS -- a host
    /// can have one, both or neither, and they need not agree. Measured:
    /// `hostname -y` answers exactly what `/proc/sys/kernel/domainname` holds.
    NisDomain,
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

// ============================================================================
// Command line
// ============================================================================

/// `hostname`'s name, and the status of a command line it will not run.
const HOSTNAME: Program = Program::new("hostname", 1);

/// net-tools `hostname` 3.23's option string, verbatim. The `?` is a real
/// option there -- help, as `-h` is -- which is how `hostname -?` prints the
/// usage.
const SHORT_OPTIONS: &str = "aAdfbF:h?iIsVy";

/// net-tools `hostname` 3.23's `long_options`, **in declaration order**, which
/// the ambiguity message makes observable.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("domain", Takes::Nothing),
    ("boot", Takes::Nothing),
    ("file", Takes::Required),
    ("fqdn", Takes::Nothing),
    ("all-fqdns", Takes::Nothing),
    ("help", Takes::Nothing),
    ("long", Takes::Nothing),
    ("short", Takes::Nothing),
    ("version", Takes::Nothing),
    ("alias", Takes::Nothing),
    ("ip-address", Takes::Nothing),
    ("all-ip-addresses", Takes::Nothing),
    ("nis", Takes::Nothing),
    ("yp", Takes::Nothing),
];

/// Spellings upstream gives one option: `--long` is `-f`, `--yp` is `-y`.
const LONG_ALIASES: &[(&str, &str)] = &[("long", "fqdn"), ("yp", "nis")];

/// An option net-tools has and this `hostname` does not: `-a` and `-A`, which
/// read the aliases and every FQDN out of name resolution this system cannot
/// yet answer (`known-issues.md` has the resolver's side of that).
fn unimplemented(item: &Opt<'_>) -> String {
    let named = match item {
        Opt::Short(flag, _) => format!("-{}", char::from(*flag)),
        Opt::Long(name, _) => format!("'--{name}'"),
        Opt::Operand(_) => "an operand".to_string(),
    };
    format!(
        "option {named} is not implemented by this hostname\nTry 'hostname --help' for more information."
    )
}

/// Parse the arguments after `argv[0]`: upstream's `getopt_long` loop, on the
/// shared parser.
///
/// So long options abbreviate to a unique prefix (`--sh`, `--dom`), short
/// ones bundle (`-sf` is `-s -f`), `--` ends the options -- the only way to be
/// sure a name taken from a variable is not read as one -- and
/// `POSIXLY_CORRECT` stops them at the first operand. For the display options
/// the last one wins, as upstream assigns them to one variable and acts on it
/// afterwards. `-h`, `-?` and `-V` are answered where they appear.
fn parse_args(args: &[OsString]) -> Result<Action, String> {
    let mut query: Option<Query> = None;
    let mut boot = false;
    let mut file: Option<OsString> = None;
    let mut operands: Vec<OsString> = Vec::new();

    for item in HOSTNAME.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, LONG_ALIASES) {
        let item = item.map_err(|e| e.message())?;
        match item {
            // An operand: the name to set. `-` alone is one, as everywhere.
            Opt::Operand(word) => operands.push(word.clone()),
            Opt::Short(b'h' | b'?', _) | Opt::Long("help", _) => return Ok(Action::Help),
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Action::Version),
            Opt::Short(b's', _) | Opt::Long("short", _) => query = Some(Query::Short),
            Opt::Short(b'f', _) | Opt::Long("fqdn" | "long", _) => query = Some(Query::Fqdn),
            Opt::Short(b'd', _) | Opt::Long("domain", _) => query = Some(Query::Domain),
            Opt::Short(b'y', _) | Opt::Long("nis" | "yp", _) => query = Some(Query::NisDomain),
            Opt::Short(b'i', _) | Opt::Long("ip-address", _) => query = Some(Query::Ip),
            Opt::Short(b'I', _) | Opt::Long("all-ip-addresses", _) => query = Some(Query::AllIp),
            Opt::Short(b'b', _) | Opt::Long("boot", _) => boot = true,
            Opt::Short(b'F', value) | Opt::Long("file", value) => file = value,
            other => return Err(unimplemented(&other)),
        }
    }

    let mut rest = operands.into_iter();
    let name = rest.next();
    if let (Some(first), Some(extra)) = (&name, rest.next()) {
        return Err(format!(
            "too many arguments: already given {}, then {}",
            quote_os(first),
            quote_os(&extra)
        ));
    }
    resolve(query, boot, file, name)
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
    domain_part(&fqdn_of(name, domain)).to_vec()
}

/// Everything after the first dot of an already-qualified name.
///
/// Split out so `-d` can derive its answer from the SAME fully qualified name
/// `-f` reports. Looking the domain up by a second route is how the two come
/// to disagree about which domain this host is in, which is worse than either
/// of them being wrong.
fn domain_part(fqdn: &[u8]) -> &[u8] {
    match fqdn.iter().position(|&b| b == b'.') {
        Some(i) => fqdn.get(i.saturating_add(1)..).unwrap_or_default(),
        None => &[],
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

/// The canonical name `/etc/hosts` gives for `name`, if it has one.
///
/// A hosts line is `ADDRESS CANONICAL [ALIAS...]`. When our short host name
/// appears anywhere in the name list, the FIRST name on that line is this
/// machine's fully qualified name -- which is what nsswitch, and therefore
/// net-tools' `hostname`, reports.
///
/// # Why this is read here rather than through the resolver
///
/// `known-issues.md` -> `B-HOSTNAME-RESOLVES-THE-DOMAIN-WITHOUT-ETC-HOSTS`
/// records the libc route and why it is blocked: `getaddrinfo` with
/// `AI_CANONNAME` now returns a non-NULL name, but the name it returns is the
/// QUERY echoed back, because `SYS_DNS_RESOLVE` carries four address bytes and
/// has nowhere to put a canonical name. Going that way would trade an invented
/// FQDN for a short one, which is not an improvement.
///
/// This program is file-based by deliberate design -- see the module header --
/// and already reads `/proc/sys/kernel/hostname`, `/etc/hostname`,
/// `/etc/resolv.conf`, `/proc/net/if_inet` and `/sys/class/net` directly. One
/// more file needs nothing from the resolver and nothing from another lane.
///
/// A line is only accepted when the short name matches a WHOLE name on it,
/// compared case-insensitively as host names are. Substring matching would let
/// `ox` claim the line for `equinox.example.com`.
fn canonical_from_hosts(name: &[u8]) -> Option<Vec<u8>> {
    canonical_in_hosts(&fs::read(ETC_HOSTS).ok()?, name)
}

/// The parsing half of [`canonical_from_hosts`], over content rather than a
/// path, so the rules can be tested on a host that has no `/etc/hosts` — which
/// is every run of this suite, since the development host is Windows. A helper
/// that can only be exercised where the file exists is a helper nothing checks.
fn canonical_in_hosts(content: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    let short = short_of(name);
    for line in content.split(|&b| b == b'\n') {
        // `#` starts a comment anywhere on the line.
        let line = match line.iter().position(|&b| b == b'#') {
            Some(i) => line.get(..i).unwrap_or(&[]),
            None => line,
        };
        let mut fields = line
            .split(|b| b.is_ascii_whitespace())
            .filter(|f| !f.is_empty());
        // The address is discarded: which address the FQDN is attached to does
        // not matter, and on a normal Linux host it is the 127.0.1.1 line.
        let Some(_addr) = fields.next() else { continue };
        let names: Vec<&[u8]> = fields.collect();
        if !names.iter().any(|n| n.eq_ignore_ascii_case(short)) {
            continue;
        }
        let canonical = names.first()?;
        // Only useful if it is actually qualified. A hosts line reading
        // `127.0.0.1 localhost` for a machine called `localhost` names no
        // domain, and answering with the short name again would be the very
        // trade the entry above warns against.
        if canonical.contains(&b'.') {
            return Some((*canonical).to_vec());
        }
    }
    None
}

/// Addresses on every interface: the address table if it has any, otherwise a
/// scan of the per-interface directories.
fn read_addresses() -> Vec<Vec<u8>> {
    // `/proc/net` FIRST, because on SlateOS it is the only one of these that
    // exists. It is a FILE, not a directory -- `procfs.rs`'s `ROOT_FILES`
    // lists `net` and `gen_net()` writes a readable block -- so
    // `/proc/net/if_inet` cannot exist here at all, whatever it may be on
    // another system.
    if let Ok(content) = fs::read(PROC_NET) {
        let ips = parse_proc_net(&content);
        if !ips.is_empty() {
            return ips;
        }
    }
    if let Ok(content) = fs::read(PROC_IF_INET) {
        let ips = parse_proc_if_inet(&content);
        if !ips.is_empty() {
            return ips;
        }
    }
    // NO FALLBACK TO THE INTERFACE DIRECTORY.
    //
    // `/sys/class/net/<if>/address` is the LINK-LAYER address, and returning
    // it here made `hostname -I` answer `bc:a8:a6:f8:91:20` -- a MAC, exit 0,
    // to a caller asking for an IP address. The old code even filtered
    // `00:00:00:00:00:00`, which is a MAC-shaped sentinel, so what it was
    // reading was never in doubt.
    //
    // Printing nothing is the right answer when no address source is
    // readable: `hostname -I` on a host with no addresses prints an empty
    // line, and a script that gets nothing can tell. A script that gets a MAC
    // cannot, and will put it in a URL.
    Vec::new()
}

/// IPv4 address from `/proc/net`, which on SlateOS is a readable block:
///
/// ```text
/// Interface: eth0  (UP)
///   MAC:     52:54:00:12:34:56
///   IPv4:    10.0.2.15
///   Netmask: 255.255.255.0
/// ```
///
/// Only the `IPv4:` line is taken. The `MAC:` line sits two lines above it and
/// is exactly what the old fallback was reporting as an address, so a parser
/// here that matched on "the value after a colon" would reintroduce the bug it
/// replaces -- the key is checked, not just the shape.
fn parse_proc_net(content: &[u8]) -> Vec<Vec<u8>> {
    let mut ips = Vec::new();
    for line in content.split(|&b| b == b'\n') {
        let line = trim(line);
        let Some(rest) = line.strip_prefix(b"IPv4:") else {
            continue;
        };
        let addr = trim(rest);
        if !addr.is_empty() && !is_loopback(addr) && addr != b"0.0.0.0" {
            ips.push(addr.to_vec());
        }
    }
    ips
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
fn write_line(bytes: &[u8]) -> u8 {
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
fn show(query: &Query) -> Result<u8, String> {
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
            // `/etc/hosts` first, the resolver's search domain only as a
            // fallback: the search list is for COMPLETING QUERIES, not for
            // naming this host, and using it was how `-f` invented
            // `Logoplex3.attlocal.net` for a machine every other resolver user
            // on the box calls `Logoplex3.localdomain`.
            match canonical_from_hosts(&name) {
                Some(fqdn) => Ok(write_line(&fqdn)),
                None => Ok(write_line(&fqdn_of(&name, read_domain().as_deref()))),
            }
        }
        Query::Domain => {
            let name = read_hostname()?;
            match canonical_from_hosts(&name) {
                // The domain is everything after the first dot of the FQDN,
                // so it is derived from the same answer rather than looked up
                // separately -- `-f` and `-d` disagreeing about which domain
                // this host is in would be worse than either being wrong.
                Some(fqdn) => Ok(write_line(domain_part(&fqdn))),
                None => Ok(write_line(&domain_of(&name, read_domain().as_deref()))),
            }
        }
        // An unset NIS domain reads as the literal `(none)` on Linux, and GNU
        // prints it unchanged rather than treating it as absent -- so this
        // does not special-case it either. A missing file is a different
        // thing from a file saying `(none)`, and only the first is an error.
        Query::NisDomain => match read_trimmed(PROC_DOMAINNAME) {
            Some(d) => Ok(write_line(&d)),
            None => Err(format!(
                "cannot determine the NIS domain name: {PROC_DOMAINNAME} could not be read"
            )),
        },
    }
}

/// Carry out a set request, honouring `--boot`.
fn set(name: &[u8], boot: bool) -> Result<u8, String> {
    if boot && read_hostname().is_ok() {
        // `--boot` means "only if nothing has set one yet", so an existing
        // name is the expected case and not an error.
        return Ok(0);
    }
    set_hostname(name)?;
    Ok(0)
}

fn run(action: &Action) -> Result<u8, String> {
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

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    // `args_os`, not `args`: `env::args()` unwraps `into_string()` and so
    // panics on an argument that is not UTF-8. A host name that is not UTF-8
    // is invalid, but the right answer to it is a diagnostic, not a crash.
    let args: Vec<OsString> = env::args_os().skip(1).collect();

    let action = match parse_args(&args) {
        Ok(action) => action,
        Err(message) => {
            diag!("hostname: {message}");
            return ExitCode::from(1);
        }
    };

    match run(&action) {
        Ok(status) => ExitCode::from(status),
        Err(message) => {
            diag!("hostname: {message}");
            ExitCode::from(1)
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

    /// The real `/etc/hosts` shape, from the development machine's WSL guest.
    const HOSTS: &[u8] = b"127.0.0.1\tlocalhost\n\
127.0.1.1\tLogoplex3.localdomain\tLogoplex3\n\
::1     ip6-localhost ip6-loopback\n";

    /// `/proc/net` as `gen_net()` writes it.
    const PROC_NET_BLOCK: &[u8] = b"Interface: eth0  (UP)\n\
  MAC:     52:54:00:12:34:56\n\
  IPv4:    10.0.2.15\n\
  Netmask: 255.255.255.0\n\
  Gateway: 10.0.2.2\n\
  DNS:     10.0.2.3\n";

    #[test]
    fn an_address_query_never_answers_with_a_mac() {
        // The bug this replaces: with no readable address source, `-I` fell
        // back to `/sys/class/net/<if>/address` -- the LINK-LAYER address --
        // and answered `bc:a8:a6:f8:91:20`, exit 0, to a caller asking for an
        // IP. A script cannot tell that from an address and will put it in a
        // URL.
        assert_eq!(parse_proc_net(PROC_NET_BLOCK), vec![b"10.0.2.15".to_vec()]);

        // The MAC sits TWO LINES ABOVE the address in the same block, so a
        // parser matching "the value after a colon" would take it. The key is
        // checked, and this is the case that says so.
        let ips = parse_proc_net(PROC_NET_BLOCK);
        assert!(
            !ips.iter().any(|a| a.contains(&b':')),
            "an IPv4 answer must not contain a colon: {ips:?}"
        );

        // Nothing to report is an empty list, not a guess. `-I` then prints an
        // empty line, which a caller can act on.
        assert!(parse_proc_net(b"Interface: eth0  (DOWN)\n  MAC: 52:54:00:12:34:56\n").is_empty());
        assert!(parse_proc_net(b"").is_empty());
        // Loopback and the unconfigured address are not answers either.
        assert!(parse_proc_net(b"  IPv4:    127.0.0.1\n").is_empty());
        assert!(parse_proc_net(b"  IPv4:    0.0.0.0\n").is_empty());
    }

    #[test]
    fn the_fqdn_comes_from_the_hosts_table_not_the_search_domain() {
        // The line that started this: `hostname -f` answered
        // `Logoplex3.attlocal.net` -- built from resolv.conf's SEARCH list --
        // where every other resolver user on the box says
        // `Logoplex3.localdomain`, which is what /etc/hosts records.
        //
        // A search list is for COMPLETING QUERIES, not for naming this host.
        assert_eq!(
            canonical_in_hosts(HOSTS, b"Logoplex3").as_deref(),
            Some(&b"Logoplex3.localdomain"[..])
        );
        // And `-d` is derived from that same answer rather than looked up
        // again, so the two cannot disagree about which domain this host is in.
        assert_eq!(domain_part(b"Logoplex3.localdomain"), b"localdomain");
    }

    #[test]
    fn a_hosts_line_matches_a_whole_name_and_not_a_substring() {
        let hosts = b"10.0.0.1 equinox.example.com equinox\n";
        // The real name matches.
        assert_eq!(
            canonical_in_hosts(hosts, b"equinox").as_deref(),
            Some(&b"equinox.example.com"[..])
        );
        // A substring of it must NOT: `ox` would otherwise claim this line and
        // report a machine called `ox` as being `equinox.example.com`.
        assert_eq!(canonical_in_hosts(hosts, b"ox"), None);
        assert_eq!(canonical_in_hosts(hosts, b"nox"), None);
        // Host names compare case-insensitively.
        assert_eq!(
            canonical_in_hosts(hosts, b"EQUINOX").as_deref(),
            Some(&b"equinox.example.com"[..])
        );
    }

    #[test]
    fn an_unqualified_hosts_entry_is_not_an_fqdn() {
        // `127.0.0.1 localhost` names no domain. Answering with the short name
        // again would replace an invented FQDN with a useless one, which is
        // the trade `B-HOSTNAME-RESOLVES-THE-DOMAIN-WITHOUT-ETC-HOSTS` warns
        // against -- so this declines and the search-domain path runs instead.
        assert_eq!(
            canonical_in_hosts(b"127.0.0.1 localhost\n", b"localhost"),
            None
        );
        // A comment is not a match, wherever it starts.
        assert_eq!(
            canonical_in_hosts(b"# 10.0.0.1 box.example.com box\n", b"box"),
            None
        );
        assert_eq!(
            canonical_in_hosts(b"10.0.0.1 box.example.com box # note\n", b"box").as_deref(),
            Some(&b"box.example.com"[..])
        );
        // No table at all, or no matching line, leaves the caller to fall back.
        assert_eq!(canonical_in_hosts(b"", b"box"), None);
        assert_eq!(canonical_in_hosts(HOSTS, b"othermachine"), None);
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
