//! Comparing two files' modification times when the answer decides whether one
//! is about to overwrite the other — gnulib's `lib/utimecmp.c`.
//!
//! This is `-u`'s question. `mv -u a b` and `cp -u a b` skip the copy when `b`
//! is already at least as new as `a`, and the naive way to ask — subtract one
//! `mtime` from the other — is wrong in exactly one case, which is also a case
//! `-u` is routinely used in: when the two files are on **different filesystems
//! that store timestamps to different precisions**.
//!
//! The shape of the failure is worth stating, because it is not a rounding
//! nicety. Copy `a` (on ext4, `mtime` = 10.500000000) to `b` on a filesystem
//! whose stamps are whole seconds; `b` arrives at 10.000000000, because that is
//! the closest thing the destination can hold. Ask again with `-u`: `a` is
//! still 10.5, `b` is 10.0, so a plain comparison says the destination is older
//! and copies it a second time — and a third, and every time thereafter. `-u`'s
//! whole purpose is to make a repeated command cheap, and on that pair it never
//! converges.
//!
//! The fix, and gnulib's, is to compare against the source *as the destination
//! would be able to store it*: round the source's timestamp down to the
//! destination filesystem's resolution first. 10.5 truncated to whole seconds
//! is 10.0, which equals `b`, so the second run correctly does nothing.
//!
//! That requires knowing the destination's resolution, which no portable
//! interface reports. gnulib deduces it in two steps and this module does the
//! same:
//!
//! 1. **An upper bound, free.** Look at the three timestamps the destination
//!    already carries (`atime`, `ctime`, `mtime`). If any of them has a
//!    non-zero digit in the nanoseconds, the filesystem can clearly store at
//!    least that much. Trailing zeros on all three are evidence — not proof —
//!    of a coarser clock.
//! 2. **The exact answer, by experiment.** Write a deliberately awkward
//!    `mtime` onto the destination (its own, plus a fraction chosen to have a
//!    non-zero digit at every position the bound admits), read it back, and see
//!    how much survived. Then put the original back.
//!
//! Step 2 writes to a file, so it is reached only when it can change the
//! answer: the whole procedure is skipped unless the two stamps are within two
//! seconds of each other, and skipped again if the bound alone already
//! separates them.
//!
//! ## Why it is a shared module rather than a function inside `mv`
//!
//! It is upstream's `lib/`, not `src/`, for the reason everything else in this
//! crate's library is here: `cp -u` and `mv -u` are documented as the same
//! option, and the only difference between them is one line — the flag that
//! selects the truncation, which GNU computes as `preserve_timestamps &&
//! !(move_mode && dst_dev == src_dev)` at `copy.c:2359`. For `mv` that reduces
//! to "the move crosses a filesystem boundary"; for `cp -p` it is always true.
//! Today only `mv` calls it, because `cp --update` is still refused by name;
//! when it lands it needs this and not a second copy of it.
//!
//! ## What is deliberately not here
//!
//! gnulib has a third step ahead of the two above: if the platform defines
//! `_PC_TIMESTAMP_RESOLUTION`, it asks `pathconf` and skips the deduction.
//! Linux and glibc do not define it — it is a POSIX 2024 addition no released
//! glibc implements — so upstream's own `#ifdef` compiles that block out on the
//! platform this is measured against. It is a shortcut to a number step 2
//! arrives at anyway, so adding it would change no answer.
//!
//! The AIX special case (a difference under 0.01 s counts as equal, because
//! jfs2's stamps wander) is likewise not ported: it is inside `#if defined
//! _AIX`.

use std::fs;
use std::path::Path;

/// How old the destination is relative to the source.
///
/// gnulib returns this as an `int` in `{-1, 0, 1, -2}` and every caller
/// compares it against zero, which reads as arithmetic and is not: `-2` is not
/// "older than older", it is "I could not find out". Spelling it as an enum
/// keeps [`Age::Unknown`] from being silently ordered against the other three,
/// and makes a caller say which side it falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    /// The destination is older than the source: `-u` should replace it.
    Older,
    /// The two are the same age, to the destination's precision.
    Same,
    /// The destination is newer than the source.
    Newer,
    /// The comparison needed the experiment in step 2 and the experiment could
    /// not be run — the destination's timestamps could not be written, or could
    /// not be read back. gnulib's `-2`.
    ///
    /// Only ever produced when the two stamps are already within two seconds of
    /// each other, so this is "too close to call", never "no idea".
    Unknown,
}

impl Age {
    /// GNU's `0 <= utimecmp (…)`, which is the test every `-u` call site makes:
    /// skip the copy when the destination is already at least as new.
    ///
    /// [`Age::Unknown`] answers `false` — a copy that cannot tell goes ahead,
    /// which is what the command would have done without `-u`. The other
    /// choice, skipping, would discard a source on no evidence.
    #[must_use]
    pub fn at_least_as_new(self) -> bool {
        matches!(self, Age::Same | Age::Newer)
    }
}

/// Compare the destination's modification time against the source's.
///
/// `truncate_source` is gnulib's `UTIMECMP_TRUNCATE_SOURCE`: round the source's
/// timestamp down to the destination filesystem's resolution before comparing,
/// which is what makes a second `cp -u`/`mv -u` of the same pair across a
/// precision boundary a no-op. Pass `false` and this is a plain comparison of
/// the two `mtime`s, which is right when nothing is going to be truncated.
///
/// `dst_name` must name the file `dst` was `lstat`ed from: under
/// `truncate_source` this may write and restore that file's timestamps, and it
/// does so by name with `AT_SYMLINK_NOFOLLOW`, exactly as `utimecmpat` does. It
/// is unused when `truncate_source` is `false`, which is why gnulib documents
/// `DST_NAME` as permitted to be null in that case.
#[must_use]
pub fn utimecmp(
    dst_name: &Path,
    dst: &fs::Metadata,
    src: &fs::Metadata,
    truncate_source: bool,
) -> Age {
    #[cfg(unix)]
    {
        unix::utimecmp(dst_name, dst, src, truncate_source)
    }
    #[cfg(not(unix))]
    {
        // Nothing on this side can be measured: `std` exposes no `atime`,
        // `ctime` or nanosecond field to bound a resolution from, and this host
        // is not the target. The comparison degrades to the one `mv` made
        // before this module existed.
        let _ = (dst_name, truncate_source);
        match (dst.modified(), src.modified()) {
            (Ok(d), Ok(s)) => match d.cmp(&s) {
                std::cmp::Ordering::Less => Age::Older,
                std::cmp::Ordering::Equal => Age::Same,
                std::cmp::Ordering::Greater => Age::Newer,
            },
            // A platform that cannot say when a file changed cannot answer at
            // all. That is a wider failure than the "too close to call" this
            // variant means on unix, but it lands on the same side: the caller
            // proceeds rather than skipping.
            _ => Age::Unknown,
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::Age;
    use crate::fsattr::{self, Link, On, Times, When};
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, SystemTime};

    /// One second, in nanoseconds. gnulib's `BILLION`.
    const BILLION: i64 = 1_000_000_000;

    /// The finest resolution the timestamp-*setting* syscall can express, which
    /// bounds how fine a resolution this module can ever measure: there is no
    /// point deducing that a filesystem stores picoseconds if nothing can write
    /// one.
    ///
    /// gnulib picks this from what `configure` found — 1 ns with `utimensat`,
    /// 1 µs with only `utimes`, 100 ns on native Windows, a whole second if
    /// neither. [`fsattr::set_times`] calls `utimensat` on every unix,
    /// including this OS, so the first case is the only one reachable here.
    const SYSCALL_RESOLUTION: i64 = 1;

    /// The coarsest resolution assumed possible, and the value an unmeasured
    /// filesystem starts at. Two seconds, because FAT stores modification times
    /// in units of two.
    ///
    /// It is what makes the whole procedure cheap: two stamps more than two
    /// seconds apart cannot be brought together by any truncation, so the
    /// comparison answers immediately and never touches the disk.
    const WORST_RESOLUTION: i64 = 2 * BILLION;

    /// One nanosecond-resolution timestamp, in the two halves `stat` reports.
    ///
    /// A pair of integers rather than a [`SystemTime`] because every step of
    /// gnulib's algorithm is integer arithmetic on exactly these two fields —
    /// divisibility by powers of ten, a parity test on the seconds, truncation
    /// by remainder — none of which a duration type exposes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Stamp {
        /// Whole seconds since the epoch. Signed: files predating 1970 exist.
        sec: i64,
        /// Nanoseconds within the second, always in `0..1_000_000_000`.
        nsec: i64,
    }

    /// What has been learned about one filesystem's timestamp resolution.
    #[derive(Clone, Copy)]
    struct FsRes {
        /// An upper bound on the resolution, in nanoseconds. Either
        /// [`WORST_RESOLUTION`] or a power of ten between
        /// [`SYSCALL_RESOLUTION`] and [`BILLION`].
        resolution: i64,
        /// Whether `resolution` is the measured answer rather than a bound.
        exact: bool,
    }

    /// Resolutions already worked out, keyed by device number.
    ///
    /// The point of the cache is that [`measure_resolution`] writes to a file:
    /// without it, a `cp -u` over a thousand destinations would stamp and
    /// restore a thousand files to learn one number about one filesystem.
    ///
    /// gnulib's equivalent is a `static Hash_table *`, and its own comment says
    /// it "is not safe in the presence of signals, multiple threads, etc." This
    /// one is behind a mutex, and the lock is deliberately *not* held across
    /// the measurement — so two threads racing on a new device can both measure
    /// it. That is harmless: the measurement restores what it found and is
    /// idempotent, so the loser repeats work rather than corrupting an answer.
    /// Holding a lock across two syscalls to prevent a duplicate would be the
    /// more expensive mistake.
    fn cache() -> &'static Mutex<HashMap<u64, FsRes>> {
        static CACHE: OnceLock<Mutex<HashMap<u64, FsRes>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// The cached resolution for a device, if one has been worked out.
    fn cached(dev: u64) -> Option<FsRes> {
        // A poisoned mutex means another thread panicked while holding it. The
        // map is still coherent — the only write is a whole `insert`, which
        // cannot be observed half-done — so the guard is taken anyway rather
        // than turning an unrelated panic into a failure of `-u`.
        let map = cache().lock().unwrap_or_else(|e| e.into_inner());
        map.get(&dev).copied()
    }

    /// Record what was measured about a device.
    fn remember(dev: u64, res: FsRes) {
        let mut map = cache().lock().unwrap_or_else(|e| e.into_inner());
        map.insert(dev, res);
    }

    /// The modification time of a `stat`.
    fn mtime(meta: &fs::Metadata) -> Stamp {
        Stamp {
            sec: meta.mtime(),
            nsec: meta.mtime_nsec(),
        }
    }

    /// The three timestamps of a `stat`, in the order the deduction reads them.
    fn stamps(meta: &fs::Metadata) -> (Stamp, Stamp, Stamp) {
        (
            Stamp {
                sec: meta.atime(),
                nsec: meta.atime_nsec(),
            },
            Stamp {
                sec: meta.ctime(),
                nsec: meta.ctime_nsec(),
            },
            mtime(meta),
        )
    }

    /// [`super::utimecmp`]'s unix body — `utimecmpat` with `dfd` fixed at
    /// `AT_FDCWD`, which is the only form either caller needs.
    pub fn utimecmp(
        dst_name: &Path,
        dst: &fs::Metadata,
        src: &fs::Metadata,
        truncate_source: bool,
    ) -> Age {
        let dst_m = mtime(dst);
        let mut src_m = mtime(src);

        if truncate_source {
            // Quick exits. Nothing coarser than two seconds is assumed to
            // exist, so a gap wider than that survives any truncation and the
            // disk need not be touched to say so.
            if dst_m == src_m {
                return Age::Same;
            }
            if dst_m.sec <= src_m.sec.saturating_sub(2) {
                return Age::Older;
            }
            if src_m.sec <= dst_m.sec.saturating_sub(2) {
                return Age::Newer;
            }

            let dev = dst.dev();
            let known = cached(dev).unwrap_or(FsRes {
                resolution: WORST_RESOLUTION,
                exact: false,
            });

            let res = if known.exact {
                known.resolution
            } else {
                match settle_resolution(dst_name, dst, known.resolution, src_m) {
                    Settled::Resolution(res) => {
                        remember(
                            dev,
                            FsRes {
                                resolution: res,
                                exact: true,
                            },
                        );
                        res
                    }
                    // The bound alone decided it, or the measurement could not
                    // be made. Nothing exact was learned, so nothing is cached:
                    // the next pair on this device may land closer together and
                    // need the real number.
                    Settled::Answer(age) => return age,
                }
            };

            src_m = truncate(src_m, res);
        }

        compare(dst_m, src_m)
    }

    /// What one attempt at pinning down a filesystem's resolution produced.
    enum Settled {
        /// The resolution, measured exactly.
        Resolution(i64),
        /// No measurement was needed or possible, and the comparison is already
        /// answered — either because the upper bound was enough to separate the
        /// two stamps, or because the destination's timestamps could not be
        /// written or read back ([`Age::Unknown`]).
        Answer(Age),
    }

    /// Steps 1 and 2 of the module header: bound the resolution from what the
    /// destination already carries, then measure it exactly if the bound leaves
    /// room to.
    fn settle_resolution(dst_name: &Path, dst: &fs::Metadata, cap: i64, src_m: Stamp) -> Settled {
        let (dst_a, dst_c, dst_m) = stamps(dst);
        let res = upper_bound(dst_a, dst_c, dst_m, cap);

        if res <= SYSCALL_RESOLUTION {
            // The destination is already storing digits as fine as anything can
            // write, so there is nothing to measure and nothing to truncate.
            return Settled::Resolution(res);
        }

        // Ignore source digits that must be lost passing through `utimensat`
        // whatever the filesystem does. A no-op while `SYSCALL_RESOLUTION` is
        // one nanosecond; kept because that constant is the thing that would
        // change on a platform without `utimensat`, and a reader comparing this
        // against `utimecmp.c` should find every line of it.
        let src_m = Stamp {
            sec: src_m.sec,
            nsec: src_m.nsec - src_m.nsec % SYSCALL_RESOLUTION,
        };

        // If even the *upper bound* on the truncation cannot bring the two
        // together, the answer is already known and the destination's
        // timestamps are left alone. This is what keeps `-u` from writing to
        // files it is only being asked about.
        if let Some(age) = bound_decides(dst_m, src_m, res) {
            return Settled::Answer(age);
        }

        match measure_resolution(dst_name, dst_a, dst_m, res) {
            Some(exact) => Settled::Resolution(exact),
            None => Settled::Answer(Age::Unknown),
        }
    }

    /// Step 1: the largest resolution consistent with the digits the
    /// destination's own three timestamps already carry, never exceeding `cap`.
    ///
    /// The reasoning is that a filesystem cannot show a digit it cannot store.
    /// If `mtime` ends in `…7`, single nanoseconds are stored; if all three
    /// stamps end in three zeros, microseconds are *plausible* — which is why
    /// this is a bound and not an answer, since three timestamps ending in
    /// zeros can also be a coincidence.
    ///
    /// Reading `atime` and `ctime` as well as `mtime` is upstream's, with
    /// upstream's justification: no known filesystem stores either at a finer
    /// precision than `mtime`, so all three are evidence about one clock.
    fn upper_bound(a: Stamp, c: Stamp, m: Stamp, cap: i64) -> i64 {
        // A stamp on an odd second cannot have come from a two-second clock.
        let odd_second = ((a.sec | c.sec | m.sec) & 1) != 0;

        let (mut a, mut c, mut m) = (a.nsec, c.nsec, m.nsec);
        // The first rung of the ladder: ten times the finest writable step.
        let step = SYSCALL_RESOLUTION * 10;

        if ((a % step) | (c % step) | (m % step)) != 0 {
            // Some stamp has a digit below `step`, so the filesystem stores at
            // least that finely and no ladder-climbing is needed.
            return SYSCALL_RESOLUTION;
        }

        let mut res = step;
        a /= step;
        c /= step;
        m /= step;
        while res < cap && ((a % 10) | (c % 10) | (m % 10)) == 0 {
            if res == BILLION {
                // A whole second of zeros. The only coarser possibility is the
                // two-second clock, and an odd second rules that out.
                if !odd_second {
                    res *= 2;
                }
                break;
            }
            res *= 10;
            a /= 10;
            c /= 10;
            m /= 10;
        }
        res
    }

    /// Whether the upper bound alone separates the two stamps, and which way.
    ///
    /// Truncating the source can only move it *earlier*, so the bound gives
    /// both a best and a worst case; when those agree there is nothing to
    /// measure.
    fn bound_decides(dst_m: Stamp, src_m: Stamp, res: i64) -> Option<Age> {
        // Untruncated, the source is already no later than the destination —
        // and truncation only moves it earlier still.
        if src_m.sec < dst_m.sec || (src_m.sec == dst_m.sec && src_m.nsec <= dst_m.nsec) {
            return Some(Age::Newer);
        }
        // Truncated as far as the bound permits, the source is still later than
        // the destination.
        let floor_sec = src_m.sec & !two_second_bit(res);
        if dst_m.sec < floor_sec
            || (dst_m.sec == floor_sec && dst_m.nsec < src_m.nsec - src_m.nsec % res)
        {
            return Some(Age::Older);
        }
        None
    }

    /// Step 2: write an awkward `mtime` onto the destination, read back what
    /// survived, and put the original back.
    ///
    /// The value written is the destination's own modification time with
    /// `res / 9` added to the nanoseconds — `111…1` for a `res` of `10…0`,
    /// which is the number with a non-zero digit at every position the bound
    /// admits. Whatever comes back says where the filesystem stopped caring.
    /// When the bound is the two-second clock the seconds are also forced odd,
    /// since that is the digit such a clock discards.
    ///
    /// The sum cannot leave the nanosecond field: the bound is only above
    /// `SYSCALL_RESOLUTION` when every stamp divides by it, so a `res` of
    /// `10^k` comes with an `mtime` whose last `k` digits are zero, and `10^k/9`
    /// has exactly `k` digits.
    ///
    /// The `atime` in the same call is the destination's existing one written
    /// back unchanged. `utimensat` takes both fields together, and passing the
    /// pair that was read is upstream's choice rather than `UTIME_OMIT` — the
    /// access time is about to be rewritten by the `stat` below in any case.
    ///
    /// Returns `None` if either syscall failed, which is gnulib's `-2`.
    fn measure_resolution(dst_name: &Path, dst_a: Stamp, dst_m: Stamp, res: i64) -> Option<i64> {
        let probe = Stamp {
            sec: dst_m.sec | two_second_bit(res),
            nsec: dst_m.nsec + res / 9,
        };
        let write = |m: Stamp| -> Option<()> {
            let times = Times {
                accessed: When::Set(instant(dst_a)?),
                modified: When::Set(instant(m)?),
            };
            fsattr::set_times(On::Path(dst_name, Link::NoFollow), times).ok()
        };

        write(probe)?;

        let read = fs::symlink_metadata(dst_name).map(|meta| mtime(&meta)).ok();

        // Put the original modification time back whenever it might not still
        // be there — including when the read failed, since then we cannot know
        // that it is. That write's own failure is ignored deliberately: there
        // is nothing further to try, and the caller is about to report that it
        // could not tell the two files apart, which is the honest answer either
        // way.
        if read != Some(dst_m) {
            let _ = write(dst_m);
        }

        // Which digits of the probe survived. The parity of the second counts
        // as one place above the nanoseconds, which is what distinguishes a
        // two-second clock from a one-second clock.
        let read = read?;
        Some(exact_from_readback(
            BILLION * (read.sec & 1) + read.nsec,
            res,
        ))
    }

    /// The resolution implied by the digits that survived the probe, given the
    /// bound `cap` that was written.
    ///
    /// Counts trailing zeros in units of [`SYSCALL_RESOLUTION`], stopping at
    /// the bound — a filesystem cannot be coarser than the bound said, and a
    /// probe that came back entirely zero at `BILLION` means the two-second
    /// clock.
    fn exact_from_readback(readback: i64, cap: i64) -> i64 {
        let mut res = SYSCALL_RESOLUTION;
        let mut a = readback / res;
        loop {
            if a % 10 != 0 {
                break;
            }
            if res == BILLION {
                res *= 2;
                break;
            }
            res *= 10;
            if res == cap {
                break;
            }
            a /= 10;
        }
        res
    }

    /// `1` when `res` is the two-second clock and `0` otherwise — the bit of
    /// the seconds field such a clock cannot store.
    fn two_second_bit(res: i64) -> i64 {
        i64::from(res == WORST_RESOLUTION)
    }

    /// A [`Stamp`] as the instant [`fsattr::set_times`] takes.
    ///
    /// Round-trips exactly, including before 1970: `fsattr`'s `to_timespec`
    /// reapplies the sign and borrows a second for a non-zero nanosecond part,
    /// which is the inverse of the borrow here.
    ///
    /// `None` for a `sec` too far from the epoch to be a [`SystemTime`], which
    /// the `+`/`-` operators would answer by panicking. That is a corrupt or
    /// hostile `st_mtime` rather than a real file's, and the caller turns it
    /// into [`Age::Unknown`] — the one answer that is safe, because the
    /// alternative is writing a *substituted* timestamp onto a file whose real
    /// one this function has just failed to express.
    fn instant(at: Stamp) -> Option<SystemTime> {
        let nanos = Duration::from_nanos(at.nsec.unsigned_abs());
        let secs = Duration::from_secs(at.sec.unsigned_abs());
        let whole = if at.sec >= 0 {
            SystemTime::UNIX_EPOCH.checked_add(secs)?
        } else {
            SystemTime::UNIX_EPOCH.checked_sub(secs)?
        };
        whole.checked_add(nanos)
    }

    /// The source's timestamp as the destination filesystem would store it:
    /// rounded down to a multiple of `res`.
    fn truncate(src: Stamp, res: i64) -> Stamp {
        Stamp {
            sec: src.sec & !two_second_bit(res),
            // At `res == WORST_RESOLUTION` this clears the nanoseconds
            // outright, since every nanosecond count is below two seconds — the
            // second itself was rounded by the line above instead.
            nsec: src.nsec - src.nsec % res,
        }
    }

    /// The final comparison, on stamps already made comparable.
    fn compare(dst: Stamp, src: Stamp) -> Age {
        let ordering = if dst.sec == src.sec {
            dst.nsec.cmp(&src.nsec)
        } else {
            dst.sec.cmp(&src.sec)
        };
        match ordering {
            std::cmp::Ordering::Less => Age::Older,
            std::cmp::Ordering::Equal => Age::Same,
            std::cmp::Ordering::Greater => Age::Newer,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn at(sec: i64, nsec: i64) -> Stamp {
            Stamp { sec, nsec }
        }

        #[test]
        fn a_nonzero_nanosecond_digit_proves_the_finest_resolution() {
            // One stamp ends in 7, so single nanoseconds are stored.
            let res = upper_bound(at(10, 0), at(10, 0), at(10, 7), WORST_RESOLUTION);
            assert_eq!(res, 1);
        }

        #[test]
        fn trailing_zeros_bound_the_resolution_at_the_matching_power_of_ten() {
            // All three end in three zeros and no more: microseconds.
            let res = upper_bound(
                at(10, 1_000),
                at(10, 2_000),
                at(10, 3_000),
                WORST_RESOLUTION,
            );
            assert_eq!(res, 1_000);
        }

        #[test]
        fn a_whole_second_of_zeros_on_an_even_second_reaches_the_two_second_clock() {
            let res = upper_bound(at(10, 0), at(10, 0), at(10, 0), WORST_RESOLUTION);
            assert_eq!(res, WORST_RESOLUTION);
        }

        #[test]
        fn an_odd_second_rules_out_the_two_second_clock() {
            // Same zeros, but one second is odd — a two-second clock could not
            // have produced it, so the bound stops at one second.
            let res = upper_bound(at(11, 0), at(10, 0), at(10, 0), WORST_RESOLUTION);
            assert_eq!(res, BILLION);
        }

        #[test]
        fn the_bound_never_exceeds_the_cap_it_is_given() {
            // A device already known to be no coarser than a millisecond is not
            // promoted to whole seconds by a stamp full of zeros.
            let res = upper_bound(at(10, 0), at(10, 0), at(10, 0), 1_000_000);
            assert_eq!(res, 1_000_000);
        }

        #[test]
        fn truncation_rounds_the_source_down_to_the_destinations_step() {
            assert_eq!(truncate(at(10, 500_000_000), BILLION), at(10, 0));
            assert_eq!(truncate(at(10, 1_500), 1_000), at(10, 1_000));
            // The two-second clock takes the odd second as well as the fraction.
            assert_eq!(truncate(at(11, 500_000_000), WORST_RESOLUTION), at(10, 0));
            // The finest resolution changes nothing.
            assert_eq!(truncate(at(10, 500_000_007), 1), at(10, 500_000_007));
        }

        #[test]
        fn the_repeated_copy_across_a_precision_boundary_converges() {
            // The case the module exists for: a source at .5 s, a destination
            // that could only store the whole second. Truncated, they are
            // equal, so a second `-u` run skips.
            let src = at(10, 500_000_000);
            let dst = at(10, 0);
            assert_eq!(compare(dst, src), Age::Older, "untruncated, it copies");
            assert_eq!(compare(dst, truncate(src, BILLION)), Age::Same);
        }

        #[test]
        fn the_bound_decides_when_the_source_is_no_later_even_untruncated() {
            assert_eq!(
                bound_decides(at(11, 0), at(10, 0), BILLION),
                Some(Age::Newer)
            );
            assert_eq!(
                bound_decides(at(10, 5), at(10, 5), BILLION),
                Some(Age::Newer),
                "equal counts as at-least-as-new"
            );
        }

        #[test]
        fn the_bound_decides_when_the_source_stays_later_after_the_widest_cut() {
            // Source a full second later; even truncating to whole seconds
            // leaves it ahead.
            assert_eq!(
                bound_decides(at(10, 0), at(11, 0), BILLION),
                Some(Age::Older)
            );
        }

        #[test]
        fn the_bound_leaves_the_close_case_to_the_measurement() {
            // Same second, source later by a fraction a one-second clock would
            // erase and a nanosecond clock would not. This is exactly the pair
            // that has to touch the disk.
            assert_eq!(bound_decides(at(10, 0), at(10, 500_000_000), BILLION), None);
        }

        #[test]
        fn the_readback_reports_the_resolution_that_survived() {
            // The probe for a one-second bound was 111_111_111 ns and all of it
            // survived, so the filesystem stores nanoseconds.
            assert_eq!(exact_from_readback(111_111_111, BILLION), 1);
            // Only the top three digits survived: milliseconds.
            assert_eq!(exact_from_readback(111_000_000, BILLION), 1_000_000);
            // Nothing survived and the bound was a whole second, so the clock
            // is whole seconds — not two, because the cap says so.
            assert_eq!(exact_from_readback(0, BILLION), BILLION);
            // Nothing survived under the two-second bound, and the forced odd
            // second did not survive either: the two-second clock.
            assert_eq!(exact_from_readback(0, WORST_RESOLUTION), WORST_RESOLUTION);
            // The forced odd second survived, so seconds are stored singly.
            assert_eq!(exact_from_readback(BILLION, WORST_RESOLUTION), BILLION);
        }

        /// A file with a known modification time, on whatever filesystem the
        /// suite runs on.
        fn scratch_file(prefix: &str, m: Stamp) -> (scratchdir::ScratchDir, std::path::PathBuf) {
            use std::io::Write as _;
            let dir = scratchdir::ScratchDir::new(prefix);
            let path = dir.path("probe");
            let mut f = fs::File::create(&path).expect("create");
            f.write_all(b"x").expect("write");
            fsattr::set_times(
                On::Path(&path, Link::NoFollow),
                Times {
                    accessed: When::Set(instant(m).expect("representable")),
                    modified: When::Set(instant(m).expect("representable")),
                },
            )
            .expect("stamp");
            (dir, path)
        }

        #[test]
        fn the_measurement_writes_a_probe_and_puts_the_original_back() {
            // The one test here that reaches the syscalls. It calls the
            // measurement directly rather than going through
            // [`super::utimecmp`], because the bound deduced from a real file's
            // three stamps is almost always one nanosecond — `ctime` is set by
            // the stamping call itself and carries whatever the clock said —
            // so the composed path would return before ever probing.
            let original = Stamp {
                sec: 1_000_000_000,
                nsec: 0,
            };
            let (_dir, path) = scratch_file("utimecmp_probe", original);

            let res = measure_resolution(&path, original, original, BILLION)
                .expect("the probe should have been writable and readable");
            assert_eq!(
                res, 1,
                "every filesystem these tests can run on stores nanoseconds"
            );

            let after = fs::symlink_metadata(&path).expect("restat");
            assert_eq!(
                mtime(&after),
                original,
                "the probe was left on the file instead of being undone"
            );
        }

        #[test]
        fn the_measurement_forces_an_odd_second_under_the_two_second_bound() {
            // At `WORST_RESOLUTION` the probe also flips the seconds' low bit,
            // since that is the digit a two-second clock cannot keep. Surviving
            // it is what rules the two-second clock out.
            let original = Stamp {
                sec: 1_000_000_000,
                nsec: 0,
            };
            let (_dir, path) = scratch_file("utimecmp_probe2", original);

            let res = measure_resolution(&path, original, original, WORST_RESOLUTION)
                .expect("the probe should have been writable and readable");
            assert_eq!(res, 1);
            let after = fs::symlink_metadata(&path).expect("restat");
            assert_eq!(mtime(&after), original, "the odd second was left behind");
        }

        #[test]
        fn a_timestamp_that_is_not_an_instant_is_refused_rather_than_substituted() {
            // Every second a 64-bit `st_mtime` can hold is a `SystemTime` on
            // this platform, so the guard is reached only by a *sum* that
            // carries past the last representable second — which is what the
            // probe builds, `nsec + res / 9`. The measurement must decline
            // rather than write some other time onto the file: a wrong
            // timestamp on a real file is worse than "cannot tell".
            assert_eq!(instant(at(i64::MAX, BILLION)), None);
            let original = Stamp {
                sec: 1_000_000_000,
                nsec: 0,
            };
            let (_dir, path) = scratch_file("utimecmp_absurd", original);
            assert_eq!(
                measure_resolution(&path, at(i64::MAX, BILLION), original, BILLION),
                None
            );
            let after = fs::symlink_metadata(&path).expect("restat");
            assert_eq!(
                mtime(&after),
                original,
                "a refused probe still moved the file's timestamps"
            );
        }

        #[test]
        fn an_instant_round_trips_through_the_fsattr_form() {
            for stamp in [at(0, 0), at(10, 500_000_007), at(-1, 250), at(-86_400, 0)] {
                let form = instant(stamp).expect("representable");
                let (sec, nsec) = match form.duration_since(SystemTime::UNIX_EPOCH) {
                    Ok(d) => (
                        i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                        i64::from(d.subsec_nanos()),
                    ),
                    Err(e) => {
                        let d = e.duration();
                        let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
                        let nanos = i64::from(d.subsec_nanos());
                        if nanos == 0 {
                            (-secs, 0)
                        } else {
                            (-(secs + 1), BILLION - nanos)
                        }
                    }
                };
                assert_eq!(at(sec, nsec), stamp, "{stamp:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsattr::{self, Link, On, Times};
    use scratchdir::ScratchDir;
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    /// A source and a destination stamped a known distance apart, on whatever
    /// filesystem the test suite runs on.
    fn pair(dir: &ScratchDir, src_offset: Duration) -> (PathBuf, PathBuf) {
        let src = dir.path("src");
        let dst = dir.path("dst");
        for name in [&src, &dst] {
            let mut f = fs::File::create(name).expect("create");
            f.write_all(b"x").expect("write");
        }
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        stamp(&dst, base);
        stamp(&src, base + src_offset);
        (src, dst)
    }

    fn stamp(path: &Path, at: SystemTime) {
        fsattr::set_times(On::Path(path, Link::NoFollow), Times::both(at)).expect("stamp");
    }

    fn meta(path: &Path) -> fs::Metadata {
        fs::symlink_metadata(path).expect("stat")
    }

    #[test]
    fn a_newer_source_makes_the_destination_older() {
        let dir = ScratchDir::new("utimecmp_newer");
        let (src, dst) = pair(&dir, Duration::from_secs(10));
        assert_eq!(utimecmp(&dst, &meta(&dst), &meta(&src), false), Age::Older);
        assert_eq!(utimecmp(&dst, &meta(&dst), &meta(&src), true), Age::Older);
    }

    #[test]
    fn an_identically_stamped_pair_is_at_least_as_new_either_way() {
        let dir = ScratchDir::new("utimecmp_same");
        let (src, dst) = pair(&dir, Duration::ZERO);
        assert_eq!(utimecmp(&dst, &meta(&dst), &meta(&src), false), Age::Same);
        assert_eq!(utimecmp(&dst, &meta(&dst), &meta(&src), true), Age::Same);
    }

    #[test]
    fn an_older_source_makes_the_destination_newer() {
        let dir = ScratchDir::new("utimecmp_older");
        let (src, dst) = pair(&dir, Duration::ZERO);
        stamp(
            &src,
            SystemTime::UNIX_EPOCH + Duration::from_secs(999_999_000),
        );
        assert_eq!(utimecmp(&dst, &meta(&dst), &meta(&src), false), Age::Newer);
        assert_eq!(utimecmp(&dst, &meta(&dst), &meta(&src), true), Age::Newer);
    }

    #[test]
    fn a_destination_stamped_from_its_source_never_reads_as_older() {
        // The convergence property, on the real filesystem the suite runs on:
        // whatever its resolution, a destination that was given the source's
        // timestamp must not read as older than that source afterwards. That is
        // the loop `-u` exists to avoid, and the loop truncation closes. With
        // `truncate_source = false` this can fail on a coarse filesystem, which
        // is the whole point of the flag.
        let dir = ScratchDir::new("utimecmp_converge");
        for nanos in [1_u64, 999, 500_000_000, 999_999_999] {
            let (src, dst) = pair(&dir, Duration::from_nanos(nanos));
            let carried = fsattr::times_of(&meta(&src)).expect("times of src");
            fsattr::set_times(On::Path(&dst, Link::NoFollow), carried).expect("carry the stamp");
            assert!(
                utimecmp(&dst, &meta(&dst), &meta(&src), true).at_least_as_new(),
                "a destination stamped from its source read as older at {nanos} ns"
            );
        }
    }

    #[test]
    fn a_measured_comparison_leaves_the_destinations_timestamps_alone() {
        // The measurement writes to the destination and must put it back. Ask
        // the question on a pair inside two seconds — the only pairs that can
        // reach the probe — and check the destination is unchanged.
        let dir = ScratchDir::new("utimecmp_restores");
        let (src, dst) = pair(&dir, Duration::from_nanos(500_000_000));
        let before = fsattr::times_of(&meta(&dst)).expect("times before");
        let _ = utimecmp(&dst, &meta(&dst), &meta(&src), true);
        let after = fsattr::times_of(&meta(&dst)).expect("times after");
        let (fsattr::When::Set(b), fsattr::When::Set(a)) = (before.modified, after.modified) else {
            panic!("times_of returned an omitted stamp");
        };
        assert_eq!(a, b, "the destination's modification time was left changed");
    }

    #[test]
    fn the_unknown_answer_does_not_count_as_at_least_as_new() {
        assert!(!Age::Unknown.at_least_as_new());
        assert!(!Age::Older.at_least_as_new());
        assert!(Age::Same.at_least_as_new());
        assert!(Age::Newer.at_least_as_new());
    }
}
