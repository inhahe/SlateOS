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

use core::sync::atomic::{AtomicBool, Ordering};

pub use tzrules::{
    Tz, TzDate, TzDst, TzInfo, TzName, TzTransition, TZ_NAME_CAP, days_from_civil, days_in_month,
    is_leap, year_of_day,
};

// ---------------------------------------------------------------------------
// Process-wide current zone
// ---------------------------------------------------------------------------

/// The zone `tzset` last resolved, and whether it has ever been resolved.
///
/// A plain `static mut` behind an `AtomicBool` rather than a lock: `tzset` is
/// documented as not thread-safe in every libc (POSIX explicitly permits it to
/// modify `tzname`/`timezone`/`daylight`, which are themselves unsynchronised
/// globals), and taking a lock here would mean `localtime` — called from
/// signal-adjacent code — could block.  Racing `tzset` calls can therefore
/// interleave, but each field write is a whole `Tz` copy of a value parsed
/// from the same environment, so the worst case is one call observing the
/// other's identical result.
static mut CURRENT: Tz = Tz::UTC;
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// NUL-terminated copies of the current zone's two abbreviations.
///
/// C callers reach these through `tzname[0]`/`tzname[1]` and through
/// `struct tm`'s `tm_zone`, both of which are `char *` and so must point at
/// storage that outlives the call. Keeping them here — rather than handing out
/// a pointer into the `Tz` value, which is `Copy` and lives on the stack —
/// gives them the process lifetime that C expects.
static mut NAME_C: [[u8; TZ_NAME_CAP + 1]; 2] = [[0; TZ_NAME_CAP + 1]; 2];

/// Refresh [`NAME_C`] from `tz`.
fn store_names(tz: &Tz) {
    // SAFETY: same contract as `CURRENT` — the zone globals are specified as
    // unsynchronised, and this writes fixed-size arrays that are never
    // reallocated, so a concurrent reader sees either the old or new bytes of
    // a NUL-terminated name, never a dangling pointer.
    let slots = unsafe { &mut *core::ptr::addr_of_mut!(NAME_C) };
    let dst_name = tz.dst.map_or(tz.std_name, |d| d.name);
    for (slot, name) in slots.iter_mut().zip([tz.std_name, dst_name]) {
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
    let tz = read_env_tz();
    // SAFETY: see the `CURRENT` doc comment — `tzset` is a non-thread-safe
    // interface by specification, and the write is a single `Tz` copy.
    unsafe {
        CURRENT = tz;
    }
    store_names(&tz);
    INITIALISED.store(true, Ordering::Release);
}

/// The process's current zone, resolving `TZ` on first use.
///
/// POSIX says `localtime` behaves as if it called `tzset`; doing the first
/// resolution lazily here means a program that never calls `tzset` still gets
/// its zone, which is what every real libc does.
#[must_use]
pub fn current() -> Tz {
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
    // SAFETY: see the `CURRENT` doc comment.
    unsafe {
        CURRENT = tz;
    }
    store_names(&tz);
    INITIALISED.store(true, Ordering::Release);
}

/// Read and parse `TZ`, falling back to UTC.
fn read_env_tz() -> Tz {
    let raw = crate::environ::getenv_bytes(b"TZ");
    match raw {
        // `TZ` unset: POSIX leaves this implementation-defined.  SlateOS has
        // no system zone setting yet, so UTC it is — documented at the module
        // head, and the one case a future settings service should change.
        None => Tz::utc(),
        // `TZ=""` explicitly requests UTC.
        Some(b"") => Tz::utc(),
        // A zoneinfo name, or junk: glibc without a tzdata file also lands on
        // UTC, so this is the familiar behaviour rather than a new failure.
        Some(s) => Tz::parse(s).unwrap_or_else(Tz::utc),
    }
}

