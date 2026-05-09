//! POSIX command-line option parsing.
//!
//! Implements `getopt()` for parsing short command-line options per
//! POSIX.1-2024.  Programs call `getopt()` in a loop; it returns the
//! next option character, or -1 when all options are consumed.
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
//! - Only short options (single character).  `getopt_long()` is not
//!   yet implemented.
//! - Not thread-safe (uses global state, matching POSIX spec).

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
