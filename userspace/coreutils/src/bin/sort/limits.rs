//! The resources `sort` is told it may use, read as upstream reads them:
//! `-S`/`--buffer-size` (`specify_sort_size`), `--batch-size`
//! (`specify_nmerge`) and `--parallel` (`specify_nthreads`), with the sizes
//! of upstream's own structures they are measured against, and the system's
//! memory and limits its defaults come from.
//!
//! What they decide -- how large a buffer is, how many files one merge reads,
//! how many threads sort a buffer -- changes nothing in the output; it
//! changes when a temporary file is made, which is observable (`-T` that
//! names no directory, `--compress-program`), and how much memory a large
//! input takes. Each refusal is upstream's word for word: `xstrtol_fatal`'s
//! straight-quoted `invalid -S argument 'x'`, `specify_nmerge`'s curly-quoted
//! pair of lines.

use coreutils::quote::quote;
use coreutils::xnum::{self, Status};

/// `sizeof (struct line)` on a 64-bit target: the text, its length, and the
/// first key's start and end -- four pointer-sized fields. Upstream's buffers
/// hold a `struct line` for each line, and how many lines fit is worked out
/// in these units.
pub const LINE: usize = 32;

/// `MIN_MERGE_BUFFER_SIZE`: room for one line and its terminator.
pub const MIN_MERGE_BUFFER_SIZE: usize = 2 + LINE;

/// `NMERGE_DEFAULT`: how many files one merge reads, unless `--batch-size`
/// says otherwise.
pub const NMERGE_DEFAULT: u32 = 16;

/// `DEFAULT_MAX_THREADS`: the most threads `sort` uses unless `--parallel`
/// asks for more.
pub const DEFAULT_MAX_THREADS: u64 = 8;

/// `MIN_SORT_SIZE`: `nmerge` merge buffers. Below this the merge could not
/// give each of its inputs a line's room.
pub fn min_sort_size(nmerge: u32) -> usize {
    usize::try_from(nmerge)
        .unwrap_or(usize::MAX)
        .saturating_mul(MIN_MERGE_BUFFER_SIZE)
}

/// The suffixes `-S` accepts, each a power of 1024 (`xstrtoumax`'s list):
/// `k`/`K`, `m`/`M`, `g`/`G`, `t`/`T`, `P`, `E`, `Z`, `Y`, `R`, `Q`.
const SIZE_SUFFIXES: &[u8] = b"EgGkKmMPQRtTYZ";

/// `-S SIZE`, upstream's `specify_sort_size`: the size in bytes, given the
/// size already asked for (`current`, 0 when none was).
///
/// A bare number is KiB; `b` is bytes and `%` a percentage of physical
/// memory, either only as the last character and only after a digit; the
/// other suffixes are powers of 1024. Given more than once, the largest wins,
/// so the order of the options does not matter. Never below
/// [`min_sort_size`].
///
/// # Errors
///
/// `invalid -S argument 'x'`, `invalid suffix in -S argument '1x'` or `-S
/// argument '10Q' too large`, naming the option as it was typed (`-S` or
/// `--buffer-size`).
pub fn specify_sort_size(
    current: usize,
    nmerge: u32,
    option: &str,
    arg: &[u8],
    physmem_total: impl FnOnce() -> f64,
) -> Result<usize, String> {
    let (mut n, mut status, end) = xnum::xstrtoumax_end(arg, 10, Some(SIZE_SUFFIXES));
    let after_digit = end
        .checked_sub(1)
        .and_then(|at| arg.get(at))
        .is_some_and(u8::is_ascii_digit);
    // The default unit is KiB.
    if status == Status::Ok && after_digit {
        match n.checked_mul(1024) {
            Some(bytes) => n = bytes,
            None => status = Status::Overflow,
        }
    }
    // `b` means bytes and `%` a percentage of memory -- each only alone at
    // the end, after a digit.
    if status == Status::InvalidSuffix && after_digit && arg.len() == end.saturating_add(1) {
        match arg.get(end) {
            Some(b'b') => status = Status::Ok,
            Some(b'%') => {
                #[allow(clippy::cast_precision_loss)] // as upstream's double
                let mem = physmem_total() * n as f64 / 100.0;
                // `<`, not `<=`: upstream's guard against rounding up past
                // the maximum.
                #[allow(clippy::cast_precision_loss)]
                if mem < u64::MAX as f64 {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        n = mem as u64;
                    }
                    status = Status::Ok;
                } else {
                    status = Status::Overflow;
                }
            }
            _ => {}
        }
    }
    if status == Status::Ok {
        let Ok(size) = usize::try_from(n) else {
            return Err(fatal_message(Status::Overflow, option, arg));
        };
        if size < current {
            return Ok(current);
        }
        return Ok(size.max(min_sort_size(nmerge)));
    }
    Err(fatal_message(status, option, arg))
}

/// `xstrtol_fatal`'s sentence for a status that is not `Ok`.
fn fatal_message(status: Status, option: &str, arg: &[u8]) -> String {
    xnum::strtol_fatal(status, option, arg)
        .unwrap_or_else(|| format!("invalid {option} argument {}", quote(arg)))
}

/// `--batch-size=NMERGE`, upstream's `specify_nmerge`: how many files one
/// merge may read, at least 2 and at most what the descriptor limit leaves
/// once standard input, output and error are counted (`max_nmerge`, which
/// [`max_nmerge`] works out).
///
/// # Errors
///
/// Upstream's words, the two-line refusals as two lines: `invalid
/// --batch-size argument '1'` then `minimum --batch-size argument is '2'`;
/// `--batch-size argument 'N' too large` then `maximum --batch-size argument
/// with current rlimit is M`; or `invalid --batch-size argument 'x'` alone.
pub fn specify_nmerge(arg: &[u8], max_nmerge: u32) -> Result<u32, String> {
    const NAME: &str = "batch-size";
    let (n, status) = xnum::xstrtoumax(arg, Some(b""));
    let status = match status {
        Status::Ok => match u32::try_from(n) {
            Ok(nmerge) if nmerge < 2 => {
                return Err(format!(
                    "invalid --{NAME} argument {}\nsort: minimum --{NAME} argument is {}",
                    quote(arg),
                    quote(b"2")
                ));
            }
            Ok(nmerge) if nmerge <= max_nmerge => return Ok(nmerge),
            _ => Status::Overflow,
        },
        other => other,
    };
    if status == Status::Overflow {
        return Err(format!(
            "--{NAME} argument {} too large\nsort: maximum --{NAME} argument with current rlimit is {max_nmerge}",
            quote(arg)
        ));
    }
    Err(fatal_message(status, &format!("--{NAME}"), arg))
}

/// The largest `--batch-size`: the descriptor limit less the three standard
/// descriptors, in upstream's `unsigned int` -- so a limit of "infinity"
/// wraps, as upstream's does. `OPEN_MAX` (20, `<limits.h>`'s historical
/// minimum, which glibc's `getrlimit` never leaves us needing) when there is
/// no limit to ask.
pub fn max_nmerge(nofile: Option<u64>) -> u32 {
    #[allow(clippy::cast_possible_truncation)] // upstream's unsigned int
    {
        nofile.unwrap_or(20).wrapping_sub(3) as u32
    }
}

/// `--parallel=N`, upstream's `specify_nthreads`: how many threads may sort.
///
/// # Errors
///
/// `number in parallel must be nonzero` for 0, and `xstrtol_fatal`'s
/// sentence for anything that is not a number. A number too large for the
/// type is not refused: it is every thread there could be.
pub fn specify_nthreads(option: &str, arg: &[u8]) -> Result<usize, String> {
    let (n, status) = xnum::xstrtoumax(arg, Some(b""));
    match status {
        Status::Overflow => return Ok(usize::MAX),
        Status::Ok => {}
        other => return Err(fatal_message(other, option, arg)),
    }
    if n == 0 {
        return Err("number in parallel must be nonzero".to_string());
    }
    Ok(usize::try_from(n).unwrap_or(usize::MAX))
}

/// gnulib's `physmem_total`: physical memory in bytes, or 0 when it cannot
/// be learned.
pub fn physmem_total() -> f64 {
    sys::pages(sys::SC_PHYS_PAGES)
}

/// gnulib's `physmem_available`: free physical memory in bytes, or 0.
pub fn physmem_available() -> f64 {
    sys::pages(sys::SC_AVPHYS_PAGES)
}

/// The soft limit on `resource`, or `None` when it cannot be asked.
pub fn rlimit(resource: Resource) -> Option<u64> {
    sys::rlimit(resource)
}

/// The limits upstream consults.
#[derive(Clone, Copy)]
pub enum Resource {
    /// `RLIMIT_DATA`.
    Data,
    /// `RLIMIT_AS`.
    AddressSpace,
    /// `RLIMIT_RSS`.
    Rss,
    /// `RLIMIT_NOFILE`.
    Files,
}

#[cfg(unix)]
mod sys {
    use super::Resource;

    /// `<unistd.h>`'s names, glibc's numbers -- which the SlateOS C library
    /// uses too (`posix::unistd::_SC_*`).
    pub const SC_PHYS_PAGES: i32 = 85;
    pub const SC_AVPHYS_PAGES: i32 = 86;
    const SC_PAGESIZE: i32 = 30;

    #[repr(C)]
    struct Rlimit {
        cur: u64,
        max: u64,
    }

    unsafe extern "C" {
        fn sysconf(name: i32) -> i64;
        fn getrlimit(resource: i32, rlim: *mut Rlimit) -> i32;
    }

    /// `sysconf (pages) * sysconf (_SC_PAGESIZE)`, or 0 when either is not
    /// positive -- gnulib's test.
    pub fn pages(name: i32) -> f64 {
        // SAFETY: `sysconf` takes an integer and touches no memory of ours.
        let (count, size) = unsafe { (sysconf(name), sysconf(SC_PAGESIZE)) };
        if count < 0 || size < 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)] // gnulib computes in double
        {
            count as f64 * size as f64
        }
    }

    pub fn rlimit(resource: Resource) -> Option<u64> {
        // Linux's numbers, which the SlateOS C library shares.
        let number = match resource {
            Resource::Data => 2,
            Resource::Rss => 5,
            Resource::Files => 7,
            Resource::AddressSpace => 9,
        };
        let mut limit = Rlimit { cur: 0, max: 0 };
        // SAFETY: `limit` is a writable `struct rlimit` -- two `rlim_t`,
        // which is `u64` on every 64-bit target this builds for -- and
        // `getrlimit` writes nothing else.
        let rc = unsafe { getrlimit(number, &raw mut limit) };
        (rc == 0).then_some(limit.cur)
    }
}

#[cfg(not(unix))]
mod sys {
    use super::Resource;

    pub const SC_PHYS_PAGES: i32 = 85;
    pub const SC_AVPHYS_PAGES: i32 = 86;

    /// No `sysconf` off unix: nothing known.
    pub fn pages(_name: i32) -> f64 {
        0.0
    }

    pub fn rlimit(_resource: Resource) -> Option<u64> {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn size(arg: &str) -> Result<usize, String> {
        specify_sort_size(0, NMERGE_DEFAULT, "-S", arg.as_bytes(), || 1024.0 * 1024.0)
    }

    #[test]
    fn a_bare_size_is_kib_and_b_is_bytes() {
        assert_eq!(size("1"), Ok(1024));
        assert_eq!(size("1000b"), Ok(1000));
        assert_eq!(size("2K"), Ok(2048));
        assert_eq!(size("1M"), Ok(1024 * 1024));
        // Never below room for sixteen merge buffers.
        assert_eq!(size("1b"), Ok(16 * 34));
        assert_eq!(size("50%"), Ok(512 * 1024));
    }

    #[test]
    fn a_bad_size_is_refused_in_xstrtol_fatals_words() {
        assert_eq!(size("x").unwrap_err(), "invalid -S argument 'x'");
        assert_eq!(
            size("1x").unwrap_err(),
            "invalid suffix in -S argument '1x'"
        );
        assert_eq!(size("10Q").unwrap_err(), "-S argument '10Q' too large");
        // `b` only after a digit and only at the end.
        assert_eq!(
            size("1bb").unwrap_err(),
            "invalid suffix in -S argument '1bb'"
        );
    }

    #[test]
    fn the_largest_size_wins() {
        let first = specify_sort_size(0, 16, "-S", b"4K", || 0.0).unwrap();
        let second = specify_sort_size(first, 16, "-S", b"1K", || 0.0).unwrap();
        assert_eq!(second, 4096);
    }

    #[test]
    fn batch_size_has_a_floor_and_a_ceiling() {
        assert_eq!(specify_nmerge(b"2", 100), Ok(2));
        assert_eq!(
            specify_nmerge(b"1", 100).unwrap_err(),
            "invalid --batch-size argument \u{2018}1\u{2019}\nsort: minimum --batch-size argument is \u{2018}2\u{2019}"
        );
        assert_eq!(
            specify_nmerge(b"101", 100).unwrap_err(),
            "--batch-size argument \u{2018}101\u{2019} too large\nsort: maximum --batch-size argument with current rlimit is 100"
        );
        assert_eq!(
            specify_nmerge(b"x", 100).unwrap_err(),
            "invalid --batch-size argument 'x'"
        );
        assert_eq!(max_nmerge(Some(1024)), 1021);
        assert_eq!(max_nmerge(Some(u64::MAX)), u32::MAX - 3);
    }

    #[test]
    fn parallel_must_be_a_positive_number() {
        assert_eq!(specify_nthreads("--parallel", b"4"), Ok(4));
        assert_eq!(
            specify_nthreads("--parallel", b"0").unwrap_err(),
            "number in parallel must be nonzero"
        );
        assert_eq!(
            specify_nthreads("--parallel", b"x").unwrap_err(),
            "invalid --parallel argument 'x'"
        );
        assert_eq!(
            specify_nthreads("--parallel", b"99999999999999999999999"),
            Ok(usize::MAX)
        );
    }
}
