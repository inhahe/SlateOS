//! C standard library conversion functions.
//!
//! Implements `atoi`, `atol`, `strtol`, `strtoul`, `abs`, `labs`.
//!
//! These are not strictly POSIX but are required by virtually every
//! C program and are part of the C standard library.


// ---------------------------------------------------------------------------
// Integer conversion
// ---------------------------------------------------------------------------

/// Convert a C string to an integer.
///
/// Skips leading whitespace, handles optional sign.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atoi(nptr: *const u8) -> i32 {
    unsafe { strtol(nptr, core::ptr::null_mut(), 10) as i32 }
}

/// Convert a C string to a long integer.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atol(nptr: *const u8) -> i64 {
    unsafe { strtol(nptr, core::ptr::null_mut(), 10) }
}

/// Convert a C string to a long integer with base and end pointer.
///
/// Skips leading whitespace, handles optional `+`/`-` sign, and
/// supports bases 2-36.  Base 0 auto-detects: `0x` = hex, `0` = octal,
/// else decimal.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
/// `endptr` may be null; if non-null, it receives a pointer to the
/// first character after the parsed number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtol(
    nptr: *const u8,
    endptr: *mut *const u8,
    mut base: i32,
) -> i64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0;
    }

    let mut i: usize = 0;

    // Skip whitespace.
    while is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Handle sign.
    let negative = unsafe { *nptr.add(i) } == b'-';
    if negative || unsafe { *nptr.add(i) } == b'+' {
        i = i.wrapping_add(1);
    }

    // Auto-detect base.
    if base == 0 {
        if unsafe { *nptr.add(i) } == b'0' {
            if unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
                || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X'
            {
                base = 16;
                i = i.wrapping_add(2);
            } else {
                base = 8;
                i = i.wrapping_add(1);
            }
        } else {
            base = 10;
        }
    } else if base == 16
        && unsafe { *nptr.add(i) } == b'0'
        && (unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
            || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X')
    {
        // Skip optional 0x prefix for hex.
        i = i.wrapping_add(2);
    }

    // Parse digits.
    let mut result: i64 = 0;
    loop {
        let c = unsafe { *nptr.add(i) };
        let digit = char_to_digit(c, base);
        if digit < 0 {
            break;
        }
        // Saturating to avoid overflow UB.
        result = result.saturating_mul(i64::from(base)).saturating_add(i64::from(digit));
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        unsafe { *endptr = nptr.add(i); }
    }

    if negative { result.saturating_neg() } else { result }
}

/// Convert a C string to an unsigned long integer.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoul(
    nptr: *const u8,
    endptr: *mut *const u8,
    mut base: i32,
) -> u64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0;
    }

    let mut i: usize = 0;

    // Skip whitespace.
    while is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Skip optional '+'.
    if unsafe { *nptr.add(i) } == b'+' {
        i = i.wrapping_add(1);
    }

    // Auto-detect base.
    if base == 0 {
        if unsafe { *nptr.add(i) } == b'0' {
            if unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
                || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X'
            {
                base = 16;
                i = i.wrapping_add(2);
            } else {
                base = 8;
                i = i.wrapping_add(1);
            }
        } else {
            base = 10;
        }
    } else if base == 16
        && unsafe { *nptr.add(i) } == b'0'
        && (unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
            || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X')
    {
        i = i.wrapping_add(2);
    }

    // Parse digits.
    let mut result: u64 = 0;
    loop {
        let c = unsafe { *nptr.add(i) };
        let digit = char_to_digit(c, base);
        if digit < 0 {
            break;
        }
        result = result.saturating_mul(base as u64).saturating_add(digit as u64);
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        unsafe { *endptr = nptr.add(i); }
    }

    result
}

// ---------------------------------------------------------------------------
// Absolute value
// ---------------------------------------------------------------------------

/// Compute absolute value of an integer.
#[unsafe(no_mangle)]
pub extern "C" fn abs(j: i32) -> i32 {
    if j < 0 { j.saturating_neg() } else { j }
}

/// Compute absolute value of a long integer.
#[unsafe(no_mangle)]
pub extern "C" fn labs(j: i64) -> i64 {
    if j < 0 { j.saturating_neg() } else { j }
}

// ---------------------------------------------------------------------------
// Sorting and searching
// ---------------------------------------------------------------------------

/// Sort an array using the comparison function.
///
/// This is a simple insertion sort — O(n²) but correct and compact.
/// A real libc would use introsort or merge sort.
///
/// # Safety
///
/// `base` must point to an array of at least `nmemb` elements, each
/// of `size` bytes.  `compar` must be a valid comparison function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort(
    base: *mut u8,
    nmemb: usize,
    size: usize,
    compar: unsafe extern "C" fn(*const u8, *const u8) -> i32,
) {
    if nmemb <= 1 || size == 0 {
        return;
    }

    // Insertion sort.  Simple, in-place, stable.
    // A 256-byte stack buffer avoids mmap for small elements.
    let mut swap_buf = [0u8; 256];
    let use_stack = size <= swap_buf.len();

    let temp = if use_stack {
        swap_buf.as_mut_ptr()
    } else {
        // Allocate temp space via mmap for large elements.
        let ptr = crate::mman::mmap(
            core::ptr::null_mut(),
            size,
            crate::mman::PROT_READ | crate::mman::PROT_WRITE,
            crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
            -1,
            0,
        );
        if ptr == crate::mman::MAP_FAILED {
            return; // Cannot sort without temp space.
        }
        ptr.cast::<u8>()
    };

    let mut i: usize = 1;
    while i < nmemb {
        // Save element[i] into temp.
        let elem_i = unsafe { base.add(i.wrapping_mul(size)) };
        unsafe { core::ptr::copy_nonoverlapping(elem_i, temp, size); }

        // Shift elements right until we find the insertion point.
        let mut j = i;
        while j > 0 {
            let elem_j_minus_1 = unsafe { base.add(j.wrapping_sub(1).wrapping_mul(size)) };
            if unsafe { compar(elem_j_minus_1, temp) } <= 0 {
                break;
            }
            let elem_j = unsafe { base.add(j.wrapping_mul(size)) };
            unsafe { core::ptr::copy_nonoverlapping(elem_j_minus_1, elem_j, size); }
            j = j.wrapping_sub(1);
        }

        // Insert the saved element at position j.
        let dest = unsafe { base.add(j.wrapping_mul(size)) };
        unsafe { core::ptr::copy_nonoverlapping(temp, dest, size); }

        i = i.wrapping_add(1);
    }

    if !use_stack {
        let _ = crate::mman::munmap(temp.cast::<core::ffi::c_void>(), size);
    }
}

/// Binary search a sorted array.
///
/// Returns a pointer to the matching element, or NULL if not found.
///
/// # Safety
///
/// `base` must point to a sorted array of at least `nmemb` elements,
/// each of `size` bytes.  `compar` must be a valid comparison function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bsearch(
    key: *const u8,
    base: *const u8,
    nmemb: usize,
    size: usize,
    compar: unsafe extern "C" fn(*const u8, *const u8) -> i32,
) -> *const u8 {
    if nmemb == 0 || size == 0 {
        return core::ptr::null();
    }

    let mut lo: usize = 0;
    let mut hi: usize = nmemb;

    while lo < hi {
        let mid = lo.wrapping_add(hi.wrapping_sub(lo) / 2);
        let elem = unsafe { base.add(mid.wrapping_mul(size)) };
        let cmp = unsafe { compar(key, elem) };
        match cmp.cmp(&0) {
            core::cmp::Ordering::Less => hi = mid,
            core::cmp::Ordering::Greater => lo = mid.wrapping_add(1),
            core::cmp::Ordering::Equal => return elem,
        }
    }

    core::ptr::null()
}

// ---------------------------------------------------------------------------
// Random number generation
// ---------------------------------------------------------------------------

/// Linear congruential PRNG state.
///
/// Not thread-safe. Uses the glibc LCG parameters.
static mut RAND_STATE: u64 = 1;

/// Seed the random number generator.
#[unsafe(no_mangle)]
pub extern "C" fn srand(seed: u32) {
    // SAFETY: Single-threaded userspace. Using addr_of_mut for Rust 2024.
    unsafe { core::ptr::addr_of_mut!(RAND_STATE).write(u64::from(seed)); }
}

/// Generate a pseudo-random integer in [0, RAND_MAX].
///
/// Uses the glibc LCG: state = state * 6364136223846793005 + 1.
/// Returns the upper 31 bits.
#[unsafe(no_mangle)]
pub extern "C" fn rand() -> i32 {
    // SAFETY: Single-threaded access.
    let state = unsafe { core::ptr::addr_of_mut!(RAND_STATE).read() };
    let new_state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1);
    unsafe { core::ptr::addr_of_mut!(RAND_STATE).write(new_state); }
    // Return upper 31 bits as a non-negative i32.
    ((new_state >> 33) & 0x7FFF_FFFF) as i32
}

/// Maximum value returned by rand().
#[unsafe(no_mangle)]
pub static RAND_MAX: i32 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// Temporary files
// ---------------------------------------------------------------------------

/// Counter for generating unique temporary filenames.
static mut MKSTEMP_COUNTER: u32 = 0;

/// Create a unique temporary file.
///
/// The `template` string must end with exactly six 'X' characters
/// (e.g., `"/tmp/fileXXXXXX"`).  These are replaced with unique
/// characters and the file is created atomically.
///
/// Returns an open file descriptor on success, or -1 on error.
///
/// # Safety
///
/// `template` must be a writable null-terminated string with at least
/// 6 trailing 'X' characters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mkstemp(template: *mut u8) -> i32 {
    if template.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let len = unsafe { crate::string::strlen(template) };
    if len < 6 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Verify the last 6 characters are 'X'.
    let suffix_start = len.wrapping_sub(6);
    let mut i: usize = 0;
    while i < 6 {
        if unsafe { *template.add(suffix_start.wrapping_add(i)) } != b'X' {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        i = i.wrapping_add(1);
    }

    // Try up to 100 unique names.
    let mut attempt: u32 = 0;
    while attempt < 100 {
        // Generate a unique suffix using counter + pid + attempt.
        let counter = unsafe { *core::ptr::addr_of!(MKSTEMP_COUNTER) };
        unsafe { core::ptr::addr_of_mut!(MKSTEMP_COUNTER).write(counter.wrapping_add(1)); }

        let pid = crate::process::getpid() as u32;
        let seed = counter
            .wrapping_mul(31)
            .wrapping_add(pid)
            .wrapping_mul(17)
            .wrapping_add(attempt);

        // Fill the 6 X's with alphanumeric characters derived from seed.
        let mut val = seed;
        let mut j: usize = 0;
        while j < 6 {
            let idx = (val % 36) as u8;
            let ch = if idx < 10 {
                b'0'.wrapping_add(idx)
            } else {
                b'a'.wrapping_add(idx.wrapping_sub(10))
            };
            // SAFETY: suffix_start + j < len, template is writable.
            unsafe { *template.add(suffix_start.wrapping_add(j)) = ch; }
            val = val.wrapping_div(36).wrapping_add(1);
            j = j.wrapping_add(1);
        }

        // Try to create the file exclusively.
        let flags = crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_EXCL;
        let fd = crate::file::open(template, flags, 0o600);
        if fd >= 0 {
            return fd;
        }

        // If EEXIST, try again.  Any other error, bail.
        if crate::errno::get_errno() != crate::errno::EEXIST {
            return -1;
        }

        attempt = attempt.wrapping_add(1);
    }

    crate::errno::set_errno(crate::errno::EEXIST);
    -1
}

/// Create a temporary file.
///
/// Returns a FILE* stream for a unique temporary file opened in "w+b"
/// mode, or null on error.  The file is automatically deleted when
/// closed.
///
/// Note: Automatic deletion is not implemented (no unlink-on-close
/// support yet).  The file persists until manually removed.
#[unsafe(no_mangle)]
pub extern "C" fn tmpfile() -> *mut u8 {
    let mut template: [u8; 20] = *b"/tmp/tmpXXXXXX\0\0\0\0\0\0";
    let fd = unsafe { mkstemp(template.as_mut_ptr()) };
    if fd < 0 {
        return core::ptr::null_mut();
    }
    fd as usize as *mut u8
}

// ---------------------------------------------------------------------------
// Character classification (internal helpers)
// ---------------------------------------------------------------------------

/// Check if a byte is ASCII whitespace.
#[inline]
#[must_use]
const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Convert an ASCII character to its digit value in a given base.
///
/// Returns -1 if the character is not a valid digit for the base.
#[inline]
#[must_use]
fn char_to_digit(c: u8, base: i32) -> i32 {
    let val = match c {
        b'0'..=b'9' => i32::from(c.wrapping_sub(b'0')),
        b'a'..=b'z' => i32::from(c.wrapping_sub(b'a')).wrapping_add(10),
        b'A'..=b'Z' => i32::from(c.wrapping_sub(b'A')).wrapping_add(10),
        _ => return -1,
    };
    if val < base { val } else { -1 }
}
