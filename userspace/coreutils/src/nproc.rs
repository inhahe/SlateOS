//! gnulib's `num_processors`: how many processing units this process may
//! use. `nproc` prints it, and `sort` sorts with as many threads, up to eight
//! -- which is not only speed: upstream sizes its buffer by the thread count,
//! so it decides when `sort` spills to a temporary file.
//!
//! # What the number is
//!
//! For [`Query::Current`] and [`Query::CurrentOverridable`], the processing
//! units *this process* may run on:
//!
//! 1. under `CurrentOverridable` only, `OMP_NUM_THREADS`, if it holds a
//!    positive number, capped by `OMP_THREAD_LIMIT` -- gnulib honours the
//!    OpenMP variables because every OpenMP program does;
//! 2. otherwise the CPUs in the affinity mask (`sched_getaffinity`), or if
//!    that fails the online count (`sysconf(_SC_NPROCESSORS_ONLN)`), capped
//!    by `OMP_THREAD_LIMIT` under `CurrentOverridable`.
//!
//! For [`Query::All`], the installed processors
//! (`sysconf(_SC_NPROCESSORS_CONF)`), raised to the affinity count when the
//! configured count is 1 or 2 and the mask shows more -- glibc answers 1 or 2
//! when `/sys` is not mounted, and gnulib guarantees it is never less than
//! the plain answer. The OpenMP variables do not apply to it.
//!
//! Moved here from the `nproc` binary when `sort` needed the same answer.

use crate::quote::os_bytes;

/// gnulib's `enum nproc_query`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    /// `NPROC_ALL`: installed processors.
    All,
    /// `NPROC_CURRENT`: what this process may run on.
    Current,
    /// `NPROC_CURRENT_OVERRIDABLE`: the same, unless OpenMP says otherwise.
    CurrentOverridable,
}

/// What `num_processors` asks the system. A trait so that the tests can pin
/// the answers a real machine will not give on demand.
pub trait Cpus {
    /// `num_processors_via_affinity_mask`: the CPUs in this process's
    /// affinity mask, or 0 if the call fails.
    fn affinity(&self) -> u64;
    /// `sysconf(_SC_NPROCESSORS_ONLN)`; not positive when unknown.
    fn online(&self) -> i64;
    /// `sysconf(_SC_NPROCESSORS_CONF)`; not positive when unknown.
    fn configured(&self) -> i64;
}

/// gnulib's `c_isspace`: the C locale's six, whatever the locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// gnulib's `parse_omp_threads`: the number at the front of an OpenMP
/// variable, or 0 for "not set, or not a number".
///
/// Leading and trailing white space is allowed, as the OpenMP specification
/// says; so is a comma after the number, since the first value of a nesting
/// list is the outermost level's. Anything else makes the whole value 0.
/// Overflow saturates, as `strtoul` does.
pub fn parse_omp_threads(value: Option<&[u8]>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    let start = value
        .iter()
        .position(|&b| !is_c_space(b))
        .unwrap_or(value.len());
    let rest = value.get(start..).unwrap_or_default();
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return 0;
    }
    let number = rest
        .get(..digits)
        .unwrap_or_default()
        .iter()
        .fold(0u64, |n, &d| {
            let digit = u64::from(char::from(d).to_digit(10).unwrap_or(0));
            n.checked_mul(10)
                .and_then(|n| n.checked_add(digit))
                .unwrap_or(u64::MAX)
        });
    let tail = rest.get(digits..).unwrap_or_default();
    match tail.iter().find(|&&b| !is_c_space(b)) {
        // Nothing but white space after the number, or the end of the first
        // level of a nesting list.
        None | Some(&b',') => number,
        Some(_) => 0,
    }
}

/// A positive `sysconf` answer as a count.
fn positive(n: i64) -> Option<u64> {
    u64::try_from(n).ok().filter(|&n| n > 0)
}

/// gnulib's `num_processors_ignoring_omp`: never less than 1.
fn ignoring_omp(query: Query, cpus: &impl Cpus) -> u64 {
    if query == Query::All {
        let mut configured = cpus.configured();
        // glibc answers 1 or 2 when /sys and /proc are not mounted; the mask
        // is the better answer then, and `--all` must never be the smaller.
        if configured == 1 || configured == 2 {
            let current = i64::try_from(cpus.affinity()).unwrap_or(i64::MAX);
            if current > configured {
                configured = current;
            }
        }
        positive(configured).unwrap_or(1)
    } else {
        let affinity = cpus.affinity();
        if affinity > 0 {
            return affinity;
        }
        positive(cpus.online()).unwrap_or(1)
    }
}

/// gnulib's `num_processors`.
pub fn num_processors(
    query: Query,
    omp_num_threads: Option<&[u8]>,
    omp_thread_limit: Option<&[u8]>,
    cpus: &impl Cpus,
) -> u64 {
    let mut limit = u64::MAX;
    let mut query = query;
    if query == Query::CurrentOverridable {
        let threads = parse_omp_threads(omp_num_threads);
        limit = match parse_omp_threads(omp_thread_limit) {
            0 => u64::MAX,
            n => n,
        };
        if threads != 0 {
            return threads.min(limit);
        }
        query = Query::Current;
    }
    ignoring_omp(query, cpus).min(limit)
}

/// The running system's answer to `query`, the OpenMP variables read from the
/// environment: upstream's `num_processors (query)`.
#[must_use]
pub fn now(query: Query) -> u64 {
    let threads = std::env::var_os("OMP_NUM_THREADS");
    let limit = std::env::var_os("OMP_THREAD_LIMIT");
    num_processors(
        query,
        threads.as_deref().map(os_bytes).as_deref(),
        limit.as_deref().map(os_bytes).as_deref(),
        &Live,
    )
}

#[cfg(unix)]
mod live {
    use super::Cpus;

    /// `<unistd.h>`'s names. 83 and 84 on glibc, and
    /// `posix::unistd::_SC_NPROCESSORS_CONF`/`_ONLN` are 83 and 84.
    const SC_NPROCESSORS_CONF: i32 = 83;
    const SC_NPROCESSORS_ONLN: i32 = 84;

    unsafe extern "C" {
        fn sched_getaffinity(pid: i32, size: usize, mask: *mut u64) -> i32;
        fn sysconf(name: i32) -> i64;
    }

    /// The running system.
    pub struct Live;

    impl Cpus for Live {
        fn affinity(&self) -> u64 {
            // glibc's `cpu_set_t`: 1024 bits. A machine with more CPUs than
            // that makes the call fail, and the online count answers instead
            // -- which is what upstream's fixed-size set does too.
            let mut set = [0u64; 16];
            // SAFETY: `set` is a writable buffer of exactly the size passed,
            // and the kernel writes no more than that into it. Pid 0 is this
            // thread.
            let rc = unsafe { sched_getaffinity(0, std::mem::size_of_val(&set), set.as_mut_ptr()) };
            if rc != 0 {
                return 0;
            }
            set.iter().map(|word| u64::from(word.count_ones())).sum()
        }

        fn online(&self) -> i64 {
            // SAFETY: `sysconf` takes an integer and touches no memory of ours.
            unsafe { sysconf(SC_NPROCESSORS_ONLN) }
        }

        fn configured(&self) -> i64 {
            // SAFETY: as above.
            unsafe { sysconf(SC_NPROCESSORS_CONF) }
        }
    }
}

#[cfg(unix)]
pub use live::Live;

/// The running system, where there is no `sched_getaffinity` and no `sysconf`
/// to ask: nothing is known, and [`num_processors`] answers 1.
#[cfg(not(unix))]
pub struct Live;

#[cfg(not(unix))]
impl Cpus for Live {
    fn affinity(&self) -> u64 {
        0
    }
    fn online(&self) -> i64 {
        -1
    }
    fn configured(&self) -> i64 {
        -1
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    struct Fake {
        affinity: u64,
        online: i64,
        configured: i64,
    }

    impl Cpus for Fake {
        fn affinity(&self) -> u64 {
            self.affinity
        }
        fn online(&self) -> i64 {
            self.online
        }
        fn configured(&self) -> i64 {
            self.configured
        }
    }

    /// Sixteen installed, twelve online, pinned to four.
    const PINNED: Fake = Fake {
        affinity: 4,
        online: 12,
        configured: 16,
    };

    fn count(query: Query, threads: Option<&str>, limit: Option<&str>, cpus: &Fake) -> u64 {
        num_processors(
            query,
            threads.map(str::as_bytes),
            limit.map(str::as_bytes),
            cpus,
        )
    }

    #[test]
    fn the_plain_answer_is_the_affinity_mask() {
        assert_eq!(count(Query::CurrentOverridable, None, None, &PINNED), 4);
        let no_mask = Fake {
            affinity: 0,
            ..PINNED
        };
        assert_eq!(count(Query::CurrentOverridable, None, None, &no_mask), 12);
        let nothing = Fake {
            affinity: 0,
            online: -1,
            configured: -1,
        };
        assert_eq!(count(Query::CurrentOverridable, None, None, &nothing), 1);
    }

    #[test]
    fn all_is_the_installed_count_and_never_less_than_the_mask() {
        assert_eq!(count(Query::All, None, None, &PINNED), 16);
        // glibc's 1-or-2 answer without /sys, beaten by the mask...
        let no_sys = Fake {
            affinity: 8,
            online: 1,
            configured: 2,
        };
        assert_eq!(count(Query::All, None, None, &no_sys), 8);
        // ...but a real 3 is believed even under a larger mask.
        let three = Fake {
            affinity: 8,
            online: 3,
            configured: 3,
        };
        assert_eq!(count(Query::All, None, None, &three), 3);
    }

    #[test]
    fn openmp_overrides_the_plain_answer_but_not_all() {
        assert_eq!(
            count(Query::CurrentOverridable, Some("7"), None, &PINNED),
            7
        );
        assert_eq!(
            count(Query::CurrentOverridable, Some("7"), Some("3"), &PINNED),
            3
        );
        // The limit alone caps the system's answer.
        assert_eq!(
            count(Query::CurrentOverridable, None, Some("2"), &PINNED),
            2
        );
        assert_eq!(
            count(Query::CurrentOverridable, None, Some("9"), &PINNED),
            4
        );
        // --all ignores both.
        assert_eq!(count(Query::All, Some("7"), Some("3"), &PINNED), 16);
    }

    #[test]
    fn openmp_values_are_parsed_as_gnulib_parses_them() {
        let p = |s: &str| parse_omp_threads(Some(s.as_bytes()));
        assert_eq!(p("5"), 5);
        assert_eq!(p("  5  "), 5);
        assert_eq!(p("\t5\n"), 5);
        assert_eq!(p("5,3,2"), 5);
        assert_eq!(p("5 ,3"), 5);
        assert_eq!(p("0"), 0);
        assert_eq!(p("-5"), 0);
        assert_eq!(p("+5"), 0);
        assert_eq!(p("5x"), 0);
        assert_eq!(p("0x10"), 0);
        assert_eq!(p(""), 0);
        assert_eq!(p("99999999999999999999999"), u64::MAX);
        assert_eq!(parse_omp_threads(None), 0);
        // A zero or invalid count means "not set".
        assert_eq!(
            count(Query::CurrentOverridable, Some("0"), None, &PINNED),
            4
        );
        assert_eq!(
            count(Query::CurrentOverridable, Some("x"), Some("0"), &PINNED),
            4
        );
    }
}
