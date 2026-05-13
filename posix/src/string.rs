//! C string functions required by the C runtime.
//!
//! These are not strictly POSIX but are required by virtually every
//! C program.  A real libc would provide optimized (SIMD) versions;
//! these are correct reference implementations.
//!
//! Includes: `memcpy`, `memmove`, `memset`, `memcmp`, `memchr`,
//! `memrchr`, `memccpy`, `mempcpy`, `memmem`, `rawmemchr`,
//! `strlen`, `strnlen`, `strcmp`, `strncmp`,
//! `strcpy`, `strncpy`, `stpcpy`, `stpncpy`, `strchr`, `strrchr`,
//! `strcat`, `strncat`, `strstr`, `strspn`, `strcspn`, `strpbrk`,
//! `strtok`, `strtok_r`, `strsep`, `strerror`, `strerror_r`,
//! `strdup`, `strndup`, `bcopy`, `bzero`, `strcasecmp`, `strncasecmp`,
//! `strcoll`, `strxfrm`, `strverscmp`, `strlcpy`, `strlcat`,
//! `sys_errlist`, `sys_nerr`
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

/// Like `strchr`, but returns a pointer to the null terminator if
/// `c` is not found (instead of null).
///
/// GNU extension — commonly used by glibc-based programs.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchrnul(s: *const u8, c: i32) -> *const u8 {
    let val = c as u8;
    let mut i: usize = 0;
    loop {
        // SAFETY: s is a valid null-terminated string.
        let ch = unsafe { *s.add(i) };
        if ch == val || ch == 0 {
            return unsafe { s.add(i) };
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
        6 => c"No such device or address".as_ptr().cast::<u8>(),
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
        19 => c"No such device".as_ptr().cast::<u8>(),
        20 => c"Not a directory".as_ptr().cast::<u8>(),
        21 => c"Is a directory".as_ptr().cast::<u8>(),
        22 => c"Invalid argument".as_ptr().cast::<u8>(),
        23 => c"Too many open files in system".as_ptr().cast::<u8>(),
        24 => c"Too many open files".as_ptr().cast::<u8>(),
        25 => c"Inappropriate ioctl for device".as_ptr().cast::<u8>(),
        27 => c"File too large".as_ptr().cast::<u8>(),
        28 => c"No space left on device".as_ptr().cast::<u8>(),
        29 => c"Illegal seek".as_ptr().cast::<u8>(),
        30 => c"Read-only file system".as_ptr().cast::<u8>(),
        26 => c"Text file busy".as_ptr().cast::<u8>(),
        31 => c"Too many links".as_ptr().cast::<u8>(),
        32 => c"Broken pipe".as_ptr().cast::<u8>(),
        33 => c"Numerical argument out of domain".as_ptr().cast::<u8>(),
        34 => c"Numerical result out of range".as_ptr().cast::<u8>(),
        35 => c"Resource deadlock avoided".as_ptr().cast::<u8>(),
        36 => c"File name too long".as_ptr().cast::<u8>(),
        37 => c"No locks available".as_ptr().cast::<u8>(),
        38 => c"Function not implemented".as_ptr().cast::<u8>(),
        39 => c"Directory not empty".as_ptr().cast::<u8>(),
        40 => c"Too many levels of symbolic links".as_ptr().cast::<u8>(),
        42 => c"No message of desired type".as_ptr().cast::<u8>(),
        43 => c"Identifier removed".as_ptr().cast::<u8>(),
        60 => c"Device not a stream".as_ptr().cast::<u8>(),
        61 => c"No data available".as_ptr().cast::<u8>(),
        62 => c"Timer expired".as_ptr().cast::<u8>(),
        63 => c"Out of streams resources".as_ptr().cast::<u8>(),
        67 => c"Link has been severed".as_ptr().cast::<u8>(),
        71 => c"Protocol error".as_ptr().cast::<u8>(),
        72 => c"Multihop attempted".as_ptr().cast::<u8>(),
        74 => c"Bad message".as_ptr().cast::<u8>(),
        75 => c"Value too large for defined data type".as_ptr().cast::<u8>(),
        84 => c"Invalid or incomplete multibyte or wide character".as_ptr().cast::<u8>(),
        88 => c"Socket operation on non-socket".as_ptr().cast::<u8>(),
        89 => c"Destination address required".as_ptr().cast::<u8>(),
        90 => c"Message too long".as_ptr().cast::<u8>(),
        91 => c"Protocol wrong type for socket".as_ptr().cast::<u8>(),
        92 => c"Protocol not available".as_ptr().cast::<u8>(),
        93 => c"Protocol not supported".as_ptr().cast::<u8>(),
        95 => c"Operation not supported".as_ptr().cast::<u8>(),
        97 => c"Address family not supported by protocol".as_ptr().cast::<u8>(),
        98 => c"Address already in use".as_ptr().cast::<u8>(),
        99 => c"Cannot assign requested address".as_ptr().cast::<u8>(),
        100 => c"Network is down".as_ptr().cast::<u8>(),
        101 => c"Network is unreachable".as_ptr().cast::<u8>(),
        102 => c"Network dropped connection on reset".as_ptr().cast::<u8>(),
        103 => c"Software caused connection abort".as_ptr().cast::<u8>(),
        104 => c"Connection reset by peer".as_ptr().cast::<u8>(),
        105 => c"No buffer space available".as_ptr().cast::<u8>(),
        106 => c"Transport endpoint is already connected".as_ptr().cast::<u8>(),
        107 => c"Transport endpoint is not connected".as_ptr().cast::<u8>(),
        108 => c"Cannot send after transport endpoint shutdown".as_ptr().cast::<u8>(),
        110 => c"Connection timed out".as_ptr().cast::<u8>(),
        111 => c"Connection refused".as_ptr().cast::<u8>(),
        112 => c"Host is down".as_ptr().cast::<u8>(),
        113 => c"No route to host".as_ptr().cast::<u8>(),
        114 => c"Operation already in progress".as_ptr().cast::<u8>(),
        115 => c"Operation now in progress".as_ptr().cast::<u8>(),
        116 => c"Stale file handle".as_ptr().cast::<u8>(),
        123 => c"No medium found".as_ptr().cast::<u8>(),
        125 => c"Operation canceled".as_ptr().cast::<u8>(),
        130 => c"Owner died".as_ptr().cast::<u8>(),
        131 => c"State not recoverable".as_ptr().cast::<u8>(),
        _ => c"Unknown error".as_ptr().cast::<u8>(),
    }
}

/// Duplicate a string.
///
/// Allocates memory for a copy of `s` using `malloc`.  The caller
/// must free the result with `free()`.
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

    // Allocate via malloc so the pointer has a valid header for free().
    // The previous implementation used mmap directly, which produced
    // pointers incompatible with free() (no [mmap_base, total_size]
    // header), causing memory corruption on free(strdup(...)).
    let dest = crate::malloc::malloc(size);
    if dest.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: malloc returned valid memory of at least `size` bytes.
    unsafe { memcpy(dest, s, size); }
    dest
}

/// Duplicate at most `n` bytes of a string.
///
/// Allocates memory for a copy of at most `n` bytes from `s`,
/// plus a null terminator.  The result is always null-terminated.
/// The caller must free the result with `free()`.
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

    // Allocate via malloc so the pointer has a valid header for free().
    let dest = crate::malloc::malloc(size);
    if dest.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: malloc returned valid memory of at least `size` bytes.
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

// ---------------------------------------------------------------------------
// ffs / ffsl / ffsll — find first set bit
// ---------------------------------------------------------------------------

/// Find the first set bit in an integer.
///
/// Returns the 1-based position of the least significant set bit,
/// or 0 if `i` is 0.
#[unsafe(no_mangle)]
pub extern "C" fn ffs(i: i32) -> i32 {
    if i == 0 {
        return 0;
    }
    // trailing_zeros gives 0-based position; POSIX wants 1-based.
    ((i as u32).trailing_zeros() as i32).wrapping_add(1)
}

/// Find the first set bit in a long integer.
#[unsafe(no_mangle)]
pub extern "C" fn ffsl(i: i64) -> i32 {
    if i == 0 {
        return 0;
    }
    ((i as u64).trailing_zeros() as i32).wrapping_add(1)
}

/// Find the first set bit in a long long integer.
#[unsafe(no_mangle)]
pub extern "C" fn ffsll(i: i64) -> i32 {
    ffsl(i)
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

/// Version-aware string comparison (GNU extension, `<string.h>`).
///
/// Like `strcmp`, but when both strings contain a run of digits at the
/// same position, the digit runs are compared *numerically* rather than
/// lexicographically.  This gives the intuitive result for version
/// strings: `"file9" < "file10"`, `"1.2.3" < "1.10.0"`.
///
/// Leading-zero handling follows the glibc convention: a digit run with
/// a leading zero is compared as a fractional part (lexicographic, so
/// longer run with same prefix is greater), while runs without leading
/// zeros are compared by numeric value (shorter run with same digits is
/// smaller).
///
/// Based on glibc `strverscmp` (`string/strverscmp.c`).
///
/// # Safety
///
/// Both pointers must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strverscmp(s1: *const u8, s2: *const u8) -> i32 {
    let mut i: usize = 0;

    // Scan forward while characters are equal.
    loop {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };

        if a != b || a == 0 {
            // Found the first difference (or end of both strings).
            // If neither byte is a digit, fall through to plain compare.
            // If at least one is a digit, we need version comparison.
            let a_dig = a.is_ascii_digit();
            let b_dig = b.is_ascii_digit();

            if !a_dig && !b_dig {
                // Neither is a digit — normal lexicographic result.
                return (a as i32) - (b as i32);
            }

            // At least one is a digit.  Walk back to find the start of
            // the digit run that includes position `i`.
            let mut start = i;
            while start > 0 && unsafe { *s1.add(start.wrapping_sub(1)) }.is_ascii_digit() {
                start = start.wrapping_sub(1);
            }

            // Check for leading zeros in the shared digit run.
            let has_leading_zero =
                unsafe { *s1.add(start) } == b'0' || unsafe { *s2.add(start) } == b'0';

            if has_leading_zero {
                // Fractional comparison: compare digit-by-digit (lexicographic).
                // A digit beats a non-digit (non-digit means the run ended),
                // but a shorter fractional part with the same prefix is less.
                return strverscmp_frac(s1, s2, start);
            }

            // Integer comparison: longer digit run = larger number.
            return strverscmp_int(s1, s2, start);
        }

        i = i.wrapping_add(1);
    }
}

/// Fractional-style digit run comparison (leading-zero case).
///
/// Compare digit-by-digit from `start`.  When one run ends (non-digit or NUL)
/// and the other continues, the continuing run is "greater."
fn strverscmp_frac(s1: *const u8, s2: *const u8, start: usize) -> i32 {
    let mut j = start;
    loop {
        let a = unsafe { *s1.add(j) };
        let b = unsafe { *s2.add(j) };
        let a_dig = a.is_ascii_digit();
        let b_dig = b.is_ascii_digit();

        if !a_dig && !b_dig {
            return 0; // Same digit run, same length.
        }
        if !a_dig {
            return -1; // s1 run ended first → s1 < s2.
        }
        if !b_dig {
            return 1; // s2 run ended first → s1 > s2.
        }
        if a != b {
            return (a as i32) - (b as i32);
        }
        j = j.wrapping_add(1);
    }
}

/// Integer-style digit run comparison (no leading-zero case).
///
/// The longer digit run represents a larger number.  If runs are the
/// same length, the first differing digit decides.
fn strverscmp_int(s1: *const u8, s2: *const u8, start: usize) -> i32 {
    let mut j = start;
    let mut first_diff: i32 = 0;
    loop {
        let a = unsafe { *s1.add(j) };
        let b = unsafe { *s2.add(j) };
        let a_dig = a.is_ascii_digit();
        let b_dig = b.is_ascii_digit();

        if !a_dig && !b_dig {
            // Same length — use first differing digit.
            return first_diff;
        }
        if !a_dig {
            return -1; // s1 run shorter → smaller number.
        }
        if !b_dig {
            return 1; // s2 run shorter → larger number.
        }
        if a != b && first_diff == 0 {
            first_diff = (a as i32) - (b as i32);
        }
        j = j.wrapping_add(1);
    }
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

/// Locale-aware string comparison (locale variant).
///
/// Since we only support the C locale, delegates to `strcmp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcoll_l(s1: *const u8, s2: *const u8, _locale: usize) -> i32 {
    unsafe { strcmp(s1, s2) }
}

/// Transform a string for locale-aware comparison (locale variant).
///
/// Since we only support the C locale, delegates to `strxfrm`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strxfrm_l(dest: *mut u8, src: *const u8, n: usize, _locale: usize) -> usize {
    unsafe { strxfrm(dest, src, n) }
}

/// Locale-aware `strerror`.
///
/// Returns the same result as `strerror` (locale is ignored).
#[unsafe(no_mangle)]
pub extern "C" fn strerror_l(errnum: i32, _locale: usize) -> *const u8 {
    strerror(errnum)
}

/// XPG variant of `strerror_r`.
///
/// Some glibc-compiled programs reference `__xpg_strerror_r` instead
/// of the GNU-specific `strerror_r`.  The XPG version returns 0 on
/// success and an error code on failure (same as our `strerror_r`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __xpg_strerror_r(errnum: i32, buf: *mut u8, buflen: usize) -> i32 {
    unsafe { strerror_r(errnum, buf, buflen) }
}

// ---------------------------------------------------------------------------
// BSD safe string functions
// ---------------------------------------------------------------------------

/// Copy a string with guaranteed NUL termination.
///
/// Copies up to `size - 1` bytes from `src` to `dst` and always
/// NUL-terminates (unless `size` is 0).  Returns the total length
/// of `src` (not including NUL) — if the return value >= `size`,
/// truncation occurred.
///
/// This is the BSD `strlcpy`, widely used as a safer `strncpy`.
///
/// # Safety
///
/// `dst` must be valid for `size` bytes.  `src` must be a valid
/// NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlcpy(dst: *mut u8, src: *const u8, size: SizeT) -> SizeT {
    let src_len = unsafe { strlen(src) };

    if size > 0 {
        let copy_len = if src_len < size { src_len } else { size.wrapping_sub(1) };
        // SAFETY: dst valid for `size` bytes, src valid for src_len.
        unsafe { core::ptr::copy_nonoverlapping(src, dst, copy_len); }
        unsafe { *dst.add(copy_len) = 0; }
    }

    src_len
}

/// Append a string with guaranteed NUL termination.
///
/// Appends `src` to `dst`, writing at most `size - strlen(dst) - 1`
/// bytes.  Always NUL-terminates (unless `size <= strlen(dst)`).
/// Returns `strlen(dst) + strlen(src)` — if the return value >= `size`,
/// truncation occurred.
///
/// This is the BSD `strlcat`, widely used as a safer `strncat`.
///
/// # Safety
///
/// `dst` must be valid for `size` bytes and contain a NUL-terminated
/// string.  `src` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlcat(dst: *mut u8, src: *const u8, size: SizeT) -> SizeT {
    let dst_len = unsafe { strnlen(dst, size) };
    let src_len = unsafe { strlen(src) };

    if dst_len >= size {
        // dst already fills the buffer — no room even for NUL.
        return size.wrapping_add(src_len);
    }

    let remaining = size.wrapping_sub(dst_len).wrapping_sub(1);
    let copy_len = if src_len < remaining { src_len } else { remaining };

    // SAFETY: dst_len < size, so dst.add(dst_len) is within bounds.
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst.add(dst_len), copy_len);
        *dst.add(dst_len.wrapping_add(copy_len)) = 0;
    }

    dst_len.wrapping_add(src_len)
}

// ---------------------------------------------------------------------------
// strcasestr — case-insensitive substring search
// ---------------------------------------------------------------------------

/// Locate a case-insensitive substring.
///
/// Returns a pointer to the first occurrence of `needle` in
/// `haystack`, ignoring ASCII case differences.  Returns null if not
/// found.  If `needle` is empty, returns `haystack`.
///
/// # Safety
///
/// Both `haystack` and `needle` must be valid null-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcasestr(
    haystack: *const u8,
    needle: *const u8,
) -> *mut u8 {
    if haystack.is_null() || needle.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: Both pointers are valid null-terminated strings.
    let nlen = unsafe { strlen(needle) };
    if nlen == 0 {
        return haystack.cast_mut();
    }

    let hlen = unsafe { strlen(haystack) };
    if nlen > hlen {
        return core::ptr::null_mut();
    }

    let end = hlen.wrapping_sub(nlen);
    let mut i: usize = 0;
    while i <= end {
        if unsafe { casecmp_n(haystack.add(i), needle, nlen) } {
            return unsafe { haystack.add(i).cast_mut() };
        }
        i = i.wrapping_add(1);
    }

    core::ptr::null_mut()
}

/// Compare `n` bytes of two strings, case-insensitively.
///
/// # Safety
///
/// Both pointers must be readable for `n` bytes.
unsafe fn casecmp_n(a: *const u8, b: *const u8, n: usize) -> bool {
    let mut j: usize = 0;
    while j < n {
        // SAFETY: j < n, both pointers valid for n bytes.
        let ca = unsafe { *a.add(j) };
        let cb = unsafe { *b.add(j) };
        if to_lower(ca) != to_lower(cb) {
            return false;
        }
        j = j.wrapping_add(1);
    }
    true
}

/// ASCII lowercase.
fn to_lower(c: u8) -> u8 {
    if c.is_ascii_uppercase() {
        #[allow(clippy::arithmetic_side_effects)]
        return c | 0x20;
    }
    c
}

// ---------------------------------------------------------------------------
// explicit_bzero — guaranteed-not-optimized-away zeroing
// ---------------------------------------------------------------------------

/// Zero a memory region, guaranteed not to be optimized away.
///
/// Unlike `memset(s, 0, n)`, the compiler cannot elide this call even
/// if the buffer is not read afterward.  Used for clearing sensitive
/// data (passwords, keys) from memory.
///
/// # Safety
///
/// `s` must be valid for `n` bytes of writing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn explicit_bzero(s: *mut u8, n: usize) {
    if s.is_null() || n == 0 {
        return;
    }

    // Use volatile writes so the compiler cannot elide them.
    let mut i: usize = 0;
    while i < n {
        // SAFETY: s is valid for n bytes; i < n.
        unsafe {
            core::ptr::write_volatile(s.add(i), 0);
        }
        i = i.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// mempcpy — copy with end-of-dest return
// ---------------------------------------------------------------------------

/// Copy `n` bytes from `src` to `dest`, returning a pointer past the
/// last written byte.
///
/// Like `memcpy` but returns `dest + n` instead of `dest`.  This is a
/// GNU extension commonly used for efficient buffer building (chain
/// multiple mempcpy calls without tracking the offset manually).
///
/// # Safety
///
/// `dest` and `src` must be valid for `n` bytes and must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mempcpy(
    dest: *mut u8,
    src: *const u8,
    n: SizeT,
) -> *mut u8 {
    let mut i: usize = 0;
    while i < n {
        // SAFETY: Caller guarantees both pointers valid for n bytes.
        unsafe { *dest.add(i) = *src.add(i); }
        i = i.wrapping_add(1);
    }
    // SAFETY: dest + n is one-past-end, valid for pointer arithmetic.
    unsafe { dest.add(n) }
}

// ---------------------------------------------------------------------------
// memmem — search for byte sequence in memory
// ---------------------------------------------------------------------------

/// Locate a byte sequence within a larger memory region.
///
/// Searches the first `haystacklen` bytes of `haystack` for the first
/// occurrence of the `needlelen`-byte sequence at `needle`.
///
/// Returns a pointer to the start of the match, or NULL if not found.
///
/// Edge cases (per POSIX / glibc):
/// - If `needlelen` is 0, returns `haystack` (empty pattern always matches).
/// - If `needlelen > haystacklen`, returns NULL.
///
/// Uses a simple linear scan.  For large inputs, a KMP or Two-Way
/// algorithm would be faster, but the simple version is correct and
/// sufficient for the buffer sizes we encounter.
///
/// # Safety
///
/// `haystack` must be valid for `haystacklen` bytes.
/// `needle` must be valid for `needlelen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmem(
    haystack: *const u8,
    haystacklen: SizeT,
    needle: *const u8,
    needlelen: SizeT,
) -> *const u8 {
    // Empty needle matches immediately.
    if needlelen == 0 {
        return haystack;
    }
    if haystack.is_null() || needle.is_null() || needlelen > haystacklen {
        return core::ptr::null();
    }

    // Scan positions: only need to check up to haystacklen - needlelen.
    let limit = haystacklen.wrapping_sub(needlelen);
    let mut i: usize = 0;
    while i <= limit {
        // Check if needle matches at position i.
        let mut j: usize = 0;
        let mut matched = true;
        while j < needlelen {
            // SAFETY: i + j < haystacklen (since i <= limit and
            // j < needlelen, so i + j <= haystacklen - needlelen +
            // needlelen - 1 = haystacklen - 1). Both pointers valid.
            if unsafe { *haystack.add(i.wrapping_add(j)) != *needle.add(j) } {
                matched = false;
                break;
            }
            j = j.wrapping_add(1);
        }
        if matched {
            // SAFETY: haystack + i is within bounds.
            return unsafe { haystack.add(i) };
        }
        i = i.wrapping_add(1);
    }

    core::ptr::null()
}

// ---------------------------------------------------------------------------
// rawmemchr — unbounded memchr (assumes byte is present)
// ---------------------------------------------------------------------------

/// Search for a byte in memory without a length bound.
///
/// Like `memchr` but assumes the byte `c` WILL be found somewhere in
/// the buffer.  This is a GNU extension used by glibc internals and
/// some programs for efficiency when the caller guarantees the
/// sentinel exists (e.g., searching for `'\0'` in a C string).
///
/// # Safety
///
/// `s` must point to memory that contains at least one occurrence of
/// `c` (as the low byte of the int).  If `c` is not present, this
/// function reads past the end of valid memory (undefined behavior).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawmemchr(s: *const u8, c: i32) -> *const u8 {
    let target = c as u8;
    let mut p = s;
    // SAFETY: Caller guarantees c exists in the buffer, so we will
    // find it before reading invalid memory.
    while unsafe { *p } != target {
        p = unsafe { p.add(1) };
    }
    p
}

// ---------------------------------------------------------------------------
// sys_errlist / sys_nerr — deprecated but widely referenced
// ---------------------------------------------------------------------------

/// Number of entries in `sys_errlist` (one past the highest defined errno).
///
/// Deprecated since POSIX.1-2001, removed in POSIX.1-2008, but many
/// programs and libraries still reference it for link compatibility.
/// Our highest errno is 131 (ENOTRECOVERABLE), so sys_nerr = 132.
#[unsafe(no_mangle)]
pub static sys_nerr: i32 = 132;

/// Wrapper to make `*const u8` usable in a static array.
///
/// Raw pointers are not `Sync`, but our pointers all point to static
/// string literals with `'static` lifetime, so sharing is safe.
#[repr(transparent)]
pub struct SyncPtr(*const u8);

// SAFETY: All wrapped pointers point to static c-string literals
// that live for the entire program lifetime and are never mutated.
unsafe impl Sync for SyncPtr {}

/// Array of error message strings indexed by errno value.
///
/// `sys_errlist[n]` points to the same static string that `strerror(n)`
/// returns.  Entries for undefined errno values point to "Unknown error".
///
/// Deprecated since POSIX.1-2001 — use `strerror()` instead.  Provided
/// for link compatibility with programs that reference the symbol.
///
/// SAFETY: All pointers are to static `c"..."` literals with `'static`
/// lifetime.  The array itself is a static, so the pointer is stable.
/// The `SyncPtr` wrapper is `repr(transparent)` so the array layout
/// matches `[*const u8; 132]` exactly — C code sees a plain pointer
/// array.
#[unsafe(no_mangle)]
pub static sys_errlist: [SyncPtr; 132] = {
    // Inline const: build the table.  Every index maps to a c-string.
    // Indices with no defined errno get "Unknown error".
    const UNK: SyncPtr = SyncPtr(c"Unknown error".as_ptr().cast::<u8>());

    let mut table: [SyncPtr; 132] = [UNK; 132];

    table[0] = SyncPtr(c"Success".as_ptr().cast::<u8>());
    table[1] = SyncPtr(c"Operation not permitted".as_ptr().cast::<u8>());
    table[2] = SyncPtr(c"No such file or directory".as_ptr().cast::<u8>());
    table[3] = SyncPtr(c"No such process".as_ptr().cast::<u8>());
    table[4] = SyncPtr(c"Interrupted system call".as_ptr().cast::<u8>());
    table[5] = SyncPtr(c"Input/output error".as_ptr().cast::<u8>());
    table[6] = SyncPtr(c"No such device or address".as_ptr().cast::<u8>());
    table[7] = SyncPtr(c"Argument list too long".as_ptr().cast::<u8>());
    table[8] = SyncPtr(c"Exec format error".as_ptr().cast::<u8>());
    table[9] = SyncPtr(c"Bad file descriptor".as_ptr().cast::<u8>());
    table[10] = SyncPtr(c"No child processes".as_ptr().cast::<u8>());
    table[11] = SyncPtr(c"Resource temporarily unavailable".as_ptr().cast::<u8>());
    table[12] = SyncPtr(c"Cannot allocate memory".as_ptr().cast::<u8>());
    table[13] = SyncPtr(c"Permission denied".as_ptr().cast::<u8>());
    table[14] = SyncPtr(c"Bad address".as_ptr().cast::<u8>());
    // 15: ENOTBLK — not defined in our errno.rs
    table[16] = SyncPtr(c"Device or resource busy".as_ptr().cast::<u8>());
    table[17] = SyncPtr(c"File exists".as_ptr().cast::<u8>());
    table[18] = SyncPtr(c"Invalid cross-device link".as_ptr().cast::<u8>());
    table[19] = SyncPtr(c"No such device".as_ptr().cast::<u8>());
    table[20] = SyncPtr(c"Not a directory".as_ptr().cast::<u8>());
    table[21] = SyncPtr(c"Is a directory".as_ptr().cast::<u8>());
    table[22] = SyncPtr(c"Invalid argument".as_ptr().cast::<u8>());
    table[23] = SyncPtr(c"Too many open files in system".as_ptr().cast::<u8>());
    table[24] = SyncPtr(c"Too many open files".as_ptr().cast::<u8>());
    table[25] = SyncPtr(c"Inappropriate ioctl for device".as_ptr().cast::<u8>());
    table[26] = SyncPtr(c"Text file busy".as_ptr().cast::<u8>());
    table[27] = SyncPtr(c"File too large".as_ptr().cast::<u8>());
    table[28] = SyncPtr(c"No space left on device".as_ptr().cast::<u8>());
    table[29] = SyncPtr(c"Illegal seek".as_ptr().cast::<u8>());
    table[30] = SyncPtr(c"Read-only file system".as_ptr().cast::<u8>());
    table[31] = SyncPtr(c"Too many links".as_ptr().cast::<u8>());
    table[32] = SyncPtr(c"Broken pipe".as_ptr().cast::<u8>());
    table[33] = SyncPtr(c"Numerical argument out of domain".as_ptr().cast::<u8>());
    table[34] = SyncPtr(c"Numerical result out of range".as_ptr().cast::<u8>());
    table[35] = SyncPtr(c"Resource deadlock avoided".as_ptr().cast::<u8>());
    table[36] = SyncPtr(c"File name too long".as_ptr().cast::<u8>());
    table[37] = SyncPtr(c"No locks available".as_ptr().cast::<u8>());
    table[38] = SyncPtr(c"Function not implemented".as_ptr().cast::<u8>());
    table[39] = SyncPtr(c"Directory not empty".as_ptr().cast::<u8>());
    table[40] = SyncPtr(c"Too many levels of symbolic links".as_ptr().cast::<u8>());
    // 41: unused on Linux
    table[42] = SyncPtr(c"No message of desired type".as_ptr().cast::<u8>());
    table[43] = SyncPtr(c"Identifier removed".as_ptr().cast::<u8>());
    // 44-59: various Linux errnos not in our set
    table[60] = SyncPtr(c"Device not a stream".as_ptr().cast::<u8>());
    table[61] = SyncPtr(c"No data available".as_ptr().cast::<u8>());
    table[62] = SyncPtr(c"Timer expired".as_ptr().cast::<u8>());
    table[63] = SyncPtr(c"Out of streams resources".as_ptr().cast::<u8>());
    // 64-66: unused in our set
    table[67] = SyncPtr(c"Link has been severed".as_ptr().cast::<u8>());
    // 68-70: unused in our set
    table[71] = SyncPtr(c"Protocol error".as_ptr().cast::<u8>());
    table[72] = SyncPtr(c"Multihop attempted".as_ptr().cast::<u8>());
    // 73: unused
    table[74] = SyncPtr(c"Bad message".as_ptr().cast::<u8>());
    table[75] = SyncPtr(c"Value too large for defined data type".as_ptr().cast::<u8>());
    // 76-83: unused in our set
    table[84] = SyncPtr(c"Invalid or incomplete multibyte or wide character".as_ptr().cast::<u8>());
    // 85-87: unused
    table[88] = SyncPtr(c"Socket operation on non-socket".as_ptr().cast::<u8>());
    table[89] = SyncPtr(c"Destination address required".as_ptr().cast::<u8>());
    table[90] = SyncPtr(c"Message too long".as_ptr().cast::<u8>());
    table[91] = SyncPtr(c"Protocol wrong type for socket".as_ptr().cast::<u8>());
    table[92] = SyncPtr(c"Protocol not available".as_ptr().cast::<u8>());
    table[93] = SyncPtr(c"Protocol not supported".as_ptr().cast::<u8>());
    // 94: ESOCKTNOSUPPORT
    table[95] = SyncPtr(c"Operation not supported".as_ptr().cast::<u8>());
    // 96: EPFNOSUPPORT
    table[97] = SyncPtr(c"Address family not supported by protocol".as_ptr().cast::<u8>());
    table[98] = SyncPtr(c"Address already in use".as_ptr().cast::<u8>());
    table[99] = SyncPtr(c"Cannot assign requested address".as_ptr().cast::<u8>());
    table[100] = SyncPtr(c"Network is down".as_ptr().cast::<u8>());
    table[101] = SyncPtr(c"Network is unreachable".as_ptr().cast::<u8>());
    table[102] = SyncPtr(c"Network dropped connection on reset".as_ptr().cast::<u8>());
    table[103] = SyncPtr(c"Software caused connection abort".as_ptr().cast::<u8>());
    table[104] = SyncPtr(c"Connection reset by peer".as_ptr().cast::<u8>());
    table[105] = SyncPtr(c"No buffer space available".as_ptr().cast::<u8>());
    table[106] = SyncPtr(c"Transport endpoint is already connected".as_ptr().cast::<u8>());
    table[107] = SyncPtr(c"Transport endpoint is not connected".as_ptr().cast::<u8>());
    table[108] = SyncPtr(c"Cannot send after transport endpoint shutdown".as_ptr().cast::<u8>());
    // 109: ETOOMANYREFS
    table[110] = SyncPtr(c"Connection timed out".as_ptr().cast::<u8>());
    table[111] = SyncPtr(c"Connection refused".as_ptr().cast::<u8>());
    table[112] = SyncPtr(c"Host is down".as_ptr().cast::<u8>());
    table[113] = SyncPtr(c"No route to host".as_ptr().cast::<u8>());
    table[114] = SyncPtr(c"Operation already in progress".as_ptr().cast::<u8>());
    table[115] = SyncPtr(c"Operation now in progress".as_ptr().cast::<u8>());
    table[116] = SyncPtr(c"Stale file handle".as_ptr().cast::<u8>());
    // 117-122: unused in our set
    table[123] = SyncPtr(c"No medium found".as_ptr().cast::<u8>());
    // 124: EMEDIUMTYPE
    table[125] = SyncPtr(c"Operation canceled".as_ptr().cast::<u8>());
    // 126-129: unused in our set
    table[130] = SyncPtr(c"Owner died".as_ptr().cast::<u8>());
    table[131] = SyncPtr(c"State not recoverable".as_ptr().cast::<u8>());

    table
};

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: call strverscmp on two byte-string literals.
    fn ver(a: &[u8], b: &[u8]) -> i32 {
        unsafe { strverscmp(a.as_ptr(), b.as_ptr()) }
    }

    #[test]
    fn test_strverscmp_equal() {
        assert_eq!(ver(b"foo\0", b"foo\0"), 0);
        assert_eq!(ver(b"\0", b"\0"), 0);
        assert_eq!(ver(b"1.2.3\0", b"1.2.3\0"), 0);
    }

    #[test]
    fn test_strverscmp_pure_text() {
        // No digits — should behave like strcmp.
        assert!(ver(b"abc\0", b"abd\0") < 0);
        assert!(ver(b"abd\0", b"abc\0") > 0);
        assert!(ver(b"abc\0", b"abcd\0") < 0);
    }

    #[test]
    fn test_strverscmp_numeric_ordering() {
        // The primary use case: "file9" < "file10".
        assert!(ver(b"file9\0", b"file10\0") < 0);
        assert!(ver(b"file10\0", b"file9\0") > 0);
    }

    #[test]
    fn test_strverscmp_version_strings() {
        assert!(ver(b"1.2.3\0", b"1.10.0\0") < 0);
        assert!(ver(b"1.10.0\0", b"1.2.3\0") > 0);
        assert!(ver(b"2.0\0", b"1.999\0") > 0);
    }

    #[test]
    fn test_strverscmp_leading_zeros() {
        // Leading zeros trigger fractional comparison:
        // "1.01" vs "1.1" — 01 vs 1: '0' < '1' lexicographically, but
        // runs are: s1="01" vs s2="1". In the fractional path, s1 has a
        // leading zero: "01" < "1" because '0' < '1' digit-by-digit.
        // Actually per glibc: "1.01" < "1.1" because "01" sorts before "1"
        // (the leading zero makes it fractional, so 0.01 < 0.1).
        assert!(ver(b"1.01\0", b"1.1\0") < 0);
        assert!(ver(b"1.001\0", b"1.01\0") < 0);
    }

    #[test]
    fn test_strverscmp_same_length_different_digits() {
        // Same number of digits, different values.
        assert!(ver(b"foo123\0", b"foo456\0") < 0);
        assert!(ver(b"bar99\0", b"bar42\0") > 0);
    }

    #[test]
    fn test_strverscmp_digit_vs_nondigit() {
        // One string has a digit where the other has a letter.
        // Digit characters ('0'=0x30..'9'=0x39) are less than letters
        // ('A'=0x41, 'a'=0x61) in ASCII.
        assert!(ver(b"a1\0", b"ab\0") < 0);
    }

    #[test]
    fn test_strverscmp_multiple_numeric_segments() {
        // "1.2.30" vs "1.2.4" — comparison triggers at the third segment.
        assert!(ver(b"1.2.30\0", b"1.2.4\0") > 0);
    }
}

// ===========================================================================
// glibc FORTIFY_SOURCE _chk functions
// ===========================================================================
//
// Programs compiled with `-D_FORTIFY_SOURCE=2` (default on many distros)
// call these `__*_chk` wrappers instead of the plain functions.  The
// `destlen` parameter enables runtime buffer overflow detection.  We
// ignore it and delegate to the underlying function — our runtime is
// the only code running, so any overflow is our own bug.

/// `__memcpy_chk` — fortified `memcpy`.
///
/// # Safety
///
/// Same as `memcpy`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __memcpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { memcpy(dest, src, n) }
}

/// `__memmove_chk` — fortified `memmove`.
///
/// # Safety
///
/// Same as `memmove`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __memmove_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { memmove(dest, src, n) }
}

/// `__memset_chk` — fortified `memset`.
///
/// # Safety
///
/// Same as `memset`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __memset_chk(
    dest: *mut u8,
    c: i32,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { memset(dest, c, n) }
}

/// `__strcpy_chk` — fortified `strcpy`.
///
/// # Safety
///
/// Same as `strcpy`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __strcpy_chk(
    dest: *mut u8,
    src: *const u8,
    _destlen: usize,
) -> *mut u8 {
    unsafe { strcpy(dest, src) }
}

/// `__strncpy_chk` — fortified `strncpy`.
///
/// # Safety
///
/// Same as `strncpy`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __strncpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { strncpy(dest, src, n) }
}

/// `__strcat_chk` — fortified `strcat`.
///
/// # Safety
///
/// Same as `strcat`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __strcat_chk(
    dest: *mut u8,
    src: *const u8,
    _destlen: usize,
) -> *mut u8 {
    unsafe { strcat(dest, src) }
}

/// `__strncat_chk` — fortified `strncat`.
///
/// # Safety
///
/// Same as `strncat`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __strncat_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { strncat(dest, src, n) }
}

/// `__stpcpy_chk` — fortified `stpcpy`.
///
/// # Safety
///
/// Same as `stpcpy`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stpcpy_chk(
    dest: *mut u8,
    src: *const u8,
    _destlen: usize,
) -> *mut u8 {
    unsafe { stpcpy(dest, src) }
}

/// `__stpncpy_chk` — fortified `stpncpy`.
///
/// # Safety
///
/// Same as `stpncpy`.  `destlen` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __stpncpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { stpncpy(dest, src, n) }
}
