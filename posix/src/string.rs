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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
        15 => c"Block device required".as_ptr().cast::<u8>(),
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
        44 => c"Channel number out of range".as_ptr().cast::<u8>(),
        45 => c"Level 2 not synchronized".as_ptr().cast::<u8>(),
        46 => c"Level 3 halted".as_ptr().cast::<u8>(),
        47 => c"Level 3 reset".as_ptr().cast::<u8>(),
        48 => c"Link number out of range".as_ptr().cast::<u8>(),
        49 => c"Protocol driver not attached".as_ptr().cast::<u8>(),
        50 => c"No CSI structure available".as_ptr().cast::<u8>(),
        51 => c"Level 2 halted".as_ptr().cast::<u8>(),
        52 => c"Invalid exchange".as_ptr().cast::<u8>(),
        53 => c"Invalid request descriptor".as_ptr().cast::<u8>(),
        54 => c"Exchange full".as_ptr().cast::<u8>(),
        55 => c"No anode".as_ptr().cast::<u8>(),
        56 => c"Invalid request code".as_ptr().cast::<u8>(),
        57 => c"Invalid slot".as_ptr().cast::<u8>(),
        59 => c"Bad font file format".as_ptr().cast::<u8>(),
        60 => c"Device not a stream".as_ptr().cast::<u8>(),
        61 => c"No data available".as_ptr().cast::<u8>(),
        62 => c"Timer expired".as_ptr().cast::<u8>(),
        63 => c"Out of streams resources".as_ptr().cast::<u8>(),
        64 => c"Machine is not on the network".as_ptr().cast::<u8>(),
        65 => c"Package not installed".as_ptr().cast::<u8>(),
        66 => c"Object is remote".as_ptr().cast::<u8>(),
        67 => c"Link has been severed".as_ptr().cast::<u8>(),
        68 => c"Advertise error".as_ptr().cast::<u8>(),
        69 => c"Srmount error".as_ptr().cast::<u8>(),
        70 => c"Communication error on send".as_ptr().cast::<u8>(),
        71 => c"Protocol error".as_ptr().cast::<u8>(),
        72 => c"Multihop attempted".as_ptr().cast::<u8>(),
        73 => c"RFS specific error".as_ptr().cast::<u8>(),
        74 => c"Bad message".as_ptr().cast::<u8>(),
        75 => c"Value too large for defined data type".as_ptr().cast::<u8>(),
        76 => c"Name not unique on network".as_ptr().cast::<u8>(),
        77 => c"File descriptor in bad state".as_ptr().cast::<u8>(),
        78 => c"Remote address changed".as_ptr().cast::<u8>(),
        79 => c"Can not access a needed shared library".as_ptr().cast::<u8>(),
        80 => c"Accessing a corrupted shared library".as_ptr().cast::<u8>(),
        81 => c".lib section in a.out corrupted".as_ptr().cast::<u8>(),
        82 => c"Attempting to link in too many shared libraries".as_ptr().cast::<u8>(),
        83 => c"Cannot exec a shared library directly".as_ptr().cast::<u8>(),
        84 => c"Invalid or incomplete multibyte or wide character".as_ptr().cast::<u8>(),
        85 => c"Interrupted system call should be restarted".as_ptr().cast::<u8>(),
        86 => c"Streams pipe error".as_ptr().cast::<u8>(),
        87 => c"Too many users".as_ptr().cast::<u8>(),
        88 => c"Socket operation on non-socket".as_ptr().cast::<u8>(),
        89 => c"Destination address required".as_ptr().cast::<u8>(),
        90 => c"Message too long".as_ptr().cast::<u8>(),
        91 => c"Protocol wrong type for socket".as_ptr().cast::<u8>(),
        92 => c"Protocol not available".as_ptr().cast::<u8>(),
        93 => c"Protocol not supported".as_ptr().cast::<u8>(),
        94 => c"Socket type not supported".as_ptr().cast::<u8>(),
        95 => c"Operation not supported".as_ptr().cast::<u8>(),
        96 => c"Protocol family not supported".as_ptr().cast::<u8>(),
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
        109 => c"Too many references: cannot splice".as_ptr().cast::<u8>(),
        110 => c"Connection timed out".as_ptr().cast::<u8>(),
        111 => c"Connection refused".as_ptr().cast::<u8>(),
        112 => c"Host is down".as_ptr().cast::<u8>(),
        113 => c"No route to host".as_ptr().cast::<u8>(),
        114 => c"Operation already in progress".as_ptr().cast::<u8>(),
        115 => c"Operation now in progress".as_ptr().cast::<u8>(),
        116 => c"Stale file handle".as_ptr().cast::<u8>(),
        117 => c"Structure needs cleaning".as_ptr().cast::<u8>(),
        118 => c"Not a XENIX named type file".as_ptr().cast::<u8>(),
        119 => c"No XENIX semaphores available".as_ptr().cast::<u8>(),
        120 => c"Is a named type file".as_ptr().cast::<u8>(),
        121 => c"Remote I/O error".as_ptr().cast::<u8>(),
        122 => c"Disk quota exceeded".as_ptr().cast::<u8>(),
        123 => c"No medium found".as_ptr().cast::<u8>(),
        124 => c"Wrong medium type".as_ptr().cast::<u8>(),
        125 => c"Operation canceled".as_ptr().cast::<u8>(),
        126 => c"Required key not available".as_ptr().cast::<u8>(),   // ENOKEY
        127 => c"Key has expired".as_ptr().cast::<u8>(),              // EKEYEXPIRED
        128 => c"Key has been revoked".as_ptr().cast::<u8>(),         // EKEYREVOKED
        129 => c"Key was rejected by service".as_ptr().cast::<u8>(),  // EKEYREJECTED
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bcopy(src: *const u8, dest: *mut u8, n: usize) {
    unsafe { memmove(dest, src, n); }
}

/// Set `n` bytes to zero.
///
/// # Safety
///
/// `s` must be valid for `n` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffs(i: i32) -> i32 {
    if i == 0 {
        return 0;
    }
    // trailing_zeros gives 0-based position; POSIX wants 1-based.
    ((i as u32).trailing_zeros() as i32).wrapping_add(1)
}

/// Find the first set bit in a long integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffsl(i: i64) -> i32 {
    if i == 0 {
        return 0;
    }
    ((i as u64).trailing_zeros() as i32).wrapping_add(1)
}

/// Find the first set bit in a long long integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ffsll(i: i64) -> i32 {
    ffsl(i)
}

/// Compare two strings, case-insensitive.
///
/// # Safety
///
/// Both strings must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
                // SAFETY: a and b are u8, so their i32 values are in [0, 255];
                // the difference is in [-255, 255] which cannot overflow i32.
                return i32::from(a).wrapping_sub(i32::from(b));
            }

            // At least one is a digit.  Walk back to find the start of
            // the digit run that includes position `i`.
            let mut start = i;
            while start > 0 && unsafe { *s1.add(start.wrapping_sub(1)) }.is_ascii_digit() {
                start = start.wrapping_sub(1);
            }

            // If we're NOT inside a digit run (start == i) and only one
            // side has a digit, fall back to plain byte comparison.  This
            // matches glibc's state-machine behaviour in state S_N (normal):
            // a lone digit vs a letter is compared by code point value.
            if start == i && (!a_dig || !b_dig) {
                return i32::from(a).wrapping_sub(i32::from(b));
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
            // SAFETY: a and b are u8, so their i32 values are in [0, 255];
            // the difference is in [-255, 255] which cannot overflow i32.
            return i32::from(a).wrapping_sub(i32::from(b));
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
            // SAFETY: a and b are u8, so their i32 values are in [0, 255];
            // the difference is in [-255, 255] which cannot overflow i32.
            first_diff = i32::from(a).wrapping_sub(i32::from(b));
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strcoll_l(s1: *const u8, s2: *const u8, _locale: usize) -> i32 {
    unsafe { strcmp(s1, s2) }
}

/// Transform a string for locale-aware comparison (locale variant).
///
/// Since we only support the C locale, delegates to `strxfrm`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strxfrm_l(dest: *mut u8, src: *const u8, n: usize, _locale: usize) -> usize {
    unsafe { strxfrm(dest, src, n) }
}

/// Locale-aware `strerror`.
///
/// Returns the same result as `strerror` (locale is ignored).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn strerror_l(errnum: i32, _locale: usize) -> *const u8 {
    strerror(errnum)
}

/// XPG variant of `strerror_r`.
///
/// Some glibc-compiled programs reference `__xpg_strerror_r` instead
/// of the GNU-specific `strerror_r`.  The XPG version returns 0 on
/// success and an error code on failure (same as our `strerror_r`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    // -- strlen tests --

    #[test]
    fn test_strlen_basic() {
        assert_eq!(unsafe { strlen(b"hello\0".as_ptr()) }, 5);
        assert_eq!(unsafe { strlen(b"\0".as_ptr()) }, 0);
        assert_eq!(unsafe { strlen(b"a\0".as_ptr()) }, 1);
    }

    #[test]
    fn test_strnlen_basic() {
        assert_eq!(unsafe { strnlen(b"hello\0".as_ptr(), 10) }, 5);
        assert_eq!(unsafe { strnlen(b"hello\0".as_ptr(), 3) }, 3);
        assert_eq!(unsafe { strnlen(b"hello\0".as_ptr(), 0) }, 0);
    }

    // -- strcmp / strncmp tests --

    #[test]
    fn test_strcmp_basic() {
        assert_eq!(unsafe { strcmp(b"abc\0".as_ptr(), b"abc\0".as_ptr()) }, 0);
        assert!(unsafe { strcmp(b"abc\0".as_ptr(), b"abd\0".as_ptr()) } < 0);
        assert!(unsafe { strcmp(b"abd\0".as_ptr(), b"abc\0".as_ptr()) } > 0);
        assert!(unsafe { strcmp(b"ab\0".as_ptr(), b"abc\0".as_ptr()) } < 0);
    }

    #[test]
    fn test_strncmp_basic() {
        assert_eq!(
            unsafe { strncmp(b"abc\0".as_ptr(), b"abd\0".as_ptr(), 2) },
            0
        );
        assert!(unsafe { strncmp(b"abc\0".as_ptr(), b"abd\0".as_ptr(), 3) } < 0);
        assert_eq!(
            unsafe { strncmp(b"abc\0".as_ptr(), b"xyz\0".as_ptr(), 0) },
            0
        );
    }

    // -- strcasecmp tests --

    #[test]
    fn test_strcasecmp_basic() {
        assert_eq!(
            unsafe { strcasecmp(b"Hello\0".as_ptr(), b"hello\0".as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { strcasecmp(b"ABC\0".as_ptr(), b"abc\0".as_ptr()) },
            0
        );
        assert!(unsafe { strcasecmp(b"a\0".as_ptr(), b"B\0".as_ptr()) } < 0);
    }

    // -- strchr / strrchr tests --

    #[test]
    fn test_strchr_found() {
        let s = b"hello world\0";
        let p = unsafe { strchr(s.as_ptr(), i32::from(b'o')) };
        assert!(!p.is_null());
        assert_eq!(unsafe { *p }, b'o');
        // First occurrence: should be at position 4.
        let offset = (p as usize).wrapping_sub(s.as_ptr() as usize);
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_strchr_not_found() {
        let s = b"hello\0";
        let p = unsafe { strchr(s.as_ptr(), i32::from(b'z')) };
        assert!(p.is_null());
    }

    #[test]
    fn test_strchr_null_terminator() {
        // strchr should find the null terminator.
        let s = b"abc\0";
        let p = unsafe { strchr(s.as_ptr(), 0) };
        assert!(!p.is_null());
        assert_eq!(unsafe { *p }, 0);
    }

    #[test]
    fn test_strrchr_found() {
        let s = b"hello world\0";
        let p = unsafe { strrchr(s.as_ptr(), i32::from(b'o')) };
        assert!(!p.is_null());
        // Last 'o' is at position 7 ("world").
        let offset = (p as usize).wrapping_sub(s.as_ptr() as usize);
        assert_eq!(offset, 7);
    }

    // -- strstr / strcasestr tests --

    #[test]
    fn test_strstr_found() {
        let hay = b"hello world\0";
        let needle = b"world\0";
        let p = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(hay.as_ptr() as usize);
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_strstr_not_found() {
        let hay = b"hello\0";
        let needle = b"xyz\0";
        let p = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(p.is_null());
    }

    #[test]
    fn test_strstr_empty_needle() {
        let hay = b"hello\0";
        let needle = b"\0";
        let p = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        // Empty needle matches at start.
        assert!(!p.is_null());
        assert_eq!(p, hay.as_ptr());
    }

    #[test]
    fn test_strcasestr_found() {
        let hay = b"Hello World\0";
        let needle = b"world\0";
        let p = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(hay.as_ptr() as usize);
        assert_eq!(offset, 6);
    }

    // -- strspn / strcspn tests --

    #[test]
    fn test_strspn_basic() {
        let s = b"aabbc123\0";
        let accept = b"abc\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 5);
    }

    #[test]
    fn test_strcspn_basic() {
        let s = b"hello123\0";
        let reject = b"0123456789\0";
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 5);
    }

    // -- strpbrk tests --

    #[test]
    fn test_strpbrk_found() {
        let s = b"hello world\0";
        let accept = b"wrd\0";
        let p = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!p.is_null());
        // First match is 'r' at position... actually 'w' at 6, 'r' at 8, 'd' at 10.
        // strpbrk returns first match in s. Let's check.
        assert_eq!(unsafe { *p }, b'w');
    }

    // -- memcmp tests --

    #[test]
    fn test_memcmp_equal() {
        let a = b"hello";
        let b_arr = b"hello";
        assert_eq!(
            unsafe { memcmp(a.as_ptr().cast(), b_arr.as_ptr().cast(), 5) },
            0
        );
    }

    #[test]
    fn test_memcmp_less() {
        let a = b"abc";
        let b_arr = b"abd";
        assert!(unsafe { memcmp(a.as_ptr().cast(), b_arr.as_ptr().cast(), 3) } < 0);
    }

    #[test]
    fn test_memcmp_zero_length() {
        let a = b"abc";
        let b_arr = b"xyz";
        assert_eq!(
            unsafe { memcmp(a.as_ptr().cast(), b_arr.as_ptr().cast(), 0) },
            0
        );
    }

    // -- memchr / memrchr tests --

    #[test]
    fn test_memchr_found() {
        let data = b"abcdef";
        let p = unsafe { memchr(data.as_ptr().cast(), i32::from(b'd'), 6) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(data.as_ptr() as usize);
        assert_eq!(offset, 3);
    }

    #[test]
    fn test_memchr_not_found() {
        let data = b"abcdef";
        let p = unsafe { memchr(data.as_ptr().cast(), i32::from(b'z'), 6) };
        assert!(p.is_null());
    }

    #[test]
    fn test_memrchr_found() {
        let data = b"abcabc";
        let p = unsafe { memrchr(data.as_ptr().cast(), i32::from(b'a'), 6) };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(data.as_ptr() as usize);
        assert_eq!(offset, 3); // Last 'a' is at index 3.
    }

    // -- memmem tests --

    #[test]
    fn test_memmem_found() {
        let hay = b"hello world";
        let needle = b"world";
        let p = unsafe {
            memmem(
                hay.as_ptr().cast(),
                11,
                needle.as_ptr().cast(),
                5,
            )
        };
        assert!(!p.is_null());
        let offset = (p as usize).wrapping_sub(hay.as_ptr() as usize);
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_memmem_empty_needle() {
        let hay = b"hello";
        let p = unsafe {
            memmem(hay.as_ptr().cast(), 5, hay.as_ptr().cast(), 0)
        };
        // Empty needle returns haystack start.
        assert_eq!(p, hay.as_ptr().cast());
    }

    // -- strtok_r tests --

    #[test]
    fn test_strtok_r_basic() {
        let mut buf = *b"hello,world,foo\0";
        let delim = b",\0";
        let mut saveptr: *mut u8 = core::ptr::null_mut();

        let tok1 = unsafe {
            strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &mut saveptr)
        };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 5); // "hello"

        let tok2 = unsafe {
            strtok_r(core::ptr::null_mut(), delim.as_ptr(), &mut saveptr)
        };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 5); // "world"

        let tok3 = unsafe {
            strtok_r(core::ptr::null_mut(), delim.as_ptr(), &mut saveptr)
        };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 3); // "foo"

        let tok4 = unsafe {
            strtok_r(core::ptr::null_mut(), delim.as_ptr(), &mut saveptr)
        };
        assert!(tok4.is_null()); // No more tokens.
    }

    // -- ffs tests --

    #[test]
    fn test_ffs_basic() {
        assert_eq!(ffs(0), 0);
        assert_eq!(ffs(1), 1);    // bit 0 set
        assert_eq!(ffs(2), 2);    // bit 1 set
        assert_eq!(ffs(4), 3);    // bit 2 set
        assert_eq!(ffs(6), 2);    // bits 1 and 2 set, first is bit 1
        assert_eq!(ffs(-1), 1);   // all bits set, first is bit 0
    }

    // -- strlcpy / strlcat tests --

    #[test]
    fn test_strlcpy_basic() {
        let mut dst = [0u8; 10];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 5);
        assert_eq!(&dst[..6], b"hello\0");
    }

    #[test]
    fn test_strlcpy_truncation() {
        let mut dst = [0u8; 4];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(len, 5); // Returns full src length.
        assert_eq!(&dst[..4], b"hel\0"); // Truncated but null-terminated.
    }

    #[test]
    fn test_strlcat_basic() {
        let mut dst = [0u8; 20];
        dst[..6].copy_from_slice(b"hello\0");
        let src = b" world\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 20) };
        assert_eq!(len, 11); // 5 + 6
        assert_eq!(&dst[..12], b"hello world\0");
    }

    // -----------------------------------------------------------------------
    // memcpy / memmove edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memcpy_zero_length() {
        let src = [1u8, 2, 3];
        let mut dst = [0u8; 3];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(dst, [0, 0, 0], "zero-length memcpy should not modify dst");
    }

    #[test]
    fn test_memmove_zero_length() {
        let mut buf = [1u8, 2, 3];
        unsafe { memmove(buf.as_mut_ptr(), buf.as_ptr(), 0) };
        assert_eq!(buf, [1, 2, 3], "zero-length memmove should not modify");
    }

    #[test]
    fn test_memmove_overlap_forward() {
        // Overlapping copy where dst > src.
        let mut buf = [1u8, 2, 3, 4, 5, 0, 0];
        unsafe { memmove(buf.as_mut_ptr().add(2), buf.as_ptr(), 5) };
        assert_eq!(buf, [1, 2, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_memmove_overlap_backward() {
        // Overlapping copy where dst < src.
        let mut buf = [0u8, 0, 1, 2, 3, 4, 5];
        unsafe { memmove(buf.as_mut_ptr(), buf.as_ptr().add(2), 5) };
        assert_eq!(buf, [1, 2, 3, 4, 5, 4, 5]);
    }

    #[test]
    fn test_memcpy_single_byte() {
        let src = [0xABu8];
        let mut dst = [0u8];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 1) };
        assert_eq!(dst[0], 0xAB);
    }

    // -----------------------------------------------------------------------
    // strncpy edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncpy_pads_with_nuls() {
        // POSIX: strncpy pads remainder with NUL bytes.
        let mut dst = [0xFFu8; 10];
        let src = b"hi\0";
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(&dst, b"hi\0\0\0\0\0\0\0\0");
    }

    #[test]
    fn test_strncpy_exact_length_no_nul() {
        // POSIX: strncpy does NOT add NUL if src is >= n chars long.
        let mut dst = [0xFFu8; 3];
        let src = b"abcde\0";
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(dst, [b'a', b'b', b'c']);
    }

    #[test]
    fn test_strncpy_zero_n() {
        let mut dst = [0xFFu8; 3];
        let src = b"abc\0";
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(dst, [0xFF, 0xFF, 0xFF], "zero n should not modify dst");
    }

    // -----------------------------------------------------------------------
    // strncat edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncat_always_nul_terminates() {
        let mut dst = [0u8; 10];
        dst[0] = b'A';
        dst[1] = 0;
        let src = b"BCDEF\0";
        unsafe { strncat(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(&dst[..5], b"ABCD\0");
    }

    #[test]
    fn test_strncat_zero_n() {
        let mut dst = [0u8; 10];
        dst[..4].copy_from_slice(b"abc\0");
        let src = b"xyz\0";
        unsafe { strncat(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(&dst[..4], b"abc\0", "zero n should append nothing");
    }

    // -----------------------------------------------------------------------
    // memcmp edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memcmp_same_bytes() {
        let a = b"hello";
        let b = b"hello";
        assert_eq!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 5) }, 0);
    }

    #[test]
    fn test_memcmp_ordering_less() {
        let a = b"abcde";
        let b = b"abcdf";
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 5) } < 0);
    }

    #[test]
    fn test_memcmp_ordering_greater() {
        let a = b"abcdf";
        let b = b"abcde";
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 5) } > 0);
    }

    #[test]
    fn test_memcmp_zero_len() {
        let a = b"abc";
        let b = b"xyz";
        assert_eq!(
            unsafe { memcmp(a.as_ptr(), b.as_ptr(), 0) },
            0,
            "zero-length memcmp should return 0"
        );
    }

    // -----------------------------------------------------------------------
    // strstr additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strstr_needle_at_start() {
        let hay = b"hello world\0";
        let needle = b"hello\0";
        let result = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(result, hay.as_ptr());
    }

    #[test]
    fn test_strstr_needle_at_end() {
        let hay = b"hello world\0";
        let needle = b"world\0";
        let result = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!result.is_null());
        assert_eq!(unsafe { *result }, b'w');
    }

    // -----------------------------------------------------------------------
    // memset edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memset_zero_length() {
        let mut buf = [0xFFu8; 4];
        unsafe { memset(buf.as_mut_ptr(), 0, 0) };
        assert_eq!(buf, [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_memset_full_buffer() {
        let mut buf = [0u8; 8];
        unsafe { memset(buf.as_mut_ptr(), 0xAB, 8) };
        assert_eq!(buf, [0xAB; 8]);
    }

    // -----------------------------------------------------------------------
    // swab
    // -----------------------------------------------------------------------

    #[test]
    fn test_swab_basic() {
        let src = [1u8, 2, 3, 4];
        let mut dst = [0u8; 4];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 4) };
        assert_eq!(dst, [2, 1, 4, 3]);
    }

    #[test]
    fn test_swab_odd_length() {
        // Odd trailing byte is not swapped.
        let src = [1u8, 2, 3, 4, 5];
        let mut dst = [0u8; 5];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 5) };
        // Only 2 complete pairs swapped.
        assert_eq!(&dst[..4], &[2, 1, 4, 3]);
    }

    #[test]
    fn test_swab_zero_length() {
        let src = [1u8, 2];
        let mut dst = [0xFFu8; 2];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 0) };
        assert_eq!(dst, [0xFF, 0xFF], "zero length should not modify dst");
    }

    // -----------------------------------------------------------------------
    // strlcpy edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strlcpy_size_zero() {
        // size=0: no bytes written, returns src length.
        let mut dst = [0xFFu8; 4];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(len, 5);
        assert_eq!(dst, [0xFF, 0xFF, 0xFF, 0xFF], "size=0 must not write");
    }

    #[test]
    fn test_strlcpy_size_one() {
        // size=1: only NUL byte written.
        let mut dst = [0xFFu8; 4];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 1) };
        assert_eq!(len, 5);
        assert_eq!(dst[0], 0, "size=1 must write only NUL");
        assert_eq!(dst[1], 0xFF);
    }

    #[test]
    fn test_strlcpy_exact_fit() {
        // Buffer exactly big enough: src_len < size.
        let mut dst = [0xFFu8; 6];
        let src = b"hello\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 6) };
        assert_eq!(len, 5);
        assert_eq!(&dst[..6], b"hello\0");
    }

    #[test]
    fn test_strlcpy_empty_src() {
        let mut dst = [0xFFu8; 4];
        let src = b"\0";
        let len = unsafe { strlcpy(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(len, 0);
        assert_eq!(dst[0], 0);
    }

    // -----------------------------------------------------------------------
    // strlcat edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strlcat_truncation() {
        // dst="hello", src=" world", size=8 → truncated to "hello w\0".
        let mut dst = [0u8; 8];
        dst[..6].copy_from_slice(b"hello\0");
        let src = b" world\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 8) };
        // Returns dst_len + src_len = 5 + 6 = 11 (truncation: 11 >= 8).
        assert_eq!(len, 11);
        assert_eq!(&dst[..8], b"hello w\0");
    }

    #[test]
    fn test_strlcat_dst_fills_buffer() {
        // dst already fills the buffer (no NUL within size).
        let mut dst = [b'X'; 4]; // No NUL within first 4 bytes.
        let src = b"abc\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 4) };
        // strnlen(dst, 4) = 4 >= size=4 → returns size + src_len = 4 + 3 = 7.
        assert_eq!(len, 7);
        // dst unchanged (no room to append).
        assert_eq!(dst, [b'X', b'X', b'X', b'X']);
    }

    #[test]
    fn test_strlcat_size_zero() {
        let mut dst = [0u8; 4];
        dst[..4].copy_from_slice(b"hi\0\0");
        let src = b"there\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 0) };
        // size=0 → strnlen(dst,0) = 0 >= 0 → returns 0 + 5 = 5.
        assert_eq!(len, 5);
    }

    #[test]
    fn test_strlcat_empty_src() {
        let mut dst = [0u8; 10];
        dst[..4].copy_from_slice(b"abc\0");
        let src = b"\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 3); // 3 + 0
        assert_eq!(&dst[..4], b"abc\0");
    }

    #[test]
    fn test_strlcat_empty_dst() {
        let mut dst = [0u8; 10];
        let src = b"hello\0";
        let len = unsafe { strlcat(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 5); // 0 + 5
        assert_eq!(&dst[..6], b"hello\0");
    }

    // -----------------------------------------------------------------------
    // strsep edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strsep_basic() {
        let mut data = *b"one,two,three\0";
        let mut ptr: *mut u8 = data.as_mut_ptr();

        let tok1 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 3);
        assert_eq!(unsafe { *tok1 }, b'o');

        let tok2 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 3);
        assert_eq!(unsafe { *tok2 }, b't');

        let tok3 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 5);
        assert_eq!(unsafe { *tok3 }, b't');

        // No more tokens.
        let tok4 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(tok4.is_null());
    }

    #[test]
    fn test_strsep_empty_tokens() {
        // ",,a" → empty, empty, "a"
        let mut data = *b",,a\0";
        let mut ptr: *mut u8 = data.as_mut_ptr();

        let tok1 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 0); // empty token

        let tok2 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 0); // empty token

        let tok3 = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 1); // "a"
        assert_eq!(unsafe { *tok3 }, b'a');
    }

    #[test]
    fn test_strsep_no_delimiter() {
        let mut data = *b"hello\0";
        let mut ptr: *mut u8 = data.as_mut_ptr();

        let tok = unsafe { strsep(&raw mut ptr, b",\0".as_ptr()) };
        assert!(!tok.is_null());
        assert_eq!(unsafe { strlen(tok) }, 5);
        assert!(ptr.is_null()); // no more tokens
    }

    #[test]
    fn test_strsep_null_stringp() {
        let result = unsafe { strsep(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // stpcpy / stpncpy return value tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_stpcpy_returns_nul() {
        let mut dst = [0u8; 10];
        let src = b"test\0";
        let end = unsafe { stpcpy(dst.as_mut_ptr(), src.as_ptr()) };
        // end should point to the NUL terminator.
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(4) });
        assert_eq!(unsafe { *end }, 0);
        assert_eq!(&dst[..5], b"test\0");
    }

    #[test]
    fn test_stpcpy_empty_src() {
        let mut dst = [0xFFu8; 4];
        let src = b"\0";
        let end = unsafe { stpcpy(dst.as_mut_ptr(), src.as_ptr()) };
        assert_eq!(end, dst.as_mut_ptr()); // Points to dst[0].
        assert_eq!(unsafe { *end }, 0);
    }

    #[test]
    fn test_stpncpy_returns_first_nul() {
        let mut dst = [0xFFu8; 10];
        let src = b"hi\0";
        let end = unsafe { stpncpy(dst.as_mut_ptr(), src.as_ptr(), 10) };
        // Should return pointer to first NUL (at dst+2).
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(2) });
        assert_eq!(unsafe { *end }, 0);
        // Remainder should be zero-filled.
        for j in 2..10 {
            assert_eq!(dst[j], 0);
        }
    }

    #[test]
    fn test_stpncpy_no_nul_in_n() {
        // src longer than n: returns dst+n, no NUL written.
        let mut dst = [0xFFu8; 3];
        let src = b"abcdef\0";
        let end = unsafe { stpncpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(3) });
        assert_eq!(dst, [b'a', b'b', b'c']);
    }

    // -----------------------------------------------------------------------
    // strerror_r edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strerror_r_success() {
        let mut buf = [0u8; 64];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 64) };
        assert_eq!(ret, 0);
        assert_eq!(unsafe { strlen(buf.as_ptr()) }, 7); // "Success"
    }

    #[test]
    fn test_strerror_r_truncation() {
        let mut buf = [0u8; 4];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 4) };
        assert_eq!(ret, crate::errno::ERANGE);
        assert_eq!(&buf[..4], b"Suc\0");
    }

    #[test]
    fn test_strerror_r_exact_fit() {
        // "Success" is 7 chars, buffer of 8 = exact fit with NUL.
        let mut buf = [0xFFu8; 8];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 8) };
        assert_eq!(ret, 0);
        assert_eq!(&buf[..8], b"Success\0");
    }

    #[test]
    fn test_strerror_r_null_buf() {
        let ret = unsafe { strerror_r(0, core::ptr::null_mut(), 64) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_strerror_r_zero_buflen() {
        let mut buf = [0xFFu8; 4];
        let ret = unsafe { strerror_r(0, buf.as_mut_ptr(), 0) };
        assert_eq!(ret, crate::errno::ERANGE);
        assert_eq!(buf[0], 0xFF, "buflen=0 must not write");
    }

    // -----------------------------------------------------------------------
    // memccpy edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memccpy_byte_found() {
        let src = b"hello world";
        let mut dst = [0u8; 11];
        let ret = unsafe {
            memccpy(dst.as_mut_ptr(), src.as_ptr(), b' ' as i32, 11)
        };
        // Should find ' ' at index 5, copy through it, return dst+6.
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { dst.as_mut_ptr().add(6) });
        assert_eq!(&dst[..6], b"hello ");
    }

    #[test]
    fn test_memccpy_byte_not_found() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        let ret = unsafe {
            memccpy(dst.as_mut_ptr(), src.as_ptr(), b'x' as i32, 5)
        };
        assert!(ret.is_null());
        assert_eq!(&dst[..5], b"hello");
    }

    #[test]
    fn test_memccpy_first_byte() {
        let src = b"abcd";
        let mut dst = [0u8; 4];
        let ret = unsafe {
            memccpy(dst.as_mut_ptr(), src.as_ptr(), b'a' as i32, 4)
        };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { dst.as_mut_ptr().add(1) });
        assert_eq!(dst[0], b'a');
    }

    // -----------------------------------------------------------------------
    // explicit_bzero
    // -----------------------------------------------------------------------

    #[test]
    fn test_explicit_bzero_zeroes_buffer() {
        let mut buf = [0xABu8; 16];
        unsafe { explicit_bzero(buf.as_mut_ptr(), 16) };
        assert_eq!(buf, [0u8; 16]);
    }

    #[test]
    fn test_explicit_bzero_partial() {
        let mut buf = [0xFFu8; 8];
        unsafe { explicit_bzero(buf.as_mut_ptr(), 4) };
        assert_eq!(&buf[..4], &[0, 0, 0, 0]);
        assert_eq!(&buf[4..], &[0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_explicit_bzero_zero_len() {
        let mut buf = [0xFFu8; 4];
        unsafe { explicit_bzero(buf.as_mut_ptr(), 0) };
        assert_eq!(buf, [0xFF; 4], "zero length should not modify");
    }

    // -----------------------------------------------------------------------
    // mempcpy
    // -----------------------------------------------------------------------

    #[test]
    fn test_mempcpy_returns_past_end() {
        let src = b"abc";
        let mut dst = [0u8; 5];
        let end = unsafe { mempcpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(end, unsafe { dst.as_mut_ptr().add(3) });
        assert_eq!(&dst[..3], b"abc");
    }

    #[test]
    fn test_mempcpy_zero_length() {
        let src = b"abc";
        let mut dst = [0xFFu8; 3];
        let end = unsafe { mempcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(end, dst.as_mut_ptr()); // Returns dest+0.
        assert_eq!(dst, [0xFF; 3], "zero-length should not modify");
    }

    // -----------------------------------------------------------------------
    // memmem edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memmem_needle_at_end() {
        let hay = b"abcdef";
        let needle = b"def";
        let ret = unsafe { memmem(hay.as_ptr(), 6, needle.as_ptr(), 3) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { hay.as_ptr().add(3) });
    }

    #[test]
    fn test_memmem_needle_longer() {
        let hay = b"abc";
        let needle = b"abcdef";
        let ret = unsafe { memmem(hay.as_ptr(), 3, needle.as_ptr(), 6) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_memmem_single_byte_match() {
        let hay = b"abcde";
        let needle = b"c";
        let ret = unsafe { memmem(hay.as_ptr(), 5, needle.as_ptr(), 1) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { hay.as_ptr().add(2) });
    }

    #[test]
    fn test_memmem_no_match() {
        let hay = b"abcde";
        let needle = b"xyz";
        let ret = unsafe { memmem(hay.as_ptr(), 5, needle.as_ptr(), 3) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // strcasecmp / strncasecmp edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strcasecmp_case_insensitive() {
        assert_eq!(unsafe { strcasecmp(b"Hello\0".as_ptr(), b"hello\0".as_ptr()) }, 0);
        assert_eq!(unsafe { strcasecmp(b"ABC\0".as_ptr(), b"abc\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_strcasecmp_different() {
        assert!(unsafe { strcasecmp(b"abc\0".as_ptr(), b"abd\0".as_ptr()) } < 0);
        assert!(unsafe { strcasecmp(b"abd\0".as_ptr(), b"abc\0".as_ptr()) } > 0);
    }

    #[test]
    fn test_strncasecmp_limited() {
        // First 3 chars match case-insensitively, differ at char 4.
        assert_eq!(
            unsafe { strncasecmp(b"ABCx\0".as_ptr(), b"abcy\0".as_ptr(), 3) },
            0
        );
        assert!(
            unsafe { strncasecmp(b"ABCx\0".as_ptr(), b"abcy\0".as_ptr(), 4) } < 0
        );
    }

    #[test]
    fn test_strncasecmp_zero_n() {
        // n=0: always returns 0.
        assert_eq!(
            unsafe { strncasecmp(b"abc\0".as_ptr(), b"xyz\0".as_ptr(), 0) },
            0
        );
    }

    // -----------------------------------------------------------------------
    // ffs / ffsl / ffsll edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffs_powers_of_two() {
        assert_eq!(ffs(1), 1);
        assert_eq!(ffs(2), 2);
        assert_eq!(ffs(4), 3);
        assert_eq!(ffs(8), 4);
        assert_eq!(ffs(16), 5);
        assert_eq!(ffs(256), 9);
        assert_eq!(ffs(1024), 11);
    }

    #[test]
    fn test_ffs_i32_min() {
        // i32::MIN = 0x80000000, LSB set at bit 31 → ffs returns 32.
        assert_eq!(ffs(i32::MIN), 32);
    }

    #[test]
    fn test_ffsl_basic() {
        assert_eq!(ffsl(0), 0);
        assert_eq!(ffsl(1), 1);
        assert_eq!(ffsl(0x100), 9);
    }

    #[test]
    fn test_ffsl_i64_min() {
        // i64::MIN = 0x8000000000000000, bit 63 → ffs returns 64.
        assert_eq!(ffsl(i64::MIN), 64);
    }

    #[test]
    fn test_ffsll_matches_ffsl() {
        assert_eq!(ffsll(0), ffsl(0));
        assert_eq!(ffsll(42), ffsl(42));
        assert_eq!(ffsll(i64::MIN), ffsl(i64::MIN));
    }

    // -----------------------------------------------------------------------
    // memrchr edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memrchr_last_occurrence() {
        let buf = b"abcabc";
        let ret = unsafe { memrchr(buf.as_ptr(), b'a' as i32, 6) };
        assert!(!ret.is_null());
        // Should find the LAST 'a', at index 3.
        assert_eq!(ret, unsafe { buf.as_ptr().add(3) });
    }

    #[test]
    fn test_memrchr_not_found() {
        let buf = b"abcdef";
        let ret = unsafe { memrchr(buf.as_ptr(), b'x' as i32, 6) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_memrchr_zero_length() {
        let buf = b"abc";
        let ret = unsafe { memrchr(buf.as_ptr(), b'a' as i32, 0) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // strcasestr edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strcasestr_not_found() {
        let hay = b"Hello World\0";
        let needle = b"xyz\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strcasestr_empty_needle() {
        let hay = b"Hello\0";
        let needle = b"\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        assert_eq!(ret, hay.as_ptr().cast_mut());
    }

    #[test]
    fn test_strcasestr_mixed_case() {
        let hay = b"The Quick Brown Fox\0";
        let needle = b"BROWN\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *ret }, b'B');
    }

    // -----------------------------------------------------------------------
    // strtok_r additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtok_r_all_delimiters() {
        // String is all delimiters — should return NULL immediately.
        let mut data = *b",,,,\0";
        let mut save: *mut u8 = core::ptr::null_mut();
        let tok = unsafe {
            strtok_r(data.as_mut_ptr(), b",\0".as_ptr(), &raw mut save)
        };
        assert!(tok.is_null());
    }

    #[test]
    fn test_strtok_r_single_token() {
        let mut data = *b"hello\0";
        let mut save: *mut u8 = core::ptr::null_mut();
        let tok = unsafe {
            strtok_r(data.as_mut_ptr(), b",\0".as_ptr(), &raw mut save)
        };
        assert!(!tok.is_null());
        assert_eq!(unsafe { strlen(tok) }, 5);

        let tok2 = unsafe {
            strtok_r(core::ptr::null_mut(), b",\0".as_ptr(), &raw mut save)
        };
        assert!(tok2.is_null());
    }

    #[test]
    fn test_strtok_r_multiple_delimiters() {
        // Multiple delimiter characters.
        let mut data = *b"one;two,three\0";
        let mut save: *mut u8 = core::ptr::null_mut();
        let tok1 = unsafe {
            strtok_r(data.as_mut_ptr(), b",;\0".as_ptr(), &raw mut save)
        };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { strlen(tok1) }, 3);

        let tok2 = unsafe {
            strtok_r(core::ptr::null_mut(), b",;\0".as_ptr(), &raw mut save)
        };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { strlen(tok2) }, 3);

        let tok3 = unsafe {
            strtok_r(core::ptr::null_mut(), b",;\0".as_ptr(), &raw mut save)
        };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { strlen(tok3) }, 5);
    }

    // -----------------------------------------------------------------------
    // strverscmp additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strverscmp_empty_strings() {
        assert_eq!(ver(b"\0", b"\0"), 0);
    }

    #[test]
    fn test_strverscmp_one_empty() {
        assert!(ver(b"\0", b"a\0") < 0);
        assert!(ver(b"a\0", b"\0") > 0);
    }

    #[test]
    fn test_strverscmp_just_digits() {
        assert!(ver(b"9\0", b"10\0") < 0);
        assert!(ver(b"100\0", b"99\0") > 0);
    }

    // -----------------------------------------------------------------------
    // strerror
    // -----------------------------------------------------------------------

    #[test]
    fn test_strerror_known_codes() {
        // Spot-check a few well-known errno values against their Linux strings.
        let msg = |n: i32| {
            let p = strerror(n);
            assert!(!p.is_null());
            let len = unsafe { strlen(p) };
            unsafe { core::slice::from_raw_parts(p, len) }
        };
        assert_eq!(msg(0), b"Success");
        assert_eq!(msg(1), b"Operation not permitted");     // EPERM
        assert_eq!(msg(2), b"No such file or directory");   // ENOENT
        assert_eq!(msg(9), b"Bad file descriptor");         // EBADF
        assert_eq!(msg(12), b"Cannot allocate memory");     // ENOMEM
        assert_eq!(msg(13), b"Permission denied");          // EACCES
        assert_eq!(msg(22), b"Invalid argument");           // EINVAL
        assert_eq!(msg(32), b"Broken pipe");                // EPIPE
        assert_eq!(msg(111), b"Connection refused");        // ECONNREFUSED
    }

    #[test]
    fn test_strerror_unknown_code() {
        let p = strerror(9999);
        let len = unsafe { strlen(p) };
        let msg = unsafe { core::slice::from_raw_parts(p, len) };
        assert_eq!(msg, b"Unknown error");
    }

    #[test]
    fn test_strerror_negative() {
        // Negative codes should also return "Unknown error".
        let p = strerror(-1);
        let len = unsafe { strlen(p) };
        let msg = unsafe { core::slice::from_raw_parts(p, len) };
        assert_eq!(msg, b"Unknown error");
    }

    // -----------------------------------------------------------------------
    // strcoll — locale-aware string comparison (C locale = strcmp)
    // -----------------------------------------------------------------------

    #[test]
    fn test_strcoll_equal() {
        assert_eq!(unsafe { strcoll(b"abc\0".as_ptr(), b"abc\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_strcoll_ordering() {
        assert!(unsafe { strcoll(b"abc\0".as_ptr(), b"abd\0".as_ptr()) } < 0);
        assert!(unsafe { strcoll(b"abd\0".as_ptr(), b"abc\0".as_ptr()) } > 0);
    }

    #[test]
    fn test_strcoll_empty() {
        assert_eq!(unsafe { strcoll(b"\0".as_ptr(), b"\0".as_ptr()) }, 0);
        assert!(unsafe { strcoll(b"\0".as_ptr(), b"a\0".as_ptr()) } < 0);
    }

    // -----------------------------------------------------------------------
    // strxfrm — locale-aware string transform (C locale = copy)
    // -----------------------------------------------------------------------

    #[test]
    fn test_strxfrm_basic() {
        let src = b"hello\0";
        let mut dst = [0u8; 10];
        let len = unsafe { strxfrm(dst.as_mut_ptr(), src.as_ptr(), 10) };
        assert_eq!(len, 5); // "hello" length
        assert_eq!(&dst[..5], b"hello");
        assert_eq!(dst[5], 0); // null terminated by strncpy
    }

    #[test]
    fn test_strxfrm_zero_n() {
        // When n=0, strxfrm should just return the length needed.
        let src = b"test\0";
        let len = unsafe { strxfrm(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(len, 4);
    }

    #[test]
    fn test_strxfrm_truncation() {
        // strxfrm copies via strncpy: copies n bytes from src.
        // "abcdef" (len 6) with n=4 copies "abcd" (no nul — src has no
        // nul in first 4 bytes, so strncpy does not null-terminate).
        let src = b"abcdef\0";
        let mut dst = [0xFFu8; 4];
        let len = unsafe { strxfrm(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(len, 6); // Full source length returned.
        assert_eq!(&dst[..4], b"abcd"); // Truncated copy.
    }

    // -----------------------------------------------------------------------
    // strchrnul — like strchr but returns pointer to NUL if not found
    // -----------------------------------------------------------------------

    #[test]
    fn test_strchrnul_found() {
        let s = b"hello world\0";
        let ret = unsafe { strchrnul(s.as_ptr(), b'w' as i32) };
        assert_eq!(ret, unsafe { s.as_ptr().add(6) });
    }

    #[test]
    fn test_strchrnul_not_found() {
        // Should return pointer to NUL terminator, not null.
        let s = b"hello\0";
        let ret = unsafe { strchrnul(s.as_ptr(), b'x' as i32) };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *ret }, 0); // Points to NUL
        assert_eq!(ret, unsafe { s.as_ptr().add(5) });
    }

    #[test]
    fn test_strchrnul_nul_char() {
        // Searching for NUL should return pointer to NUL terminator.
        let s = b"abc\0";
        let ret = unsafe { strchrnul(s.as_ptr(), 0) };
        assert_eq!(ret, unsafe { s.as_ptr().add(3) });
    }

    #[test]
    fn test_strchrnul_first_char() {
        let s = b"abc\0";
        let ret = unsafe { strchrnul(s.as_ptr(), b'a' as i32) };
        assert_eq!(ret, s.as_ptr());
    }

    // -----------------------------------------------------------------------
    // rawmemchr — memchr without length bound
    // -----------------------------------------------------------------------

    #[test]
    fn test_rawmemchr_found() {
        let s = b"find the X here\0";
        let ret = unsafe { rawmemchr(s.as_ptr(), b'X' as i32) };
        assert_eq!(ret, unsafe { s.as_ptr().add(9) });
    }

    #[test]
    fn test_rawmemchr_first_byte() {
        let s = b"abc\0";
        let ret = unsafe { rawmemchr(s.as_ptr(), b'a' as i32) };
        assert_eq!(ret, s.as_ptr());
    }

    #[test]
    fn test_rawmemchr_nul_sentinel() {
        // Common use: find the NUL terminator.
        let s = b"hello\0";
        let ret = unsafe { rawmemchr(s.as_ptr(), 0) };
        assert_eq!(ret, unsafe { s.as_ptr().add(5) });
    }

    // -----------------------------------------------------------------------
    // bcopy / bzero — BSD memory functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_bcopy_basic() {
        let src = b"hello";
        let mut dst = [0u8; 5];
        unsafe { bcopy(src.as_ptr(), dst.as_mut_ptr(), 5) };
        assert_eq!(&dst, b"hello");
    }

    #[test]
    fn test_bcopy_zero_length() {
        let src = b"hello";
        let mut dst = [0xFFu8; 5];
        unsafe { bcopy(src.as_ptr(), dst.as_mut_ptr(), 0) };
        assert_eq!(dst, [0xFF; 5], "zero-length bcopy should not modify");
    }

    #[test]
    fn test_bzero_basic() {
        let mut buf = [0xABu8; 8];
        unsafe { bzero(buf.as_mut_ptr(), 8) };
        assert_eq!(buf, [0; 8]);
    }

    #[test]
    fn test_bzero_zero_length() {
        let mut buf = [0xFFu8; 4];
        unsafe { bzero(buf.as_mut_ptr(), 0) };
        assert_eq!(buf, [0xFF; 4]);
    }

    // -----------------------------------------------------------------------
    // memmem — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memmem_repeated_pattern() {
        // Should find the FIRST occurrence.
        let hay = b"ababab";
        let needle = b"ab";
        let ret = unsafe { memmem(hay.as_ptr(), 6, needle.as_ptr(), 2) };
        assert_eq!(ret, hay.as_ptr()); // First occurrence at index 0.
    }

    #[test]
    fn test_memmem_overlapping_match() {
        // Needle pattern overlaps: "aaa" in "aaaa" — should find at index 0.
        let hay = b"aaaa";
        let needle = b"aaa";
        let ret = unsafe { memmem(hay.as_ptr(), 4, needle.as_ptr(), 3) };
        assert_eq!(ret, hay.as_ptr());
    }

    #[test]
    fn test_memmem_exact_match() {
        // Needle is the entire haystack.
        let hay = b"exact";
        let needle = b"exact";
        let ret = unsafe { memmem(hay.as_ptr(), 5, needle.as_ptr(), 5) };
        assert_eq!(ret, hay.as_ptr());
    }

    #[test]
    fn test_memmem_zero_length_haystack() {
        let needle = b"ab";
        let ret = unsafe { memmem(needle.as_ptr(), 0, needle.as_ptr(), 2) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // strncpy — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncpy_src_shorter_than_n_pads() {
        let src = b"hi\0";
        let mut dst = [0xFFu8; 8];
        unsafe { strncpy(dst.as_mut_ptr(), src.as_ptr(), 8) };
        assert_eq!(&dst[..2], b"hi");
        // Remaining bytes should be zero-padded.
        assert_eq!(&dst[2..], &[0, 0, 0, 0, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // strncat — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strncat_appends_and_terminates() {
        let mut buf = [0u8; 20];
        buf[0] = b'H';
        buf[1] = b'i';
        buf[2] = 0;
        let src = b"!!!\0";
        unsafe { strncat(buf.as_mut_ptr(), src.as_ptr(), 2) };
        assert_eq!(&buf[..5], b"Hi!!\0");
    }

    // -----------------------------------------------------------------------
    // swab — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_swab_negative_nbytes() {
        // Negative nbytes should be a no-op.
        let src = b"abcd";
        let mut dst = [0xFFu8; 4];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), -1) };
        assert_eq!(dst, [0xFF; 4]);
    }

    #[test]
    fn test_swab_single_pair() {
        let src = b"ab";
        let mut dst = [0u8; 2];
        unsafe { swab(src.as_ptr(), dst.as_mut_ptr(), 2) };
        assert_eq!(&dst, b"ba");
    }

    // -----------------------------------------------------------------------
    // sys_nerr constant
    // -----------------------------------------------------------------------

    #[test]
    fn test_sys_nerr_value() {
        // Should be one past the highest defined errno (131 → 132).
        assert_eq!(sys_nerr, 132);
    }

    // -----------------------------------------------------------------------
    // memmove — overlapping edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memmove_overlap_src_equals_dest() {
        let mut buf = *b"hello";
        let p = buf.as_mut_ptr();
        let ret = unsafe { memmove(p, p, 5) };
        assert_eq!(ret, p);
        assert_eq!(&buf, b"hello"); // Unchanged.
    }

    #[test]
    fn test_memmove_large_overlap_backward() {
        // Overlap where dest > src: [0..8] → [2..10].
        let mut buf = [0u8; 10];
        buf[..8].copy_from_slice(b"ABCDEFGH");
        unsafe { memmove(buf.as_mut_ptr().add(2), buf.as_ptr(), 8) };
        assert_eq!(&buf[2..10], b"ABCDEFGH");
    }

    // -----------------------------------------------------------------------
    // memcmp — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_memcmp_single_byte_diff() {
        let a = [0x00u8];
        let b = [0xFFu8];
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 1) } < 0);
        assert!(unsafe { memcmp(b.as_ptr(), a.as_ptr(), 1) } > 0);
    }

    #[test]
    fn test_memcmp_diff_at_last_byte() {
        let a = b"abcx";
        let b = b"abcy";
        assert!(unsafe { memcmp(a.as_ptr(), b.as_ptr(), 4) } < 0);
    }

    // -----------------------------------------------------------------------
    // strspn / strcspn — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strspn_all_match() {
        let s = b"aaaa\0";
        let accept = b"a\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 4);
    }

    #[test]
    fn test_strspn_no_match() {
        let s = b"xyz\0";
        let accept = b"abc\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 0);
    }

    #[test]
    fn test_strspn_empty_string() {
        let s = b"\0";
        let accept = b"abc\0";
        assert_eq!(unsafe { strspn(s.as_ptr(), accept.as_ptr()) }, 0);
    }

    #[test]
    fn test_strcspn_all_reject() {
        let s = b"aaa\0";
        let reject = b"a\0";
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 0);
    }

    #[test]
    fn test_strcspn_no_reject() {
        let s = b"abc\0";
        let reject = b"xyz\0";
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 3);
    }

    #[test]
    fn test_strcspn_empty_reject() {
        let s = b"abc\0";
        let reject = b"\0";
        // Empty reject means no chars are rejected — span the whole string.
        assert_eq!(unsafe { strcspn(s.as_ptr(), reject.as_ptr()) }, 3);
    }

    // -----------------------------------------------------------------------
    // strpbrk — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strpbrk_not_found() {
        let s = b"hello\0";
        let accept = b"xyz\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strpbrk_first_char() {
        let s = b"hello\0";
        let accept = b"h\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, s.as_ptr());
    }

    // -------------------------------------------------------------------
    // Stress tests — strstr
    // -------------------------------------------------------------------

    #[test]
    fn test_strstr_needle_repeated_in_haystack() {
        // Needle appears multiple times — must find the first.
        let hay = b"abcabcabcabc\0";
        let needle = b"abc\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(ret, hay.as_ptr()); // first occurrence at pos 0
    }

    #[test]
    fn test_strstr_needle_at_various_positions() {
        // Build a 200-byte haystack with needle at position 150.
        let mut buf = [b'x'; 201];
        buf[150] = b'N';
        buf[151] = b'D';
        buf[152] = b'L';
        buf[200] = 0;
        let needle = b"NDL\0";
        let ret = unsafe { strstr(buf.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        // Check offset.
        let offset = ret as usize - buf.as_ptr() as usize;
        assert_eq!(offset, 150);
    }

    #[test]
    fn test_strstr_partial_match_then_full() {
        // "aab" in "aaab" — the first "aa" is a partial match for "aab",
        // actual match starts at position 1.
        let hay = b"aaab\0";
        let needle = b"aab\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 1);
    }

    #[test]
    fn test_strstr_needle_equals_haystack() {
        let s = b"hello\0";
        let ret = unsafe { strstr(s.as_ptr(), s.as_ptr()) };
        assert_eq!(ret, s.as_ptr());
    }

    #[test]
    fn test_strstr_needle_longer_than_haystack() {
        let hay = b"hi\0";
        let needle = b"hello\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strstr_single_char_needle() {
        let hay = b"abcde\0";
        let needle = b"d\0";
        let ret = unsafe { strstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 3);
    }

    #[test]
    fn test_strstr_repeated_partial_prefix() {
        // Pathological case: many near-matches before real match.
        // 101 'a's followed by 'b' then NUL: "aaa...aab\0"
        let mut buf = [b'a'; 103];
        buf[101] = b'b';
        buf[102] = 0;
        let needle = b"ab\0";
        let ret = unsafe { strstr(buf.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        // "ab" first occurs at position 100 (the 'a' at [100] followed by 'b' at [101]).
        let offset = ret as usize - buf.as_ptr() as usize;
        assert_eq!(offset, 100);
    }

    // -------------------------------------------------------------------
    // Stress tests — strcasestr
    // -------------------------------------------------------------------

    #[test]
    fn test_strcasestr_stress_full_uppercase() {
        let hay = b"Hello World\0";
        let needle = b"WORLD\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 6);
    }

    #[test]
    fn test_strcasestr_stress_alternating_case() {
        let hay = b"ABCDEFG\0";
        let needle = b"cDe\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - hay.as_ptr() as usize;
        assert_eq!(offset, 2);
    }

    #[test]
    fn test_strcasestr_stress_no_match() {
        let hay = b"hello world\0";
        let needle = b"xyz\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_strcasestr_stress_empty() {
        let hay = b"test\0";
        let needle = b"\0";
        let ret = unsafe { strcasestr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(ret, hay.as_ptr().cast_mut());
    }

    // -------------------------------------------------------------------
    // Stress tests — strtok_r comprehensive
    // -------------------------------------------------------------------

    #[test]
    fn test_strtok_r_consecutive_delimiters() {
        // Multiple consecutive delimiters should be treated as one.
        let mut buf = *b"a,,b,,c\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(!t1.is_null());
        assert_eq!(unsafe { *t1 }, b'a');

        let t2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!t2.is_null());
        assert_eq!(unsafe { *t2 }, b'b');

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!t3.is_null());
        assert_eq!(unsafe { *t3 }, b'c');

        let t4 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(t4.is_null());
    }

    #[test]
    fn test_strtok_r_leading_trailing_delimiters() {
        let mut buf = *b",,hello,,world,,\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(!t1.is_null());
        assert_eq!(unsafe { cstr_eq(t1, b"hello") }, true);

        let t2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!t2.is_null());
        assert_eq!(unsafe { cstr_eq(t2, b"world") }, true);

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(t3.is_null());
    }

    #[test]
    fn test_strtok_r_all_delimiters_only() {
        let mut buf = *b",,,\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(t1.is_null());
    }

    #[test]
    fn test_strtok_r_varying_delimiters() {
        // Change delimiter set between calls.
        let mut buf = *b"a,b:c\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), b",\0".as_ptr(), &raw mut save) };
        assert!(!t1.is_null());
        assert_eq!(unsafe { *t1 }, b'a');

        // Now use ':' as delimiter.
        let t2 = unsafe { strtok_r(core::ptr::null_mut(), b":\0".as_ptr(), &raw mut save) };
        assert!(!t2.is_null());
        assert_eq!(unsafe { cstr_eq(t2, b"b") }, true);

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), b":\0".as_ptr(), &raw mut save) };
        assert!(!t3.is_null());
        assert_eq!(unsafe { *t3 }, b'c');
    }

    #[test]
    fn test_strtok_r_multi_char_delimiters() {
        // Multiple characters in delimiter set.
        let mut buf = *b"one two\tthree\nfour\0";
        let delim = b" \t\n\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t1, b"one") });

        let t2 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t2, b"two") });

        let t3 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t3, b"three") });

        let t4 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(unsafe { cstr_eq(t4, b"four") });

        let t5 = unsafe { strtok_r(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(t5.is_null());
    }

    #[test]
    fn test_strtok_r_empty_string() {
        let mut buf = *b"\0";
        let delim = b",\0";
        let mut save: *mut u8 = core::ptr::null_mut();

        let t1 = unsafe { strtok_r(buf.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(t1.is_null());
    }

    // -------------------------------------------------------------------
    // Stress tests — strspn / strcspn exhaustive
    // -------------------------------------------------------------------

    #[test]
    fn test_strspn_entire_string_accepted() {
        let s = b"aaabbbccc\0";
        let accept = b"abc\0";
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 9);
    }

    #[test]
    fn test_strspn_reject_at_position_zero() {
        let s = b"xyz\0";
        let accept = b"abc\0";
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_strspn_empty_accept_set() {
        let s = b"hello\0";
        let accept = b"\0";
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_strcspn_entire_string_no_reject() {
        let s = b"hello\0";
        let reject = b"xyz\0";
        let ret = unsafe { strcspn(s.as_ptr(), reject.as_ptr()) };
        assert_eq!(ret, 5);
    }

    #[test]
    fn test_strcspn_first_char_in_reject() {
        let s = b"hello\0";
        let reject = b"h\0";
        let ret = unsafe { strcspn(s.as_ptr(), reject.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_strcspn_last_char_in_reject() {
        let s = b"hello\0";
        let reject = b"o\0";
        let ret = unsafe { strcspn(s.as_ptr(), reject.as_ptr()) };
        assert_eq!(ret, 4);
    }

    #[test]
    fn test_strspn_binary_values() {
        // Test with non-ASCII byte values in accept set.
        let s: &[u8] = &[0x80, 0x81, 0x82, b'A', 0];
        let accept: &[u8] = &[0x80, 0x81, 0x82, 0];
        let ret = unsafe { strspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 3);
    }

    // -------------------------------------------------------------------
    // Stress tests — strpbrk additional
    // -------------------------------------------------------------------

    #[test]
    fn test_strpbrk_last_char_matches() {
        let s = b"abcde\0";
        let accept = b"e\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!ret.is_null());
        let offset = ret as usize - s.as_ptr() as usize;
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_strpbrk_multiple_matches_returns_first() {
        let s = b"hello world\0";
        let accept = b"ow\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!ret.is_null());
        // 'o' appears at position 4, 'w' at position 6.
        let offset = ret as usize - s.as_ptr() as usize;
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_strpbrk_empty_accept() {
        let s = b"hello\0";
        let accept = b"\0";
        let ret = unsafe { strpbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(ret.is_null());
    }

    // -------------------------------------------------------------------
    // Stress tests — memmove with various overlaps
    // -------------------------------------------------------------------

    #[test]
    fn test_memmove_no_overlap() {
        let src = [1u8, 2, 3, 4, 5];
        let mut dest = [0u8; 5];
        unsafe { memmove(dest.as_mut_ptr(), src.as_ptr(), 5); }
        assert_eq!(dest, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_memmove_overlap_one_byte_forward() {
        // [1,2,3,4,5] — copy [0..4] to [1..5].
        let mut buf = [1u8, 2, 3, 4, 5];
        unsafe { memmove(buf.as_mut_ptr().add(1), buf.as_ptr(), 4); }
        assert_eq!(buf, [1, 1, 2, 3, 4]);
    }

    #[test]
    fn test_memmove_overlap_one_byte_backward() {
        // [1,2,3,4,5] — copy [1..5] to [0..4].
        let mut buf = [1u8, 2, 3, 4, 5];
        unsafe { memmove(buf.as_mut_ptr(), buf.as_ptr().add(1), 4); }
        assert_eq!(buf, [2, 3, 4, 5, 5]);
    }

    #[test]
    fn test_memmove_complete_overlap() {
        // Copy region onto itself — should be no-op.
        let mut buf = [10u8, 20, 30, 40, 50];
        unsafe { memmove(buf.as_mut_ptr(), buf.as_ptr(), 5); }
        assert_eq!(buf, [10, 20, 30, 40, 50]);
    }

    #[test]
    fn test_memmove_large_region() {
        // 512-byte overlapping copy (bigger than cache line).
        let mut buf = [0u8; 1024];
        for i in 0..512 {
            buf[i] = (i & 0xFF) as u8;
        }
        // Copy first 512 bytes to offset 256 (overlap of 256 bytes).
        unsafe { memmove(buf.as_mut_ptr().add(256), buf.as_ptr(), 512); }
        // Verify: buf[256..768] should equal original [0..512].
        for i in 0..512 {
            assert_eq!(buf[256 + i], (i & 0xFF) as u8);
        }
    }

    // -------------------------------------------------------------------
    // Stress tests — strsep edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_strsep_single_char_tokens() {
        let mut buf = *b"a,b,c\0";
        let mut ptr: *mut u8 = buf.as_mut_ptr();
        let delim = b",\0";

        let t1 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t1, b"a") });

        let t2 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t2, b"b") });

        let t3 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t3, b"c") });

        // ptr should be null after exhausting.
        assert!(ptr.is_null());
    }

    #[test]
    fn test_strsep_preserves_empty_fields() {
        // Unlike strtok, strsep returns empty strings for consecutive delims.
        let mut buf = *b"a,,b\0";
        let mut ptr: *mut u8 = buf.as_mut_ptr();
        let delim = b",\0";

        let t1 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t1, b"a") });

        let t2 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        // Should be empty string (the field between two commas).
        assert!(unsafe { cstr_eq(t2, b"") });

        let t3 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t3, b"b") });
    }

    #[test]
    fn test_strsep_trailing_delimiter() {
        let mut buf = *b"hello,\0";
        let mut ptr: *mut u8 = buf.as_mut_ptr();
        let delim = b",\0";

        let t1 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        assert!(unsafe { cstr_eq(t1, b"hello") });

        let t2 = unsafe { strsep(&raw mut ptr, delim.as_ptr()) };
        // Empty field after trailing comma.
        assert!(unsafe { cstr_eq(t2, b"") });

        assert!(ptr.is_null());
    }

    // -------------------------------------------------------------------
    // Stress tests — swab
    // -------------------------------------------------------------------

    #[test]
    fn test_swab_stress_six_bytes() {
        let src = [1u8, 2, 3, 4, 5, 6];
        let mut dest = [0u8; 6];
        unsafe { swab(src.as_ptr(), dest.as_mut_ptr(), 6); }
        assert_eq!(dest, [2, 1, 4, 3, 6, 5]);
    }

    #[test]
    fn test_swab_stress_odd_drops_last() {
        // Odd nbytes: last byte ignored.
        let src = [1u8, 2, 3, 4, 5];
        let mut dest = [0u8; 5];
        unsafe { swab(src.as_ptr(), dest.as_mut_ptr(), 5); }
        // Only first 4 bytes swapped, 5th untouched.
        assert_eq!(dest[0], 2);
        assert_eq!(dest[1], 1);
        assert_eq!(dest[2], 4);
        assert_eq!(dest[3], 3);
        assert_eq!(dest[4], 0); // not written
    }

    #[test]
    fn test_swab_stress_empty() {
        let src = [1u8, 2];
        let mut dest = [0u8; 2];
        unsafe { swab(src.as_ptr(), dest.as_mut_ptr(), 0); }
        assert_eq!(dest, [0, 0]); // untouched
    }

    #[test]
    fn test_swab_stress_single_pair() {
        let src = [0xAB_u8, 0xCD];
        let mut dest = [0u8; 2];
        unsafe { swab(src.as_ptr(), dest.as_mut_ptr(), 2); }
        assert_eq!(dest, [0xCD, 0xAB]);
    }

    // -------------------------------------------------------------------
    // Helper for strtok_r / strsep tests
    // -------------------------------------------------------------------

    /// Check if a C string pointer equals an expected byte slice.
    unsafe fn cstr_eq(p: *const u8, expected: &[u8]) -> bool {
        if p.is_null() {
            return false;
        }
        for (i, &b) in expected.iter().enumerate() {
            if unsafe { *p.add(i) } != b {
                return false;
            }
        }
        unsafe { *p.add(expected.len()) == 0 }
    }

    // -------------------------------------------------------------------
    // FORTIFY _chk wrappers — smoke tests
    // -------------------------------------------------------------------

    #[test]
    fn test_memcpy_chk_delegates() {
        let src = [1u8, 2, 3, 4];
        let mut dest = [0u8; 4];
        let ret = unsafe { __memcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(dest, [1, 2, 3, 4]);
        assert_eq!(ret, dest.as_mut_ptr());
    }

    #[test]
    fn test_mempcpy_chk_delegates() {
        let src = [9u8, 8, 7, 6];
        let mut dest = [0u8; 4];
        let ret = unsafe { __mempcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(dest, [9, 8, 7, 6]);
        // mempcpy returns dest + n (one past the last written byte).
        let offset = ret as usize - dest.as_ptr() as usize;
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_memmove_chk_delegates() {
        let src = [5u8, 6, 7, 8];
        let mut dest = [0u8; 4];
        let ret = unsafe { __memmove_chk(dest.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(dest, [5, 6, 7, 8]);
        assert_eq!(ret, dest.as_mut_ptr());
    }

    #[test]
    fn test_memset_chk_delegates() {
        let mut buf = [0xFFu8; 4];
        let ret = unsafe { __memset_chk(buf.as_mut_ptr(), 0, 4, 4) };
        assert_eq!(buf, [0, 0, 0, 0]);
        assert_eq!(ret, buf.as_mut_ptr());
    }

    #[test]
    fn test_strcpy_chk_delegates() {
        let src = b"hi\0";
        let mut dest = [0u8; 4];
        let ret = unsafe { __strcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(&dest[..3], b"hi\0");
        assert_eq!(ret, dest.as_mut_ptr());
    }

    #[test]
    fn test_strcat_chk_delegates() {
        let mut buf = [0u8; 16];
        buf[0] = b'A';
        buf[1] = 0;
        let src = b"BC\0";
        let ret = unsafe { __strcat_chk(buf.as_mut_ptr(), src.as_ptr(), 16) };
        assert_eq!(&buf[..4], b"ABC\0");
        assert_eq!(ret, buf.as_mut_ptr());
    }

    #[test]
    fn test_stpcpy_chk_delegates() {
        let src = b"ok\0";
        let mut dest = [0u8; 4];
        let ret = unsafe { __stpcpy_chk(dest.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(&dest[..3], b"ok\0");
        // stpcpy returns pointer to the NUL.
        let offset = ret as usize - dest.as_ptr() as usize;
        assert_eq!(offset, 2);
    }

    // -- strtok (non-reentrant) --

    #[test]
    fn test_strtok_basic() {
        let mut buf = *b"hello,world\0";
        let tok1 = unsafe { strtok(buf.as_mut_ptr(), b",\0".as_ptr()) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { *tok1 }, b'h');
        let tok2 = unsafe { strtok(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { *tok2 }, b'w');
        let tok3 = unsafe { strtok(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(tok3.is_null());
    }

    #[test]
    fn test_strtok_no_delimiters() {
        let mut buf = *b"single\0";
        let tok = unsafe { strtok(buf.as_mut_ptr(), b",\0".as_ptr()) };
        assert!(!tok.is_null());
        assert_eq!(unsafe { *tok }, b's');
        let tok2 = unsafe { strtok(core::ptr::null_mut(), b",\0".as_ptr()) };
        assert!(tok2.is_null());
    }

    #[test]
    fn test_strtok_all_delimiters() {
        let mut buf = *b",,,\0";
        let tok = unsafe { strtok(buf.as_mut_ptr(), b",\0".as_ptr()) };
        assert!(tok.is_null());
    }

    // -- strdup --
    // Note: strdup/strndup allocate via malloc which uses mmap syscalls.
    // Only null-pointer handling can be tested without kernel support.

    #[test]
    fn test_strdup_null() {
        let dup = unsafe { strdup(core::ptr::null()) };
        assert!(dup.is_null());
    }

    // -- strndup --

    #[test]
    fn test_strndup_null() {
        let dup = unsafe { strndup(core::ptr::null(), 10) };
        assert!(dup.is_null());
    }

    // -- strcoll_l --

    #[test]
    fn test_strcoll_l_equal() {
        let a = b"abc\0";
        let b_str = b"abc\0";
        assert_eq!(unsafe { strcoll_l(a.as_ptr(), b_str.as_ptr(), 0) }, 0);
    }

    #[test]
    fn test_strcoll_l_ordering() {
        let a = b"abc\0";
        let b_str = b"abd\0";
        assert!(unsafe { strcoll_l(a.as_ptr(), b_str.as_ptr(), 0) } < 0);
    }

    // -- strxfrm_l --

    #[test]
    fn test_strxfrm_l_basic() {
        let src = b"hello\0";
        let mut dst = [0u8; 16];
        let len = unsafe { strxfrm_l(dst.as_mut_ptr(), src.as_ptr(), 16, 0) };
        assert_eq!(len, 5);
        assert_eq!(&dst[..6], b"hello\0");
    }

    // -- strerror_l --

    #[test]
    fn test_strerror_l_known_code() {
        let msg = strerror_l(2, 0); // ENOENT
        assert!(!msg.is_null());
        // Should be "No such file or directory"
        assert_eq!(unsafe { *msg }, b'N');
    }

    #[test]
    fn test_strerror_l_matches_strerror() {
        let msg1 = strerror(13); // EACCES
        let msg2 = strerror_l(13, 0);
        assert_eq!(msg1, msg2);
    }

    // -- __xpg_strerror_r --

    #[test]
    fn test_xpg_strerror_r_success() {
        let mut buf = [0u8; 64];
        let ret = unsafe { __xpg_strerror_r(0, buf.as_mut_ptr(), 64) };
        assert_eq!(ret, 0);
        // Should contain "Success".
        assert_eq!(buf[0], b'S');
    }

    #[test]
    fn test_xpg_strerror_r_truncation() {
        let mut buf = [0u8; 4];
        let ret = unsafe { __xpg_strerror_r(2, buf.as_mut_ptr(), 4) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    // -- __strncpy_chk --

    #[test]
    fn test_strncpy_chk_delegates() {
        let src = b"test\0";
        let mut dst = [0u8; 8];
        let ret = unsafe { __strncpy_chk(dst.as_mut_ptr(), src.as_ptr(), 8, 8) };
        assert_eq!(ret, dst.as_mut_ptr());
        assert_eq!(&dst[..5], b"test\0");
    }

    // -- __strncat_chk --

    #[test]
    fn test_strncat_chk_delegates() {
        let mut buf = [0u8; 16];
        buf[0] = b'A';
        buf[1] = 0;
        let src = b"BCD\0";
        let ret = unsafe { __strncat_chk(buf.as_mut_ptr(), src.as_ptr(), 3, 16) };
        assert_eq!(&buf[..5], b"ABCD\0");
        assert_eq!(ret, buf.as_mut_ptr());
    }

    // -- __stpncpy_chk --

    #[test]
    fn test_stpncpy_chk_delegates() {
        let src = b"ab\0";
        let mut dst = [0u8; 4];
        let ret = unsafe { __stpncpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4, 4) };
        assert_eq!(&dst[..3], b"ab\0");
        // stpncpy returns pointer to first NUL within n.
        let offset = ret as usize - dst.as_ptr() as usize;
        assert_eq!(offset, 2);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __stpncpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { stpncpy(dest, src, n) }
}

/// `__mempcpy_chk` — fortified `mempcpy`.
///
/// # Safety
///
/// Same as `mempcpy`.  `destlen` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __mempcpy_chk(
    dest: *mut u8,
    src: *const u8,
    n: usize,
    _destlen: usize,
) -> *mut u8 {
    unsafe { mempcpy(dest, src, n) }
}

// ---------------------------------------------------------------------------
// swab — byte pair swap
// ---------------------------------------------------------------------------

/// Copy `nbytes` bytes from `src` to `dest`, swapping adjacent byte pairs.
///
/// POSIX requires `nbytes` to be even.  If `nbytes` is odd, the last
/// byte is silently ignored (not copied).  This matches glibc behavior.
///
/// # Safety
///
/// `src` and `dest` must point to valid memory of at least `nbytes`
/// bytes.  The regions must not overlap.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn swab(
    src: *const u8,
    dest: *mut u8,
    nbytes: isize,
) {
    if src.is_null() || dest.is_null() || nbytes <= 1 {
        return;
    }
    // Process pairs.
    let pairs = (nbytes as usize) / 2;
    let mut i: usize = 0;
    while i < pairs {
        let off = i.wrapping_mul(2);
        // SAFETY: off < nbytes (since i < pairs = nbytes/2, off = 2*i < nbytes).
        let a = unsafe { *src.add(off) };
        let b = unsafe { *src.add(off.wrapping_add(1)) };
        unsafe { *dest.add(off) = b; }
        unsafe { *dest.add(off.wrapping_add(1)) = a; }
        i = i.wrapping_add(1);
    }
}
