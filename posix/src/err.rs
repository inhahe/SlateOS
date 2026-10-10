//! BSD error/warning functions (`<err.h>`).
//!
//! Provides `err`, `errx`, `warn`, `warnx`, `verr`, `verrx`, `vwarn`,
//! `vwarnx` for formatted error messages to stderr.  These are not
//! strictly POSIX but are very widely used by Unix utilities (BSD,
//! macOS, and glibc all provide them).
//!
//! ## What they print: glibc's, byte for byte
//!
//! - `warn`/`vwarn`: `progname: fmt-args: strerror(errno)\n`, and with a NULL
//!   format `progname: strerror(errno)\n`
//! - `warnx`/`vwarnx`: `progname: fmt-args\n`, and with a NULL format
//!   `progname: \n`
//! - `err`/`verr`: `warn`, then `exit(eval)`
//! - `errx`/`verrx`: `warnx`, then `exit(eval)`
//!
//! `progname` is `__progname`, the last component of argv[0] -- unlike
//! `error`'s, which is argv[0] whole. `errno` is the caller's, read before
//! anything is written. `stdout` is not flushed first, as glibc's are not
//! (`error` is the family that does). As `posix/tools/oracle/errfns_harness.py`
//! recorded glibc 2.39's answers (`errfns_oracle.txt`), replayed by the
//! tests.
//!
//! The message goes through the `stderr` stream, under its lock, whole: it
//! was formatted into a 1024-byte buffer and cut there until 2026-09-29, and
//! written around the stream to file descriptor 2 (known-issues.md ->
//! D-POSIX-ERROR-PRINTED-THE-SHORT-NAME-AND-CUT-LONG-MESSAGES).
//!
//! ## Implementation
//!
//! The C prototypes are variadic, e.g. `void err(int, const char *fmt, ...)`.
//! Like [`crate::printf`], the variadic entry points are assembly trampolines
//! that perform a real `va_start` — spilling the argument registers into a
//! System V register save area and building a `va_list` over it — and then
//! call the matching `v*` variant.  Those variants are plain Rust and take
//! the `va_list` directly, so they are host-testable and there is exactly one
//! argument-delivery path.  Both funnel through [`emit`].

use crate::errno;
use crate::error::{put, put_cstr};
use crate::printf::{self, VaList};

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

// ---------------------------------------------------------------------------
// Assembly trampolines — `va_start`, then call the matching `v*` variant.
//
// The named-argument counts decide the initial `gp_offset` and which register
// carries the `va_list*`:
//   warn/warnx  : 1 named arg (fmt)        — same shape as `printf`
//   err/errx    : 2 named args (eval, fmt) — same shape as `fprintf`
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
va_trampoline!("warn", "vwarn", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("warnx", "vwarnx", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("err", "verr", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("errx", "verrx", "16", "rdx");

/// The whole family, into `out`: `progname: `, the expanded format, and with
/// `with_errno` `: strerror(errno)` (without the `: ` for a NULL format),
/// then a newline.
fn emit(out: *mut u8, fmt: *const u8, args: &mut printf::Args, with_errno: bool) {
    // The caller's error, before the writes below can change it.
    let saved_errno = errno::get_errno();
    crate::stdio::flockfile(out.cast());
    // SAFETY: a plain read of the pointer `__libc_start_main` set.
    put_cstr(out, unsafe { crate::crt::progname_slot().read() });
    put(out, b": ");
    if !fmt.is_null() {
        // What a failed write loses is the message itself, as in `put`.
        let _ = printf::_fprintf_impl(out, fmt, args);
        if with_errno {
            put(out, b": ");
        }
    }
    if with_errno {
        put_cstr(out, crate::string::strerror(saved_errno));
    }
    put(out, b"\n");
    crate::stdio::funlockfile(out.cast());
}

// ---------------------------------------------------------------------------
// v* variants — take a `va_list` (pointer); pure Rust, host-testable, and the
// delegation target of the trampolines above.
// ---------------------------------------------------------------------------

/// `vwarn(fmt, ap)` — `warn` with a `va_list`.
///
/// # Safety
/// `fmt` must be a valid NUL-terminated format string and `ap` a valid
/// `va_list` whose arguments match `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vwarn(fmt: *const u8, ap: *mut VaList) {
    // SAFETY: the caller guarantees `ap` is a valid va_list matching `fmt`;
    // a null one is rendered as zero arguments rather than a fault.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(crate::stdio::stderr_stream(), fmt, &mut args, true);
}

/// `vwarnx(fmt, ap)` — `warnx` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vwarnx(fmt: *const u8, ap: *mut VaList) {
    // SAFETY: as `vwarn`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(crate::stdio::stderr_stream(), fmt, &mut args, false);
}

/// `verr(eval, fmt, ap)` — `err` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verr(eval: i32, fmt: *const u8, ap: *mut VaList) -> ! {
    // SAFETY: as `vwarn`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(crate::stdio::stderr_stream(), fmt, &mut args, true);
    crate::crt::exit(eval);
}

/// `verrx(eval, fmt, ap)` — `errx` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verrx(eval: i32, fmt: *const u8, ap: *mut VaList) -> ! {
    // SAFETY: as `vwarn`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(crate::stdio::stderr_stream(), fmt, &mut args, false);
    crate::crt::exit(eval);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::capture::Pair;
    use std::string::String;

    /// A synthetic SysV `va_list` over up to six integer arguments.
    fn with_valist<R>(ints: &[u64], f: impl FnOnce(*mut VaList) -> R) -> R {
        let mut reg = [0u8; 176];
        for (i, &v) in ints.iter().enumerate().take(6) {
            let off = i * 8;
            reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
        let mut overflow = [0u8; 64];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        f(&mut va)
    }

    /// `warn` (`with_errno`) or `warnx` into `pair`'s `stderr`.
    fn call(pair: &Pair, fmt: Option<&core::ffi::CStr>, ints: &[u64], with_errno: bool) {
        with_valist(ints, |va| {
            // SAFETY: `va` holds `ints`, which the format's conversions read.
            let mut args = unsafe { printf::Args::from_raw(va) };
            let f = fmt.map_or(core::ptr::null(), |f| f.as_ptr().cast());
            emit(pair.stderr, f, &mut args, with_errno);
        });
    }

    /// The short name in the host's messages: `crt`'s default.
    fn short() -> String {
        // SAFETY: a plain read of this thread's name, a C string's pointer.
        let p = unsafe { crate::crt::progname_slot().read() };
        // SAFETY: a C string.
        unsafe { core::ffi::CStr::from_ptr(p.cast()) }
            .to_string_lossy()
            .into_owned()
    }

    /// Every probe `errfns_harness.py` ran for this module's functions,
    /// against glibc 2.39, the program's short name standing for glibc's.
    /// `err` and `errx` are `warn` and `warnx` followed by `exit(eval)`,
    /// which the test process cannot take: their output is replayed, and
    /// their exit status is `eval`, as the oracle shows.
    #[test]
    fn err_and_warn_are_glibcs() {
        let oracle = include_str!("errfns_oracle.txt");
        let mut n = 0;
        for line in oracle
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (probe, rest) = line.split_once(" = ").unwrap();
            let (want, how) = rest.rsplit_once(" | ").unwrap();
            if (!probe.starts_with("warn") && !probe.starts_with("err"))
                || probe.starts_with("error")
            {
                continue;
            }
            let want = want.replace("\\n", "\n").replace("<short>", &short());
            let pair = Pair::new();
            let mut eval = 0;
            match probe {
                "warn" => {
                    errno::set_errno(errno::EPERM);
                    call(&pair, Some(c"w %d"), &[1], true);
                }
                "warn(NULL)" => {
                    errno::set_errno(errno::EPERM);
                    call(&pair, None, &[], true);
                }
                "warnx" => call(&pair, Some(c"x %d"), &[2], false),
                "warnx(NULL)" => call(&pair, None, &[], false),
                "err" => {
                    eval = 4;
                    errno::set_errno(errno::ENOENT);
                    call(&pair, Some(c"e"), &[], true);
                }
                "errx" => {
                    eval = 5;
                    call(&pair, Some(c"e"), &[], false);
                }
                "warnx does not flush stdout" => {
                    crate::error::put(pair.stdout, b"partial");
                    call(&pair, Some(c"w"), &[], false);
                    crate::error::put(pair.stdout, b"rest\n");
                }
                other => panic!("the oracle has a probe the test does not know: {other}"),
            }
            assert_eq!(pair.text(), want, "{probe}");
            assert_eq!(how, std::format!("exit {eval}"), "{probe}");
            n += 1;
        }
        assert!(n >= 7, "the oracle has {n} err and warn probes");
    }

    /// Longer than the 1024 bytes the message was cut at.
    #[test]
    fn a_long_message_is_whole() {
        let pair = Pair::new();
        let big = std::ffi::CString::new("z".repeat(3000)).unwrap();
        call(&pair, Some(c"%s"), &[big.as_ptr() as u64], false);
        assert_eq!(
            pair.text(),
            std::format!("{}: {}\n", short(), "z".repeat(3000))
        );
    }

    /// The error reported is the caller's, whatever writing the message did
    /// to `errno`.
    #[test]
    fn the_callers_errno_is_the_one_reported() {
        let pair = Pair::new();
        errno::set_errno(errno::EACCES);
        call(&pair, Some(c"x"), &[], true);
        assert_eq!(
            pair.text(),
            std::format!("{}: x: Permission denied\n", short())
        );
    }
}
