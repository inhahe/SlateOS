//! C string functions required by the C runtime.
//!
//! These are not strictly POSIX but are required by virtually every
//! C program.  A real libc would provide optimized (SIMD) versions;
//! these are correct reference implementations.
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
