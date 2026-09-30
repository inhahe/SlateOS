//! GNU error-reporting functions (`<error.h>`).
//!
//! Provides `error`, `error_at_line`, their `va_list` variants `verror` and
//! `verror_at_line`, and the three global variables `error_message_count`,
//! `error_one_per_line`, and `error_print_progname`.  These are GNU
//! extensions (not POSIX) but are used pervasively by GNU coreutils and a
//! large fraction of ported command-line tools, so a libc that omits them
//! cannot link those programs.
//!
//! ## What they print: glibc's, byte for byte
//!
//! As glibc 2.39's manual and `misc/error.c` have it, and as
//! `posix/tools/oracle/errfns_harness.py` recorded it (`errfns_oracle.txt`,
//! replayed by the tests):
//!
//! 1. `stdout` is flushed first, so that what the program wrote there before
//!    the message is not overtaken by it.
//! 2. The program's name -- `program_invocation_name`, argv[0] as the program
//!    was started, not its last component -- and `: `; for `error_at_line`
//!    `:` and then `file:line: `, or a single space when `file` is NULL. If
//!    [`error_print_progname`] is set, it is called instead of printing the
//!    name, and the `:` or `: ` after it is not printed either.
//! 3. The `printf`-expanded format, however long.
//! 4. `: strerror(errnum)` when `errnum` is not 0.
//! 5. A newline; `stderr` is flushed, and [`error_message_count`] counted.
//! 6. With `status` not 0, `exit(status)`.
//!
//! When [`error_one_per_line`] is nonzero, `error_at_line` prints nothing,
//! counts nothing and does not exit for a file and line the same as the last
//! `error_at_line`'s -- an `error` between them does not change the last.
//!
//! The whole message goes through the `stderr` stream, under its lock, so
//! that two threads' messages do not interleave and one written after
//! `setvbuf` or `freopen` goes where the program sent `stderr`.
//!
//! Until 2026-09-29 this printed `__progname`, the short name; cut the
//! message at 1023 bytes; flushed neither stream; wrote around `stderr` to
//! file descriptor 2; and printed `error_at_line`'s place as `prog: file:12: `,
//! with a space glibc's does not have (known-issues.md ->
//! D-POSIX-ERROR-PRINTED-THE-SHORT-NAME-AND-CUT-LONG-MESSAGES).
//!
//! ## Implementation
//!
//! The C prototypes are variadic.  Like [`crate::printf`] and [`crate::err`],
//! the variadic entry points (`error`, `error_at_line`) are assembly
//! trampolines that perform a real `va_start` — spilling the argument
//! registers into a System V register save area and building a `va_list` over
//! it — and then call the matching `v*` variant.  Those are plain Rust and
//! host-testable, and they funnel through [`report`], which streams the
//! format through the tested `fprintf` engine.
//!
//! As with every real libc, a literal `%` in the format must be written `%%`.

use crate::printf::{self, VaList};

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

// ---------------------------------------------------------------------------
// Global variables (exported C symbols).
// ---------------------------------------------------------------------------

/// Number of messages printed by `error`/`error_at_line` so far.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut error_message_count: u32 = 0;

/// When nonzero, `error_at_line` suppresses consecutive duplicate
/// `filename`/`linenum` messages.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut error_one_per_line: i32 = 0;

/// Optional callback that replaces the default `progname: ` prefix.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut error_print_progname: Option<extern "C" fn()> = None;

/// Last `filename` passed to `error_at_line` (for `error_one_per_line`).
static mut OLD_FILENAME: *const u8 = core::ptr::null();
/// Last `linenum` passed to `error_at_line` (for `error_one_per_line`).
static mut OLD_LINENUM: u32 = 0;

// ---------------------------------------------------------------------------
// Assembly trampolines — `va_start`, then call the matching `v*` variant.
//
// The named-argument counts decide the initial `gp_offset` and which register
// carries the `va_list*`:
//   error(status, errnum, fmt, ...)                  : 3 named args
//   error_at_line(status, errnum, file, line, fmt, …): 5 named args
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
va_trampoline!("error", "verror", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("error_at_line", "verror_at_line", "40", "r9");

// ---------------------------------------------------------------------------
// The message
// ---------------------------------------------------------------------------

/// Where the family writes, and what it flushes first: `stderr` and `stdout`
/// -- the tests' own streams in the tests.
#[derive(Clone, Copy)]
pub(crate) struct Streams {
    /// The stream the message goes to.
    pub(crate) out: *mut u8,
    /// The stream flushed before it.
    pub(crate) first: *mut u8,
}

impl Streams {
    /// `stderr`, after `stdout`.
    pub(crate) fn standard() -> Self {
        Self {
            out: crate::stdio::stderr_stream(),
            first: crate::stdio::stdout_stream(),
        }
    }
}

/// Writing a message's pieces, which `err.rs`'s family does too -- so an
/// archive member of their own. Were they `error`'s, a program calling `warn`
/// would extract `error`'s member with them, and one that also defines its
/// own `error` (gnulib's `error` module does, wherever the C library lacks
/// one, as musl does) would link two. See string.rs's module header, and
/// CHECK 5 in scripts/check-libc-shape.py, which holds the archive to it.
mod output {
    /// Write `bytes` to `out`. What a failed write loses is the message
    /// itself, which has nowhere else to be reported -- as in glibc, whose
    /// `error` returns nothing.
    pub(crate) fn put(out: *mut u8, bytes: &[u8]) {
        // SAFETY: `out` is a live stream; `bytes` is `bytes.len()` readable
        // bytes.
        let _ = unsafe { crate::stdio::fwrite(bytes.as_ptr(), 1, bytes.len(), out) };
    }

    /// Write the C string `s` to `out`, `(null)` for NULL, as `printf("%s")`
    /// does.
    pub(crate) fn put_cstr(out: *mut u8, s: *const u8) {
        if s.is_null() {
            put(out, b"(null)");
            return;
        }
        // SAFETY: a C string, the caller's.
        let len = unsafe { crate::string::strlen(s) };
        // SAFETY: `len` bytes at `s`.
        put(out, unsafe { core::slice::from_raw_parts(s, len) });
    }
}
pub(crate) use output::{put, put_cstr};

/// Is this `error_at_line` a repeat that [`error_one_per_line`] suppresses?
/// If not, it becomes the last one.
fn repeated(file: *const u8, line: u32) -> bool {
    // SAFETY: plain reads and writes of this module's statics, as glibc's are
    // unsynchronized statics too.
    unsafe {
        if core::ptr::addr_of!(error_one_per_line).read() == 0 {
            return false;
        }
        let old_file = core::ptr::addr_of!(OLD_FILENAME).read();
        let same_file = file == old_file
            || (!old_file.is_null()
                && !file.is_null()
                // SAFETY: both non-null C strings.
                && crate::string::strcmp(old_file, file) == 0);
        if core::ptr::addr_of!(OLD_LINENUM).read() == line && same_file {
            return true;
        }
        core::ptr::addr_of_mut!(OLD_FILENAME).write(file);
        core::ptr::addr_of_mut!(OLD_LINENUM).write(line);
    }
    false
}

/// The whole family: `at` is `error_at_line`'s file and line. Returns only if
/// `status` is 0 or `exit_on_status` is false (the tests').
#[allow(clippy::too_many_arguments)]
fn report(
    streams: Streams,
    status: i32,
    errnum: i32,
    at: Option<(*const u8, u32)>,
    fmt: *const u8,
    args: &mut printf::Args,
    exit_on_status: bool,
) {
    if let Some((file, line)) = at
        && repeated(file, line)
    {
        return;
    }
    // Earlier output first. A failure to flush it is the program's stdout's,
    // which that stream's own error flag keeps; it is not this message's.
    let _ = crate::stdio::fflush(streams.first);
    let out = streams.out;
    crate::stdio::flockfile(out.cast());
    // SAFETY: a plain read of the callback the program may have set.
    match unsafe { core::ptr::addr_of!(error_print_progname).read() } {
        Some(print_progname) => print_progname(),
        None => {
            // SAFETY: a plain read of the pointer `__libc_start_main` set.
            put_cstr(out, unsafe {
                core::ptr::addr_of!(crate::crt::__progname_full).read()
            });
            put(out, if at.is_some() { b":" } else { b": " });
        }
    }
    if let Some((file, line)) = at {
        if file.is_null() {
            put(out, b" ");
        } else {
            put_cstr(out, file);
            let mut digits = [0u8; 10];
            let mut i = digits.len();
            let mut n = line;
            loop {
                i = i.saturating_sub(1);
                if let Some(d) = digits.get_mut(i) {
                    *d = b'0'.wrapping_add((n % 10) as u8);
                }
                n /= 10;
                if n == 0 || i == 0 {
                    break;
                }
            }
            put(out, b":");
            put(out, digits.get(i..).unwrap_or(b"0"));
            put(out, b": ");
        }
    }
    if !fmt.is_null() {
        let _ = printf::_fprintf_impl(out, fmt, args); // as `put`
    }
    // SAFETY: plain read and write of the counter, as glibc's is.
    unsafe {
        let c = core::ptr::addr_of!(error_message_count).read();
        core::ptr::addr_of_mut!(error_message_count).write(c.wrapping_add(1));
    }
    if errnum != 0 {
        put(out, b": ");
        put_cstr(out, crate::string::strerror(errnum));
    }
    put(out, b"\n");
    let _ = crate::stdio::fflush(out); // as `put`
    crate::stdio::funlockfile(out.cast());
    if status != 0 && exit_on_status {
        crate::crt::exit(status);
    }
}

// ---------------------------------------------------------------------------
// v* variants — take a `va_list` (pointer); pure Rust, host-testable, and the
// delegation target of the trampolines above.
// ---------------------------------------------------------------------------

/// `verror(status, errnum, format, ap)` — `error` with a `va_list`.
///
/// # Safety
///
/// `fmt` must be a valid NUL-terminated format string (or null) and `ap` a
/// valid `va_list` whose arguments match `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verror(status: i32, errnum: i32, fmt: *const u8, ap: *mut VaList) {
    // SAFETY: the caller guarantees `ap` is a valid va_list matching `fmt`;
    // a null one is rendered as zero arguments rather than a fault.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    report(
        Streams::standard(),
        status,
        errnum,
        None,
        fmt,
        &mut args,
        true,
    );
}

/// `verror_at_line(status, errnum, filename, linenum, format, ap)`.
///
/// # Safety
///
/// As [`verror`]; `filename` must be null or a valid C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verror_at_line(
    status: i32,
    errnum: i32,
    filename: *const u8,
    linenum: u32,
    fmt: *const u8,
    ap: *mut VaList,
) {
    // SAFETY: as `verror`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    report(
        Streams::standard(),
        status,
        errnum,
        Some((filename, linenum)),
        fmt,
        &mut args,
        true,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test builds: two streams over one byte sink, as a program's `stdout` and
/// `stderr` are over one pipe or terminal -- the first fully buffered, the
/// second not -- so that what reaches the sink, and in what order, is what a
/// reader of both would see. Shared with `err.rs`'s tests.
#[cfg(test)]
pub(crate) mod capture {
    extern crate std;
    use std::boxed::Box;
    use std::vec::Vec;

    /// The sink, and the two streams over it.
    // Boxed: the streams' cookie is the `Vec` itself, which must not move
    // when the `Pair` does.
    #[allow(clippy::box_collection)]
    pub(crate) struct Pair {
        pub(crate) sink: Box<Vec<u8>>,
        pub(crate) stdout: *mut u8,
        pub(crate) stderr: *mut u8,
    }

    // Not `write`: scripts/check-libc-prototypes.py reads every `extern "C" fn`
    // by its name, and this one is no definition of the C library's.
    unsafe extern "C" fn sink_write(
        cookie: *mut core::ffi::c_void,
        buf: *const u8,
        n: usize,
    ) -> isize {
        // SAFETY: `cookie` is the pair's sink, `buf` `n` bytes of the
        // stream's.
        unsafe {
            (*cookie.cast::<Vec<u8>>()).extend_from_slice(core::slice::from_raw_parts(buf, n));
        }
        isize::try_from(n).unwrap_or(isize::MAX)
    }

    const IO: crate::stdio::CookieIoFunctions = crate::stdio::CookieIoFunctions {
        read: None,
        write: Some(sink_write),
        seek: None,
        close: None,
    };

    impl Pair {
        pub(crate) fn new() -> Self {
            let mut sink = Box::new(Vec::new());
            let cookie: *mut Vec<u8> = &raw mut *sink;
            // SAFETY: the sink outlives both streams (`Drop` closes them
            // first); "w" is a mode fopencookie takes.
            let (stdout, stderr) = unsafe {
                (
                    crate::stdio::fopencookie(cookie.cast(), c"w".as_ptr().cast(), IO),
                    crate::stdio::fopencookie(cookie.cast(), c"w".as_ptr().cast(), IO),
                )
            };
            assert!(!stdout.is_null() && !stderr.is_null());
            assert_eq!(
                crate::stdio::setvbuf(stdout, core::ptr::null_mut(), crate::stdio::_IOFBF, 4096),
                0
            );
            assert_eq!(
                crate::stdio::setvbuf(stderr, core::ptr::null_mut(), crate::stdio::_IONBF, 0),
                0
            );
            Self {
                sink,
                stdout,
                stderr,
            }
        }

        /// Everything both streams have written, `stdout`'s pending bytes
        /// last, as a program's exit would flush them.
        pub(crate) fn text(&self) -> std::string::String {
            let _ = crate::stdio::fflush(self.stdout);
            std::string::String::from_utf8_lossy(&self.sink).into_owned()
        }
    }

    impl Drop for Pair {
        fn drop(&mut self) {
            let _ = crate::stdio::fclose(self.stdout);
            let _ = crate::stdio::fclose(self.stderr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errno;
    use std::string::String;

    /// Serialises the tests that use this module's statics: the count, the
    /// last place, `error_one_per_line` and `error_print_progname`.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn count() -> u32 {
        // SAFETY: a plain read.
        unsafe { core::ptr::addr_of!(error_message_count).read() }
    }

    fn set_one_per_line(v: i32) {
        // SAFETY: plain writes, under the lock.
        unsafe {
            core::ptr::addr_of_mut!(error_one_per_line).write(v);
            core::ptr::addr_of_mut!(OLD_FILENAME).write(core::ptr::null());
            core::ptr::addr_of_mut!(OLD_LINENUM).write(0);
        }
    }

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

    /// `error` or `error_at_line` into `pair`, with integer or pointer
    /// arguments, not exiting.
    fn call(
        pair: &capture::Pair,
        status: i32,
        errnum: i32,
        at: Option<(*const u8, u32)>,
        fmt: &core::ffi::CStr,
        ints: &[u64],
    ) {
        let streams = Streams {
            out: pair.stderr,
            first: pair.stdout,
        };
        with_valist(ints, |va| {
            // SAFETY: `va` holds `ints`, which the format's conversions read.
            let mut args = unsafe { printf::Args::from_raw(va) };
            report(
                streams,
                status,
                errnum,
                at,
                fmt.as_ptr().cast(),
                &mut args,
                false,
            );
        });
    }

    /// The program's name in the host's messages: `crt`'s default.
    fn full() -> String {
        // SAFETY: a plain read of a static C string's pointer.
        let p = unsafe { core::ptr::addr_of!(crate::crt::__progname_full).read() };
        // SAFETY: a C string.
        unsafe { core::ffi::CStr::from_ptr(p.cast()) }
            .to_string_lossy()
            .into_owned()
    }

    std::thread_local! {
        /// The stream `print_cb` writes to: the test's `stderr`.
        static CB_OUT: core::cell::Cell<*mut u8> =
            const { core::cell::Cell::new(core::ptr::null_mut()) };
    }

    /// The probe's `error_print_progname`, which writes `[cb]` to `stderr`.
    extern "C" fn print_cb() {
        CB_OUT.with(|c| put(c.get(), b"[cb]"));
    }

    /// Every probe `errfns_harness.py` ran for this module's functions,
    /// against glibc 2.39: the same calls, the same output, the program's
    /// name standing for glibc's. (`warn` and `err` are `err.rs`'s.)
    #[test]
    fn error_is_glibcs() {
        let _g = lock();
        let oracle = include_str!("errfns_oracle.txt");
        let mut n = 0;
        for line in oracle
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (probe, rest) = line.split_once(" = ").unwrap();
            let (want, how) = rest.rsplit_once(" | ").unwrap();
            if !probe.starts_with("error") {
                continue;
            }
            let want = want
                .replace("\\n", "\n")
                .replace("<full>", &full())
                .replace("<5000 y>", &"y".repeat(5000));
            set_one_per_line(0);
            let pair = capture::Pair::new();
            let before = count();
            let f = c"f.c".as_ptr().cast::<u8>();
            let g = c"g.c".as_ptr().cast::<u8>();
            let z = c"z".as_ptr() as u64;
            let x = c"x".as_ptr() as u64;
            let mut status = 0;
            match probe {
                "error" => call(&pair, 0, 0, None, c"msg %d", &[5]),
                "error(ENOENT)" => call(&pair, 0, errno::ENOENT, None, c"open %s", &[x]),
                "error(-5)" => call(&pair, 0, -5, None, c"odd", &[]),
                "error(status 3)" => {
                    status = 3;
                    call(&pair, 3, 0, None, c"bye", &[]);
                }
                "error_at_line" => call(&pair, 0, 0, Some((f, 12)), c"m %s", &[z]),
                "error_at_line(NULL file)" => {
                    call(
                        &pair,
                        0,
                        errno::EACCES,
                        Some((core::ptr::null(), 12)),
                        c"m",
                        &[],
                    );
                }
                "error_one_per_line" => {
                    set_one_per_line(1);
                    call(&pair, 0, 0, Some((f, 12)), c"first", &[]);
                    call(&pair, 0, 0, Some((f, 12)), c"second", &[]);
                    call(&pair, 0, 0, Some((f, 13)), c"third", &[]);
                    call(&pair, 0, 0, Some((g, 13)), c"fourth", &[]);
                    call(&pair, 0, 0, Some((g, 13)), c"fifth", &[]);
                    call(&pair, 0, 0, None, c"plain", &[]);
                    call(&pair, 0, 0, Some((g, 13)), c"sixth", &[]);
                    set_one_per_line(0);
                    let s = std::format!("count={}\n", count() - before);
                    put(pair.stderr, s.as_bytes());
                }
                "error_print_progname" => {
                    // SAFETY: plain writes, under the lock.
                    unsafe {
                        core::ptr::addr_of_mut!(error_print_progname).write(Some(print_cb));
                    }
                    // The callback writes "[cb]" to the stream the message
                    // goes to, as the probe's writes to stderr.
                    CB_OUT.with(|c| c.set(pair.stderr));
                    call(&pair, 0, 0, None, c"a", &[]);
                    call(&pair, 0, 0, Some((f, 1)), c"b", &[]);
                    // SAFETY: as above.
                    unsafe {
                        core::ptr::addr_of_mut!(error_print_progname).write(None);
                    }
                }
                "error flushes stdout" => {
                    put(pair.stdout, b"partial");
                    call(&pair, 0, 0, None, c"after", &[]);
                    put(pair.stdout, b"rest\n");
                }
                "error 5000 bytes" => {
                    let big = std::ffi::CString::new("y".repeat(5000)).unwrap();
                    call(&pair, 0, 0, None, c"%s", &[big.as_ptr() as u64]);
                }
                "error_message_count" => {
                    call(&pair, 0, 0, None, c"a", &[]);
                    call(&pair, 0, 0, None, c"b", &[]);
                    let s = std::format!("count={}\n", count() - before);
                    put(pair.stderr, s.as_bytes());
                }
                other => panic!("the oracle has a probe the test does not know: {other}"),
            }
            assert_eq!(pair.text(), want, "{probe}");
            assert_eq!(how, std::format!("exit {status}"), "{probe}");
            n += 1;
        }
        assert!(n >= 11, "the oracle has {n} error probes");
    }

    #[test]
    fn a_null_format_prints_the_frame_and_no_body() {
        let _g = lock();
        let pair = capture::Pair::new();
        let streams = Streams {
            out: pair.stderr,
            first: pair.stdout,
        };
        // SAFETY: a NULL va_list is zero arguments.
        let mut args = unsafe { printf::Args::from_raw(core::ptr::null_mut()) };
        report(
            streams,
            0,
            errno::EIO,
            None,
            core::ptr::null(),
            &mut args,
            false,
        );
        assert_eq!(
            pair.text(),
            std::format!("{}: : Input/output error\n", full())
        );
    }

    #[test]
    fn a_line_number_is_decimal_to_the_last_digit() {
        let _g = lock();
        for (line, text) in [(0u32, "0"), (7, "7"), (10, "10"), (u32::MAX, "4294967295")] {
            let pair = capture::Pair::new();
            call(&pair, 0, 0, Some((c"a.c".as_ptr().cast(), line)), c"m", &[]);
            assert_eq!(pair.text(), std::format!("{}:a.c:{text}: m\n", full()));
        }
    }

    #[test]
    fn more_than_six_arguments_reach_the_format() {
        let _g = lock();
        let pair = capture::Pair::new();
        call(&pair, 0, 0, None, c"%d%d%d%d%d%d", &[1, 2, 3, 4, 5, 6]);
        assert_eq!(pair.text(), std::format!("{}: 123456\n", full()));
    }
}
