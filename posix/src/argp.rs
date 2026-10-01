//! GNU's argp, `<argp.h>`: command-line parsing described by tables of
//! options, with the `--help` and `--usage` messages made from the same
//! tables -- glibc 2.39's (`posix/tools/oracle/argp_harness.py`,
//! `argp_oracle.txt`; design-decisions §1163).
//!
//! A program gives `argp_parse` a `struct argp`: its options, a parser
//! function that is called for each option and argument found, the
//! documentation for its help, and child argps whose options join its own
//! -- each with a parser, an input of its own, and a place in the help.
//! argp adds `--help`, `--usage` and, when the program has a version,
//! `--version`, parses the arguments with getopt (the library's own,
//! `posix/src/getopt.rs`, in a state of argp's, so the program's `optind`
//! is not touched), and calls the parsers for each option, each argument,
//! and the moments around them (`ARGP_KEY_INIT` ... `ARGP_KEY_FINI`).
//!
//! - `parse.rs`: the parse, glibc's argp-parse.
//! - `help.rs`: the help and usage messages, glibc's argp-help -- options
//!   sorted by group and cluster and name, columns, the usage line.
//! - `fmt.rs`: the line filler both are written through, glibc's
//!   argp-fmtstream, buffer and all, since where a line breaks depends on
//!   it.
//!
//! Each of the four variables -- `argp_program_version`,
//! `argp_program_version_hook`, `argp_program_bug_address`,
//! `argp_err_exit_status` -- is in an archive member of its own: a program
//! commonly defines `argp_program_version` and `argp_program_bug_address`
//! itself, and a member that defined one of them beside anything the
//! program needs would be a second definition.
//!
//! Where it differs from glibc's -- each for a reason in §1163 -- the
//! tests' list says so (`DEVIATIONS` in the tests): `ARGP_HELP_FMT` is read
//! at each message rather than once a process; `argc` 0 with a NULL `argv`
//! is no fault; a help message whose margins leave no room ends; and an
//! `OPTION_DOC` entry is not listed in the usage line as an option, as the
//! manual says it is not one.

use core::ffi::c_void;

/// A function argp calls through a pointer, defined `extern "C"` -- and
/// `C-unwind` in the host tests, whose exits unwind through it
/// (`exit_now`). Not `C-unwind` on the target: the library is built to
/// abort, and there a call that may unwind gets a landing pad that aborts
/// if it does, which names a personality routine, `rust_eh_personality`
/// -- which nothing a C program links defines (check-libc-shape.py's
/// CHECK 6). The types such a function is called through are paired the
/// same way: `ArgpParserFn`, `ArgpHelpFilterFn`, `ArgpVersionHookFn`.
macro_rules! callback {
    ($(#[$m:meta])* $vis:vis unsafe fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$m])*
        #[cfg(not(test))]
        $vis unsafe extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
        $(#[$m])*
        #[cfg(test)]
        $vis unsafe extern "C-unwind" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
}

mod fmt;
mod help;
mod parse;

// ---------------------------------------------------------------------------
// The interface's types, glibc's layouts.
// ---------------------------------------------------------------------------

/// `struct argp_option`.
#[repr(C)]
pub struct ArgpOption {
    /// The long name, or NULL; for an `OPTION_DOC` entry, the text shown.
    pub name: *const u8,
    /// The key the parser is called with: a printable character is also
    /// the short option.
    pub key: i32,
    /// The argument's name, or NULL for none.
    pub arg: *const u8,
    /// `OPTION_*`.
    pub flags: i32,
    /// The help text; with a NULL name and key, a group's header.
    pub doc: *const u8,
    /// Where it goes in the help: 0 is the previous entry's group.
    pub group: i32,
}

/// The parser function: a key, its argument, the parse state; 0, an
/// `errno` value, or `ARGP_ERR_UNKNOWN`. (`C-unwind` in the host tests, so
/// that a test's parser may end the parse as `exit` does: `callback!`.)
#[cfg(not(test))]
pub type ArgpParserFn = unsafe extern "C" fn(key: i32, arg: *mut u8, state: *mut ArgpState) -> i32;
/// The parser function, as the host tests have it (above).
#[cfg(test)]
pub type ArgpParserFn =
    unsafe extern "C-unwind" fn(key: i32, arg: *mut u8, state: *mut ArgpState) -> i32;

/// The help filter: a key, the text argp would print, the argp's input;
/// the text to print -- the same, a new `malloc`ed one, or NULL for none.
#[cfg(not(test))]
pub type ArgpHelpFilterFn =
    unsafe extern "C" fn(key: i32, text: *const u8, input: *mut c_void) -> *mut u8;
/// The help filter, as the host tests have it (above).
#[cfg(test)]
pub type ArgpHelpFilterFn =
    unsafe extern "C-unwind" fn(key: i32, text: *const u8, input: *mut c_void) -> *mut u8;

/// `argp_program_version_hook`'s function: the stream to print the
/// version on, and the state.
#[cfg(not(test))]
pub type ArgpVersionHookFn = unsafe extern "C" fn(stream: *mut u8, state: *mut ArgpState);
/// The version hook, as the host tests have it (above).
#[cfg(test)]
pub type ArgpVersionHookFn = unsafe extern "C-unwind" fn(stream: *mut u8, state: *mut ArgpState);

/// `struct argp`.
#[repr(C)]
pub struct Argp {
    /// The options, ended by an entry of zeros; or NULL.
    pub options: *const ArgpOption,
    /// The parser, or NULL.
    pub parser: Option<ArgpParserFn>,
    /// What the arguments are, for the usage line; `\n` between alternatives.
    pub args_doc: *const u8,
    /// The help's text: before the options, and after a `\v` after them.
    pub doc: *const u8,
    /// The children, ended by an entry whose argp is NULL; or NULL.
    pub children: *const ArgpChild,
    /// The help filter, or NULL.
    pub help_filter: Option<ArgpHelpFilterFn>,
    /// The translation domain the texts are looked up in.
    pub argp_domain: *const u8,
}

/// `struct argp_child`.
#[repr(C)]
pub struct ArgpChild {
    /// The child, or NULL to end the list.
    pub argp: *const Argp,
    /// Unused (glibc's too).
    pub flags: i32,
    /// A header for the child's options in the help, or NULL.
    pub header: *const u8,
    /// Where the child's options go in the help.
    pub group: i32,
}

/// `struct argp_state`.
#[repr(C)]
pub struct ArgpState {
    /// The argp the parse began from -- argp's own, holding the program's
    /// and the built-in options, unless `ARGP_NO_HELP`.
    pub root_argp: *const Argp,
    pub argc: i32,
    pub argv: *mut *mut u8,
    /// The index in `argv` of the next argument to parse; a parser may move it.
    pub next: i32,
    /// `argp_parse`'s flags.
    pub flags: u32,
    /// For `ARGP_KEY_ARG`, the number of arguments this parser has had.
    pub arg_num: u32,
    /// The index in `argv` after a `--`, or 0.
    pub quoted: i32,
    /// This parser's input.
    pub input: *mut c_void,
    /// Its children's inputs, which it sets at `ARGP_KEY_INIT`.
    pub child_inputs: *mut *mut c_void,
    /// The parser's own word, kept from call to call.
    pub hook: *mut c_void,
    /// The program's name, for messages.
    pub name: *mut u8,
    /// Where errors and help go: `FILE *`s.
    pub err_stream: *mut u8,
    pub out_stream: *mut u8,
    /// argp's own.
    pub pstate: *mut c_void,
}

// SAFETY: plain C data -- the library's own tables of these are immutable
// statics, read by every thread.
unsafe impl Sync for ArgpOption {}
// SAFETY: as above.
unsafe impl Sync for Argp {}
// SAFETY: as above.
unsafe impl Sync for ArgpChild {}

/// glibc's layouts, which a program's tables are written in.
const _: () = {
    assert!(size_of::<ArgpOption>() == 48);
    assert!(core::mem::offset_of!(ArgpOption, doc) == 32);
    assert!(size_of::<Argp>() == 56);
    assert!(size_of::<ArgpChild>() == 32);
    assert!(size_of::<ArgpState>() == 96);
    assert!(core::mem::offset_of!(ArgpState, name) == 64);
};

/// `OPTION_*` flags.
pub const OPTION_ARG_OPTIONAL: i32 = 0x1;
pub const OPTION_HIDDEN: i32 = 0x2;
pub const OPTION_ALIAS: i32 = 0x4;
pub const OPTION_DOC: i32 = 0x8;
pub const OPTION_NO_USAGE: i32 = 0x10;

/// A parser's "not mine": E2BIG, as glibc's.
pub const ARGP_ERR_UNKNOWN: i32 = crate::errno::E2BIG;

/// The special keys.
pub const ARGP_KEY_ARG: i32 = 0;
pub const ARGP_KEY_ARGS: i32 = 0x100_0006;
pub const ARGP_KEY_END: i32 = 0x100_0001;
pub const ARGP_KEY_NO_ARGS: i32 = 0x100_0002;
pub const ARGP_KEY_INIT: i32 = 0x100_0003;
pub const ARGP_KEY_FINI: i32 = 0x100_0007;
pub const ARGP_KEY_SUCCESS: i32 = 0x100_0004;
pub const ARGP_KEY_ERROR: i32 = 0x100_0005;

/// The help filter's keys.
pub const ARGP_KEY_HELP_PRE_DOC: i32 = 0x200_0001;
pub const ARGP_KEY_HELP_POST_DOC: i32 = 0x200_0002;
pub const ARGP_KEY_HELP_HEADER: i32 = 0x200_0003;
pub const ARGP_KEY_HELP_EXTRA: i32 = 0x200_0004;
pub const ARGP_KEY_HELP_DUP_ARGS_NOTE: i32 = 0x200_0005;
pub const ARGP_KEY_HELP_ARGS_DOC: i32 = 0x200_0006;

/// `argp_parse`'s flags.
pub const ARGP_PARSE_ARGV0: u32 = 0x01;
pub const ARGP_NO_ERRS: u32 = 0x02;
pub const ARGP_NO_ARGS: u32 = 0x04;
pub const ARGP_IN_ORDER: u32 = 0x08;
pub const ARGP_NO_HELP: u32 = 0x10;
pub const ARGP_NO_EXIT: u32 = 0x20;
pub const ARGP_LONG_ONLY: u32 = 0x40;

/// `argp_help`'s flags.
pub const ARGP_HELP_USAGE: u32 = 0x01;
pub const ARGP_HELP_SHORT_USAGE: u32 = 0x02;
pub const ARGP_HELP_SEE: u32 = 0x04;
pub const ARGP_HELP_LONG: u32 = 0x08;
pub const ARGP_HELP_PRE_DOC: u32 = 0x10;
pub const ARGP_HELP_POST_DOC: u32 = 0x20;
pub const ARGP_HELP_BUG_ADDR: u32 = 0x40;
pub const ARGP_HELP_LONG_ONLY: u32 = 0x80;
pub const ARGP_HELP_EXIT_ERR: u32 = 0x100;
pub const ARGP_HELP_EXIT_OK: u32 = 0x200;
pub const ARGP_HELP_STD_ERR: u32 = ARGP_HELP_SEE | ARGP_HELP_EXIT_ERR;
pub const ARGP_HELP_STD_USAGE: u32 = ARGP_HELP_SHORT_USAGE | ARGP_HELP_SEE | ARGP_HELP_EXIT_ERR;
pub const ARGP_HELP_STD_HELP: u32 = ARGP_HELP_SHORT_USAGE
    | ARGP_HELP_LONG
    | ARGP_HELP_EXIT_OK
    | ARGP_HELP_PRE_DOC
    | ARGP_HELP_POST_DOC
    | ARGP_HELP_BUG_ADDR;

// ---------------------------------------------------------------------------
// The program's variables: each its own member (the module documentation).
// ---------------------------------------------------------------------------

/// Own archive member.
mod gnu_argp_pv {
    /// The version `--version` prints; NULL for no `--version`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub static mut argp_program_version: *const u8 = core::ptr::null();
}
pub use gnu_argp_pv::argp_program_version;

/// Own archive member.
mod gnu_argp_pvh {
    /// Called for `--version` instead, with the stream and the state.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub static mut argp_program_version_hook: Option<super::ArgpVersionHookFn> = None;
}
pub use gnu_argp_pvh::argp_program_version_hook;

/// Own archive member.
mod gnu_argp_ba {
    /// The address the help ends by giving, or NULL.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub static mut argp_program_bug_address: *const u8 = core::ptr::null();
}
pub use gnu_argp_ba::argp_program_bug_address;

/// Own archive member.
mod gnu_argp_eexst {
    /// The status a usage error exits with: EX_USAGE, 64.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub static mut argp_err_exit_status: i32 = 64;
}
pub use gnu_argp_eexst::argp_err_exit_status;

/// The version string, the hook, the bug address and the exit status, read
/// as they are now.
fn version() -> *const u8 {
    // SAFETY: a plain read of a C variable.
    unsafe { (&raw const argp_program_version).read() }
}

fn version_hook() -> Option<ArgpVersionHookFn> {
    // SAFETY: as above.
    unsafe { (&raw const argp_program_version_hook).read() }
}

fn bug_address() -> *const u8 {
    // SAFETY: as above.
    unsafe { (&raw const argp_program_bug_address).read() }
}

fn err_exit_status() -> i32 {
    // SAFETY: as above.
    unsafe { (&raw const argp_err_exit_status).read() }
}

// ---------------------------------------------------------------------------
// The process's end, and its name.
// ---------------------------------------------------------------------------

/// The process ended, as argp ends it: `exit(status)`. Host tests: a panic
/// carrying the status, which the test catches, as the oracle's harness
/// catches its child's exit.
fn exit_now(status: i32) -> ! {
    #[cfg(test)]
    std::panic::panic_any(tests::Exited(status));
    #[cfg(not(test))]
    crate::crt::exit(status)
}

/// `program_invocation_short_name`, which messages use without a state.
fn short_program_name() -> *mut u8 {
    // SAFETY: a plain read of the word `__libc_start_main` set.
    unsafe { (&raw const crate::crt::__progname).read().cast_mut() }
}

/// A C string's bytes, without its NUL: `(null)` for NULL, as `%s` prints it.
///
/// # Safety
///
/// `s` is NULL or a C string, alive as long as the bytes are used.
unsafe fn text<'a>(s: *const u8) -> &'a [u8] {
    if s.is_null() {
        return b"(null)";
    }
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) }
}

/// A stream locked for the length of a message, and unlocked however it
/// ends -- a test's exit unwinds through.
struct Locked(*mut u8);

impl Locked {
    fn new(stream: *mut u8) -> Self {
        crate::stdio::flockfile(stream.cast());
        Self(stream)
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        crate::stdio::funlockfile(self.0.cast());
    }
}

/// `bytes` on `stream`; a failure is the stream's to report, as glibc's is.
fn put(stream: *mut u8, bytes: &[u8]) {
    if !bytes.is_empty() {
        // SAFETY: the caller's stream; the bytes are live.
        unsafe { crate::stdio::fwrite(bytes.as_ptr(), 1, bytes.len(), stream) };
    }
}

// ---------------------------------------------------------------------------
// The calls.
// ---------------------------------------------------------------------------

/// `argp_parse(argp, argc, argv, flags, arg_index, input)`: parse `argv` with
/// `argp`, calling its parsers; 0, or an error number. `*arg_index` (when
/// not NULL) is the index of the first argument not parsed.
///
/// # Safety
///
/// `argp` is NULL or a valid argp tree; `argv` holds `argc` pointers to C
/// strings (or is NULL with `argc` 0), and may be permuted.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn argp_parse(
    argp: *const Argp,
    argc: i32,
    argv: *mut *mut u8,
    flags: u32,
    arg_index: *mut i32,
    input: *mut c_void,
) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { parse::parse(argp, argc, argv, flags, arg_index, input) }
}

/// `argp_help(argp, stream, flags, name)`: the help `flags` asks for, of
/// `argp`, on `stream`, as program `name`. It never exits.
///
/// # Safety
///
/// `argp` is a valid argp tree; `stream` a `FILE *` or NULL; `name` NULL
/// or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn argp_help(argp: *const Argp, stream: *mut u8, flags: u32, name: *mut u8) {
    // SAFETY: the caller's contract.
    unsafe { help::help(argp, core::ptr::null(), stream, flags, name) };
}

/// `argp_state_help(state, stream, flags)`: the help of `state`'s parse --
/// nothing under `ARGP_NO_ERRS` -- and the exit `flags` asks for, unless
/// `ARGP_NO_EXIT`.
///
/// # Safety
///
/// `state` is NULL or a parse's state; `stream` a `FILE *` or NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn argp_state_help(state: *const ArgpState, stream: *mut u8, flags: u32) {
    // SAFETY: the caller's contract.
    unsafe { state_help(state, stream, flags) };
}

/// glibc's `__argp_state_help`.
///
/// # Safety
///
/// As [`argp_state_help`].
pub(crate) unsafe fn state_help(state: *const ArgpState, stream: *mut u8, mut flags: u32) {
    // SAFETY: NULL or a parse's state.
    let st = unsafe { state.as_ref() };
    if st.is_some_and(|s| s.flags & ARGP_NO_ERRS != 0) || stream.is_null() {
        return;
    }
    if st.is_some_and(|s| s.flags & ARGP_LONG_ONLY != 0) {
        flags |= ARGP_HELP_LONG_ONLY;
    }
    let (root, name) = st.map_or((core::ptr::null(), short_program_name()), |s| {
        (s.root_argp, s.name)
    });
    // SAFETY: the parse's root argp and name, or NULL.
    unsafe { help::help(root, state, stream, flags, name) };
    if st.is_none_or(|s| s.flags & ARGP_NO_EXIT == 0) {
        if flags & ARGP_HELP_EXIT_ERR != 0 {
            exit_now(err_exit_status());
        }
        if flags & ARGP_HELP_EXIT_OK != 0 {
            exit_now(0);
        }
    }
}

/// `argp_usage(state)`: the short usage and how to see more, on standard
/// error, and the usage error's exit.
///
/// # Safety
///
/// As [`argp_state_help`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn argp_usage(state: *const ArgpState) {
    // SAFETY: the caller's contract.
    unsafe { usage(state) };
}

/// `argp_usage`'s body -- the tests' way in, since a test's exit unwinds
/// and an `extern "C"` function may not be unwound through.
///
/// # Safety
///
/// As [`argp_usage`].
pub(crate) unsafe fn usage(state: *const ArgpState) {
    // SAFETY: the caller's contract.
    unsafe { state_help(state, crate::stdio::stderr_stream(), ARGP_HELP_STD_USAGE) };
}

/// `argp_error(state, fmt, ...)`'s body: `name: message`, how to see more,
/// and the usage error's exit -- nothing under `ARGP_NO_ERRS`.
///
/// # Safety
///
/// `state` is NULL or a parse's state; `fmt` and `ap` as for `vprintf`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __argp_verror(
    state: *const ArgpState,
    fmt: *const u8,
    ap: *mut crate::printf::VaList,
) {
    // SAFETY: the caller's contract.
    unsafe { verror(state, fmt, ap) };
}

/// `__argp_verror`'s body, as [`usage`] is `argp_usage`'s.
///
/// # Safety
///
/// As [`__argp_verror`].
pub(crate) unsafe fn verror(
    state: *const ArgpState,
    fmt: *const u8,
    ap: *mut crate::printf::VaList,
) {
    // SAFETY: as `vprintf`'s.
    let mut args = unsafe { crate::printf::Args::from_raw(ap) };
    // SAFETY: the caller's contract.
    unsafe { error(state, fmt, &mut args) };
}

/// glibc's `__argp_error`, its arguments taken.
///
/// # Safety
///
/// As [`__argp_verror`].
pub(crate) unsafe fn error(
    state: *const ArgpState,
    fmt: *const u8,
    args: &mut crate::printf::Args,
) {
    // SAFETY: the caller's contract.
    unsafe {
        error_with(state, |stream| {
            if !fmt.is_null() {
                let _ = crate::printf::_fprintf_impl(stream, fmt, args); // as `put`
            }
        });
    }
}

/// `argp_error` with a message of pieces put as they are: argp's own,
/// some of whose pieces (an option's name) are no format.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
pub(crate) unsafe fn error_message(state: *const ArgpState, pieces: &[&[u8]]) {
    // SAFETY: the caller's contract.
    unsafe {
        error_with(state, |stream| {
            for p in pieces {
                put(stream, p);
            }
        });
    }
}

/// `name: <body>`, how to see more, and the exit: `argp_error`'s frame.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
unsafe fn error_with(state: *const ArgpState, body: impl FnOnce(*mut u8)) {
    // SAFETY: NULL or a parse's state.
    let st = unsafe { state.as_ref() };
    if st.is_some_and(|s| s.flags & ARGP_NO_ERRS != 0) {
        return;
    }
    let stream = st.map_or_else(crate::stdio::stderr_stream, |s| s.err_stream);
    if stream.is_null() {
        return;
    }
    let _l = Locked::new(stream);
    let name = st.map_or_else(short_program_name, |s| s.name);
    // SAFETY: the state's name, or the program's.
    put(stream, unsafe { text(name) });
    put(stream, b": ");
    body(stream);
    put(stream, b"\n");
    // SAFETY: the caller's state, and its stream.
    unsafe { state_help(state, stream, ARGP_HELP_STD_ERR) };
}

/// `argp_failure(state, status, errnum, fmt, ...)`'s body: `name: message:
/// strerror(errnum)`, and `exit(status)` when `status` is not 0, unless
/// `ARGP_NO_EXIT` -- nothing under `ARGP_NO_ERRS`.
///
/// # Safety
///
/// As [`__argp_verror`]; `fmt` may be NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __argp_vfailure(
    state: *const ArgpState,
    status: i32,
    errnum: i32,
    fmt: *const u8,
    ap: *mut crate::printf::VaList,
) {
    // SAFETY: the caller's contract.
    unsafe { vfailure(state, status, errnum, fmt, ap) };
}

/// `__argp_vfailure`'s body, as [`usage`] is `argp_usage`'s.
///
/// # Safety
///
/// As [`__argp_vfailure`].
pub(crate) unsafe fn vfailure(
    state: *const ArgpState,
    status: i32,
    errnum: i32,
    fmt: *const u8,
    ap: *mut crate::printf::VaList,
) {
    // SAFETY: as `vprintf`'s.
    let mut args = unsafe { crate::printf::Args::from_raw(ap) };
    // SAFETY: the caller's contract.
    unsafe { failure(state, status, errnum, fmt, &mut args) };
}

/// glibc's `__argp_failure`, its arguments taken.
///
/// # Safety
///
/// As [`__argp_vfailure`].
pub(crate) unsafe fn failure(
    state: *const ArgpState,
    status: i32,
    errnum: i32,
    fmt: *const u8,
    args: &mut crate::printf::Args,
) {
    let body = (!fmt.is_null()).then_some(|stream| {
        let _ = crate::printf::_fprintf_impl(stream, fmt, args); // as `put`
    });
    // SAFETY: the caller's contract.
    unsafe { failure_with(state, status, errnum, body) };
}

/// `argp_failure` with a message of pieces put as they are.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
pub(crate) unsafe fn failure_message(
    state: *const ArgpState,
    status: i32,
    errnum: i32,
    pieces: &[&[u8]],
) {
    let body = |stream| {
        for p in pieces {
            put(stream, p);
        }
    };
    // SAFETY: the caller's contract.
    unsafe { failure_with(state, status, errnum, Some(body)) };
}

/// `name[: <body>][: strerror(errnum)]`, and the exit: `argp_failure`'s frame.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
unsafe fn failure_with(
    state: *const ArgpState,
    status: i32,
    errnum: i32,
    body: Option<impl FnOnce(*mut u8)>,
) {
    // SAFETY: NULL or a parse's state.
    let st = unsafe { state.as_ref() };
    if st.is_some_and(|s| s.flags & ARGP_NO_ERRS != 0) {
        return;
    }
    let stream = st.map_or_else(crate::stdio::stderr_stream, |s| s.err_stream);
    if stream.is_null() {
        return;
    }
    {
        let _l = Locked::new(stream);
        let name = st.map_or_else(short_program_name, |s| s.name);
        // SAFETY: the state's name, or the program's.
        put(stream, unsafe { text(name) });
        if let Some(body) = body {
            put(stream, b": ");
            body(stream);
        }
        if errnum != 0 {
            put(stream, b": ");
            // SAFETY: strerror's answer is a C string.
            put(stream, unsafe { text(crate::string::strerror(errnum)) });
        }
        put(stream, b"\n");
    }
    if status != 0 && st.is_none_or(|s| s.flags & ARGP_NO_EXIT == 0) {
        exit_now(status);
    }
}

/// Own archive member, as each variadic's is.
#[cfg(target_os = "none")]
mod gnu_argp_error {
    use crate::printf::va_trampoline;
    va_trampoline!("argp_error", "__argp_verror", "16", "rdx");
}

/// Own archive member.
#[cfg(target_os = "none")]
mod gnu_argp_failure {
    use crate::printf::va_trampoline;
    va_trampoline!("argp_failure", "__argp_vfailure", "32", "r8");
}

/// `_option_is_short(opt)`: 1 if `opt` has a short option -- its key a
/// printable character, and not documentation -- else 0.
///
/// # Safety
///
/// `opt` points at an option.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _option_is_short(opt: *const ArgpOption) -> i32 {
    // SAFETY: the caller's option.
    i32::from(unsafe { opt.as_ref() }.is_some_and(is_short))
}

/// `_option_is_end(opt)`: 1 if `opt` ends its table -- no key, name, doc or
/// group -- else 0.
///
/// # Safety
///
/// As [`_option_is_short`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _option_is_end(opt: *const ArgpOption) -> i32 {
    // SAFETY: the caller's option.
    i32::from(unsafe { opt.as_ref() }.is_none_or(is_end))
}

/// `_argp_input(argp, state)`: the input `argp`'s parser has in `state`'s
/// parse, or NULL. glibc declares it and does not export it.
///
/// # Safety
///
/// `state` is NULL or a parse's state.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _argp_input(argp: *const Argp, state: *const ArgpState) -> *mut c_void {
    // SAFETY: the caller's contract.
    unsafe { parse::input_of(argp, state) }
}

/// A short option: a printable key, not documentation.
pub(crate) fn is_short(o: &ArgpOption) -> bool {
    o.flags & OPTION_DOC == 0
        && u8::try_from(o.key).is_ok_and(|c| c > 0 && crate::ctype::isprint(i32::from(c)) != 0)
}

/// The entry that ends a table.
pub(crate) const fn is_end(o: &ArgpOption) -> bool {
    o.key == 0 && o.name.is_null() && o.doc.is_null() && o.group == 0
}

#[cfg(test)]
pub(crate) mod tests;
