//! C string functions required by the C runtime.
//!
//! These are not strictly POSIX but are required by virtually every
//! C program.  A real libc would provide optimized (SIMD) versions;
//! these are correct reference implementations.
//!
//! Includes: `memcpy`, `memmove`, `memset`, `memcmp`, `memchr`,
//! `memrchr`, `memccpy`, `strlen`, `strnlen`, `strcmp`, `strncmp`,
//! `strcpy`, `strncpy`, `stpcpy`, `stpncpy`, `strchr`, `strrchr`,
//! `strcat`, `strncat`, `strstr`, `strspn`, `strcspn`, `strpbrk`,
//! `strtok`, `strtok_r`, `strsep`, `strerror`, `strerror_r`,
//! `strdup`, `strndup`, `bcopy`, `bzero`, `strcasecmp`, `strncasecmp`,
//! `strcoll`, `strxfrm`, `strverscmp`
//!
//! Exported as `extern "C"` with standard names so the linker finds
//! them when C code calls `memcpy`, `memset`, `strlen`, etc.

use crate::types::SizeT;

/// Copy `n` bytes from `src` to `dest`.  Regions must not overlap.
///
/// Returns `dest`.
///
/// # Safety
///
/// `dest` and `src` must be valid for `n` bytes and must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(
    dest: *mut u8,
    src: *const u8,
    n: SizeT,
) -> *mut u8 {
    // SAFETY: Caller guarantees no overlap and valid pointers.
    let mut i: usize = 0;
    while i < n {
        unsafe { *dest.add(i) = *src.add(i); }
        i = i.wrapping_add(1);
    }
    dest
}

/// Copy `n` bytes from `src` to `dest`.  Regions may overlap.
///
/// Returns `dest`.
///
/// # Safety
///
/// `dest` and `src` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(
    dest: *mut u8,
    src: *const u8,
    n: SizeT,
) -> *mut u8 {
    if (dest as usize) < (src as usize) {
        // Copy forward.
        let mut i: usize = 0;
        while i < n {
            unsafe { *dest.add(i) = *src.add(i); }
            i = i.wrapping_add(1);
        }
    } else if (dest as usize) > (src as usize) {
        // Copy backward.
        let mut i = n;
        while i > 0 {
            i = i.wrapping_sub(1);
            unsafe { *dest.add(i) = *src.add(i); }
        }
    }
    // If dest == src, no copy needed.
    dest
}

/// Fill `n` bytes of `dest` with byte value `c`.
///
/// Returns `dest`.
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(
    dest: *mut u8,
    c: i32,
    n: SizeT,
) -> *mut u8 {
    let val = c as u8;
    let mut i: usize = 0;
    while i < n {
        unsafe { *dest.add(i) = val; }
        i = i.wrapping_add(1);
    }
    dest
}

/// Compare `n` bytes of `s1` and `s2`.
///
/// Returns 0 if equal, negative if s1 < s2, positive if s1 > s2.
///
/// # Safety
///
/// `s1` and `s2` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(
    s1: *const u8,
    s2: *const u8,
    n: SizeT,
) -> i32 {
    let mut i: usize = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b {
            return i32::from(a).wrapping_sub(i32::from(b));
        }
        i = i.wrapping_add(1);
    }
    0
}

/// Find the first occurrence of byte `c` in the first `n` bytes of `s`.
///
/// Returns a pointer to the byte, or NULL if not found.
///
/// # Safety
///
/// `s` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memchr(
    s: *const u8,
    c: i32,
    n: SizeT,
) -> *const u8 {
    let val = c as u8;
    let mut i: usize = 0;
    while i < n {
        if unsafe { *s.add(i) } == val {
            return unsafe { s.add(i) };
        }
        i = i.wrapping_add(1);
    }
    core::ptr::null()
}

/// Compute the length of a C string (excluding null terminator).
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const u8) -> SizeT {
    let mut len: usize = 0;
    while unsafe { *s.add(len) } != 0 {
        len = len.wrapping_add(1);
    }
    len
}

/// Compute the length of a C string, limited to `maxlen`.
///
/// # Safety
///
/// `s` must be valid for at least `maxlen` bytes, or be
/// null-terminated before `maxlen`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strnlen(s: *const u8, maxlen: SizeT) -> SizeT {
    let mut len: usize = 0;
    while len < maxlen && unsafe { *s.add(len) } != 0 {
        len = len.wrapping_add(1);
    }
    len
}

/// Compare two C strings.
///
/// Returns 0 if equal, negative if s1 < s2, positive if s1 > s2.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcmp(s1: *const u8, s2: *const u8) -> i32 {
    let mut i: usize = 0;
    loop {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b || a == 0 {
            return i32::from(a).wrapping_sub(i32::from(b));
        }
        i = i.wrapping_add(1);
    }
}

/// Compare at most `n` bytes of two C strings.
///
/// # Safety
///
/// Both strings must be valid for at least `n` bytes or be
/// null-terminated before `n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncmp(s1: *const u8, s2: *const u8, n: SizeT) -> i32 {
    let mut i: usize = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b || a == 0 {
            return i32::from(a).wrapping_sub(i32::from(b));
        }
        i = i.wrapping_add(1);
    }
    0
}

/// Copy a C string (including null terminator).
///
/// # Safety
///
/// `dest` must be large enough to hold the string.  `src` must be
/// a valid null-terminated string.  Regions must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcpy(dest: *mut u8, src: *const u8) -> *mut u8 {
    let mut i: usize = 0;
    loop {
        let c = unsafe { *src.add(i) };
        unsafe { *dest.add(i) = c; }
        if c == 0 {
            break;
        }
        i = i.wrapping_add(1);
    }
    dest
}

/// Copy at most `n` bytes of a C string (pad with nulls).
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.  `src` must be a valid
/// null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncpy(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
    let mut i: usize = 0;
    // Copy up to null or n bytes.
    while i < n {
        let c = unsafe { *src.add(i) };
        unsafe { *dest.add(i) = c; }
        if c == 0 {
            // Pad remainder with nulls.
            i = i.wrapping_add(1);
            while i < n {
                unsafe { *dest.add(i) = 0; }
                i = i.wrapping_add(1);
            }
            return dest;
        }
        i = i.wrapping_add(1);
    }
    dest
}

/// Find the first occurrence of `c` in string `s`.
///
/// Returns pointer to the character, or NULL.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchr(s: *const u8, c: i32) -> *const u8 {
    let val = c as u8;
    let mut i: usize = 0;
    loop {
        let ch = unsafe { *s.add(i) };
        if ch == val {
            return unsafe { s.add(i) };
        }
        if ch == 0 {
            return core::ptr::null();
        }
        i = i.wrapping_add(1);
    }
}

/// Find the last occurrence of `c` in string `s`.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrchr(s: *const u8, c: i32) -> *const u8 {
    let val = c as u8;
    let mut last: *const u8 = core::ptr::null();
    let mut i: usize = 0;
    loop {
        let ch = unsafe { *s.add(i) };
        if ch == val {
            last = unsafe { s.add(i) };
        }
        if ch == 0 {
            return last;
        }
        i = i.wrapping_add(1);
    }
}

/// Concatenate two C strings.
///
/// Appends `src` to the end of `dest`.
///
/// # Safety
///
/// `dest` must have enough space for the combined string.
/// Both must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcat(dest: *mut u8, src: *const u8) -> *mut u8 {
    // Find end of dest.
    let mut i: usize = 0;
    while unsafe { *dest.add(i) } != 0 {
        i = i.wrapping_add(1);
    }
    // Copy src.
    let mut j: usize = 0;
    loop {
        let c = unsafe { *src.add(j) };
        unsafe { *dest.add(i) = c; }
        if c == 0 {
            break;
        }
        i = i.wrapping_add(1);
        j = j.wrapping_add(1);
    }
    dest
}

/// Concatenate at most `n` bytes of `src` to `dest`.
///
/// # Safety
///
/// `dest` must have enough space for the combined string (up to n extra
/// bytes + null terminator).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncat(dest: *mut u8, src: *const u8, n: SizeT) -> *mut u8 {
    // Find end of dest.
    let mut i: usize = 0;
    while unsafe { *dest.add(i) } != 0 {
        i = i.wrapping_add(1);
    }
    // Copy up to n bytes from src.
    let mut j: usize = 0;
    while j < n {
        let c = unsafe { *src.add(j) };
        unsafe { *dest.add(i) = c; }
        if c == 0 {
            return dest;
        }
        i = i.wrapping_add(1);
        j = j.wrapping_add(1);
    }
    // Null-terminate.
    unsafe { *dest.add(i) = 0; }
    dest
}

/// Find the first occurrence of substring `needle` in `haystack`.
///
/// Returns a pointer to the beginning of the match, or NULL if not found.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strstr(haystack: *const u8, needle: *const u8) -> *const u8 {
    // Empty needle matches everything.
    if unsafe { *needle } == 0 {
        return haystack;
    }

    let mut h: usize = 0;
    while unsafe { *haystack.add(h) } != 0 {
        let mut j: usize = 0;
        loop {
            let n = unsafe { *needle.add(j) };
            if n == 0 {
                // Full match.
                return unsafe { haystack.add(h) };
            }
            let hc = unsafe { *haystack.add(h.wrapping_add(j)) };
            if hc != n {
                break;
            }
            j = j.wrapping_add(1);
        }
        h = h.wrapping_add(1);
    }
    core::ptr::null()
}

/// Compute the length of the initial segment of `s` consisting
/// entirely of bytes in `accept`.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
#[allow(clippy::many_single_char_names)] // POSIX function signature, C convention variables.
pub unsafe extern "C" fn strspn(s: *const u8, accept: *const u8) -> SizeT {
    let mut i: usize = 0;
    'outer: loop {
        let c = unsafe { *s.add(i) };
        if c == 0 {
            return i;
        }
        let mut j: usize = 0;
        loop {
            let a = unsafe { *accept.add(j) };
            if a == 0 {
                break 'outer;
            }
            if c == a {
                break;
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    i
}

/// Compute the length of the initial segment of `s` consisting
/// entirely of bytes NOT in `reject`.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
#[allow(clippy::many_single_char_names)]
pub unsafe extern "C" fn strcspn(s: *const u8, reject: *const u8) -> SizeT {
    let mut i: usize = 0;
    loop {
        let c = unsafe { *s.add(i) };
        if c == 0 {
            return i;
        }
        let mut j: usize = 0;
        loop {
            let r = unsafe { *reject.add(j) };
            if r == 0 {
                break;
            }
            if c == r {
                return i;
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
}

/// Find the first occurrence in `s` of any byte in `accept`.
///
/// Returns a pointer to the byte, or NULL if none found.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
#[allow(clippy::many_single_char_names)]
pub unsafe extern "C" fn strpbrk(s: *const u8, accept: *const u8) -> *const u8 {
    let mut i: usize = 0;
    loop {
        let c = unsafe { *s.add(i) };
        if c == 0 {
            return core::ptr::null();
        }
        let mut j: usize = 0;
        loop {
            let a = unsafe { *accept.add(j) };
            if a == 0 {
                break;
            }
            if c == a {
                return unsafe { s.add(i) };
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
}

/// Tokenize a string.
///
/// On the first call, `s` should point to the string to tokenize.
/// On subsequent calls, `s` should be NULL. The `delim` set may
/// change between calls.
///
/// Returns a pointer to the next token, or NULL when done.
///
/// # Safety
///
/// `s` (if non-null) and `delim` must be valid null-terminated strings.
/// Not thread-safe (uses a static saved position).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtok(s: *mut u8, delim: *const u8) -> *mut u8 {
    // Static saved position (POSIX strtok is not reentrant).
    static mut SAVED: *mut u8 = core::ptr::null_mut();

    // SAFETY: Single-threaded access; POSIX strtok is explicitly not
    // thread-safe. Using addr_of_mut to comply with Rust 2024 rules.
    let start = if s.is_null() {
        let p = unsafe { core::ptr::addr_of_mut!(SAVED).read() };
        if p.is_null() {
            return core::ptr::null_mut();
        }
        p
    } else {
        s
    };

    // Skip leading delimiters.
    let mut i: usize = 0;
    loop {
        let c = unsafe { *start.add(i) };
        if c == 0 {
            // All delimiters, no token.
            unsafe { core::ptr::addr_of_mut!(SAVED).write(core::ptr::null_mut()); }
            return core::ptr::null_mut();
        }
        if !unsafe { is_delim(c, delim) } {
            break;
        }
        i = i.wrapping_add(1);
    }

    let token = unsafe { start.add(i) };

    // Find end of token.
    let mut k: usize = 0;
    loop {
        let c = unsafe { *token.add(k) };
        if c == 0 {
            unsafe { core::ptr::addr_of_mut!(SAVED).write(core::ptr::null_mut()); }
            return token;
        }
        if unsafe { is_delim(c, delim) } {
            unsafe { *token.add(k) = 0; }
            unsafe { core::ptr::addr_of_mut!(SAVED).write(token.add(k.wrapping_add(1))); }
            return token;
        }
        k = k.wrapping_add(1);
    }
}

/// Check if a byte is in the delimiter set.
#[inline]
unsafe fn is_delim(c: u8, delim: *const u8) -> bool {
    let mut j: usize = 0;
    loop {
        let d = unsafe { *delim.add(j) };
        if d == 0 {
            return false;
        }
        if c == d {
            return true;
        }
        j = j.wrapping_add(1);
    }
}

/// Return a string describing an error number.
///
/// Returns a pointer to a static string.  The returned string must
/// not be modified by the caller.
#[unsafe(no_mangle)]
pub extern "C" fn strerror(errnum: i32) -> *const u8 {
    // Return a pointer to a static null-terminated C string.
    // These match the Linux error descriptions for the codes we support.
    match errnum {
        0 => c"Success".as_ptr().cast::<u8>(),
        1 => c"Operation not permitted".as_ptr().cast::<u8>(),
        2 => c"No such file or directory".as_ptr().cast::<u8>(),
        3 => c"No such process".as_ptr().cast::<u8>(),
        4 => c"Interrupted system call".as_ptr().cast::<u8>(),
        5 => c"Input/output error".as_ptr().cast::<u8>(),
        7 => c"Argument list too long".as_ptr().cast::<u8>(),
        8 => c"Exec format error".as_ptr().cast::<u8>(),
        9 => c"Bad file descriptor".as_ptr().cast::<u8>(),
        10 => c"No child processes".as_ptr().cast::<u8>(),
        11 => c"Resource temporarily unavailable".as_ptr().cast::<u8>(),
        12 => c"Cannot allocate memory".as_ptr().cast::<u8>(),
        13 => c"Permission denied".as_ptr().cast::<u8>(),
        14 => c"Bad address".as_ptr().cast::<u8>(),
        16 => c"Device or resource busy".as_ptr().cast::<u8>(),
        17 => c"File exists".as_ptr().cast::<u8>(),
        18 => c"Invalid cross-device link".as_ptr().cast::<u8>(),
        20 => c"Not a directory".as_ptr().cast::<u8>(),
        21 => c"Is a directory".as_ptr().cast::<u8>(),
        22 => c"Invalid argument".as_ptr().cast::<u8>(),
        24 => c"Too many open files".as_ptr().cast::<u8>(),
        25 => c"Inappropriate ioctl for device".as_ptr().cast::<u8>(),
        27 => c"File too large".as_ptr().cast::<u8>(),
        28 => c"No space left on device".as_ptr().cast::<u8>(),
        29 => c"Illegal seek".as_ptr().cast::<u8>(),
        30 => c"Read-only file system".as_ptr().cast::<u8>(),
        32 => c"Broken pipe".as_ptr().cast::<u8>(),
        34 => c"Numerical result out of range".as_ptr().cast::<u8>(),
        36 => c"File name too long".as_ptr().cast::<u8>(),
        38 => c"Function not implemented".as_ptr().cast::<u8>(),
        39 => c"Directory not empty".as_ptr().cast::<u8>(),
        40 => c"Too many levels of symbolic links".as_ptr().cast::<u8>(),
        95 => c"Operation not supported".as_ptr().cast::<u8>(),
        110 => c"Connection timed out".as_ptr().cast::<u8>(),
        _ => c"Unknown error".as_ptr().cast::<u8>(),
    }
}

/// Duplicate a string.
///
/// Allocates memory for a copy of `s` using `mmap`.  The caller must
/// free the result with `free()` (when we have a heap) or `munmap`.
///
/// Note: This is a no_std stub — returns NULL since we have no heap.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strdup(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return core::ptr::null_mut();
    }

    let len = unsafe { strlen(s) };
    let size = len.wrapping_add(1);

    // Allocate via mmap (anonymous mapping).
    let ptr = crate::mman::mmap(
        core::ptr::null_mut(),
        size,
        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if ptr == crate::mman::MAP_FAILED {
        return core::ptr::null_mut();
    }

    let dest = ptr.cast::<u8>();
    // SAFETY: mmap returned valid memory of sufficient size.
    unsafe { strcpy(dest, s); }
    dest
}

/// Duplicate at most `n` bytes of a string.
///
/// Allocates memory for a copy of at most `n` bytes from `s`,
/// plus a null terminator.  The result is always null-terminated.
///
/// # Safety
///
/// `s` must be a valid null-terminated string (or valid for `n` bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strndup(s: *const u8, n: usize) -> *mut u8 {
    if s.is_null() {
        return core::ptr::null_mut();
    }

    // Find actual length (min of strlen and n).
    let len = unsafe { strnlen(s, n) };
    let size = len.wrapping_add(1);

    let ptr = crate::mman::mmap(
        core::ptr::null_mut(),
        size,
        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if ptr == crate::mman::MAP_FAILED {
        return core::ptr::null_mut();
    }

    let dest = ptr.cast::<u8>();
    // SAFETY: mmap returned valid memory, len bytes + null.
    unsafe { memcpy(dest, s, len); }
    unsafe { *dest.add(len) = 0; }
    dest
}

/// Find the last occurrence of byte `c` in the first `n` bytes of `s`.
///
/// Scans backward from position `n-1`.
///
/// # Safety
///
/// `s` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memrchr(s: *const u8, c: i32, n: usize) -> *const u8 {
    let val = c as u8;
    let mut i = n;
    while i > 0 {
        i = i.wrapping_sub(1);
        if unsafe { *s.add(i) } == val {
            return unsafe { s.add(i) };
        }
    }
    core::ptr::null()
}

/// Copy `n` bytes from `src` to `dest`, guaranteeing non-overlap.
///
/// Identical to `memcpy` — exists for C programs that reference
/// `bcopy` (BSD legacy).
///
/// # Safety
///
/// `src` and `dest` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcopy(src: *const u8, dest: *mut u8, n: usize) {
    unsafe { memmove(dest, src, n); }
}

/// Set `n` bytes to zero.
///
/// # Safety
///
/// `s` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bzero(s: *mut u8, n: usize) {
    unsafe { memset(s, 0, n); }
}

/// Compare two strings, case-insensitive.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcasecmp(s1: *const u8, s2: *const u8) -> i32 {
    let mut i: usize = 0;
    loop {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        let la = a.to_ascii_lowercase();
        let lb = b.to_ascii_lowercase();
        if la != lb || a == 0 {
            return i32::from(la).wrapping_sub(i32::from(lb));
        }
        i = i.wrapping_add(1);
    }
}

/// Compare at most `n` bytes of two strings, case-insensitive.
///
/// # Safety
///
/// Both strings must be valid for at least `n` bytes or be
/// null-terminated before `n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncasecmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    let mut i: usize = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        let la = a.to_ascii_lowercase();
        let lb = b.to_ascii_lowercase();
        if la != lb || a == 0 {
            return i32::from(la).wrapping_sub(i32::from(lb));
        }
        i = i.wrapping_add(1);
    }
    0
}

// ---------------------------------------------------------------------------
// Additional string functions
// ---------------------------------------------------------------------------

/// Copy a string, returning a pointer to the END (the null terminator).
///
/// This is the BSD/POSIX `stpcpy` — unlike `strcpy`, it returns a
/// pointer to the terminating null byte, making chained copies efficient.
///
/// # Safety
///
/// `dest` must have enough space for the full `src` string plus null.
/// `src` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stpcpy(dest: *mut u8, src: *const u8) -> *mut u8 {
    let mut i: usize = 0;
    loop {
        let c = unsafe { *src.add(i) };
        unsafe { *dest.add(i) = c; }
        if c == 0 {
            return unsafe { dest.add(i) };
        }
        i = i.wrapping_add(1);
    }
}

/// Copy at most `n` bytes from `src` to `dest`, returning a pointer
/// past the last character written.
///
/// If `src` is shorter than `n`, remaining bytes are filled with null
/// and a pointer to the first null byte is returned.
///
/// # Safety
///
/// `dest` must have space for at least `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stpncpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i: usize = 0;
    // Copy up to n chars from src.
    while i < n {
        let c = unsafe { *src.add(i) };
        unsafe { *dest.add(i) = c; }
        if c == 0 {
            let result = unsafe { dest.add(i) };
            // Fill remainder with nulls.
            i = i.wrapping_add(1);
            while i < n {
                unsafe { *dest.add(i) = 0; }
                i = i.wrapping_add(1);
            }
            return result;
        }
        i = i.wrapping_add(1);
    }
    unsafe { dest.add(n) }
}

/// Extract token from string (reentrant, modifies input).
///
/// `strsep` is the BSD replacement for `strtok`.  It modifies the
/// string pointer `*stringp` to point past the delimiter (or sets
/// it to NULL when no more tokens remain).
///
/// Returns the original `*stringp` value (the token start), or NULL
/// if `*stringp` was NULL.
///
/// # Safety
///
/// `stringp` must point to a valid `*mut u8` pointer (which itself
/// points to a writable null-terminated string or is null).
/// `delim` must be a valid null-terminated string.
#[unsafe(no_mangle)]
#[allow(clippy::many_single_char_names)] // POSIX function, C convention variables.
pub unsafe extern "C" fn strsep(stringp: *mut *mut u8, delim: *const u8) -> *mut u8 {
    if stringp.is_null() {
        return core::ptr::null_mut();
    }

    let s = unsafe { *stringp };
    if s.is_null() {
        return core::ptr::null_mut();
    }

    let begin = s;
    let mut i: usize = 0;
    loop {
        let c = unsafe { *s.add(i) };
        if c == 0 {
            // Reached end of string — no more tokens.
            unsafe { *stringp = core::ptr::null_mut(); }
            return begin;
        }

        // Check if c is a delimiter.
        let mut j: usize = 0;
        loop {
            let d = unsafe { *delim.add(j) };
            if d == 0 {
                break;
            }
            if c == d {
                // Replace delimiter with null and advance past it.
                unsafe { *s.add(i) = 0; }
                unsafe { *stringp = s.add(i.wrapping_add(1)); }
                return begin;
            }
            j = j.wrapping_add(1);
        }

        i = i.wrapping_add(1);
    }
}

/// Compare memory regions ignoring case (non-standard but common).
///
/// # Safety
///
/// Both pointers must be valid for at least `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strverscmp(s1: *const u8, s2: *const u8) -> i32 {
    // Simple lexicographic compare — true version comparison
    // would handle embedded numbers, but this matches the common
    // usage pattern as a strcmp variant.
    unsafe { strcmp(s1, s2) }
}

/// Reentrant string tokenizer.
///
/// Like `strtok`, but uses caller-provided `saveptr` instead of a
/// static variable, making it thread-safe.
///
/// # Safety
///
/// `s` (if non-null) and `delim` must be valid null-terminated strings.
/// `saveptr` must point to a valid `*mut u8`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtok_r(
    s: *mut u8,
    delim: *const u8,
    saveptr: *mut *mut u8,
) -> *mut u8 {
    if saveptr.is_null() {
        return core::ptr::null_mut();
    }

    let start = if s.is_null() {
        let p = unsafe { *saveptr };
        if p.is_null() {
            return core::ptr::null_mut();
        }
        p
    } else {
        s
    };

    // Skip leading delimiters.
    let mut i: usize = 0;
    loop {
        let c = unsafe { *start.add(i) };
        if c == 0 {
            unsafe { *saveptr = core::ptr::null_mut(); }
            return core::ptr::null_mut();
        }
        if !unsafe { is_delim(c, delim) } {
            break;
        }
        i = i.wrapping_add(1);
    }

    let token = unsafe { start.add(i) };

    // Find end of token.
    let mut k: usize = 0;
    loop {
        let c = unsafe { *token.add(k) };
        if c == 0 {
            unsafe { *saveptr = core::ptr::null_mut(); }
            return token;
        }
        if unsafe { is_delim(c, delim) } {
            unsafe { *token.add(k) = 0; }
            unsafe { *saveptr = token.add(k.wrapping_add(1)); }
            return token;
        }
        k = k.wrapping_add(1);
    }
}

/// Copy bytes until a given byte is found, or `n` bytes have been copied.
///
/// Copies from `src` to `dest`, stopping after the first occurrence
/// of byte `c` (which IS copied), or after `n` bytes.  Returns a
/// pointer to the byte after `c` in `dest`, or NULL if `c` was not
/// found in the first `n` bytes.
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.  `src` must be valid for `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memccpy(
    dest: *mut u8,
    src: *const u8,
    c: i32,
    n: usize,
) -> *mut u8 {
    let val = c as u8;
    let mut i: usize = 0;
    while i < n {
        let byte = unsafe { *src.add(i) };
        unsafe { *dest.add(i) = byte; }
        if byte == val {
            return unsafe { dest.add(i.wrapping_add(1)) };
        }
        i = i.wrapping_add(1);
    }
    core::ptr::null_mut()
}

/// Locale-aware string comparison.
///
/// Since we don't have locale support, this is identical to `strcmp`.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcoll(s1: *const u8, s2: *const u8) -> i32 {
    unsafe { strcmp(s1, s2) }
}

/// Transform a string for locale-aware comparison.
///
/// Copies at most `n` bytes of `src` into `dest` in a form such that
/// `strcmp` on two transformed strings gives the same result as `strcoll`
/// on the originals.  Since we have no locale, this is just `strncpy`.
///
/// Returns the length of the transformed string (not counting null).
///
/// # Safety
///
/// `dest` must be valid for `n` bytes.  `src` must be null-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strxfrm(dest: *mut u8, src: *const u8, n: usize) -> usize {
    let len = unsafe { strlen(src) };
    if n > 0 {
        unsafe { strncpy(dest, src, n); }
    }
    len
}

/// Thread-safe version of `strerror`.
///
/// Copies the error description into the user-provided buffer.
/// Returns 0 on success, or `ERANGE` if the buffer is too small.
///
/// # Safety
///
/// `buf` must be valid for `buflen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strerror_r(errnum: i32, buf: *mut u8, buflen: usize) -> i32 {
    if buf.is_null() || buflen == 0 {
        return crate::errno::ERANGE;
    }

    let msg = strerror(errnum);
    let msg_len = unsafe { strlen(msg) };
    let copy_len = if msg_len < buflen { msg_len } else { buflen.wrapping_sub(1) };

    unsafe { memcpy(buf, msg, copy_len); }
    unsafe { *buf.add(copy_len) = 0; }

    if msg_len >= buflen {
        crate::errno::ERANGE
    } else {
        0
    }
}
