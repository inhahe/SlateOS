//! POSIX pathname pattern expansion.
//!
//! Implements `glob()` and `globfree()` for shell-style pathname
//! matching per POSIX.1-2024.
//!
//! ## How It Works
//!
//! 1. Split the pattern at the last `/` to get (directory, filename-pattern).
//! 2. Open the directory with `opendir()`.
//! 3. Match each entry against the filename pattern using `fnmatch()`.
//! 4. Collect matching full paths into a dynamically-allocated result list.
//!
//! ## Limitations
//!
//! - No recursive glob (`**`) support — that is a GNU extension.
//! - Only matches in a single directory (no multi-component patterns
//!   like `src/*/foo.rs`).
//! - Maximum 512 matches per glob call.
//! - Does not expand `~` (tilde) — that is a shell feature.

use crate::dirent;
use crate::fnmatch;
use crate::malloc;
use crate::string;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Return on read error (stop scanning).
pub const GLOB_ERR: i32 = 1;
/// Mark directories with a trailing slash.
pub const GLOB_MARK: i32 = 2;
/// Return the pattern itself if no matches.
pub const GLOB_NOCHECK: i32 = 16;
/// Append results to an existing `glob_t`.
pub const GLOB_APPEND: i32 = 32;

/// No matches found.
pub const GLOB_NOMATCH: i32 = 3;
/// Memory allocation error.
pub const GLOB_NOSPACE: i32 = 1;
/// Read error.
pub const GLOB_ABORTED: i32 = 2;

/// Maximum matches per glob() call.
const MAX_MATCHES: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result structure for glob().
#[repr(C)]
pub struct GlobT {
    /// Number of matched pathnames.
    pub gl_pathc: usize,
    /// Array of matched pathnames (null-terminated).
    pub gl_pathv: *mut *mut u8,
    /// Slots reserved at the front of `gl_pathv`.
    pub gl_offs: usize,
}

// SAFETY: GlobT contains a raw pointer to heap-allocated path arrays.
// Only one thread accesses the glob result at a time per POSIX.
unsafe impl Sync for GlobT {}

// ---------------------------------------------------------------------------
// Pattern / directory split
// ---------------------------------------------------------------------------

/// Parsed pattern split into directory and filename portions.
struct PatternParts {
    /// Buffer holding the null-terminated directory path.
    dir_buf: [u8; 4096],
    /// Offset into the original pattern where the filename portion starts.
    file_start: usize,
    /// Whether the original pattern contained a '/'.
    has_slash: bool,
    /// Position of the last '/' in the pattern (only valid if `has_slash`).
    last_slash: usize,
    /// Total length of the original pattern.
    pat_len: usize,
}

/// Split a pattern into directory and filename components.
fn split_pattern(pattern: *const u8) -> PatternParts {
    let pat_len = unsafe { string::strlen(pattern) };
    let mut parts = PatternParts {
        dir_buf: [0u8; 4096],
        file_start: 0,
        has_slash: false,
        last_slash: 0,
        pat_len,
    };

    // Find last '/'.
    let mut idx: usize = 0;
    while idx < pat_len {
        if unsafe { *pattern.add(idx) } == b'/' {
            parts.last_slash = idx;
            parts.has_slash = true;
        }
        idx = idx.wrapping_add(1);
    }

    if parts.has_slash {
        let dir_len = parts.last_slash.wrapping_add(1);
        let mut ci: usize = 0;
        while ci < dir_len {
            if let Some(slot) = parts.dir_buf.get_mut(ci) {
                *slot = unsafe { *pattern.add(ci) };
            }
            ci = ci.wrapping_add(1);
        }
        if let Some(slot) = parts.dir_buf.get_mut(dir_len) {
            *slot = 0;
        }
        parts.file_start = dir_len;
    } else {
        parts.dir_buf[0] = b'.';
        parts.dir_buf[1] = 0;
    }

    parts
}

// ---------------------------------------------------------------------------
// Match collection
// ---------------------------------------------------------------------------

/// Collect matching entries from a directory into a match array.
///
/// Returns:
/// - `>= 0`: number of matches found
/// - `-1`: allocation failure
/// - `-2`: directory open failure
fn collect_matches(
    pattern: *const u8,
    parts: &PatternParts,
    flags: i32,
    match_ptrs: &mut [*mut u8; MAX_MATCHES],
) -> i32 {
    let file_pattern = unsafe { pattern.add(parts.file_start) };

    let dirp = dirent::opendir(parts.dir_buf.as_ptr());
    if dirp.is_null() {
        return -2; // Distinct from -1 (alloc failure): dir open error.
    }

    let mut count: usize = 0;

    loop {
        let entry = dirent::readdir(dirp);
        if entry.is_null() {
            break;
        }

        let dir_entry = unsafe { &*entry };
        let name = dir_entry.d_name.as_ptr();

        // Skip . and .. unless pattern explicitly starts with '.'.
        if should_skip_dot(name, file_pattern) {
            continue;
        }

        // Match against pattern.
        // FNM_PERIOD: POSIX requires that leading dots in filenames
        // only match explicit dots in the pattern (e.g. "*" must not
        // match ".bashrc").
        if unsafe { fnmatch::fnmatch(file_pattern, name, fnmatch::FNM_PERIOD) } != 0 {
            continue;
        }

        // Build full path.
        let path = build_match_path(pattern, parts, name, dir_entry.d_type, flags);
        if path.is_null() {
            cleanup_matches(match_ptrs, count);
            dirent::closedir(dirp);
            return -1; // Allocation failure.
        }

        if count < MAX_MATCHES {
            if let Some(slot) = match_ptrs.get_mut(count) {
                *slot = path;
            }
            count = count.wrapping_add(1);
        } else {
            // SAFETY: path was allocated by malloc above.
            unsafe { malloc::free(path); }
        }
    }

    dirent::closedir(dirp);
    count as i32
}

/// Check if a directory entry name (`.` or `..`) should be skipped.
fn should_skip_dot(name: *const u8, file_pattern: *const u8) -> bool {
    let first = unsafe { *name };
    if first != b'.' {
        return false;
    }
    let second = unsafe { *name.add(1) };
    if second == 0 || (second == b'.' && unsafe { *name.add(2) } == 0) {
        // Skip unless pattern starts with '.'.
        return unsafe { *file_pattern } != b'.';
    }
    false
}

/// Allocate and build the full path for a matched entry.
fn build_match_path(
    pattern: *const u8,
    parts: &PatternParts,
    name: *const u8,
    entry_type: u8,
    flags: i32,
) -> *mut u8 {
    let name_len = unsafe { string::strlen(name) };

    let full_len = if parts.has_slash {
        parts.last_slash.wrapping_add(1).wrapping_add(name_len)
    } else {
        name_len
    };

    let needs_slash = flags & GLOB_MARK != 0 && entry_type == dirent::DT_DIR;
    let alloc_len = full_len
        .wrapping_add(usize::from(needs_slash))
        .wrapping_add(1);

    let path = malloc::malloc(alloc_len);
    if path.is_null() {
        return core::ptr::null_mut();
    }

    let mut pos: usize = 0;

    // Copy directory prefix.
    if parts.has_slash {
        let dir_len = parts.last_slash.wrapping_add(1);
        let mut ci: usize = 0;
        while ci < dir_len {
            unsafe { *path.add(pos) = *pattern.add(ci); }
            pos = pos.wrapping_add(1);
            ci = ci.wrapping_add(1);
        }
    }

    // Copy filename.
    let mut ci: usize = 0;
    while ci < name_len {
        unsafe { *path.add(pos) = *name.add(ci); }
        pos = pos.wrapping_add(1);
        ci = ci.wrapping_add(1);
    }

    if needs_slash {
        unsafe { *path.add(pos) = b'/'; }
        pos = pos.wrapping_add(1);
    }

    unsafe { *path.add(pos) = 0; }
    path
}

// ---------------------------------------------------------------------------
// Result assembly
// ---------------------------------------------------------------------------

/// Build the pathv array from collected matches and store in GlobT.
///
/// Returns 0 on success, `GLOB_NOSPACE` on allocation failure.
fn assemble_results(
    glob_res: &mut GlobT,
    match_ptrs: &mut [*mut u8; MAX_MATCHES],
    match_count: usize,
) -> i32 {
    sort_paths(match_ptrs, match_count);

    let old_count = glob_res.gl_pathc;
    let new_count = old_count.wrapping_add(match_count);
    // Each entry is a pointer (8 bytes on x86_64), plus one null sentinel.
    let array_bytes = new_count.wrapping_add(1).wrapping_mul(core::mem::size_of::<*mut u8>());

    let new_pathv = if glob_res.gl_pathv.is_null() {
        malloc::malloc(array_bytes)
    } else {
        // SAFETY: gl_pathv was allocated by malloc, realloc is valid.
        unsafe { malloc::realloc(glob_res.gl_pathv.cast::<u8>(), array_bytes) }
    };

    if new_pathv.is_null() {
        cleanup_matches(match_ptrs, match_count);
        return GLOB_NOSPACE;
    }

    // SAFETY: malloc returns 8-byte aligned pointers on x86_64, which
    // satisfies the alignment requirement for *mut u8 pointers.
    #[allow(clippy::cast_ptr_alignment)]
    let pathv = new_pathv.cast::<*mut u8>();

    let mut idx: usize = 0;
    while idx < match_count {
        let ptr = match_ptrs.get(idx).copied().unwrap_or(core::ptr::null_mut());
        unsafe { *pathv.add(old_count.wrapping_add(idx)) = ptr; }
        idx = idx.wrapping_add(1);
    }

    // Null-terminate the array.
    unsafe { *pathv.add(new_count) = core::ptr::null_mut(); }

    glob_res.gl_pathv = pathv;
    glob_res.gl_pathc = new_count;

    0
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Expand a pathname pattern into matching filenames.
///
/// Returns 0 on success, `GLOB_NOMATCH` if no matches (unless
/// `GLOB_NOCHECK`), `GLOB_NOSPACE` on allocation failure.
///
/// # Safety
///
/// `pattern` must be a valid null-terminated string.
/// `pglob` must point to a valid `GlobT`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn glob(
    pattern: *const u8,
    flags: i32,
    _errfunc: Option<unsafe extern "C" fn(*const u8, i32) -> i32>,
    pglob: *mut GlobT,
) -> i32 {
    if pattern.is_null() || pglob.is_null() {
        return GLOB_ABORTED;
    }

    let glob_res = unsafe { &mut *pglob };

    // If not appending, initialize.
    if flags & GLOB_APPEND == 0 {
        glob_res.gl_pathc = 0;
        glob_res.gl_pathv = core::ptr::null_mut();
        glob_res.gl_offs = 0;
    }

    let parts = split_pattern(pattern);

    // Collect matches from the directory.
    let mut match_ptrs: [*mut u8; MAX_MATCHES] = [core::ptr::null_mut(); MAX_MATCHES];
    let match_result = collect_matches(pattern, &parts, flags, &mut match_ptrs);

    if match_result == -1 {
        return GLOB_NOSPACE;
    }

    // -2 = directory open failure (distinct from "no matches").
    if match_result == -2 {
        if flags & GLOB_ERR != 0 {
            return GLOB_ABORTED;
        }
        // Treat unreadable dir same as no matches.
        if flags & GLOB_NOCHECK != 0 {
            return add_single_path(glob_res, pattern, parts.pat_len);
        }
        return GLOB_NOMATCH;
    }

    let match_count = match_result as usize;

    if match_count == 0 {
        // Directory opened fine but nothing matched the pattern.
        if flags & GLOB_NOCHECK != 0 {
            return add_single_path(glob_res, pattern, parts.pat_len);
        }
        return GLOB_NOMATCH;
    }

    assemble_results(glob_res, &mut match_ptrs, match_count)
}

/// Free memory allocated by `glob()`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn globfree(pglob: *mut GlobT) {
    if pglob.is_null() {
        return;
    }

    let glob_res = unsafe { &mut *pglob };

    if !glob_res.gl_pathv.is_null() {
        let mut idx: usize = 0;
        while idx < glob_res.gl_pathc {
            let entry = unsafe { *glob_res.gl_pathv.add(idx) };
            if !entry.is_null() {
                // SAFETY: each entry was allocated by malloc.
                unsafe { malloc::free(entry); }
            }
            idx = idx.wrapping_add(1);
        }
        // SAFETY: gl_pathv was allocated via malloc, cast back to *mut u8.
        #[allow(clippy::cast_ptr_alignment)]
        unsafe { malloc::free(glob_res.gl_pathv.cast::<u8>()); }
    }

    glob_res.gl_pathc = 0;
    glob_res.gl_pathv = core::ptr::null_mut();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Add the pattern itself as a match (for `GLOB_NOCHECK`).
///
/// Appends to existing results when `GLOB_APPEND` was used, rather
/// than replacing them.
fn add_single_path(glob_res: &mut GlobT, pattern: *const u8, pat_len: usize) -> i32 {
    let alloc_len = pat_len.wrapping_add(1);
    let path = malloc::malloc(alloc_len);
    if path.is_null() {
        return GLOB_NOSPACE;
    }

    let mut idx: usize = 0;
    while idx < pat_len {
        unsafe { *path.add(idx) = *pattern.add(idx); }
        idx = idx.wrapping_add(1);
    }
    unsafe { *path.add(pat_len) = 0; }

    let old_count = glob_res.gl_pathc;
    let new_count = old_count.wrapping_add(1);
    // Allocate space for existing entries + new entry + null sentinel.
    let array_bytes = new_count.wrapping_add(1).wrapping_mul(core::mem::size_of::<*mut u8>());

    let pathv_raw = if glob_res.gl_pathv.is_null() {
        malloc::malloc(array_bytes)
    } else {
        // SAFETY: gl_pathv was allocated by malloc.
        unsafe { malloc::realloc(glob_res.gl_pathv.cast::<u8>(), array_bytes) }
    };

    if pathv_raw.is_null() {
        // SAFETY: path was allocated by malloc above.
        unsafe { malloc::free(path); }
        return GLOB_NOSPACE;
    }

    // SAFETY: malloc returns 8-byte aligned on x86_64.
    #[allow(clippy::cast_ptr_alignment)]
    let pathv = pathv_raw.cast::<*mut u8>();
    // Place new entry after existing entries (preserves GLOB_APPEND data).
    unsafe {
        *pathv.add(old_count) = path;
        *pathv.add(new_count) = core::ptr::null_mut();
    }

    glob_res.gl_pathv = pathv;
    glob_res.gl_pathc = new_count;

    0
}

/// Free partially-collected matches on error.
fn cleanup_matches(ptrs: &mut [*mut u8; MAX_MATCHES], count: usize) {
    let mut idx: usize = 0;
    while idx < count {
        if let Some(&ptr) = ptrs.get(idx)
            && !ptr.is_null()
        {
            // SAFETY: each ptr was allocated by malloc.
            unsafe { malloc::free(ptr); }
        }
        idx = idx.wrapping_add(1);
    }
}

/// Insertion sort on an array of C string pointers.
fn sort_paths(ptrs: &mut [*mut u8; MAX_MATCHES], count: usize) {
    if count <= 1 {
        return;
    }

    let mut outer: usize = 1;
    while outer < count {
        let key = ptrs.get(outer).copied().unwrap_or(core::ptr::null_mut());
        let mut inner = outer;
        while inner > 0 {
            let prev = ptrs.get(inner.wrapping_sub(1)).copied().unwrap_or(core::ptr::null_mut());
            if unsafe { string::strcmp(prev, key) } <= 0 {
                break;
            }
            if let Some(slot) = ptrs.get_mut(inner) {
                *slot = prev;
            }
            inner = inner.wrapping_sub(1);
        }
        if let Some(slot) = ptrs.get_mut(inner) {
            *slot = key;
        }
        outer = outer.wrapping_add(1);
    }
}
