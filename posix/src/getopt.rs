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
#[unsafe(no_mangle)]
pub static mut optarg: *const u8 = core::ptr::null();

/// Index of the next element of argv to be processed.
///
/// Initialized to 1 (skip argv[0] which is the program name).
#[unsafe(no_mangle)]
pub static mut optind: i32 = 1;

/// If non-zero, print error messages to stderr.
#[unsafe(no_mangle)]
pub static mut opterr: i32 = 1;

/// The unrecognized option character (set on '?' return).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
#[allow(clippy::similar_names)] // argc/argv are standard POSIX names.
pub unsafe extern "C" fn getopt(
    argc: i32,
    argv: *const *const u8,
    optstring: *const u8,
) -> i32 {
    if argv.is_null() || optstring.is_null() {
        return -1;
    }

    // SAFETY: We use raw pointers to access the global mutable statics.
    // getopt is inherently not thread-safe (POSIX spec).
    let ind = unsafe { *core::ptr::addr_of!(optind) };
    let pos = unsafe { *core::ptr::addr_of!(OPTPOS) };

    // Reset optarg.
    unsafe { core::ptr::addr_of_mut!(optarg).write(core::ptr::null()); }

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
            unsafe { core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1)); }
            unsafe { core::ptr::addr_of_mut!(OPTPOS).write(0); }
            return -1;
        }

        // Start scanning at position 1 (after '-').
        unsafe { core::ptr::addr_of_mut!(OPTPOS).write(1); }
    }

    let cur_pos = unsafe { *core::ptr::addr_of!(OPTPOS) };
    // SAFETY: arg is valid, cur_pos >= 1.
    let opt_char = unsafe { *arg.add(cur_pos) };

    if opt_char == 0 {
        // End of this argument — move to next.
        unsafe { core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1)); }
        unsafe { core::ptr::addr_of_mut!(OPTPOS).write(0); }
        // Recursively try the next argument.
        return unsafe { getopt(argc, argv, optstring) };
    }

    // Look up opt_char in optstring.
    let found_pos = find_in_optstring(optstring, opt_char);

    if found_pos < 0 {
        // Unknown option.
        unsafe { core::ptr::addr_of_mut!(optopt).write(i32::from(opt_char)); }

        // Advance past this character.
        unsafe { core::ptr::addr_of_mut!(OPTPOS).write(cur_pos.wrapping_add(1)); }
        if unsafe { *arg.add(cur_pos.wrapping_add(1)) } == 0 {
            unsafe { core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1)); }
            unsafe { core::ptr::addr_of_mut!(OPTPOS).write(0); }
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
        unsafe { core::ptr::addr_of_mut!(OPTPOS).write(cur_pos.wrapping_add(1)); }
        if unsafe { *arg.add(cur_pos.wrapping_add(1)) } == 0 {
            unsafe { core::ptr::addr_of_mut!(optind).write(ind.wrapping_add(1)); }
            unsafe { core::ptr::addr_of_mut!(OPTPOS).write(0); }
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
                unsafe { *longindex = idx as i32; }
            }

            // Return value based on flag.
            if opt.flag.is_null() {
                return Some(opt.val);
            }
            unsafe { *opt.flag = opt.val; }
            return Some(0);
        }

        idx = idx.wrapping_add(1);
    }

    None
}

/// Check if the name in argv matches a long option name.
fn names_match(
    arg: *const u8,
    name_start: usize,
    name_len: usize,
    opt_name: *const u8,
) -> bool {
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
            unsafe { core::ptr::addr_of_mut!(optarg).write(core::ptr::null()); }
            return -1;
        }
        // Argument is after the '='.
        let arg_ptr = unsafe { arg.add(i.wrapping_add(1)) };
        unsafe { core::ptr::addr_of_mut!(optarg).write(arg_ptr); }
    } else if opt.has_arg == REQUIRED_ARGUMENT {
        // Argument is the next argv element.
        if *next_ind < argc {
            let next_arg = unsafe { *argv.add(*next_ind as usize) };
            unsafe { core::ptr::addr_of_mut!(optarg).write(next_arg); }
            *next_ind = next_ind.wrapping_add(1);
        } else {
            // Missing required argument.
            unsafe { core::ptr::addr_of_mut!(optarg).write(core::ptr::null()); }
            return -1;
        }
    } else {
        unsafe { core::ptr::addr_of_mut!(optarg).write(core::ptr::null()); }
    }
    0
}
