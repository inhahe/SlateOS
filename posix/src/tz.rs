//! The process's current timezone.
//!
//! The `TZ`-string grammar, the transition arithmetic and the civil-date
//! helpers live in the [`tzrules`] crate, which `userspace/oils` links too so
//! the shell and the libc can never disagree about what time it is.  What
//! stays here is the part that is inherently libc: the *process-wide current
//! zone*, resolved from the `TZ` environment variable, plus the
//! process-lifetime NUL-terminated name storage that `tzname[]` and
//! `struct tm`'s `tm_zone` hand out to C callers.
//!
//! This is the zone engine behind `tzset`, `localtime`, `mktime` and
//! `strftime`'s `%z`/`%Z` (see [`crate::time`]).  Before it existed the whole
//! libc was hard-wired to UTC — `localtime` was literally an alias for
//! `gmtime` — so every program on the OS printed wall-clock times that were
//! hours wrong outside Greenwich.
//!
//! ## Two kinds of zone
//!
//! `TZ` may be a POSIX rule string (`EST5EDT,M3.2.0,M11.1.0`) or the name of a
//! binary zoneinfo file (`America/New_York`), and a machine with no `TZ` at all
//! should follow `/etc/localtime`.  [`Zone`] is the union of the two: a
//! [`Tz`] rule, or a [`TzFile`] view over [`ZONE_FILE`], the process-lifetime
//! buffer this module reads zoneinfo files into.  Only a rule string can be
//! stored by value, because a transition table is unbounded in a way a `Copy`
//! struct cannot be — hence the buffer.
//!
//! A file zone is strictly more accurate: a POSIX string carries one rule set,
//! so it renders 1995 with today's daylight-saving dates.  Whichever kind is
//! installed, the *tail* of a zoneinfo file is a POSIX string parsed by the
//! very same [`Tz`] engine, so the two paths cannot disagree about a future
//! date.

use core::sync::atomic::{AtomicBool, Ordering};

pub use tzrules::{
    TZ_NAME_CAP, Tz, TzDate, TzDst, TzFile, TzInfo, TzName, TzTransition, days_from_civil,
    days_in_month, is_leap, year_of_day,
};

// ---------------------------------------------------------------------------
// Process-wide current zone
// ---------------------------------------------------------------------------

/// The process's timezone: a POSIX rule, or a loaded zoneinfo file.
///
/// `Copy`, so callers hold a snapshot rather than a borrow of the mutable
/// global — but note that the [`TzFile`] arm borrows [`ZONE_FILE`], so a
/// snapshot taken before a `tzset` that loads a *different* file describes the
/// new file's bytes.  That is the same hazard `tzname[]` has had in every libc
/// since 1988, and for the same reason: POSIX specifies `tzset` as
/// non-thread-safe and as invalidating what came before it.
#[derive(Clone, Copy)]
pub enum Zone {
    /// A POSIX `TZ` rule string, or the UTC default.
    Posix(Tz),
    /// A binary zoneinfo file read into [`ZONE_FILE`].
    File(TzFile<'static>),
}

impl Zone {
    /// The zone state at UTC instant `t`.
    #[must_use]
    pub fn lookup(&self, t: i64) -> TzInfo {
        match self {
            Self::Posix(tz) => tz.lookup(t),
            Self::File(f) => f.lookup(t),
        }
    }

    /// Convert a local wall-clock reading to UTC; see [`Tz::local_to_utc`].
    #[must_use]
    pub fn local_to_utc(&self, local: i64, isdst_hint: i32) -> (i64, TzInfo) {
        match self {
            Self::Posix(tz) => tz.local_to_utc(local, isdst_hint),
            Self::File(f) => f.local_to_utc(local, isdst_hint),
        }
    }

    /// The zone's standard-time state, for `tzname[0]` and `timezone`.
    #[must_use]
    pub fn standard(&self) -> TzInfo {
        match self {
            Self::Posix(tz) => {
                TzInfo { gmtoff: tz.std_gmtoff, is_dst: false, name: tz.std_name }
            }
            Self::File(f) => f.standard(),
        }
    }

    /// The zone's daylight-saving state, for `tzname[1]`, or `None` if it does
    /// not currently observe daylight saving.
    #[must_use]
    pub fn daylight(&self) -> Option<TzInfo> {
        match self {
            Self::Posix(tz) => tz
                .dst
                .map(|d| TzInfo { gmtoff: d.gmtoff, is_dst: true, name: d.name }),
            Self::File(f) => f.daylight(),
        }
    }

    /// Whether this zone observes daylight saving (the POSIX `daylight`
    /// global).
    #[must_use]
    pub fn has_dst(&self) -> bool {
        self.daylight().is_some()
    }
}

/// The zone `tzset` last resolved, and whether it has ever been resolved.
///
/// A plain `static mut` behind an `AtomicBool` rather than a lock: `tzset` is
/// documented as not thread-safe in every libc (POSIX explicitly permits it to
/// modify `tzname`/`timezone`/`daylight`, which are themselves unsynchronised
/// globals), and taking a lock here would mean `localtime` — called from
/// signal-adjacent code — could block.  Racing `tzset` calls can therefore
/// interleave, but each field write is a whole `Zone` copy of a value resolved
/// from the same environment, so the worst case is one call observing the
/// other's identical result.
static mut CURRENT: Zone = Zone::Posix(Tz::UTC);
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// NUL-terminated copies of the current zone's two abbreviations.
///
/// C callers reach these through `tzname[0]`/`tzname[1]` and through
/// `struct tm`'s `tm_zone`, both of which are `char *` and so must point at
/// storage that outlives the call. Keeping them here — rather than handing out
/// a pointer into the `Tz` value, which is `Copy` and lives on the stack —
/// gives them the process lifetime that C expects.
static mut NAME_C: [[u8; TZ_NAME_CAP + 1]; 2] = [[0; TZ_NAME_CAP + 1]; 2];

/// Refresh [`NAME_C`] from `zone`.
fn store_names(zone: &Zone) {
    // SAFETY: same contract as `CURRENT` — the zone globals are specified as
    // unsynchronised, and this writes fixed-size arrays that are never
    // reallocated, so a concurrent reader sees either the old or new bytes of
    // a NUL-terminated name, never a dangling pointer.
    let slots = unsafe { &mut *core::ptr::addr_of_mut!(NAME_C) };
    let std_name = zone.standard().name;
    // POSIX says `tzname[1]` is the daylight abbreviation; with no daylight
    // half it repeats `tzname[0]` rather than being empty, which is what glibc
    // does and what `%Z` on a `tm_isdst`-confused struct then prints.
    let dst_name = zone.daylight().map_or(std_name, |d| d.name);
    for (slot, name) in slots.iter_mut().zip([std_name, dst_name]) {
        *slot = [0; TZ_NAME_CAP + 1];
        let bytes = name.as_bytes();
        if let Some(dest) = slot.get_mut(..bytes.len()) {
            dest.copy_from_slice(bytes);
        }
    }
}

/// The NUL-terminated abbreviation for standard (`index` 0) or daylight
/// (`index` 1) time, for `tzname` and `tm_zone`.
///
/// Never null: an unresolved zone yields an empty string rather than a null
/// pointer, because C code does `printf("%s", tzname[0])` without a check.
#[must_use]
pub fn name_ptr(index: usize) -> *const u8 {
    if !INITIALISED.load(Ordering::Acquire) {
        set_from_env();
    }
    // SAFETY: see `NAME_C`. The index is clamped, so this is always in bounds,
    // and the storage is a process-lifetime static.
    unsafe {
        let slots = &*core::ptr::addr_of!(NAME_C);
        slots.get(index.min(1)).map_or(core::ptr::null(), |s| s.as_ptr())
    }
}

/// Re-read `TZ` and install the resulting zone as the process's current one.
///
/// This is the engine behind [`crate::time::tzset`].
pub fn set_from_env() {
    install(resolve_env_zone());
}

/// Make `zone` the process's current one and refresh the name storage.
fn install(zone: Zone) {
    // SAFETY: see the `CURRENT` doc comment — `tzset` is a non-thread-safe
    // interface by specification, and the write is a single `Zone` copy.
    unsafe {
        CURRENT = zone;
    }
    store_names(&zone);
    INITIALISED.store(true, Ordering::Release);
}

/// The process's current zone, resolving `TZ` on first use.
///
/// POSIX says `localtime` behaves as if it called `tzset`; doing the first
/// resolution lazily here means a program that never calls `tzset` still gets
/// its zone, which is what every real libc does.
#[must_use]
pub fn current() -> Zone {
    if !INITIALISED.load(Ordering::Acquire) {
        set_from_env();
    }
    // SAFETY: see the `CURRENT` doc comment.  `INITIALISED` is `Release`d
    // after the write, so an `Acquire` load that observes `true` also observes
    // the completed store.
    unsafe { CURRENT }
}

/// Install a zone directly, bypassing the environment.
///
/// Used by the tests, and by any future system-settings path that wants to
/// supply the machine's zone without round-tripping through `TZ`.
pub fn set(tz: Tz) {
    install(Zone::Posix(tz));
}

// ---------------------------------------------------------------------------
// Resolving `TZ`
// ---------------------------------------------------------------------------

/// Where zoneinfo files live when `TZ` names one without a leading `/`.
const TZDIR_DEFAULT: &[u8] = b"/usr/share/zoneinfo";

/// The system-wide zone, followed when `TZ` is unset.
///
/// `/etc/localtime` is the portable spelling — a TZif file, or a symlink into
/// the zoneinfo tree — and is what every ported program already expects, which
/// is why it is preferred over inventing a SlateOS-specific setting.
const LOCALTIME_PATH: &[u8] = b"/etc/localtime\0";

/// Resolve `TZ` (or the system default) into a zone, falling back to UTC.
fn resolve_env_zone() -> Zone {
    match crate::environ::getenv_bytes(b"TZ") {
        // `TZ=""` explicitly requests UTC.  This is not the same as unset: a
        // program that clears `TZ` is asking for UTC, not for the machine's
        // zone, and scripts rely on the distinction.
        Some(b"") => Zone::Posix(Tz::utc()),
        Some(s) => resolve_tz_value(s),
        // Unset: follow the machine's own zone, as glibc does.  Absent that
        // file — which is where a freshly installed SlateOS still is, since no
        // tzdata is shipped yet — UTC.
        None => load_zoneinfo(LOCALTIME_PATH).map_or(Zone::Posix(Tz::utc()), Zone::File),
    }
}

/// Resolve a non-empty `TZ` value.
///
/// A leading `:` means "the rest is a file name" (POSIX reserves the prefix for
/// implementation-defined forms and every libc spells it this way).  Without
/// it, a POSIX rule string is tried first and the file only if that fails,
/// which is the order glibc uses — it matters because `EST5EDT` is both a valid
/// rule string *and* a file name in the zoneinfo tree, and the rule string is
/// the cheaper and more predictable of the two.
fn resolve_tz_value(value: &[u8]) -> Zone {
    if let Some(name) = value.strip_prefix(b":") {
        return zone_from_name(name);
    }
    if let Some(tz) = Tz::parse(value) {
        return Zone::Posix(tz);
    }
    zone_from_name(value)
}

/// Load the zoneinfo file `name` refers to, falling back to UTC.
fn zone_from_name(name: &[u8]) -> Zone {
    let mut path = [0u8; crate::unistd::PATH_MAX];
    let Some(()) = zoneinfo_path(name, &mut path) else {
        return Zone::Posix(Tz::utc());
    };
    // A name we cannot read, or a file we cannot parse, lands on UTC — which is
    // what glibc does with a missing tzdata file, so a program that names a
    // zone we do not ship is no worse off than before this path existed.
    load_zoneinfo(&path).map_or(Zone::Posix(Tz::utc()), Zone::File)
}

/// Build the NUL-terminated path of the zoneinfo file `name` names.
///
/// Returns `None` for a name that must not be resolved:
///
/// * one containing a `..` component, because `TZ` is attacker-controlled in
///   any program that inherits an environment, and without this check
///   `TZ=../../../etc/shadow` would make the libc open an arbitrary file and
///   report whether it parses as TZif — a small oracle, but a free one;
/// * one containing an interior NUL, which would truncate the path handed to
///   the kernel and so name a *different* file than the one checked here;
/// * an absolute path, or any path at all, in a set-user-ID program (`TZ` is
///   then attacker-controlled *and* the file is opened with elevated
///   privilege).  `AT_SECURE` is always 0 today because SlateOS has no
///   set-user-ID execution yet; the check is here so the day it gains some,
///   this path is not the hole.
fn zoneinfo_path(name: &[u8], out: &mut [u8; crate::unistd::PATH_MAX]) -> Option<()> {
    if name.is_empty() || name.contains(&0) {
        return None;
    }
    // Reject `..` as a whole component; a name like `Europe/Bu..dapest` is
    // fine, and refusing it would be a surprise.
    if name.split(|&b| b == b'/').any(|part| part == b"..") {
        return None;
    }
    let secure = crate::crt::getauxval(crate::linux_auxv_types::AT_SECURE.into()) != 0;
    if secure && name.starts_with(b"/") {
        return None;
    }

    let mut len = 0usize;
    let mut push = |bytes: &[u8]| -> Option<()> {
        let end = len.checked_add(bytes.len())?;
        out.get_mut(len..end)?.copy_from_slice(bytes);
        len = end;
        Some(())
    };
    if name.starts_with(b"/") {
        push(name)?;
    } else {
        // `TZDIR` is the glibc spelling for an alternate tree; honouring it is
        // what lets a test or a self-contained package point at its own copy.
        let dir = crate::environ::getenv_bytes(b"TZDIR")
            .filter(|d| !d.is_empty() && !secure)
            .unwrap_or(TZDIR_DEFAULT);
        push(dir.strip_suffix(b"/").unwrap_or(dir))?;
        push(b"/")?;
        push(name)?;
    }
    // Room for the terminator was not reserved above, so check it now rather
    // than silently handing the kernel an unterminated buffer.
    *out.get_mut(len)? = 0;
    Some(())
}

/// Capacity of [`ZONE_FILE`]: one SlateOS page.
///
/// The largest file in tzdata is under 4 KiB, so this is roughly four times the
/// worst case.  It is a fixed buffer rather than a heap allocation because
/// `tzset` must work before `malloc` is usable (the C runtime resolves the zone
/// during start-up) and because the libc has no allocator of its own on the
/// target.  A file larger than this is rejected rather than truncated: half a
/// transition table would render confidently wrong times.
const ZONE_FILE_CAP: usize = 16 * 1024;

/// The bytes of the zoneinfo file the current zone was read from.
///
/// Process-lifetime storage, because [`TzFile`] is a borrowed view: it reads
/// the transition table out of these bytes on every lookup rather than copying
/// it, which is what lets a zone with 200 transitions cost 200 bytes of index
/// instead of a fixed inline cap.
static mut ZONE_FILE: [u8; ZONE_FILE_CAP] = [0; ZONE_FILE_CAP];

/// Read the NUL-terminated `path` into [`ZONE_FILE`] and parse it.
///
/// Returns `None` if the file cannot be opened, does not fit, or is not a valid
/// TZif file — every one of which leaves the caller on UTC.
fn load_zoneinfo(path: &[u8]) -> Option<TzFile<'static>> {
    let fd = crate::file::open(path.as_ptr(), crate::fcntl::O_RDONLY, 0);
    if fd < 0 {
        return None;
    }
    // SAFETY: `ZONE_FILE` is a process-lifetime static and this is the only
    // writer; `tzset` is specified as non-thread-safe, which is what makes a
    // single unsynchronised writer the documented contract rather than a race.
    // See `CURRENT`.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(ZONE_FILE) };
    let mut len = 0usize;
    let ok = loop {
        let Some(rest) = buf.get_mut(len..) else { break false };
        if rest.is_empty() {
            // The buffer filled exactly.  One more byte would mean the file is
            // too big for us, so probe for it rather than accepting a prefix.
            let mut probe = [0u8; 1];
            let n = crate::file::read(fd, probe.as_mut_ptr(), 1);
            break n == 0;
        }
        let n = crate::file::read(fd, rest.as_mut_ptr(), rest.len());
        if n < 0 {
            break false;
        }
        if n == 0 {
            break true;
        }
        let Ok(n) = usize::try_from(n) else { break false };
        let Some(next) = len.checked_add(n) else { break false };
        if next > buf.len() {
            // A `read` that claims to have written more than it was given.
            break false;
        }
        len = next;
    };
    crate::file::close(fd);
    if !ok {
        return None;
    }
    parse_zone_file(len)
}

/// Parse the first `len` bytes of [`ZONE_FILE`].
fn parse_zone_file(len: usize) -> Option<TzFile<'static>> {
    // SAFETY: an immutable reborrow of the same static the loader filled.  Any
    // `&mut` to it has ended, and the returned `&'static [u8]` is invalidated
    // only by the next load — which POSIX already licenses `tzset` to do, and
    // which is the same contract `tzname[]` has always carried.
    let bytes = unsafe { &*core::ptr::addr_of!(ZONE_FILE) };
    TzFile::parse(bytes.get(..len)?)
}

/// Install `bytes` as the current zone, as if they had been read from a file.
///
/// Test-only, and the only way to exercise the [`Zone::File`] arm on the host
/// build, whose file syscalls all return `ENOSYS`.  Returns `false` (leaving
/// the zone unchanged) if the bytes do not fit or are not valid TZif.
#[cfg(test)]
fn install_zoneinfo_bytes(bytes: &[u8]) -> bool {
    // SAFETY: see `load_zoneinfo` — same static, same single-writer contract,
    // and the tests that call this hold the environment lock.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(ZONE_FILE) };
    let Some(dest) = buf.get_mut(..bytes.len()) else {
        return false;
    };
    dest.copy_from_slice(bytes);
    match parse_zone_file(bytes.len()) {
        Some(file) => {
            install(Zone::File(file));
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    /// Assemble a TZif v2 file for a US-eastern-like zone with two recorded
    /// transitions in 2020 and an `EST5EDT` tail — the shape `zic -b slim`
    /// emits, and enough to tell a file zone from a rule zone.
    fn eastern_tzif() -> Vec<u8> {
        fn header(out: &mut Vec<u8>, timecnt: u32, typecnt: u32, charcnt: u32) {
            out.extend_from_slice(b"TZif2");
            out.extend_from_slice(&[0; 15]);
            for count in [0, 0, 0, timecnt, typecnt, charcnt] {
                out.extend_from_slice(&u32::to_be_bytes(count));
            }
        }
        fn ttinfo(out: &mut Vec<u8>, utoff: i32, isdst: u8, idx: u8) {
            out.extend_from_slice(&utoff.to_be_bytes());
            out.extend_from_slice(&[isdst, idx]);
        }

        let mut f = Vec::new();
        // The 32-bit block, empty of transitions as a slim file's is.
        header(&mut f, 0, 2, 8);
        ttinfo(&mut f, -5 * 3600, 0, 0);
        ttinfo(&mut f, -4 * 3600, 1, 4);
        f.extend_from_slice(b"EST\0EDT\0");
        // The 64-bit block.
        header(&mut f, 2, 2, 8);
        f.extend_from_slice(&1_583_650_800_i64.to_be_bytes()); // 2020-03-08, into EDT
        f.extend_from_slice(&1_604_210_400_i64.to_be_bytes()); // 2020-11-01, back to EST
        f.extend_from_slice(&[1, 0]);
        ttinfo(&mut f, -5 * 3600, 0, 0);
        ttinfo(&mut f, -4 * 3600, 1, 4);
        f.extend_from_slice(b"EST\0EDT\0");
        f.extend_from_slice(b"\nEST5EDT,M3.2.0,M11.1.0\n");
        f
    }

    /// The path `zoneinfo_path` builds for `name`, as bytes without the NUL.
    fn path_for(name: &[u8]) -> Option<Vec<u8>> {
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        zoneinfo_path(name, &mut buf)?;
        let end = buf.iter().position(|&b| b == 0)?;
        Some(buf.get(..end)?.to_vec())
    }

    #[test]
    fn a_zone_name_resolves_under_the_zoneinfo_directory() {
        let _env = crate::environ::lock_env_for_test();
        assert_eq!(
            path_for(b"America/New_York").as_deref(),
            Some(&b"/usr/share/zoneinfo/America/New_York"[..])
        );
    }

    #[test]
    fn an_absolute_tz_names_the_file_directly() {
        let _env = crate::environ::lock_env_for_test();
        assert_eq!(path_for(b"/etc/localtime").as_deref(), Some(&b"/etc/localtime"[..]));
    }

    #[test]
    fn a_dot_dot_component_is_refused() {
        let _env = crate::environ::lock_env_for_test();
        // `TZ` is inherited from whoever launched the process, so without this
        // the libc would open any file the caller named and reveal whether it
        // parses as TZif.
        assert!(path_for(b"../../../etc/shadow").is_none());
        assert!(path_for(b"America/../../etc/shadow").is_none());
        // A `..` inside a component is not a traversal and must still work.
        assert_eq!(
            path_for(b"Europe/Bu..dapest").as_deref(),
            Some(&b"/usr/share/zoneinfo/Europe/Bu..dapest"[..])
        );
    }

    #[test]
    fn an_interior_nul_is_refused() {
        let _env = crate::environ::lock_env_for_test();
        // The kernel sees a NUL-terminated path, so a name with an interior
        // NUL would open a different file than the one checked here.
        assert!(path_for(b"America/New_York\0/../../etc/shadow").is_none());
        assert!(path_for(b"").is_none());
    }

    #[test]
    fn a_name_too_long_for_the_path_buffer_is_refused() {
        let _env = crate::environ::lock_env_for_test();
        let long = std::vec![b'a'; crate::unistd::PATH_MAX];
        assert!(path_for(&long).is_none());
    }

    #[test]
    fn tzdir_overrides_the_default_tree() {
        let _env = crate::environ::lock_env_for_test();
        // SAFETY: both arguments are NUL-terminated and outlive the call, and
        // the environment lock is held.
        unsafe {
            crate::environ::setenv(c"TZDIR".as_ptr().cast(), c"/opt/zones/".as_ptr().cast(), 1)
        };
        let got = path_for(b"America/New_York");
        // SAFETY: as above.
        unsafe { crate::environ::unsetenv(c"TZDIR".as_ptr().cast()) };
        // The trailing slash on `TZDIR` must not double up.
        assert_eq!(got.as_deref(), Some(&b"/opt/zones/America/New_York"[..]));
    }

    #[test]
    fn a_posix_rule_wins_over_a_file_of_the_same_name() {
        let _env = crate::environ::lock_env_for_test();
        // `EST5EDT` is both a valid rule string and a file in the zoneinfo
        // tree; resolving it as a rule is cheaper and is what glibc does.
        assert!(matches!(resolve_tz_value(b"EST5EDT"), Zone::Posix(_)));
        // A leading colon forces the file interpretation — and since the host
        // build has no filesystem, that lands on UTC rather than on the rule.
        let colon = resolve_tz_value(b":EST5EDT");
        assert!(matches!(colon, Zone::Posix(tz) if tz == Tz::UTC));
    }

    #[test]
    fn a_zoneinfo_name_we_cannot_read_falls_back_to_utc() {
        let _env = crate::environ::lock_env_for_test();
        let zone = resolve_tz_value(b"America/New_York");
        assert!(matches!(zone, Zone::Posix(tz) if tz == Tz::UTC));
    }

    #[test]
    fn a_loaded_zoneinfo_file_becomes_the_current_zone() {
        let _env = crate::environ::lock_env_for_test();
        assert!(install_zoneinfo_bytes(&eastern_tzif()), "fixture must parse");
        let zone = current();
        assert!(matches!(zone, Zone::File(_)));
        assert_eq!(zone.standard().name.as_bytes(), b"EST");
        assert_eq!(zone.standard().gmtoff, -5 * 3600);
        assert!(zone.has_dst());
        assert_eq!(zone.daylight().expect("EDT").gmtoff, -4 * 3600);
        // A recorded transition, and a date past the last one where the
        // footer rule takes over.
        assert!(zone.lookup(1_593_561_600).is_dst); // 2020-07-01
        assert!(zone.lookup(1_909_267_200).is_dst); // 2030-07-01
        assert_eq!(zone.lookup(0).gmtoff, -5 * 3600); // 1970, before the table
        // `tzname[]` must follow a file zone as it does a rule zone.
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(name_ptr(0).cast()) }.to_bytes(), b"EST");
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(name_ptr(1).cast()) }.to_bytes(), b"EDT");
        set(Tz::UTC);
    }

    #[test]
    fn a_file_that_is_not_tzif_leaves_the_zone_alone() {
        let _env = crate::environ::lock_env_for_test();
        set(Tz::UTC);
        assert!(!install_zoneinfo_bytes(b"not a zoneinfo file"));
        assert!(matches!(current(), Zone::Posix(tz) if tz == Tz::UTC));
    }

    #[test]
    fn a_file_larger_than_the_buffer_is_refused() {
        let _env = crate::environ::lock_env_for_test();
        set(Tz::UTC);
        // Truncating a transition table would render confidently wrong times,
        // so an oversized file must be rejected outright.
        let big = std::vec![0u8; ZONE_FILE_CAP + 1];
        assert!(!install_zoneinfo_bytes(&big));
        assert!(matches!(current(), Zone::Posix(tz) if tz == Tz::UTC));
    }
}

