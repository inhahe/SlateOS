//! The parts of init that are functions of their input alone, kept apart
//! from `main.rs`'s system calls so that the host can test them:
//!
//! ```sh
//! cd services/init
//! cargo test --lib --target x86_64-pc-windows-gnu
//! ```
//!
//! On the target this is `no_std`, like the binary that uses it.

#![cfg_attr(not(test), no_std)]

/// Linux's `HOST_NAME_MAX`, and the most `SYS_HOSTNAME_SET` takes.
pub const HOST_NAME_MAX: usize = 64;

/// What `/etc/hostname` says, read the way systemd's `read_etc_hostname()`
/// reads it: the first line that is neither empty nor a comment, with the
/// white space around it removed.
#[derive(Debug, PartialEq, Eq)]
pub enum EtcHostname<'a> {
    /// A valid host name, with one trailing dot dropped.
    Name(&'a [u8]),
    /// No name at all: an empty file, or only blank lines and comments.
    /// The kernel's name stands, as systemd falls back to its default.
    Empty,
    /// A line that is not a valid host name ([`hostname_is_valid`]),
    /// whole and trimmed, for the boot message that says so.
    Invalid(&'a [u8]),
}

/// The name `/etc/hostname` holds; see [`EtcHostname`].
///
/// systemd also "cleans up" the line before it checks it -- drops the
/// characters a host name cannot hold, collapses repeated dots -- and sets
/// whatever is left. This does not: a line that is not a host name is
/// reported and the kernel's name kept, so that a typo in the file is seen
/// at boot rather than turned silently into some other name. That is the
/// rule `requests/b-d-init-should-set-the-host-name-from-etc-hostname.md`
/// asks for.
#[must_use]
pub fn read_etc_hostname(file: &[u8]) -> EtcHostname<'_> {
    for line in file.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let name = line.strip_suffix(b".").unwrap_or(line);
        return if hostname_is_valid(name) {
            EtcHostname::Name(name)
        } else {
            EtcHostname::Invalid(line)
        };
    }
    EtcHostname::Empty
}

/// Whether `name` is a host name the way systemd's `hostname_is_valid()`
/// judges one (its trailing dot already dropped): at most [`HOST_NAME_MAX`]
/// bytes of ASCII letters, digits, hyphens and dots, in labels that are not
/// empty and neither begin nor end with a hyphen.
#[must_use]
pub fn hostname_is_valid(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > HOST_NAME_MAX {
        return false;
    }
    name.split(|&b| b == b'.').all(|label| {
        !label.is_empty()
            && label.first() != Some(&b'-')
            && label.last() != Some(&b'-')
            && label
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// One record of `/var/run/utmp` and `/var/log/wtmp`: glibc's x86-64
/// `struct utmp`, 384 bytes -- the layout every program here reads and
/// writes them in, through `posix/src/utmpx.rs`'s `FileRecord`, and the
/// one every Linux tool reads.
pub const UTMP_RECORD: usize = 384;

/// `<utmp.h>`'s `BOOT_TIME`: the type of the record a system writes once
/// at every boot, which `who -b` and `last reboot` read back.
pub const BOOT_TIME: i16 = 2;

/// Offsets of the fields [`boot_record`] fills, in glibc's layout:
/// `ut_type` (a `short`) at 0, then after two bytes of padding `ut_pid`,
/// `ut_line[32]`, `ut_id[4]`, `ut_user[32]`, `ut_host[256]`, `ut_exit`,
/// `ut_session`, and the time as two 32-bit fields.
mod field {
    pub const TYPE: usize = 0;
    pub const LINE: usize = 8;
    pub const ID: usize = 40;
    pub const USER: usize = 44;
    pub const HOST: usize = 76;
    /// `ut_host`'s size, as `UT_HOSTSIZE`.
    pub const HOST_LEN: usize = 256;
    pub const TV_SEC: usize = 340;
    pub const TV_USEC: usize = 344;
}

/// The record an init writes at boot to both files, as systemd's
/// `utmp_put_reboot` and sysvinit's `write_utmp_wtmp` do: `BOOT_TIME`,
/// `ut_line` "~", `ut_id` "~~", `ut_user` "reboot", no pid, the kernel's
/// release in `ut_host` -- the version `last reboot` prints -- and the time
/// the system started, each half cut to 32 bits, as glibc's structure holds
/// them. A release longer than `ut_host` keeps its end, as systemd's
/// `copy_suffix` does; a string that fills its field has no NUL, as in
/// every utmp field.
#[must_use]
pub fn boot_record(boot_sec: i64, boot_usec: i64, release: &[u8]) -> [u8; UTMP_RECORD] {
    let mut rec = [0u8; UTMP_RECORD];
    put(&mut rec, field::TYPE, &BOOT_TIME.to_le_bytes());
    put(&mut rec, field::LINE, b"~");
    put(&mut rec, field::ID, b"~~");
    put(&mut rec, field::USER, b"reboot");
    let host = release
        .get(release.len().saturating_sub(field::HOST_LEN)..)
        .unwrap_or(release);
    put(&mut rec, field::HOST, host);
    // Cut to 32 bits on purpose: glibc's `ut_tv` is two `int32_t`s on
    // x86-64 (`__WORDSIZE_TIME64_COMPAT32`).
    #[allow(clippy::cast_possible_truncation)]
    let (sec, usec) = (boot_sec as i32, boot_usec as i32);
    put(&mut rec, field::TV_SEC, &sec.to_le_bytes());
    put(&mut rec, field::TV_USEC, &usec.to_le_bytes());
    rec
}

/// When the system started, from the two clocks: now on the real-time clock,
/// less the time since boot -- the moment systemd stamps the record with.
/// Seconds and microseconds; a real-time clock behind the monotonic one (an
/// unset RTC) gives the epoch rather than a time before it.
#[must_use]
pub fn boot_time(realtime_ns: i64, monotonic_ns: i64) -> (i64, i64) {
    let boot = realtime_ns.saturating_sub(monotonic_ns).max(0);
    (boot / 1_000_000_000, (boot % 1_000_000_000) / 1_000)
}

/// Copy `bytes` into `rec` at `at`, as far as it fits.
fn put(rec: &mut [u8], at: usize, bytes: &[u8]) {
    let room = rec.get_mut(at..).unwrap_or_default();
    let n = bytes.len().min(room.len());
    if let (Some(dst), Some(src)) = (room.get_mut(..n), bytes.get(..n)) {
        dst.copy_from_slice(src);
    }
}

/// `s` without the ASCII white space around it -- spaces, tabs, and the
/// carriage return of a file written on Windows.
fn trim(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i.saturating_add(1));
    s.get(start..end).unwrap_or(&[])
}

// ---------------------------------------------------------------------------
// /etc/startup.conf: one service a line
// ---------------------------------------------------------------------------

/// The most capabilities one service's `caps:` may name.
pub const MAX_SVC_CAPS: usize = 8;

/// The most bytes a service's program path may have.
pub const MAX_SVC_PATH: usize = 128;
/// The most bytes a service's name -- its path's last component, which
/// `depends:` and `svc` name it by -- may have.
pub const MAX_SVC_NAME: usize = 32;
/// The most services one `depends:` may name.
pub const MAX_DEPS: usize = 4;
/// The most arguments `args:` may give (argv[1..]).
pub const MAX_SVC_ARGS: usize = 8;
/// The most bytes one argument may have.
pub const MAX_SVC_ARG_LEN: usize = 64;
/// The most variables `env:` may set.
pub const MAX_SVC_ENV: usize = 4;
/// The most bytes one `KEY=VALUE` may have.
pub const MAX_SVC_ENV_LEN: usize = 128;

/// A service's name: the last component of its program's path.
#[must_use]
pub fn service_name(path: &[u8]) -> &[u8] {
    path.rsplit(|&b| b == b'/').next().unwrap_or(path)
}

/// Whether init can run `path` as a service: an absolute path to a file, no
/// longer than [`MAX_SVC_PATH`], whose name is no longer than
/// [`MAX_SVC_NAME`]. `/etc/startup.conf`'s lines and `svc start` are both
/// held to it.
///
/// # Errors
///
/// [`LineError::BadPath`], [`LineError::PathTooLong`] or
/// [`LineError::NameTooLong`].
pub fn check_path(path: &[u8]) -> Result<(), LineError<'_>> {
    if path.len() > MAX_SVC_PATH {
        return Err(LineError::PathTooLong(path));
    }
    let name = service_name(path);
    if path.first() != Some(&b'/') || name.is_empty() {
        return Err(LineError::BadPath(path));
    }
    if name.len() > MAX_SVC_NAME {
        return Err(LineError::NameTooLong(name));
    }
    Ok(())
}

/// Hold a comma-separated list -- `args:`, `env:` or `depends:`, named by
/// `keyword` -- to what init keeps of it: at most `max` entries of at most
/// `max_len` bytes. Entries are trimmed and empty ones skipped, as init
/// stores them; for `env:`, each must be `KEY=VALUE` with a key.
fn check_list<'a>(
    keyword: &'a [u8],
    value: &'a [u8],
    max: usize,
    max_len: usize,
) -> Result<(), LineError<'a>> {
    let mut n = 0usize;
    for entry in value
        .split(|&b| b == b',')
        .map(trim)
        .filter(|e| !e.is_empty())
    {
        if entry.len() > max_len {
            return Err(LineError::EntryTooLong(keyword, entry));
        }
        if keyword == b"env"
            && entry
                .iter()
                .position(|&b| b == b'=')
                .is_none_or(|at| at == 0)
        {
            return Err(LineError::BadEnv(entry));
        }
        n = n.saturating_add(1);
        if n > max {
            return Err(LineError::TooMany(keyword));
        }
    }
    Ok(())
}

/// A capability a service's `caps:` names: the kernel's `ResourceType`
/// discriminant, the resource's id -- 0 for the whole class -- and the
/// rights, as the kernel's `Rights` bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CapGrant {
    pub resource_type: u16,
    pub resource_id: u64,
    pub rights: u64,
}

impl CapGrant {
    /// No capability: what an unused slot holds.
    pub const NONE: Self = Self {
        resource_type: 0,
        resource_id: 0,
        rights: 0,
    };
}

/// One entry of a capability table as `SYS_CAP_QUERY` writes it and
/// `SYS_PROCESS_SPAWN_EX2` reads it: the kernel's `cap::CapEntryInfo`, 24
/// bytes, its reserved words zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CapEntryInfo {
    pub resource_type: u16,
    pub reserved: [u16; 3],
    pub rights: u64,
    pub resource_id: u64,
}

/// The capability types a service is given only by naming them in `caps:`,
/// by the names `caps:` spells them: init holds these to hand to the one
/// service that needs each -- the keyboard and mouse to the compositor -- and
/// starts every other process without them (design-decisions 706 and 1174).
///
/// The table holds what init delegates, not every type the kernel has: "list
/// what you use, not what exists" (706). A name not here refuses the line, so
/// a type the kernel renumbers or a name misspelt is a service that does not
/// start, said so on the console -- not one that starts without what it was
/// meant to have. The numbers are the kernel's `cap::ResourceType`.
pub const DELEGATED_TYPES: &[(&[u8], u16)] = &[(b"InputDevice", 30), (b"Service", 14)];

/// The rights `caps:` spells, by letter: the kernel's `cap::Rights` bits.
const CAP_RIGHTS: &[(u8, u64)] = &[(b'r', 1 << 0), (b'w', 1 << 1)];

/// Whether a capability of kernel type `resource_type` is one init passes
/// on only to a service that names it.
pub fn is_delegated_type(resource_type: u16) -> bool {
    DELEGATED_TYPES.iter().any(|&(_, t)| t == resource_type)
}

/// What is wrong with a line of `/etc/startup.conf`. Each refuses the line:
/// the service it names is not started, and init says why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineError<'a> {
    /// A word after the path that is not `keyword:value` with a keyword
    /// init knows -- `args`, `env`, `depends` or `caps`.
    UnknownKeyword(&'a [u8]),
    /// A keyword given twice.
    Repeated(&'a [u8]),
    /// A `caps:` entry that is not `Type/id/rights` with a type in
    /// [`DELEGATED_TYPES`], a decimal id, and rights from `r` and `w`, each
    /// at most once; or one named twice.
    BadCap(&'a [u8]),
    /// More than [`MAX_SVC_CAPS`] `caps:` entries.
    TooManyCaps,
    /// The program is not an absolute path to a file: the path as written.
    BadPath(&'a [u8]),
    /// The program's path is longer than [`MAX_SVC_PATH`].
    PathTooLong(&'a [u8]),
    /// The program's name -- the last component, which `depends:` and `svc`
    /// name it by -- is longer than [`MAX_SVC_NAME`].
    NameTooLong(&'a [u8]),
    /// `args:`, `env:` or `depends:` (the keyword) names more entries than
    /// init keeps: [`MAX_SVC_ARGS`], [`MAX_SVC_ENV`], [`MAX_DEPS`].
    TooMany(&'a [u8]),
    /// One entry of `args:`, `env:` or `depends:` (the keyword, then the
    /// entry) is longer than init keeps: [`MAX_SVC_ARG_LEN`],
    /// [`MAX_SVC_ENV_LEN`], [`MAX_SVC_NAME`].
    EntryTooLong(&'a [u8], &'a [u8]),
    /// An `env:` entry that is not `KEY=VALUE` with a key.
    BadEnv(&'a [u8]),
}

/// A line of `/etc/startup.conf`, read: the program's path, then each
/// keyword's value as written (`args:` and `env:` comma-separated, as the
/// registry splits them; `depends:` names).
#[derive(Debug, PartialEq, Eq)]
pub struct ServiceLine<'a> {
    pub path: &'a [u8],
    pub args: &'a [u8],
    pub env: &'a [u8],
    pub deps: &'a [u8],
    /// `caps:`, read: the first `cap_count` entries.
    pub caps: [CapGrant; MAX_SVC_CAPS],
    pub cap_count: usize,
}

impl ServiceLine<'_> {
    /// The capabilities `caps:` names.
    pub fn caps(&self) -> &[CapGrant] {
        self.caps.get(..self.cap_count).unwrap_or_default()
    }
}

/// Read a line of `/etc/startup.conf` -- not empty, not a comment:
///
/// ```text
/// /bin/webserver args:--port,8080 env:PORT=8080 depends:logger,network
/// /bin/compositor depends:logger caps:InputDevice/0/r,Service/0/w
/// ```
///
/// The first word is the program; each word after it is `keyword:value`,
/// in any order. Until 2026-10-05 init found each keyword by searching the
/// line and kept only what came before it, so a keyword after another one
/// was lost: in the first line above, `env:` went with `args:`.
///
/// A line is held to what init keeps of it ([`check_path`], and
/// [`MAX_SVC_ARGS`] and the other limits) and refused whole where it does
/// not fit. Until 2026-10-06 init cut what did not fit and started the
/// service anyway -- a path cut short, an argument cut mid-word, a ninth
/// argument dropped -- so a service ran with a configuration nobody wrote.
pub fn parse_service_line(line: &[u8]) -> Result<ServiceLine<'_>, LineError<'_>> {
    let mut words = line
        .split(|&b| b == b' ' || b == b'\t')
        .filter(|w| !w.is_empty());
    let mut out = ServiceLine {
        path: words.next().unwrap_or_default(),
        args: &[],
        env: &[],
        deps: &[],
        caps: [CapGrant::NONE; MAX_SVC_CAPS],
        cap_count: 0,
    };
    check_path(out.path)?;
    let mut seen = [false; 4];
    for word in words {
        let Some(colon) = word.iter().position(|&b| b == b':') else {
            return Err(LineError::UnknownKeyword(word));
        };
        let (key, value) = (
            word.get(..colon).unwrap_or_default(),
            word.get(colon.saturating_add(1)..).unwrap_or_default(),
        );
        let slot = match key {
            b"args" => 0,
            b"env" => 1,
            b"depends" => 2,
            b"caps" => 3,
            _ => return Err(LineError::UnknownKeyword(word)),
        };
        if let Some(seen) = seen.get_mut(slot) {
            if *seen {
                return Err(LineError::Repeated(word));
            }
            *seen = true;
        }
        match slot {
            0 => out.args = value,
            1 => out.env = value,
            2 => out.deps = value,
            _ => out.cap_count = parse_caps(value, &mut out.caps)?,
        }
    }
    check_list(b"args", out.args, MAX_SVC_ARGS, MAX_SVC_ARG_LEN)?;
    check_list(b"env", out.env, MAX_SVC_ENV, MAX_SVC_ENV_LEN)?;
    check_list(b"depends", out.deps, MAX_DEPS, MAX_SVC_NAME)?;
    Ok(out)
}

/// Read a `caps:` value -- `Type/id/rights` entries, comma-separated, as
/// `InputDevice/0/r,Service/0/w` -- into `out`, and say how many. The id is
/// the resource's, in decimal, 0 meaning the whole class: the kernel
/// delegates an id only to a parent holding that same id, so init can grant
/// the class it holds and not one device of it, nor the class from one
/// device (lane A, `requests/a-b-the-two-fields-you-want-shipped-on-2026-08-22-as-spawn-ex2.md`).
pub fn parse_caps<'a>(
    value: &'a [u8],
    out: &mut [CapGrant; MAX_SVC_CAPS],
) -> Result<usize, LineError<'a>> {
    let mut n = 0usize;
    for entry in value.split(|&b| b == b',') {
        let bad = LineError::BadCap(entry);
        let mut parts = entry.split(|&b| b == b'/');
        let (Some(name), Some(id), Some(letters), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad);
        };
        let resource_type = DELEGATED_TYPES
            .iter()
            .find(|&&(spelt, _)| spelt == name)
            .map(|&(_, t)| t)
            .ok_or(bad)?;
        let resource_id = parse_decimal(id).ok_or(bad)?;
        let mut rights = 0u64;
        for &c in letters {
            let bit = CAP_RIGHTS
                .iter()
                .find(|&&(l, _)| l == c)
                .map(|&(_, b)| b)
                .ok_or(bad)?;
            if rights & bit != 0 {
                return Err(bad);
            }
            rights |= bit;
        }
        if rights == 0 {
            return Err(bad);
        }
        let grant = CapGrant {
            resource_type,
            resource_id,
            rights,
        };
        let named = out.get(..n).unwrap_or_default();
        if named
            .iter()
            .any(|g| g.resource_type == resource_type && g.resource_id == resource_id)
        {
            return Err(bad);
        }
        *out.get_mut(n).ok_or(LineError::TooManyCaps)? = grant;
        n = n.saturating_add(1);
    }
    Ok(n)
}

/// A decimal `u64`: digits only, at least one, no overflow.
fn parse_decimal(text: &[u8]) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    text.iter().try_fold(0u64, |n, &c| {
        if c.is_ascii_digit() {
            n.checked_mul(10)?
                .checked_add(u64::from(c.wrapping_sub(b'0')))
        } else {
            None
        }
    })
}

/// The capabilities init hands a process it starts: its own class-wide
/// grants (`resource_id` 0) except the [`DELEGATED_TYPES`], then `grants`,
/// the service's `caps:`. Written to `out`; how many, or `None` when they do
/// not fit.
///
/// What it leaves out, and why:
/// - **the delegated types**, which init holds only to pass to the service
///   that names one. Started with the kernel's "inherit everything", every
///   process init starts would hold them, and nothing can give a
///   capability up -- the reason 706 kept `InputDevice` off init entirely.
/// - **init's grants on single objects** -- above all the `Process`
///   capability the kernel gives it over each child it starts. They are
///   init's handles on its own children; a service has no use for one on
///   its siblings.
///
/// A `grants` entry init does not hold is not checked here: the kernel
/// refuses the whole spawn (`PermissionDenied`), and init says so.
pub fn child_caps(
    held: &[CapEntryInfo],
    grants: &[CapGrant],
    out: &mut [CapEntryInfo],
) -> Option<usize> {
    let kept = held
        .iter()
        .filter(|e| e.resource_id == 0 && !is_delegated_type(e.resource_type))
        .copied();
    let granted = grants.iter().map(|g| CapEntryInfo {
        resource_type: g.resource_type,
        reserved: [0; 3],
        rights: g.rights,
        resource_id: g.resource_id,
    });
    let mut n = 0usize;
    for entry in kept.chain(granted) {
        *out.get_mut(n)? = entry;
        n = n.saturating_add(1);
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_is_the_name() {
        assert_eq!(
            read_etc_hostname(b"slate-test\n"),
            EtcHostname::Name(b"slate-test")
        );
        assert_eq!(
            read_etc_hostname(b"slate-test"),
            EtcHostname::Name(b"slate-test")
        );
    }

    #[test]
    fn blank_lines_and_comments_come_first() {
        let file = b"\n   \n# the machine's name\n\t# another\n  box.example.org  \nignored\n";
        assert_eq!(
            read_etc_hostname(file),
            EtcHostname::Name(b"box.example.org")
        );
    }

    #[test]
    fn a_file_with_no_name_is_empty() {
        assert_eq!(read_etc_hostname(b""), EtcHostname::Empty);
        assert_eq!(read_etc_hostname(b"\n\n  \n"), EtcHostname::Empty);
        assert_eq!(read_etc_hostname(b"# only a comment\n"), EtcHostname::Empty);
    }

    #[test]
    fn white_space_and_a_windows_line_end_are_trimmed() {
        assert_eq!(
            read_etc_hostname(b"  slate \r\n"),
            EtcHostname::Name(b"slate")
        );
    }

    #[test]
    fn one_trailing_dot_is_dropped() {
        assert_eq!(
            read_etc_hostname(b"box.example.org.\n"),
            EtcHostname::Name(b"box.example.org")
        );
        assert_eq!(
            read_etc_hostname(b"box..\n"),
            EtcHostname::Invalid(b"box..")
        );
    }

    #[test]
    fn what_is_not_a_host_name_is_reported_whole() {
        for bad in [
            &b"my_host"[..],
            b"two words",
            b"-leading",
            b"trailing-",
            b"a..b",
            b".lead",
            b"caf\xc3\xa9",
            b".",
        ] {
            let mut file = bad.to_vec();
            file.push(b'\n');
            assert_eq!(
                read_etc_hostname(&file),
                EtcHostname::Invalid(bad),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn sixty_four_bytes_is_the_limit() {
        let ok = [b'a'; HOST_NAME_MAX];
        assert!(hostname_is_valid(&ok));
        let too_long = [b'a'; HOST_NAME_MAX + 1];
        assert!(!hostname_is_valid(&too_long));
        // A trailing dot does not count against the limit: it is dropped first.
        let mut with_dot = ok.to_vec();
        with_dot.push(b'.');
        assert_eq!(read_etc_hostname(&with_dot), EtcHostname::Name(&ok));
    }

    #[test]
    fn digits_and_hyphens_inside_labels_are_fine() {
        assert!(hostname_is_valid(b"node-01.rack-7.example"));
        assert!(hostname_is_valid(b"3com"));
        assert!(!hostname_is_valid(b""));
    }

    /// The field at `at`, up to its first NUL or `len` bytes.
    fn string_at(rec: &[u8], at: usize, len: usize) -> &[u8] {
        let f = &rec[at..at + len];
        &f[..f.iter().position(|&b| b == 0).unwrap_or(len)]
    }

    fn i32_at(rec: &[u8], at: usize) -> i32 {
        i32::from_le_bytes(rec[at..at + 4].try_into().unwrap())
    }

    /// Each field where glibc's x86-64 `struct utmp` keeps it -- read back by
    /// offset, as `utmpfile` and `posix/src/utmpx.rs` read the file.
    #[test]
    fn the_boot_record_is_systemds_in_glibcs_layout() {
        let rec = boot_record(1_782_921_600, 250_000, b"6.6.0-slateos");
        assert_eq!(rec.len(), 384);
        assert_eq!(i16::from_le_bytes([rec[0], rec[1]]), 2, "BOOT_TIME");
        assert_eq!(i32_at(&rec, 4), 0, "no pid");
        assert_eq!(string_at(&rec, 8, 32), b"~", "ut_line");
        assert_eq!(string_at(&rec, 40, 4), b"~~", "ut_id");
        assert_eq!(string_at(&rec, 44, 32), b"reboot", "ut_user");
        assert_eq!(string_at(&rec, 76, 256), b"6.6.0-slateos", "ut_host");
        assert_eq!(i32_at(&rec, 340), 1_782_921_600, "ut_tv.tv_sec");
        assert_eq!(i32_at(&rec, 344), 250_000, "ut_tv.tv_usec");
        // Nothing else is written: the exit status, the session, the
        // address and the reserved bytes stay zero.
        assert!(rec[332..340].iter().all(|&b| b == 0));
        assert!(rec[348..].iter().all(|&b| b == 0));
    }

    /// A release too long for `ut_host` keeps its end, as systemd's
    /// `copy_suffix` does, and fills the field with no NUL.
    #[test]
    fn a_long_release_keeps_its_end() {
        let mut release = vec![b'x'; 300];
        release.extend_from_slice(b"-tail");
        let rec = boot_record(0, 0, &release);
        let host = &rec[76..76 + 256];
        assert!(host.ends_with(b"-tail"));
        assert!(host.iter().all(|&b| b != 0));
        assert_eq!(i32_at(&rec, 340), 0, "the field after ut_host is untouched");
    }

    #[test]
    fn the_boot_time_is_now_less_the_uptime() {
        // 2026-07-01 00:00:00.5 UTC, two seconds and a quarter after boot.
        let now = 1_782_864_000_500_000_000_i64;
        assert_eq!(boot_time(now, 2_250_000_000), (1_782_863_998, 250_000));
        // An unset real-time clock -- behind the monotonic one -- is the
        // epoch, not a time before it.
        assert_eq!(boot_time(1_000, 5_000_000_000), (0, 0));
    }

    // -- /etc/startup.conf --

    fn line(text: &[u8]) -> ServiceLine<'_> {
        parse_service_line(text).expect("a line init reads")
    }

    /// Each keyword is kept wherever it stands. Until 2026-10-05 a keyword
    /// after another was lost with it: this line started its service
    /// without `PORT=8080`.
    #[test]
    fn every_keyword_is_kept_wherever_it_stands() {
        let l = line(b"/bin/webserver args:--port,8080 env:PORT=8080 depends:logger,network");
        assert_eq!(l.path, b"/bin/webserver");
        assert_eq!(l.args, b"--port,8080");
        assert_eq!(l.env, b"PORT=8080");
        assert_eq!(l.deps, b"logger,network");
        assert!(l.caps().is_empty());

        let l = line(b"/bin/x\tdepends:a  env:K=V\targs:-v caps:Service/0/w");
        assert_eq!(
            (l.path, l.args, l.env, l.deps),
            (&b"/bin/x"[..], &b"-v"[..], &b"K=V"[..], &b"a"[..])
        );
        assert_eq!(l.caps().len(), 1);

        let l = line(b"/bin/ticker");
        assert_eq!(l.path, b"/bin/ticker");
        assert!(l.args.is_empty() && l.env.is_empty() && l.deps.is_empty());
    }

    /// `caps:`, as lane F's compositor line spells it, and an id and both
    /// rights.
    #[test]
    fn caps_are_type_id_and_rights() {
        let l = line(b"/bin/compositor depends:logger caps:InputDevice/0/r,Service/0/w");
        assert_eq!(
            l.caps(),
            [
                CapGrant {
                    resource_type: 30,
                    resource_id: 0,
                    rights: 1
                },
                CapGrant {
                    resource_type: 14,
                    resource_id: 0,
                    rights: 2
                },
            ]
        );
        let l = line(b"/bin/shell caps:Service/8925341740578567520/wr");
        assert_eq!(
            l.caps(),
            [CapGrant {
                resource_type: 14,
                resource_id: 8_925_341_740_578_567_520,
                rights: 3
            }]
        );
    }

    /// What refuses a line -- the service is not started, and init says
    /// which word.
    #[test]
    fn what_a_line_may_not_say() {
        use LineError::{BadCap, Repeated, TooManyCaps, UnknownKeyword};
        let cases: &[(&[u8], LineError<'_>)] = &[
            (b"/bin/x verbose", UnknownKeyword(b"verbose")),
            (b"/bin/x user:root", UnknownKeyword(b"user:root")),
            (b"/bin/x args:a args:b", Repeated(b"args:b")),
            (b"/bin/x caps:", BadCap(b"")),
            (b"/bin/x caps:Keyboard/0/r", BadCap(b"Keyboard/0/r")),
            (b"/bin/x caps:inputdevice/0/r", BadCap(b"inputdevice/0/r")),
            (b"/bin/x caps:InputDevice/0", BadCap(b"InputDevice/0")),
            (
                b"/bin/x caps:InputDevice/0/r/x",
                BadCap(b"InputDevice/0/r/x"),
            ),
            (b"/bin/x caps:InputDevice//r", BadCap(b"InputDevice//r")),
            (b"/bin/x caps:InputDevice/+1/r", BadCap(b"InputDevice/+1/r")),
            (b"/bin/x caps:InputDevice/0/", BadCap(b"InputDevice/0/")),
            (b"/bin/x caps:InputDevice/0/rr", BadCap(b"InputDevice/0/rr")),
            (b"/bin/x caps:InputDevice/0/x", BadCap(b"InputDevice/0/x")),
            (
                b"/bin/x caps:InputDevice/18446744073709551616/r",
                BadCap(b"InputDevice/18446744073709551616/r"),
            ),
            (
                b"/bin/x caps:InputDevice/0/r,InputDevice/0/w",
                BadCap(b"InputDevice/0/w"),
            ),
            (b"/bin/x caps:InputDevice/0/r,,Service/0/w", BadCap(b"")),
            (
                b"/bin/x caps:Service/1/r,Service/2/r,Service/3/r,Service/4/r,Service/5/r,\
Service/6/r,Service/7/r,Service/8/r,Service/9/r",
                TooManyCaps,
            ),
        ];
        for &(text, want) in cases {
            assert_eq!(
                parse_service_line(text),
                Err(want),
                "{:?}",
                core::str::from_utf8(text)
            );
        }
    }

    /// `n` copies of `b`, as text.
    fn repeat(b: u8, n: usize) -> std::vec::Vec<u8> {
        std::vec![b; n]
    }

    /// What does not fit what init keeps refuses the line, whole and with
    /// the part that did not fit. Until 2026-10-06 init cut it to fit and
    /// started the service with what was left.
    #[test]
    fn what_does_not_fit_refuses_the_line() {
        use LineError::{BadEnv, BadPath, EntryTooLong, NameTooLong, PathTooLong, TooMany};
        let long_path = [b"/".as_slice(), &repeat(b'd', 120), b"/x2345678"].concat();
        let long_name = [b"/bin/".as_slice(), &repeat(b'n', 33)].concat();
        let long_arg = [b"/bin/x args:".as_slice(), &repeat(b'a', 65)].concat();
        let long_env = [b"/bin/x env:K=".as_slice(), &repeat(b'v', 127)].concat();
        let long_dep = [b"/bin/x depends:".as_slice(), &repeat(b'd', 33)].concat();
        let cases: &[(&[u8], LineError<'_>)] = &[
            (b"x", BadPath(b"x")),
            (b"bin/x", BadPath(b"bin/x")),
            (b"/bin/", BadPath(b"/bin/")),
            (b"args:-v", BadPath(b"args:-v")),
            (&long_path, PathTooLong(&long_path)),
            (&long_name, NameTooLong(&long_name[5..])),
            (b"/bin/x args:1,2,3,4,5,6,7,8,9", TooMany(b"args")),
            (&long_arg, EntryTooLong(b"args", &long_arg[12..])),
            (b"/bin/x env:A=1,B=2,C=3,D=4,E=5", TooMany(b"env")),
            (&long_env, EntryTooLong(b"env", &long_env[11..])),
            (b"/bin/x env:PORT", BadEnv(b"PORT")),
            (b"/bin/x env:=8080", BadEnv(b"=8080")),
            (b"/bin/x env:A=1,,B", BadEnv(b"B")),
            (b"/bin/x depends:a,b,c,d,e", TooMany(b"depends")),
            (&long_dep, EntryTooLong(b"depends", &long_dep[15..])),
        ];
        for &(text, want) in cases {
            assert_eq!(
                parse_service_line(text),
                Err(want),
                "{:?}",
                core::str::from_utf8(text)
            );
        }
        assert_eq!(long_path.len(), 130);
    }

    /// Exactly the limits fit, and empty entries are skipped as init skips
    /// them when it stores the list.
    #[test]
    fn exactly_the_limits_fit() {
        let path = [b"/".as_slice(), &repeat(b'd', 94), b"/", &repeat(b'n', 32)].concat();
        assert_eq!((path.len(), service_name(&path).len()), (128, 32));
        let arg = repeat(b'a', 64);
        let env = [b"K=".as_slice(), &repeat(b'v', 126)].concat();
        let dep = repeat(b'd', 32);
        let mut text = path.clone();
        text.extend_from_slice(b" args:");
        for i in 0..8 {
            if i > 0 {
                text.push(b',');
            }
            text.extend_from_slice(&arg);
        }
        text.extend_from_slice(b" env:");
        for i in 0..4 {
            if i > 0 {
                text.push(b',');
            }
            text.extend_from_slice(&env);
        }
        text.extend_from_slice(b" depends:");
        for i in 0..4 {
            if i > 0 {
                text.push(b',');
            }
            text.extend_from_slice(&dep);
        }
        let l = line(&text);
        assert_eq!(l.path, &path[..]);
        assert_eq!(l.args.split(|&b| b == b',').count(), 8);
        // Empty entries are not entries: nine commas, two arguments.
        assert!(parse_service_line(b"/bin/x args:a,,,,,,,,,b").is_ok());
    }

    #[test]
    fn a_service_is_named_by_its_last_component() {
        assert_eq!(service_name(b"/bin/webserver"), b"webserver");
        assert_eq!(service_name(b"/webserver"), b"webserver");
        assert_eq!(service_name(b"/bin/"), b"");
        assert_eq!(check_path(b"/bin/ticker"), Ok(()));
        assert_eq!(check_path(b"/"), Err(LineError::BadPath(b"/")));
    }

    fn entry(resource_type: u16, resource_id: u64, rights: u64) -> CapEntryInfo {
        CapEntryInfo {
            resource_type,
            reserved: [0; 3],
            rights,
            resource_id,
        }
    }

    /// A child gets init's class-wide grants, less the types init holds
    /// only to delegate, then what its `caps:` names; never init's grants on
    /// single objects, its children's `Process` capabilities above all.
    #[test]
    fn a_child_gets_inits_class_grants_less_the_delegated_types_then_its_own() {
        let held = [
            entry(10, 0, 0x7fff), // File
            entry(11, 0, 0x7fff), // Socket
            entry(6, 0, 0xff),    // Process, the class
            entry(6, 490, 0x0f),  // Process, init's child 490
            entry(30, 0, 1),      // InputDevice
            entry(14, 0, 2),      // Service
            entry(14, 12_345, 1), // Service, one key
        ];
        let mut out = [CapEntryInfo::default(); 16];

        let n = child_caps(&held, &[], &mut out).expect("fits");
        assert_eq!(out[..n], [held[0], held[1], held[2]]);

        let grants = [CapGrant {
            resource_type: 30,
            resource_id: 0,
            rights: 1,
        }];
        let n = child_caps(&held, &grants, &mut out).expect("fits");
        assert_eq!(out[..n], [held[0], held[1], held[2], entry(30, 0, 1)]);

        assert_eq!(child_caps(&held, &grants, &mut out[..3]), None);
        assert_eq!(child_caps(&[], &[], &mut out[..0]), Some(0));
    }

    /// The mirror of the kernel's `cap::CapEntryInfo`: 24 bytes, the rights
    /// at 8 and the id at 16 -- `SYS_CAP_QUERY` writes this layout and
    /// `SYS_PROCESS_SPAWN_EX2` reads it.
    #[test]
    fn cap_entry_info_is_the_kernels() {
        assert_eq!(core::mem::size_of::<CapEntryInfo>(), 24);
        assert_eq!(core::mem::offset_of!(CapEntryInfo, rights), 8);
        assert_eq!(core::mem::offset_of!(CapEntryInfo, resource_id), 16);
        let kernel = include_str!("../../../kernel/src/cap/mod.rs");
        let fields: std::vec::Vec<&str> = kernel
            .split("pub struct CapEntryInfo {")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect("the kernel's CapEntryInfo")
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("pub "))
            .collect();
        assert_eq!(
            fields,
            [
                "pub resource_type: u16,",
                "pub _reserved: [u16; 3],",
                "pub rights: u64,",
                "pub resource_id: u64,",
            ]
        );
    }

    /// The numbers init gives the types and rights `caps:` names are the
    /// kernel's: each `Name = N,` in its `ResourceType`, each right's bit in
    /// its `Rights`. A renumbering fails here, not as a service started with
    /// the wrong capability.
    #[test]
    fn the_names_caps_spells_are_the_kernels() {
        let types = include_str!("../../../kernel/src/cap/mod.rs");
        for &(name, number) in DELEGATED_TYPES {
            let name = core::str::from_utf8(name).expect("ASCII");
            let want = std::format!("{name} = {number},");
            assert!(
                types.lines().any(|l| l.trim() == want),
                "kernel/src/cap/mod.rs has no `{want}`"
            );
        }
        let rights = include_str!("../../../kernel/src/cap/rights.rs");
        for (name, bit) in [("READ", 0), ("WRITE", 1)] {
            let want = std::format!("pub const {name}: Self = Self(1 << {bit});");
            assert!(
                rights.lines().any(|l| l.trim() == want),
                "kernel/src/cap/rights.rs has no `{want}`"
            );
        }
        assert_eq!(CAP_RIGHTS, &[(b'r', 1u64 << 0), (b'w', 1 << 1)][..]);
    }
}
