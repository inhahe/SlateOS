// Every index and sum is of positions within `argv` (below `argc`, which the
// caller vouches for) or within one of its NUL-terminated strings, walked no
// further than its NUL. Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! POSIX and GNU command-line option parsing: `getopt`, `getopt_long` and
//! `getopt_long_only`, glibc 2.39's behaviour throughout -- its answers to
//! 2,571 parses are replayed by the tests (posix/tools/oracle/getopt_harness.py,
//! posix/src/getopt_oracle.txt).
//!
//! What that means beyond POSIX's `getopt`:
//!
//! - **Order.** By default the arguments that are not options may come
//!   between the options: they are stepped over, and moved after the
//!   options as the parse goes (argv is *permuted*), so that when -1 comes
//!   back `optind` is the first of them. A `+` first in `optstring`, or
//!   `POSIXLY_CORRECT` in the environment when the parse starts, stops at the
//!   first one instead, as POSIX asks; a `-` first returns each as the
//!   option 1, `optarg` pointing at it. `--` ends the options either way.
//! - **Restart.** `optind = 0` starts a new parse, reading the order again.
//! - **Short options.** `x::` takes an optional argument, which must be
//!   attached (`-xval`). With `W;` in `optstring`, `-W foo` is `--foo`.
//! - **Long options.** `--name`, `--name=value` and `--name value` (for a
//!   required argument); a name may be cut short to any prefix that is not
//!   ambiguous -- one only several entries' names begin with is ambiguous
//!   unless they all take the same argument and answer the same way.
//!   `getopt_long_only` takes `-name` too, falling back to the short
//!   options when no long one matches and the first letter is one.
//! - **Complaints.** Unless `opterr` is 0 or `optstring` starts with `:`
//!   (after any `+` or `-`), each error is written to `stderr` as glibc
//!   words it, with `argv[0]`: `invalid option -- 'x'`, `option requires an
//!   argument -- 'x'`, `unrecognized option '--x'`, `option '--x' is
//!   ambiguous; possibilities: '--xa' '--xb'`, `option '--x' doesn't allow
//!   an argument`, `option '--x' requires an argument`. `optopt` is the
//!   character (or the long option's `val`, or 0) the error was about.
//!
//! Until 2026-09-30 this was POSIX's `getopt` alone: it never permuted,
//! never complained, and matched long options only whole.
//!
//! One deviation: a NULL `argv` or `optstring` answers -1 here, where
//! glibc's would fault.

/// Pointer to the argument of the current option.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut optarg: *const u8 = core::ptr::null();

/// Index of the next element of argv to be processed; 0 restarts the parse.
///
/// Initialized to 1 (skip argv[0] which is the program name).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut optind: i32 = 1;

/// If non-zero, errors are written to `stderr`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut opterr: i32 = 1;

/// The option character (or long option's `val`) the last error was about:
/// `'?'` before any, as glibc's starts.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut optopt: i32 = b'?' as i32;

/// Own archive member: the parse itself, with no C name in it. getopt's
/// functions call it, and so does argp (`posix/src/argp/parse.rs`), which
/// must not bring getopt's own member into a program that has getopt of
/// its own -- gnulib's, as GNU programs do -- beside this library's argp
/// (scripts/check-libc-shape.py, CHECK 5).
pub(crate) mod engine {
    use super::{NO_ARGUMENT, Option, REQUIRED_ARGUMENT};
    use crate::stdio;

    /// How the options and the other arguments may be mixed.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Ordering {
        /// Stop at the first argument that is not an option (`+`, or
        /// `POSIXLY_CORRECT`).
        RequireOrder,
        /// Options may follow the other arguments, which are moved after them.
        Permute,
        /// Each other argument is returned as the option 1 (`-`).
        ReturnInOrder,
    }

    /// The parser's own state (glibc's `_getopt_data`): its copies of the
    /// globals -- `optind` read from the program's at each call, the others
    /// written back to the program's after it -- where it is within an argument
    /// of grouped short options, and the run of other arguments it has stepped
    /// over and not yet moved. argp parses with one of its own, as glibc's
    /// does with `_getopt_long_r`, so that the program's globals are not
    /// touched (`posix/src/argp/parse.rs`).
    pub(crate) struct State {
        pub(crate) optind: i32,
        pub(crate) optarg: *const u8,
        pub(crate) optopt: i32,
        pub(crate) initialized: bool,
        /// The next short option character, within `argv[optind]`; NULL when a
        /// new argument is to be read.
        pub(crate) nextchar: *const u8,
        pub(crate) ordering: Ordering,
        /// The other arguments stepped over are `argv[first_nonopt..last_nonopt]`.
        pub(crate) first_nonopt: i32,
        pub(crate) last_nonopt: i32,
    }

    impl State {
        /// A parse not begun: glibc's `_GETOPT_DATA_INITIALIZER`.
        pub(crate) const fn new() -> Self {
            Self {
                optind: 1,
                optarg: core::ptr::null(),
                optopt: 0,
                initialized: false,
                nextchar: core::ptr::null(),
                ordering: Ordering::Permute,
                first_nonopt: 1,
                last_nonopt: 1,
            }
        }
    }

    /// Where a parse reads `POSIXLY_CORRECT` and writes its complaints: the
    /// environment and `stderr` -- other ones for the tests.
    pub(crate) struct Io {
        pub(crate) posixly_correct: fn() -> bool,
        pub(crate) messages: *mut u8,
    }

    impl Io {
        pub(crate) fn process() -> Self {
            Self {
                // SAFETY: a NUL-terminated name.
                posixly_correct: || unsafe {
                    !crate::environ::getenv(c"POSIXLY_CORRECT".as_ptr().cast()).is_null()
                },
                messages: stdio::stderr_stream(),
            }
        }
    }

    /// A C string's bytes; `(null)` for NULL, as glibc's `%s` prints it.
    ///
    /// # Safety
    ///
    /// `s` is NULL or a NUL-terminated string.
    pub(crate) unsafe fn text<'a>(s: *const u8) -> &'a [u8] {
        if s.is_null() {
            return b"(null)";
        }
        // SAFETY: the caller's contract.
        unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) }
    }

    /// `argv[i]`.
    ///
    /// # Safety
    ///
    /// `i` is below the caller's `argc`.
    pub(crate) unsafe fn arg(argv: *mut *const u8, i: i32) -> *const u8 {
        // SAFETY: the caller's contract.
        unsafe { *argv.add(usize::try_from(i).unwrap_or(0)) }
    }

    /// Whether `a` is not an option: it does not start with `-`, or is `-`.
    ///
    /// # Safety
    ///
    /// `a` is a NUL-terminated string.
    pub(crate) unsafe fn is_nonoption(a: *const u8) -> bool {
        // SAFETY: the caller's contract; the second byte is read only when the
        // first is not the NUL.
        unsafe { *a != b'-' || *a.add(1) == 0 }
    }

    /// Where `c` is in `optstring` -- `strchr`'s answer -- as long as it is a
    /// character an option can be (not `:` or `;`, and not the NUL).
    ///
    /// # Safety
    ///
    /// `optstring` is a NUL-terminated string.
    pub(crate) unsafe fn spec_of(optstring: *const u8, c: u8) -> core::option::Option<*const u8> {
        if c == 0 || c == b':' || c == b';' {
            return None;
        }
        // SAFETY: the caller's contract.
        let s = unsafe { text(optstring) };
        let at = s.iter().position(|&b| b == c)?;
        // SAFETY: `at` is within the string.
        Some(unsafe { optstring.add(at) })
    }

    /// The complaint made of `parts`, after `argv[0]` and `": "`, written to the
    /// messages stream in one piece -- or, without memory to put it together,
    /// piece by piece with the stream held, as glibc's `fprintf`s write it.
    pub(crate) fn complain(io: &Io, argv0: &[u8], parts: &[&[u8]]) {
        let head: [&[u8]; 2] = [argv0, b": "];
        let all = || head.iter().chain(parts.iter());
        let total = all().fold(0usize, |n, p| n.saturating_add(p.len()));
        let buf = crate::malloc::malloc(total);
        // SAFETY (both branches): `buf`, when not NULL, holds `total` bytes, the
        // parts' lengths summed; the stream is a live one. A failed write loses
        // only the message: the parse's answer is the same either way.
        unsafe {
            if buf.is_null() {
                stdio::flockfile(io.messages.cast());
                for p in all() {
                    let _ = stdio::fwrite(p.as_ptr(), 1, p.len(), io.messages);
                }
                stdio::funlockfile(io.messages.cast());
                return;
            }
            let mut at = 0usize;
            for p in all() {
                core::ptr::copy_nonoverlapping(p.as_ptr(), buf.add(at), p.len());
                at += p.len();
            }
            let _ = stdio::fwrite(buf, 1, total, io.messages);
            crate::malloc::free(buf);
        }
    }

    /// Move the other arguments stepped over, `argv[first_nonopt..last_nonopt]`,
    /// after the options that followed them, `argv[last_nonopt..optind]`, each
    /// run keeping its order -- glibc's `exchange`.
    ///
    /// # Safety
    ///
    /// `argv` holds at least `d.optind` pointers.
    pub(crate) unsafe fn exchange(argv: *mut *const u8, d: &mut State) {
        let (bottom, middle, top) = (d.first_nonopt, d.last_nonopt, d.optind);
        let (Ok(b), Ok(m), Ok(t)) = (
            usize::try_from(bottom),
            usize::try_from(middle),
            usize::try_from(top),
        ) else {
            return;
        };
        if b < m && m < t {
            // SAFETY: the caller's contract; `b < t <= argc`.
            let run = unsafe { core::slice::from_raw_parts_mut(argv.add(b), t - b) };
            run.rotate_left(m - b);
        }
        d.first_nonopt += top - middle;
        d.last_nonopt = top;
    }

    /// A new parse: `optind` 0 becomes 1, nothing is stepped over yet, and the
    /// order is read from `optstring`'s first character or the environment;
    /// `optstring` past that `+` or `-`.
    ///
    /// # Safety
    ///
    /// `optstring` is a NUL-terminated string.
    pub(crate) unsafe fn initialize(d: &mut State, optstring: *const u8, io: &Io) -> *const u8 {
        if d.optind == 0 {
            d.optind = 1;
        }
        d.first_nonopt = d.optind;
        d.last_nonopt = d.optind;
        d.nextchar = core::ptr::null();
        // SAFETY: the caller's contract; a first byte past the NUL is not read.
        let first = unsafe { *optstring };
        let rest = if first == b'-' || first == b'+' {
            // SAFETY: as above: the first byte is not the NUL.
            unsafe { optstring.add(1) }
        } else {
            optstring
        };
        d.ordering = match first {
            b'-' => Ordering::ReturnInOrder,
            b'+' => Ordering::RequireOrder,
            _ if (io.posixly_correct)() => Ordering::RequireOrder,
            _ => Ordering::Permute,
        };
        d.initialized = true;
        rest
    }

    /// One call's parse (glibc's `_getopt_internal_r`), on the state.
    ///
    /// # Safety
    ///
    /// `argv` holds `argc` pointers to NUL-terminated strings; `optstring` is a
    /// NUL-terminated string; `longopts` is NULL or an array ended by an entry
    /// whose name is NULL; `longind` is NULL or writable.
    #[allow(clippy::too_many_arguments)] // glibc's own parameters, and the I/O
    #[allow(clippy::too_many_lines)] // glibc's one function, step by step
    pub(crate) unsafe fn parse_one(
        d: &mut State,
        argc: i32,
        argv: *mut *const u8,
        mut optstring: *const u8,
        longopts: *const Option,
        longind: *mut i32,
        long_only: bool,
        mut print_errors: bool,
        io: &Io,
    ) -> i32 {
        if argc < 1 {
            return -1;
        }
        d.optarg = core::ptr::null();
        // SAFETY (the whole body): the caller's contract. Every `argv[i]` read
        // has `i < argc`; every string is walked no further than its NUL.
        unsafe {
            if d.optind == 0 || !d.initialized {
                optstring = initialize(d, optstring, io);
            } else if *optstring == b'-' || *optstring == b'+' {
                optstring = optstring.add(1);
            }
            let colon = *optstring == b':';
            if colon {
                print_errors = false;
            }
            let argv0 = text(arg(argv, 0));

            if d.nextchar.is_null() || *d.nextchar == 0 {
                // The next argument. The program may have moved `optind` back.
                d.last_nonopt = d.last_nonopt.min(d.optind);
                d.first_nonopt = d.first_nonopt.min(d.optind);
                if d.ordering == Ordering::Permute {
                    if d.first_nonopt != d.last_nonopt && d.last_nonopt != d.optind {
                        exchange(argv, d);
                    } else if d.last_nonopt != d.optind {
                        d.first_nonopt = d.optind;
                    }
                    while d.optind < argc && is_nonoption(arg(argv, d.optind)) {
                        d.optind += 1;
                    }
                    d.last_nonopt = d.optind;
                }
                // `--` ends the options: what was stepped over goes after the
                // options before it, and `optind` to the first of it.
                if d.optind != argc && text(arg(argv, d.optind)) == b"--" {
                    d.optind += 1;
                    if d.first_nonopt != d.last_nonopt && d.last_nonopt != d.optind {
                        exchange(argv, d);
                    } else if d.first_nonopt == d.last_nonopt {
                        d.first_nonopt = d.optind;
                    }
                    d.last_nonopt = argc;
                    d.optind = argc;
                }
                if d.optind == argc {
                    if d.first_nonopt != d.last_nonopt {
                        d.optind = d.first_nonopt;
                    }
                    return -1;
                }
                let a = arg(argv, d.optind);
                if is_nonoption(a) {
                    if d.ordering == Ordering::RequireOrder {
                        return -1;
                    }
                    d.optarg = a;
                    d.optind += 1;
                    return 1;
                }
                if !longopts.is_null() {
                    if *a.add(1) == b'-' {
                        d.nextchar = a.add(2);
                        return long_option(
                            d,
                            argc,
                            argv,
                            optstring,
                            longopts,
                            longind,
                            long_only,
                            print_errors,
                            io,
                            b"--",
                        );
                    }
                    // `getopt_long_only`: `-name` is tried as a long option
                    // unless it is one letter that is a short option.
                    if long_only && (*a.add(2) != 0 || spec_of(optstring, *a.add(1)).is_none()) {
                        d.nextchar = a.add(1);
                        let code = long_option(
                            d,
                            argc,
                            argv,
                            optstring,
                            longopts,
                            longind,
                            long_only,
                            print_errors,
                            io,
                            b"-",
                        );
                        if code != -1 {
                            return code;
                        }
                    }
                }
                d.nextchar = a.add(1);
            }

            // A short option: `optind` moves on as the argument's last
            // character is taken.
            let c = *d.nextchar;
            d.nextchar = d.nextchar.add(1);
            let spec = spec_of(optstring, c);
            if *d.nextchar == 0 {
                d.optind += 1;
            }
            let Some(spec) = spec else {
                if print_errors {
                    complain(io, argv0, &[b"invalid option -- '", &[c], b"'\n"]);
                }
                d.optopt = i32::from(c);
                return i32::from(b'?');
            };
            let after = *spec.add(1);
            if c == b'W' && after == b';' && !longopts.is_null() {
                // `-W foo` (or `-Wfoo`) is `--foo`.
                if *d.nextchar != 0 {
                    d.optarg = d.nextchar;
                } else if d.optind == argc {
                    if print_errors {
                        complain(
                            io,
                            argv0,
                            &[b"option requires an argument -- '", &[c], b"'\n"],
                        );
                    }
                    d.optopt = i32::from(c);
                    return i32::from(if colon { b':' } else { b'?' });
                } else {
                    d.optarg = arg(argv, d.optind);
                }
                d.nextchar = d.optarg;
                d.optarg = core::ptr::null();
                return long_option(
                    d,
                    argc,
                    argv,
                    optstring,
                    longopts,
                    longind,
                    false,
                    print_errors,
                    io,
                    b"-W ",
                );
            }
            if after == b':' {
                if *spec.add(2) == b':' {
                    // An optional argument: only one attached to the option.
                    if *d.nextchar != 0 {
                        d.optarg = d.nextchar;
                        d.optind += 1;
                    } else {
                        d.optarg = core::ptr::null();
                    }
                } else if *d.nextchar != 0 {
                    d.optarg = d.nextchar;
                    d.optind += 1;
                } else if d.optind == argc {
                    if print_errors {
                        complain(
                            io,
                            argv0,
                            &[b"option requires an argument -- '", &[c], b"'\n"],
                        );
                    }
                    d.optopt = i32::from(c);
                    d.nextchar = core::ptr::null();
                    return i32::from(if colon { b':' } else { b'?' });
                } else {
                    d.optarg = arg(argv, d.optind);
                    d.optind += 1;
                }
                d.nextchar = core::ptr::null();
            }
            i32::from(c)
        }
    }

    /// Whether two long options mean the same: the same argument, flag and
    /// answer. A prefix of several such names is not ambiguous.
    pub(crate) fn same_meaning(a: &Option, b: &Option) -> bool {
        a.has_arg == b.has_arg && a.flag == b.flag && a.val == b.val
    }

    /// An ambiguous option's candidates as glibc lists them, ` '<prefix><name>'`
    /// each, into `buf` when it is not NULL: their length.
    ///
    /// # Safety
    ///
    /// Each candidate's name is a NUL-terminated string; `buf` is NULL or holds
    /// the length a call with NULL answered.
    pub(crate) unsafe fn candidate_list<'a>(
        candidates: impl Iterator<Item = &'a Option>,
        prefix: &[u8],
        buf: *mut u8,
    ) -> usize {
        let mut n = 0usize;
        for p in candidates {
            // SAFETY: the caller's contract.
            let name = unsafe { text(p.name) };
            for part in [&b" '"[..], prefix, name, b"'"] {
                if !buf.is_null() {
                    // SAFETY: `buf` holds the whole list, the caller's contract.
                    unsafe {
                        core::ptr::copy_nonoverlapping(part.as_ptr(), buf.add(n), part.len())
                    };
                }
                n += part.len();
            }
        }
        n
    }

    /// The long option at `d.nextchar` (the name, then any `=value`), `prefix`
    /// being what came before it (`--`, `-`, or `-W `): its answer, or -1 for
    /// `getopt_long_only` to take the argument as short options instead.
    ///
    /// # Safety
    ///
    /// As [`parse_one`]; `d.nextchar` points into `argv[d.optind]`.
    #[allow(clippy::too_many_arguments)] // glibc's own parameters, and the I/O
    #[allow(clippy::too_many_lines)] // glibc's one function, step by step
    pub(crate) unsafe fn long_option(
        d: &mut State,
        argc: i32,
        argv: *mut *const u8,
        optstring: *const u8,
        longopts: *const Option,
        longind: *mut i32,
        long_only: bool,
        print_errors: bool,
        io: &Io,
        prefix: &[u8],
    ) -> i32 {
        // SAFETY (the whole body): the caller's contract; the options are read
        // up to the entry whose name is NULL.
        unsafe {
            let argv0 = text(arg(argv, 0));
            let rest = text(d.nextchar);
            let namelen = rest.iter().position(|&b| b == b'=').unwrap_or(rest.len());
            let name = &rest[..namelen];
            // The entries up to the one whose name is NULL.
            let mut n_options = 0usize;
            while !(*longopts.add(n_options)).name.is_null() {
                n_options += 1;
            }
            let opts = core::slice::from_raw_parts(longopts, n_options);

            // A whole name first; else the first name it begins, unless another
            // that means something else begins with it too.
            let mut found = opts.iter().position(|p| text(p.name) == name);
            if found.is_none() {
                let prefixed = |p: &Option| text(p.name).starts_with(name);
                let first = opts.iter().position(prefixed);
                if let Some(f) = first {
                    let pf = &opts[f];
                    let conflicts = |p: &Option| long_only || !same_meaning(pf, p);
                    if opts[f + 1..].iter().any(|p| prefixed(p) && conflicts(p)) {
                        if print_errors {
                            // The candidates: the first, and each later one that
                            // conflicts with it -- measured, then written.
                            let candidates = || {
                                core::iter::once(pf).chain(
                                    opts[f + 1..]
                                        .iter()
                                        .filter(|&p| prefixed(p) && conflicts(p)),
                                )
                            };
                            let len = candidate_list(candidates(), prefix, core::ptr::null_mut());
                            let buf = crate::malloc::malloc(len);
                            if buf.is_null() {
                                // glibc's word for it without memory for the list.
                                complain(
                                    io,
                                    argv0,
                                    &[b"option '", prefix, rest, b"' is ambiguous\n"],
                                );
                            } else {
                                candidate_list(candidates(), prefix, buf);
                                let listed = core::slice::from_raw_parts(buf, len);
                                complain(
                                    io,
                                    argv0,
                                    &[
                                        b"option '",
                                        prefix,
                                        rest,
                                        b"' is ambiguous; possibilities:",
                                        listed,
                                        b"\n",
                                    ],
                                );
                                crate::malloc::free(buf);
                            }
                        }
                        d.nextchar = d.nextchar.add(rest.len());
                        d.optind += 1;
                        d.optopt = 0;
                        return i32::from(b'?');
                    }
                }
                found = first;
            }

            let Some(i) = found else {
                // No such long option: an error, unless `getopt_long_only` may
                // take it as short options.
                let a = arg(argv, d.optind);
                if !long_only || *a.add(1) == b'-' || spec_of(optstring, *d.nextchar).is_none() {
                    if print_errors {
                        complain(io, argv0, &[b"unrecognized option '", prefix, rest, b"'\n"]);
                    }
                    d.nextchar = core::ptr::null();
                    d.optind += 1;
                    d.optopt = 0;
                    return i32::from(b'?');
                }
                return -1;
            };

            let p = &opts[i];
            d.optind += 1;
            d.nextchar = core::ptr::null();
            if namelen < rest.len() {
                if p.has_arg != NO_ARGUMENT {
                    d.optarg = rest.as_ptr().add(namelen + 1);
                } else {
                    if print_errors {
                        complain(
                            io,
                            argv0,
                            &[
                                b"option '",
                                prefix,
                                text(p.name),
                                b"' doesn't allow an argument\n",
                            ],
                        );
                    }
                    d.optopt = p.val;
                    return i32::from(b'?');
                }
            } else if p.has_arg == REQUIRED_ARGUMENT {
                if d.optind < argc {
                    d.optarg = arg(argv, d.optind);
                    d.optind += 1;
                } else {
                    if print_errors {
                        complain(
                            io,
                            argv0,
                            &[
                                b"option '",
                                prefix,
                                text(p.name),
                                b"' requires an argument\n",
                            ],
                        );
                    }
                    d.optopt = p.val;
                    return i32::from(if *optstring == b':' { b':' } else { b'?' });
                }
            }
            if !longind.is_null() {
                *longind = i32::try_from(i).unwrap_or(i32::MAX);
            }
            if !p.flag.is_null() {
                *p.flag = p.val;
                return 0;
            }
            p.val
        }
    }
}
use engine::*;

/// The one parse state, as glibc's: getopt is not reentrant, by POSIX.
static mut STATE: State = State::new();

/// One call, through the globals: `optind` and `opterr` read from the
/// program's, `optind`, `optarg` and `optopt` written back to them after.
///
/// # Safety
///
/// As [`parse_one`], `argv` and `optstring` may be NULL (-1).
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
unsafe fn call(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
    longopts: *const Option,
    longind: *mut i32,
    long_only: bool,
    io: &Io,
) -> i32 {
    if argv.is_null() || optstring.is_null() {
        return -1;
    }
    // SAFETY: the parse state is the process's, as getopt's is by POSIX
    // (not reentrant); the globals are plain words.
    unsafe {
        let d = &mut *core::ptr::addr_of_mut!(STATE);
        d.optind = core::ptr::addr_of!(optind).read();
        let print_errors = core::ptr::addr_of!(opterr).read() != 0;
        // glibc's argv is `char *const []`, and it permutes it anyway.
        let r = parse_one(
            d,
            argc,
            argv.cast_mut(),
            optstring,
            longopts,
            longind,
            long_only,
            print_errors,
            io,
        );
        core::ptr::addr_of_mut!(optind).write(d.optind);
        core::ptr::addr_of_mut!(optarg).write(d.optarg);
        core::ptr::addr_of_mut!(optopt).write(d.optopt);
        r
    }
}

/// Parse command-line options: the next option character from `argv`, as
/// `optstring` describes them -- `'?'` for an error, `':'` for a missing
/// argument when `optstring` starts with `:`, 1 for another argument when
/// it starts with `-`, and -1 when the options are done. See the module
/// documentation for the order, the complaints and the rest.
///
/// # Safety
///
/// `argv` must be NULL or an array of at least `argc` pointers to
/// NUL-terminated strings, which the parse may reorder; `optstring` must be
/// NULL or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub unsafe extern "C" fn getopt(argc: i32, argv: *const *const u8, optstring: *const u8) -> i32 {
    // SAFETY: the caller's contract.
    unsafe {
        call(
            argc,
            argv,
            optstring,
            core::ptr::null(),
            core::ptr::null_mut(),
            false,
            &Io::process(),
        )
    }
}

// ---------------------------------------------------------------------------
// GNU getopt_long / getopt_long_only
// ---------------------------------------------------------------------------

/// Long option descriptor.
///
/// Matches the GNU `struct option` layout.
#[repr(C)]
pub struct Option {
    /// Long option name (without leading "--").
    pub name: *const u8,
    /// Argument requirement: 0=none, 1=required, 2=optional.
    pub has_arg: i32,
    /// If non-null, set `*flag = val` and return 0.
    /// If null, return `val`.
    pub flag: *mut i32,
    /// Value to return (or store in `*flag`).
    pub val: i32,
}

/// `has_arg` value: option takes no argument.
pub const NO_ARGUMENT: i32 = 0;
/// `has_arg` value: option requires an argument.
pub const REQUIRED_ARGUMENT: i32 = 1;
/// `has_arg` value: option takes an optional argument.
pub const OPTIONAL_ARGUMENT: i32 = 2;

/// Parse long (and short) command-line options: as [`getopt`], and
/// `--name` from `longopts` -- whose `val` is the answer, or 0 with `*flag`
/// set to it when `flag` is not NULL -- with `*longindex` (when not NULL)
/// the entry's index.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub extern "C" fn getopt_long(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
    longopts: *const Option,
    longindex: *mut i32,
) -> i32 {
    // SAFETY: the caller's contract, as `getopt`'s; `longopts` is NULL or
    // ended by an entry whose name is NULL.
    unsafe {
        call(
            argc,
            argv,
            optstring,
            longopts,
            longindex,
            false,
            &Io::process(),
        )
    }
}

/// As [`getopt_long`], and `-name` as a long option too, unless it is one
/// letter that is a short option or no long option matches.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub extern "C" fn getopt_long_only(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
    longopts: *const Option,
    longindex: *mut i32,
) -> i32 {
    // SAFETY: as `getopt_long`.
    unsafe {
        call(
            argc,
            argv,
            optstring,
            longopts,
            longindex,
            true,
            &Io::process(),
        )
    }
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

/// Cross-test serialisation for getopt's process-global parser state.
///
/// `optarg`/`optind`/`opterr`/`optopt` and the parse state are mutable
/// statics by POSIX requirement, so every getopt test (and every test of
/// code that calls getopt) races against every other under cargo's default
/// parallel runner. Holding this mutex for the duration of a test
/// guarantees the globals reflect that test's call sequence end-to-end.
#[cfg(all(test, not(target_os = "none")))]
static GETOPT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The globals and the parse state back to their values at program start.
///
/// Internal helper: assumes the caller already holds `GETOPT_TEST_LOCK`
/// (typically via `reset_getopt_state`).
#[cfg(test)]
unsafe fn zero_getopt_globals() {
    // SAFETY: caller guarantees serialised access to the globals.
    unsafe {
        core::ptr::addr_of_mut!(optind).write(1);
        core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
        core::ptr::addr_of_mut!(opterr).write(1);
        core::ptr::addr_of_mut!(optopt).write(i32::from(b'?'));
        core::ptr::addr_of_mut!(STATE).write(State {
            optind: 1,
            optarg: core::ptr::null(),
            optopt: 0,
            initialized: false,
            nextchar: core::ptr::null(),
            ordering: Ordering::Permute,
            first_nonopt: 1,
            last_nonopt: 1,
        });
    }
}

/// Reset all getopt global state AND acquire the cross-test serialisation
/// lock. Returns a `MutexGuard` that the caller must keep alive for the
/// test; a poisoned one is recovered.
///
/// Must be called as `let _g = unsafe { reset_getopt_state() };` at the top
/// of every getopt-touching test; a mid-test reset uses
/// `zero_getopt_globals()`, since taking the lock again would deadlock.
#[cfg(test)]
#[must_use = "the returned guard serialises getopt tests; bind it to `_g`"]
unsafe fn reset_getopt_state() -> std::sync::MutexGuard<'static, ()> {
    let guard = GETOPT_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: we now hold the cross-test lock; no other thread can touch
    // the globals concurrently.
    unsafe { zero_getopt_globals() };
    guard
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
// These tests use the global state (optind, optarg, ...): each holds
// `GETOPT_TEST_LOCK` through `reset_getopt_state`.
mod tests {
    use super::*;

    /// Build a null-terminated C string on the stack and return its pointer.
    /// The returned pointer is valid for the lifetime of the `Vec`.
    fn cstr(s: &str) -> Vec<u8> {
        cstr_bytes(s.as_bytes())
    }

    /// `b` and a NUL.
    fn cstr_bytes(b: &[u8]) -> Vec<u8> {
        let mut v = b.to_vec();
        v.push(0);
        v
    }

    /// Build argc/argv from a slice of string slices.
    /// Returns (argc, argv_ptrs, _backing) — `_backing` must be kept alive
    /// so `argv_ptrs` remains valid.
    fn make_argv(args: &[&str]) -> (i32, Vec<*const u8>, Vec<Vec<u8>>) {
        let backing: Vec<Vec<u8>> = args.iter().map(|s| cstr(s)).collect();
        let ptrs: Vec<*const u8> = backing.iter().map(|v| v.as_ptr()).collect();
        (args.len() as i32, ptrs, backing)
    }

    // -----------------------------------------------------------------------
    // Basic short option parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_short_options_abc() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-a", "-b", "-c"]);
        let opts = cstr("abc");

        let r1 = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r1, i32::from(b'a'));

        let r2 = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r2, i32::from(b'b'));

        let r3 = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r3, i32::from(b'c'));

        let r4 = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r4, -1, "should return -1 after all options consumed");
    }

    #[test]
    fn test_grouped_short_options() {
        let _g = unsafe { reset_getopt_state() };
        // "-abc" is equivalent to "-a -b -c"
        let (argc, argv, _b) = make_argv(&["prog", "-abc"]);
        let opts = cstr("abc");

        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'a')
        );
        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'b')
        );
        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'c')
        );
        assert_eq!(unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) }, -1);
    }

    // -----------------------------------------------------------------------
    // Option with required argument
    // -----------------------------------------------------------------------

    #[test]
    fn test_option_with_arg_separate() {
        // "-o value" — argument in the next argv element.
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-o", "myfile"]);
        let opts = cstr("o:");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r, i32::from(b'o'));

        // optarg should point to "myfile".
        let arg_ptr = unsafe { core::ptr::addr_of!(optarg).read() };
        assert!(!arg_ptr.is_null());
        // Compare the first byte.
        assert_eq!(unsafe { *arg_ptr }, b'm');

        assert_eq!(unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) }, -1);
    }

    #[test]
    fn test_option_with_arg_attached() {
        // "-omyfile" — argument attached to the option.
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-omyfile"]);
        let opts = cstr("o:");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r, i32::from(b'o'));

        let arg_ptr = unsafe { core::ptr::addr_of!(optarg).read() };
        assert!(!arg_ptr.is_null());
        assert_eq!(unsafe { *arg_ptr }, b'm');
        assert_eq!(unsafe { *arg_ptr.add(1) }, b'y');

        assert_eq!(unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) }, -1);
    }

    // -----------------------------------------------------------------------
    // Unknown option handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_unknown_option_returns_question() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-z"]);
        let opts = cstr("abc");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r, i32::from(b'?'));

        // optopt should be set to the unknown character.
        let oo = unsafe { core::ptr::addr_of!(optopt).read() };
        assert_eq!(oo, i32::from(b'z'));
    }

    #[test]
    fn test_unknown_among_known() {
        let _g = unsafe { reset_getopt_state() };
        // "-axb" — 'a' is known, 'x' is unknown, 'b' is known.
        let (argc, argv, _b) = make_argv(&["prog", "-axb"]);
        let opts = cstr("ab");

        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'a')
        );
        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'?')
        );
        let oo = unsafe { core::ptr::addr_of!(optopt).read() };
        assert_eq!(oo, i32::from(b'x'));
        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'b')
        );
        assert_eq!(unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) }, -1);
    }

    // -----------------------------------------------------------------------
    // optind tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_optind_advances() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-a", "-b"]);
        let opts = cstr("ab");

        assert_eq!(unsafe { core::ptr::addr_of!(optind).read() }, 1);
        unsafe {
            getopt(argc, argv.as_ptr(), opts.as_ptr());
        }
        assert_eq!(unsafe { core::ptr::addr_of!(optind).read() }, 2);
        unsafe {
            getopt(argc, argv.as_ptr(), opts.as_ptr());
        }
        assert_eq!(unsafe { core::ptr::addr_of!(optind).read() }, 3);
    }

    #[test]
    fn test_reset_allows_reparse() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-a"]);
        let opts = cstr("a");

        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'a')
        );
        assert_eq!(unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) }, -1);

        // Reset and parse again. Use zero_getopt_globals to avoid re-acquiring
        // the lock (which would deadlock — `_g` from above is still alive).
        unsafe { zero_getopt_globals() };
        assert_eq!(
            unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) },
            i32::from(b'a')
        );
        assert_eq!(unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) }, -1);
    }

    // -----------------------------------------------------------------------
    // Double dash stops parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_double_dash_stops_parsing() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--", "-a"]);
        let opts = cstr("a");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r, -1, "-- should stop option parsing");

        // optind should be at the element after "--".
        let ind = unsafe { core::ptr::addr_of!(optind).read() };
        assert_eq!(ind, 2);
    }

    #[test]
    fn test_bare_dash_stops_parsing() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-"]);
        let opts = cstr("a");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r, -1, "bare - should not be treated as an option");
    }

    // -----------------------------------------------------------------------
    // Null / empty optstring
    // -----------------------------------------------------------------------

    #[test]
    fn test_null_optstring() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-a"]);

        let r = unsafe { getopt(argc, argv.as_ptr(), core::ptr::null()) };
        assert_eq!(r, -1);
    }

    #[test]
    fn test_null_argv() {
        let _g = unsafe { reset_getopt_state() };
        let opts = cstr("a");

        let r = unsafe { getopt(3, core::ptr::null(), opts.as_ptr()) };
        assert_eq!(r, -1);
    }

    #[test]
    fn test_empty_optstring_returns_unknown() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-a"]);
        let opts = cstr("");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(
            r,
            i32::from(b'?'),
            "all options unknown with empty optstring"
        );
    }

    // -----------------------------------------------------------------------
    // Missing required argument
    // -----------------------------------------------------------------------

    #[test]
    fn test_missing_required_arg() {
        let _g = unsafe { reset_getopt_state() };
        // "-o" without a following argument.
        let (argc, argv, _b) = make_argv(&["prog", "-o"]);
        let opts = cstr("o:");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(r, i32::from(b'?'), "missing arg should return '?'");

        let oo = unsafe { core::ptr::addr_of!(optopt).read() };
        assert_eq!(oo, i32::from(b'o'));
    }

    #[test]
    fn test_missing_required_arg_colon_mode() {
        let _g = unsafe { reset_getopt_state() };
        // Leading ':' in optstring changes missing-arg return to ':'.
        let (argc, argv, _b) = make_argv(&["prog", "-o"]);
        let opts = cstr(":o:");

        let r = unsafe { getopt(argc, argv.as_ptr(), opts.as_ptr()) };
        assert_eq!(
            r,
            i32::from(b':'),
            "missing arg with ':'-prefix should return ':'"
        );
    }

    // -----------------------------------------------------------------------
    // getopt_long — long options
    // -----------------------------------------------------------------------

    /// Helper to build a null-terminated `Option` array for getopt_long.
    fn make_longopts(specs: &[(&[u8], i32, i32)]) -> (Vec<Option>, Vec<Vec<u8>>) {
        let mut backing: Vec<Vec<u8>> = Vec::new();
        let mut opts: Vec<Option> = Vec::new();

        for &(name, _, _) in specs {
            let mut n = name.to_vec();
            n.push(0);
            backing.push(n);
        }

        for (i, &(_, has_arg, val)) in specs.iter().enumerate() {
            opts.push(Option {
                name: backing.get(i).map_or(core::ptr::null(), |v| v.as_ptr()),
                has_arg,
                flag: core::ptr::null_mut(),
                val,
            });
        }

        // Sentinel entry with null name.
        opts.push(Option {
            name: core::ptr::null(),
            has_arg: 0,
            flag: core::ptr::null_mut(),
            val: 0,
        });

        (opts, backing)
    }

    #[test]
    fn test_long_option_verbose() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--verbose"]);
        let opts = cstr(""); // no short options
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'v'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r, i32::from(b'v'));
        assert_eq!(longindex, 0);

        let r2 = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r2, -1);
    }

    #[test]
    fn test_long_option_with_equals_arg() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--output=myfile"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"output", REQUIRED_ARGUMENT, i32::from(b'o'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r, i32::from(b'o'));
        assert_eq!(longindex, 0);

        // optarg should point to "myfile" (after the '=').
        let arg_ptr = unsafe { core::ptr::addr_of!(optarg).read() };
        assert!(!arg_ptr.is_null());
        assert_eq!(unsafe { *arg_ptr }, b'm');
    }

    #[test]
    fn test_long_option_with_separate_arg() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--output", "myfile"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"output", REQUIRED_ARGUMENT, i32::from(b'o'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r, i32::from(b'o'));

        let arg_ptr = unsafe { core::ptr::addr_of!(optarg).read() };
        assert!(!arg_ptr.is_null());
        assert_eq!(unsafe { *arg_ptr }, b'm');

        // optind should have advanced past both "--output" and "myfile".
        let ind = unsafe { core::ptr::addr_of!(optind).read() };
        assert_eq!(ind, 3);
    }

    #[test]
    fn test_long_option_flag_mode() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--debug"]);
        let opts = cstr("");

        let mut flag_val: i32 = 0;
        // Build longopts manually to use the `flag` field.
        let name = cstr("debug");
        let longopts = [
            Option {
                name: name.as_ptr(),
                has_arg: NO_ARGUMENT,
                flag: &mut flag_val,
                val: 42,
            },
            Option {
                name: core::ptr::null(),
                has_arg: 0,
                flag: core::ptr::null_mut(),
                val: 0,
            },
        ];

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            core::ptr::null_mut(),
        );
        // When flag is non-null, getopt_long should return 0 and set *flag.
        assert_eq!(r, 0);
        assert_eq!(flag_val, 42);
    }

    #[test]
    fn test_long_option_unknown() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--nonexistent"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'v'))]);

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            core::ptr::null_mut(),
        );
        assert_eq!(r, i32::from(b'?'), "unknown long option should return '?'");
    }

    #[test]
    fn test_long_option_missing_required_arg() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--output"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"output", REQUIRED_ARGUMENT, i32::from(b'o'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(
            r,
            i32::from(b'?'),
            "missing required arg for long opt should return '?'"
        );
    }

    #[test]
    fn test_long_and_short_mixed() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "-a", "--verbose", "-b"]);
        let opts = cstr("ab");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'v'))]);
        let mut longindex: i32 = -1;

        let r1 = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r1, i32::from(b'a'));

        let r2 = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r2, i32::from(b'v'));
        assert_eq!(longindex, 0);

        let r3 = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r3, i32::from(b'b'));

        let r4 = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r4, -1);
    }

    #[test]
    fn test_long_option_no_arg_with_equals_is_error() {
        let _g = unsafe { reset_getopt_state() };
        // "--verbose=foo" when verbose takes no argument.
        let (argc, argv, _b) = make_argv(&["prog", "--verbose=foo"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'v'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(
            r,
            i32::from(b'?'),
            "=value on no_argument option is an error"
        );
    }

    #[test]
    fn test_long_option_multiple_defined() {
        let _g = unsafe { reset_getopt_state() };
        let (argc, argv, _b) = make_argv(&["prog", "--beta"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[
            (b"alpha", NO_ARGUMENT, 1),
            (b"beta", NO_ARGUMENT, 2),
            (b"gamma", NO_ARGUMENT, 3),
        ]);
        let mut longindex: i32 = -1;

        let r = getopt_long(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r, 2);
        assert_eq!(longindex, 1, "longindex should point to the matched entry");
    }

    // -----------------------------------------------------------------------
    // Option characters
    // -----------------------------------------------------------------------

    #[test]
    fn colon_and_semicolon_are_never_options() {
        let opts = cstr("a:b;c-");
        // SAFETY: a NUL-terminated optstring.
        unsafe {
            assert!(spec_of(opts.as_ptr(), b':').is_none());
            assert!(spec_of(opts.as_ptr(), b';').is_none());
            assert!(spec_of(opts.as_ptr(), 0).is_none());
            // glibc's strchr: `-` is an option character like any other.
            assert!(spec_of(opts.as_ptr(), b'-').is_some());
            assert!(spec_of(opts.as_ptr(), b'a').is_some());
            assert!(spec_of(opts.as_ptr(), b'c').is_some());
            assert!(spec_of(opts.as_ptr(), b'z').is_none());
        }
        let colon = cstr(":ab");
        // SAFETY: as above.
        unsafe {
            assert!(spec_of(colon.as_ptr(), b'a').is_some());
            assert!(spec_of(colon.as_ptr(), b'z').is_none());
        }
    }

    // -----------------------------------------------------------------------
    // getopt_long_only — single-dash long options
    // -----------------------------------------------------------------------

    #[test]
    fn test_long_only_single_dash() {
        let _g = unsafe { reset_getopt_state() };
        // "-verbose" should match long option "verbose" with long_only.
        let (argc, argv, _b) = make_argv(&["prog", "-verbose"]);
        let opts = cstr("v");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'V'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long_only(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        // Should match the long option, not the short 'v'.
        assert_eq!(r, i32::from(b'V'));
        assert_eq!(longindex, 0);
    }

    #[test]
    fn test_long_only_with_arg() {
        let _g = unsafe { reset_getopt_state() };
        // "-output=file.txt" should match long option with =arg.
        let (argc, argv, _b) = make_argv(&["prog", "-output=file.txt"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"output", REQUIRED_ARGUMENT, i32::from(b'o'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long_only(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r, i32::from(b'o'));
        assert_eq!(longindex, 0);

        // optarg should point to "file.txt".
        let arg_ptr = unsafe { core::ptr::addr_of!(optarg).read() };
        assert!(!arg_ptr.is_null());
        assert_eq!(unsafe { *arg_ptr }, b'f');
    }

    #[test]
    fn test_long_only_double_dash_still_works() {
        let _g = unsafe { reset_getopt_state() };
        // "--verbose" should still work with getopt_long_only.
        let (argc, argv, _b) = make_argv(&["prog", "--verbose"]);
        let opts = cstr("");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'V'))]);
        let mut longindex: i32 = -1;

        let r = getopt_long_only(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            &mut longindex,
        );
        assert_eq!(r, i32::from(b'V'));
        assert_eq!(longindex, 0);
    }

    #[test]
    fn test_long_only_no_match_falls_to_short() {
        let _g = unsafe { reset_getopt_state() };
        // "-a" should still work as short option when no long option matches.
        let (argc, argv, _b) = make_argv(&["prog", "-a"]);
        let opts = cstr("ab");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'V'))]);

        let r = getopt_long_only(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            core::ptr::null_mut(),
        );
        assert_eq!(r, i32::from(b'a'));
    }

    #[test]
    fn test_long_only_unknown_option() {
        let _g = unsafe { reset_getopt_state() };
        // "-nonexistent" with no matching long option and not a valid short.
        let (argc, argv, _b) = make_argv(&["prog", "-nonexistent"]);
        let opts = cstr("ab");
        let (longopts, _lb) = make_longopts(&[(b"verbose", NO_ARGUMENT, i32::from(b'V'))]);

        let r = getopt_long_only(
            argc,
            argv.as_ptr(),
            opts.as_ptr(),
            longopts.as_ptr(),
            core::ptr::null_mut(),
        );
        // Should return '?' for unrecognized option.
        assert_eq!(r, i32::from(b'?'));
    }

    // -----------------------------------------------------------------------
    // glibc's answers
    // -----------------------------------------------------------------------

    /// glibc 2.39's answers, one line a parse (`getopt_harness.py`).
    const ORACLE: &str = include_str!("getopt_oracle.txt");

    /// The harness's flag variable, which its tables' `flag` entries set.
    static mut FLAGVAR: i32 = 0;

    /// A long-option table of the harness's, and the names it points at.
    struct Table {
        opts: Vec<Option>,
        _names: Vec<Vec<u8>>,
    }

    /// The harness's tables, by name: (name, has_arg, sets the flag, val).
    fn table(which: &str) -> std::option::Option<Table> {
        let entries: &[(&str, i32, bool, i32)] = match which {
            "t1" => &[
                ("verbose", 0, false, b'v' as i32),
                ("version", 0, false, b'V' as i32),
                ("file", 1, false, b'f' as i32),
                ("opt", 2, false, b'o' as i32),
                ("flag", 0, true, 42),
                ("nofile", 0, false, b'n' as i32),
                ("verbatim", 0, false, b'v' as i32),
                ("color", 2, false, 300),
                ("colour", 2, false, 300),
                ("x", 0, false, b'x' as i32),
            ],
            "t2" => &[
                ("aa", 0, false, b'1' as i32),
                ("aab", 1, false, b'2' as i32),
                ("ab", 2, false, b'3' as i32),
                ("b", 1, false, b'4' as i32),
                ("bee", 0, true, 7),
                ("a", 0, false, b'5' as i32),
            ],
            _ => return None,
        };
        let names: Vec<Vec<u8>> = entries.iter().map(|e| cstr(e.0)).collect();
        let mut opts: Vec<Option> = entries
            .iter()
            .zip(&names)
            .map(|(e, n)| Option {
                name: n.as_ptr(),
                has_arg: e.1,
                flag: if e.2 {
                    core::ptr::addr_of_mut!(FLAGVAR)
                } else {
                    core::ptr::null_mut()
                },
                val: e.3,
            })
            .collect();
        opts.push(Option {
            name: core::ptr::null(),
            has_arg: 0,
            flag: core::ptr::null_mut(),
            val: 0,
        });
        Some(Table {
            opts,
            _names: names,
        })
    }

    /// A text as the harness writes it: `\xHH` for a byte outside `!`..`~`
    /// and for `\`, `\x` for none.
    fn token(b: &[u8]) -> String {
        use core::fmt::Write;
        if b.is_empty() {
            return "\\x".into();
        }
        b.iter().fold(String::new(), |mut s, &c| {
            if (0x21..=0x7e).contains(&c) && c != b'\\' {
                s.push(c as char);
            } else {
                // Writing into a String cannot fail.
                let _ = write!(s, "\\x{c:02x}");
            }
            s
        })
    }

    /// The harness's text form, back to bytes.
    fn untoken(t: &str) -> Vec<u8> {
        if t == "\\x" {
            return Vec::new();
        }
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn posixly_on() -> bool {
        true
    }

    fn posixly_off() -> bool {
        false
    }

    /// What the harness prints after ` = ` for `case`.
    fn replay(case: &str) -> String {
        use core::fmt::Write;
        let f: Vec<&str> = case.split(' ').collect();
        let (how, which, posixly, err) = (f[0], f[1], f[2] == "1", f[3].parse::<i32>().unwrap());
        let optstring = cstr_bytes(&untoken(f[4]));
        let mut strings: Vec<Vec<u8>> = std::vec![cstr("prog")];
        strings.extend(f[5..].iter().map(|a| cstr_bytes(&untoken(a))));
        let orig: Vec<*const u8> = strings.iter().map(|s| s.as_ptr()).collect();
        let mut argv = orig.clone();
        argv.push(core::ptr::null());
        let argc = i32::try_from(orig.len()).unwrap();
        let tab = table(which);
        let longopts = tab.as_ref().map_or(core::ptr::null(), |t| t.opts.as_ptr());
        let pair = crate::error::capture::Pair::new();
        let io = Io {
            posixly_correct: if posixly { posixly_on } else { posixly_off },
            messages: pair.stderr,
        };
        // SAFETY: the test holds the getopt lock; the globals are plain words.
        unsafe {
            core::ptr::addr_of_mut!(optind).write(0);
            core::ptr::addr_of_mut!(opterr).write(err);
            core::ptr::addr_of_mut!(optopt).write(1234);
            core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
            core::ptr::addr_of_mut!(FLAGVAR).write(0);
        }
        let mut out = String::new();
        for _ in 0..40 {
            let mut li = -1;
            // SAFETY: `argv` holds `argc` strings and a NULL; the table is
            // ended by a NULL name; `li` is writable.
            let rc = unsafe {
                call(
                    argc,
                    argv.as_ptr(),
                    optstring.as_ptr(),
                    longopts,
                    &raw mut li,
                    how == "o",
                    &io,
                )
            };
            // SAFETY: as above.
            let (ind, opt, arg, flag) = unsafe {
                (
                    core::ptr::addr_of!(optind).read(),
                    core::ptr::addr_of!(optopt).read(),
                    core::ptr::addr_of!(optarg).read(),
                    core::ptr::addr_of!(FLAGVAR).read(),
                )
            };
            let _ = write!(out, " {rc},{ind},{opt},");
            if arg.is_null() {
                out.push('-');
            } else {
                // SAFETY: `optarg` points into one of the strings.
                let s = unsafe { core::ffi::CStr::from_ptr(arg.cast()) }.to_bytes();
                out.push_str(&token(s));
                let at = orig.iter().enumerate().rev().find(|&(_, &o)| {
                    // SAFETY: each `o` is one of the strings.
                    let len = unsafe { core::ffi::CStr::from_ptr(o.cast()) }
                        .to_bytes()
                        .len();
                    arg as usize >= o as usize && arg as usize <= o as usize + len
                });
                match at {
                    Some((i, _)) => {
                        let _ = write!(out, "@{i}");
                    }
                    None => out.push_str("@?"),
                }
            }
            let _ = write!(out, ",{li},{flag}");
            if rc == -1 {
                break;
            }
        }
        out.push_str(" |");
        for &p in &argv[..orig.len()] {
            // SAFETY: argv holds the strings, reordered.
            let s = unsafe { core::ffi::CStr::from_ptr(p.cast()) }.to_bytes();
            let _ = write!(out, " {}", token(s));
        }
        let _ = write!(out, " | {}", token(&pair.sink));
        out.trim_start().to_string()
    }

    /// Every parse `getopt_harness.py` ran against glibc 2.39, replayed in
    /// its order, the parse state carried from one to the next as glibc's
    /// is (its `optopt` is only reset by an error).
    #[test]
    fn every_parse_is_glibcs() {
        let _g = unsafe { reset_getopt_state() };
        let mut wrong = Vec::new();
        let mut n = 0;
        for line in ORACLE
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (case, want) = line.split_once(" = ").unwrap();
            n += 1;
            let got = replay(case);
            if got != want {
                wrong.push(format!("{case}\n  glibc: {want}\n  ours:  {got}"));
            }
        }
        assert!(n > 2500, "the oracle has {n} lines");
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
