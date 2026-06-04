//! POSIX and GNU command-line option parsing.
//!
//! Implements `getopt()` for parsing short command-line options per
//! POSIX.1-2024, and `getopt_long()`/`getopt_long_only()` for GNU-style
//! long options.
//!
//! ## Global State
//!
//! Per POSIX, the following globals control getopt behavior:
//!
//! - `optarg`: pointer to option argument (for options with `:`)
//! - `optind`: index of the next argv element to process (starts at 1)
//! - `opterr`: if non-zero, print error messages to stderr
//! - `optopt`: the unrecognized option character on error
//!
//! ## Limitations
//!
//! - Not thread-safe (uses global state, matching POSIX spec).
//! - Long option matching is exact only (no unambiguous prefix
//!   matching like GNU getopt).

/// Pointer to the argument of the current option.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut optarg: *const u8 = core::ptr::null();

/// Index of the next element of argv to be processed.
///
/// Initialized to 1 (skip argv[0] which is the program name).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut optind: i32 = 1;

/// If non-zero, print error messages to stderr.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut opterr: i32 = 1;

/// The unrecognized option character (set on '?' return).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut optopt: i32 = 0;

/// Position within the current argv element (for grouped options like `-abc`).
static mut OPTPOS: usize = 0;

/// Parse command-line options.
///
/// `optstring` is a null-terminated string of valid option characters.
/// A character followed by `:` takes a required argument.
///
/// Returns the option character, `'?'` on error, or -1 when done.
///
/// # Safety
///
/// `argv` must be a valid array of at least `argc` pointers to
/// null-terminated strings.  `optstring` must be null-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub unsafe extern "C" fn getopt(argc: i32, argv: *const *const u8, optstring: *const u8) -> i32 {
    if argv.is_null() || optstring.is_null() {
        return -1;
    }

    // SAFETY: We use raw pointers to access the global mutable statics.
    // getopt is inherently not thread-safe (POSIX spec).
    let ind = unsafe { *core::ptr::addr_of!(optind) };
    let pos = unsafe { *core::ptr::addr_of!(OPTPOS) };

    // Reset optarg.
    unsafe {
        core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
    }

    if ind >= argc || ind < 1 {
        return -1;
    }

    // SAFETY: ind < argc, so argv.add(ind as usize) is valid.
    let arg = unsafe { *argv.add(ind as usize) };
    if arg.is_null() {
        return -1;
    }

    // If we're not in the middle of an arg, check if this is an option.
    if pos == 0 {
        // Must start with '-'.
        if unsafe { *arg } != b'-' {
            return -1;
        }

        // "--" or "-" ends option processing.
        let second = unsafe { *arg.add(1) };
        if second == 0 {
            return -1; // Bare "-".
        }
        if second == b'-' && unsafe { *arg.add(2) } == 0 {
            // "--" — skip it and stop.
            unsafe {
                core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
            }
            unsafe {
                core::ptr::addr_of_mut!(OPTPOS).write(0);
            }
            return -1;
        }

        // Start scanning at position 1 (after '-').
        unsafe {
            core::ptr::addr_of_mut!(OPTPOS).write(1);
        }
    }

    let cur_pos = unsafe { *core::ptr::addr_of!(OPTPOS) };
    // SAFETY: arg is valid, cur_pos >= 1.
    let opt_char = unsafe { *arg.add(cur_pos) };

    if opt_char == 0 {
        // End of this argument — move to next.
        unsafe {
            core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
        }
        unsafe {
            core::ptr::addr_of_mut!(OPTPOS).write(0);
        }
        // Recursively try the next argument.
        return unsafe { getopt(argc, argv, optstring) };
    }

    // Look up opt_char in optstring.
    let found_pos = find_in_optstring(optstring, opt_char);

    if found_pos < 0 {
        // Unknown option.
        unsafe {
            core::ptr::addr_of_mut!(optopt).write(i32::from(opt_char));
        }

        // Advance past this character.
        unsafe {
            core::ptr::addr_of_mut!(OPTPOS).write(cur_pos.wrapping_add(1));
        }
        if unsafe { *arg.add(cur_pos.wrapping_add(1)) } == 0 {
            unsafe {
                core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
            }
            unsafe {
                core::ptr::addr_of_mut!(OPTPOS).write(0);
            }
        }

        return i32::from(b'?');
    }

    // Check if this option takes an argument.
    let takes_arg = unsafe { *optstring.add((found_pos as usize).wrapping_add(1)) } == b':';

    if takes_arg {
        // Check if argument is in the rest of this argv element.
        let next_pos = cur_pos.wrapping_add(1);
        if unsafe { *arg.add(next_pos) } != 0 {
            // Argument is the rest of this element.
            unsafe {
                core::ptr::addr_of_mut!(optarg).write(arg.add(next_pos));
                core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
                core::ptr::addr_of_mut!(OPTPOS).write(0);
            }
        } else {
            // Argument is the next argv element.
            let next_ind = ind.wrapping_add(1);
            if next_ind >= argc {
                // Missing argument.
                unsafe {
                    core::ptr::addr_of_mut!(optopt).write(i32::from(opt_char));
                    core::ptr::addr_of_mut!(optind).write(next_ind);
                    core::ptr::addr_of_mut!(OPTPOS).write(0);
                }
                // If optstring starts with ':', return ':'.
                if unsafe { *optstring } == b':' {
                    return i32::from(b':');
                }
                return i32::from(b'?');
            }
            unsafe {
                core::ptr::addr_of_mut!(optarg).write(*argv.add(next_ind as usize));
                core::ptr::addr_of_mut!(optind).write(next_ind.wrapping_add(1));
                core::ptr::addr_of_mut!(OPTPOS).write(0);
            }
        }
    } else {
        // No argument — advance within the current argv element.
        unsafe {
            core::ptr::addr_of_mut!(OPTPOS).write(cur_pos.wrapping_add(1));
        }
        if unsafe { *arg.add(cur_pos.wrapping_add(1)) } == 0 {
            unsafe {
                core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
            }
            unsafe {
                core::ptr::addr_of_mut!(OPTPOS).write(0);
            }
        }
    }

    i32::from(opt_char)
}

/// Find a character in the optstring.
///
/// Returns the index of the character, or -1 if not found.
/// Skips a leading ':' in optstring (it controls error behavior).
fn find_in_optstring(optstring: *const u8, ch: u8) -> i32 {
    if ch == b':' || ch == b'-' {
        // These are never valid option characters.
        return -1;
    }

    let mut i: usize = 0;
    // Skip leading ':' (it means "return ':' on missing arg").
    if unsafe { *optstring } == b':' {
        i = 1;
    }

    loop {
        let c = unsafe { *optstring.add(i) };
        if c == 0 {
            return -1;
        }
        if c == ch {
            return i as i32;
        }
        i = i.wrapping_add(1);
        // Skip ':' argument specifier.
        if unsafe { *optstring.add(i) } == b':' {
            i = i.wrapping_add(1);
        }
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

/// Parse long (and short) command-line options.
///
/// Processes `argv` looking for both short options (from `optstring`)
/// and long options (from `longopts`).  When a long option is matched,
/// `*longindex` (if non-null) is set to the index in `longopts`.
///
/// Returns the option character (short) or `val`/0 (long), '?' on error,
/// or -1 when all options are consumed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub extern "C" fn getopt_long(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
    longopts: *const Option,
    longindex: *mut i32,
) -> i32 {
    getopt_long_impl(argc, argv, optstring, longopts, longindex, false)
}

/// Like `getopt_long` but also tries to match long options for
/// `-option` (single dash), not just `--option`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub extern "C" fn getopt_long_only(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
    longopts: *const Option,
    longindex: *mut i32,
) -> i32 {
    getopt_long_impl(argc, argv, optstring, longopts, longindex, true)
}

/// Shared implementation for `getopt_long` and `getopt_long_only`.
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
fn getopt_long_impl(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
    longopts: *const Option,
    longindex: *mut i32,
    long_only: bool,
) -> i32 {
    let ind = unsafe { core::ptr::addr_of!(optind).read() };
    let pos = unsafe { core::ptr::addr_of!(OPTPOS).read() };
    if ind >= argc || argv.is_null() || optstring.is_null() {
        return -1;
    }

    // SAFETY: ind is in [0, argc), argv is non-null.
    let arg = unsafe { *argv.add(ind as usize) };
    if arg.is_null() {
        return -1;
    }

    let c0 = unsafe { *arg };
    if c0 != b'-' || unsafe { *arg.add(1) } == 0 {
        // Not an option — stop.
        return -1;
    }

    // Check for "--" (end of options).
    if unsafe { *arg.add(1) } == b'-' && unsafe { *arg.add(2) } == 0 {
        unsafe {
            core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
            core::ptr::addr_of_mut!(OPTPOS).write(0);
        }
        return -1;
    }

    // Only try long option matching when we're NOT in the middle of
    // grouped short options (pos == 0). If OPTPOS > 0, we're mid-scan
    // on a grouped arg like "-abc" and must continue with short options.
    if pos == 0 {
        // Check for long option: "--something" or (with long_only) "-something".
        let is_long = unsafe { *arg.add(1) } == b'-';
        if is_long || (long_only && !longopts.is_null()) {
            let name_start = if is_long { 2usize } else { 1usize };
            let result = try_match_long(arg, name_start, longopts, longindex);
            if let Some(ret) = result {
                // Consume the argument.
                let mut next_ind = ind.wrapping_add(1);

                // Handle required/optional argument.
                let matched_idx = if longindex.is_null() {
                    -1
                } else {
                    unsafe { *longindex }
                };

                if matched_idx >= 0 {
                    let opt = unsafe { &*longopts.add(matched_idx as usize) };
                    let arg_result =
                        handle_long_opt_arg(arg, name_start, opt, argc, argv, &mut next_ind);
                    if arg_result < 0 {
                        // Error: either unwanted "=val" or missing required arg.
                        unsafe {
                            core::ptr::addr_of_mut!(optind).write(next_ind);
                            core::ptr::addr_of_mut!(OPTPOS).write(0);
                        }
                        return i32::from(b'?');
                    }
                }

                unsafe {
                    core::ptr::addr_of_mut!(optind).write(next_ind);
                    core::ptr::addr_of_mut!(OPTPOS).write(0);
                }
                return ret;
            }

            // If this was "--something" (double dash) and no match, it's an error.
            if is_long {
                unsafe {
                    core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1));
                    core::ptr::addr_of_mut!(OPTPOS).write(0);
                }
                return i32::from(b'?');
            }
            // For long_only with single dash, fall through to short option handling.
        }
    }

    // Delegate to short-option getopt.
    // SAFETY: argc/argv/optstring are forwarded from our caller, which
    // has the same safety requirements as getopt.
    unsafe { getopt(argc, argv, optstring) }
}

/// Try to match a long option from the argv element.
///
/// Returns `Some(val_or_0)` if matched, `None` if no match.
fn try_match_long(
    arg: *const u8,
    name_start: usize,
    longopts: *const Option,
    longindex: *mut i32,
) -> core::option::Option<i32> {
    if longopts.is_null() {
        return None;
    }

    // Extract the option name (up to '=' or NUL).
    let mut name_len: usize = 0;
    loop {
        let c = unsafe { *arg.add(name_start.wrapping_add(name_len)) };
        if c == 0 || c == b'=' {
            break;
        }
        name_len = name_len.wrapping_add(1);
    }

    if name_len == 0 {
        return None;
    }

    // Search longopts for a match.
    let mut idx: usize = 0;
    loop {
        let opt = unsafe { &*longopts.add(idx) };
        if opt.name.is_null() {
            break; // End of longopts array.
        }

        // Compare names.
        if names_match(arg, name_start, name_len, opt.name) {
            if !longindex.is_null() {
                unsafe {
                    *longindex = idx as i32;
                }
            }

            // Return value based on flag.
            if opt.flag.is_null() {
                return Some(opt.val);
            }
            unsafe {
                *opt.flag = opt.val;
            }
            return Some(0);
        }

        idx = idx.wrapping_add(1);
    }

    None
}

/// Check if the name in argv matches a long option name.
fn names_match(arg: *const u8, name_start: usize, name_len: usize, opt_name: *const u8) -> bool {
    let opt_len = unsafe { crate::string::strlen(opt_name) };
    if name_len != opt_len {
        return false;
    }
    let mut i: usize = 0;
    while i < name_len {
        let a = unsafe { *arg.add(name_start.wrapping_add(i)) };
        let b = unsafe { *opt_name.add(i) };
        if a != b {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}

/// Cross-test serialisation for getopt's process-global parser state.
///
/// `optarg`/`optind`/`opterr`/`optopt`/`OPTPOS` are mutable statics by
/// POSIX requirement, so every getopt test (and every test of code
/// that calls getopt) races against every other under cargo's default
/// parallel runner. Holding this mutex for the duration of a test
/// guarantees the globals reflect that test's call sequence end-to-
/// end, eliminating the spurious `optind`-corruption failures
/// previously documented in todo.txt
/// (test_long_only_double_dash_still_works flake, 2026-05-27).
///
/// Only compiled on host builds (`std::sync::Mutex` is unavailable in
/// the no_std `target_os = "none"` build).
#[cfg(all(test, not(target_os = "none")))]
static GETOPT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Zero the getopt globals back to their post-init values.
///
/// Internal helper: assumes the caller already holds
/// `GETOPT_TEST_LOCK` (typically via `reset_getopt_state`) or runs in
/// a context where parallel access is impossible.
#[cfg(test)]
unsafe fn zero_getopt_globals() {
    // SAFETY: caller guarantees serialised access to the globals.
    unsafe {
        core::ptr::addr_of_mut!(optind).write(1);
        core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
        core::ptr::addr_of_mut!(opterr).write(1);
        core::ptr::addr_of_mut!(optopt).write(0);
        core::ptr::addr_of_mut!(OPTPOS).write(0);
    }
}

/// Reset all getopt global state AND acquire the cross-test
/// serialisation lock.
///
/// Returns a `MutexGuard` that the caller must keep alive for the
/// duration of the test. Poisoned guards are recovered so a prior
/// panicking test doesn't wedge subsequent ones.
///
/// Must be called as `let _g = unsafe { reset_getopt_state() };`
/// at the top of every getopt-touching test. Tests that need a
/// mid-test reset (e.g. re-parse the same argv) should use
/// `zero_getopt_globals()` instead of re-acquiring the lock — calling
/// `reset_getopt_state` a second time would deadlock because Rust
/// shadowing keeps the prior guard alive.
#[cfg(test)]
#[must_use = "the returned guard serialises getopt tests; bind it to `_g`"]
unsafe fn reset_getopt_state() -> std::sync::MutexGuard<'static, ()> {
    let guard = GETOPT_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: we now hold the cross-test lock; no other thread can
    // touch the globals concurrently.
    unsafe { zero_getopt_globals() };
    guard
}

/// Handle the argument for a matched long option.
///
/// Returns 0 on success, -1 on error (unwanted `=value` for no_argument
/// options, or missing required argument).
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
fn handle_long_opt_arg(
    arg: *const u8,
    name_start: usize,
    opt: &Option,
    argc: i32,
    argv: *const *const u8,
    next_ind: &mut i32,
) -> i32 {
    // Find the '=' if present (inline argument).
    let mut i = name_start;
    let mut has_eq = false;
    loop {
        let c = unsafe { *arg.add(i) };
        if c == 0 {
            break;
        }
        if c == b'=' {
            has_eq = true;
            break;
        }
        i = i.wrapping_add(1);
    }

    if has_eq {
        if opt.has_arg == NO_ARGUMENT {
            // "--foo=bar" when foo takes no argument — error.
            unsafe {
                core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
            }
            return -1;
        }
        // Argument is after the '='.
        let arg_ptr = unsafe { arg.add(i.wrapping_add(1)) };
        unsafe {
            core::ptr::addr_of_mut!(optarg).write(arg_ptr);
        }
    } else if opt.has_arg == REQUIRED_ARGUMENT {
        // Argument is the next argv element.
        if *next_ind < argc {
            let next_arg = unsafe { *argv.add(*next_ind as usize) };
            unsafe {
                core::ptr::addr_of_mut!(optarg).write(next_arg);
            }
            *next_ind = next_ind.wrapping_add(1);
        } else {
            // Missing required argument.
            unsafe {
                core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
            }
            return -1;
        }
    } else {
        unsafe {
            core::ptr::addr_of_mut!(optarg).write(core::ptr::null());
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::undocumented_unsafe_blocks)]
// NOTE: these tests use global state (optind, optarg, etc.) and MUST be
// run single-threaded: `cargo test -- --test-threads=1`
mod tests {
    use super::*;

    /// Build a null-terminated C string on the stack and return its pointer.
    /// The returned pointer is valid for the lifetime of the `Vec`.
    fn cstr(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
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
    // find_in_optstring edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_colon_and_dash_never_match() {
        let opts = cstr("a:b-c");
        // ':' and '-' should never be recognized as option characters.
        assert_eq!(find_in_optstring(opts.as_ptr(), b':'), -1);
        assert_eq!(find_in_optstring(opts.as_ptr(), b'-'), -1);
        // But 'a', 'b', 'c' should match.
        assert!(find_in_optstring(opts.as_ptr(), b'a') >= 0);
        assert!(find_in_optstring(opts.as_ptr(), b'b') >= 0);
    }

    #[test]
    fn test_find_in_optstring_leading_colon() {
        // Leading ':' should be skipped.
        let opts = cstr(":ab");
        assert!(find_in_optstring(opts.as_ptr(), b'a') >= 0);
        assert!(find_in_optstring(opts.as_ptr(), b'b') >= 0);
        assert_eq!(find_in_optstring(opts.as_ptr(), b'z'), -1);
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
}
