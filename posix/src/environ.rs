//! Environment variable access.
//!
//! Implements `getenv`, `secure_getenv`, `setenv`, `unsetenv`, `putenv` and
//! `clearenv`, over the one list POSIX defines: the array `environ` points at.
//!
//! ## The environment *is* `environ`
//!
//! Every function here reads or edits that array and nothing else, and exec
//! hands the new image exactly that array (see [`current_environ`]). This is
//! musl's design, down to the bookkeeping, and it replaced one in which the
//! environment lived in a private table — 128 slots of 256 bytes — with
//! `environ` rebuilt from it after each change. That table was wrong in four
//! ways at once, each of them silent:
//!
//! * **It lost what did not fit.** A variable longer than 255 bytes, or any
//!   past the 128th, was dropped at start-up with no error: a child whose
//!   parent had a long `PATH` or `LS_COLORS` simply did not have it.
//!   `setenv` at least said `ENOMEM`; the inheritance path said nothing.
//! * **`getenv` did not read `environ`.** A program that assigns `environ` to
//!   an array of its own — `env -i`, privilege-dropping launchers, Rust's `std`
//!   before it `execvp`s — was invisible to `getenv`, and so to `execvp`'s own
//!   `PATH` search, which searched the parent's `PATH` instead of the one the
//!   child was being given.
//! * **`putenv` copied its argument.** POSIX: "the string pointed to by
//!   `string` shall become part of the environment, so altering the string
//!   shall change the environment." Programs that update a variable by
//!   rewriting the buffer they `putenv`ed saw no change.
//! * **`__environ`** was only ever a copy (it still is — see below), but it is
//!   now kept in step on every change rather than only on a rebuild.
//!
//! ## Who owns what
//!
//! The array `environ` points at is one of three things, and only the third is
//! ours to resize or free:
//!
//! 1. the start-up array `crt.rs` builds over the arguments the kernel handed
//!    this process ([`adopt_initial_envp`]) — lives for the whole process;
//! 2. an array the program assigned — its to manage;
//! 3. an array this module allocated (`owned_array`) — resized with
//!    `realloc` while `environ` still points at it, and freed when replaced.
//!
//! Likewise a string in the list is either the caller's (`putenv`, start-up)
//! or ours (`setenv` builds `NAME=VALUE` with `malloc`). Ours are recorded in
//! `owned_strings` so that replacing or removing one frees it, and only it:
//! freeing a `putenv` string would free the caller's buffer.
//!
//! ## Thread safety
//!
//! None, as POSIX specifies for this family: a `setenv` racing a `getenv` is
//! the caller's bug on every libc. On the host, where libtest runs tests in
//! parallel inside one process, the environment is per test thread (see
//! `environ_slot`), so a test cannot see — or free — another's. Tests that
//! also depend on *other* process-wide state derived from it, such as a
//! cached time zone, still serialise on `lock_env_for_test`.

use crate::string;

/// Why there is an array here at all: `environ` must never be NULL for a
/// program that iterates it without checking, and an empty environment needs
/// somewhere to point. Never written — every edit of a list this short
/// allocates a new one.
static mut EMPTY_ENV: [*const u8; 1] = [core::ptr::null()];

/// The environment list (POSIX `environ`): a NULL-terminated array of
/// `NAME=VALUE` strings.
///
/// Exported under its C name. Starts at [`EMPTY_ENV`] and is pointed at the
/// kernel-provided list by `__libc_start_main` before `main`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub static mut environ: *mut *const u8 = (&raw mut EMPTY_ENV).cast::<*const u8>();

/// The address of `environ` on the target.
#[cfg(target_os = "none")]
fn environ_slot() -> *mut *mut *const u8 {
    &raw mut environ
}

// On the host, the environment is per test thread, like the rest of this
// crate's per-process state (`perprocess.rs`). It has to be here in a way it
// did not have to be before: the lists and strings are freed now, so a test
// reading the environment unlocked — `localtime` reading `TZ`, a `PATH`
// search — beside one that edits it would walk a freed array. The target
// keeps the exported `environ` above, which is the only copy that exists
// there; nothing outside this crate links the host build.
#[cfg(not(target_os = "none"))]
crate::perprocess::process_global! {
    /// Host stand-in for `environ`. NULL until first set, which every reader
    /// treats as the empty list.
    fn environ_slot() -> *mut *const u8 = core::ptr::null_mut();
}

crate::perprocess::process_global! {
    /// The array this module allocated for `environ`, if any. `environ` is
    /// resized in place only while it still points here; a program that has
    /// assigned `environ` elsewhere gets a fresh copy instead, as in musl.
    fn owned_array() -> *mut *const u8 = core::ptr::null_mut();

    /// The strings `setenv` allocated that the list still holds, in a
    /// `malloc`ed array — `OwnedStrings::len` slots used of `cap`, a slot
    /// becoming NULL once its string is freed.
    fn owned_strings() -> OwnedStrings = OwnedStrings {
        ptr: core::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
}

/// The bookkeeping behind [`owned_strings`].
struct OwnedStrings {
    ptr: *mut *mut u8,
    len: usize,
    cap: usize,
}

/// The current `environ`.
fn env_list() -> *mut *const u8 {
    // SAFETY: a plain read of a pointer-sized slot through a raw pointer.
    unsafe { environ_slot().read() }
}

/// Point `environ` — and, on the target, the glibc alias `__environ`, which is
/// a separate static because Rust cannot alias one symbol to another — at
/// `list`.
fn set_env_list(list: *mut *const u8) {
    // SAFETY: plain writes of pointer-sized slots through raw pointers.
    unsafe {
        environ_slot().write(list);
        #[cfg(target_os = "none")]
        core::ptr::addr_of_mut!(crate::crt::__environ).write(list);
    }
}

/// Where `name`'s `=` would be: its length, if it is a valid variable name
/// (non-empty, no `=`), else `None`.
///
/// # Safety
///
/// `name` must be a valid C string.
unsafe fn valid_name_len(name: *const u8) -> Option<usize> {
    // SAFETY: the caller's contract.
    let len = unsafe { string::strlen(name) };
    // SAFETY: readable for `len` bytes, just measured.
    let bytes = unsafe { core::slice::from_raw_parts(name, len) };
    if len == 0 || bytes.contains(&b'=') {
        None
    } else {
        Some(len)
    }
}

/// Does the entry `e` define the variable whose name is `name[..len]`?
///
/// # Safety
///
/// Both must be valid C strings, `name` at least `len` bytes long.
unsafe fn defines(e: *const u8, name: *const u8, len: usize) -> bool {
    // `strncmp` stops at a NUL in `e`, so a shorter entry cannot match, and
    // the `=` check stops `PATH` matching `PATHEXT=...`.
    // SAFETY: the caller's contract; `e[len]` is inside `e` because the first
    // `len` bytes compared equal to non-NUL bytes of `name`.
    unsafe { string::strncmp(name, e, len) == 0 && *e.add(len) == b'=' }
}

/// Record ownership: `old` leaves the list (freed, if it was ours) and `new`
/// joins it (tracked, if not NULL). musl's `__env_rm_add`, whose shape this
/// keeps so that one function is the only place a string is ever freed.
fn owned_replace(old: *const u8, new: *mut u8) {
    // SAFETY: the bookkeeping is touched only here and in `clearenv`, and its
    // array holds `len` initialised slots of `cap`.
    unsafe {
        let owned = &mut *owned_strings();
        let mut new = new;
        for i in 0..owned.len {
            let slot = owned.ptr.add(i);
            if !old.is_null() && slot.read().cast_const() == old {
                crate::malloc::free(slot.read());
                slot.write(new);
                return;
            }
            if slot.read().is_null() && !new.is_null() {
                slot.write(new);
                new = core::ptr::null_mut();
            }
        }
        if new.is_null() {
            return;
        }
        if owned.len >= owned.cap {
            let grown = owned.cap.saturating_mul(2).max(8);
            let bytes = grown.saturating_mul(core::mem::size_of::<*mut u8>());
            let fresh = crate::malloc::realloc(owned.ptr.cast::<u8>(), bytes).cast::<*mut u8>();
            if fresh.is_null() {
                // Out of memory for the bookkeeping only: the string stays in
                // the list and is simply never freed, which is the leak glibc
                // always has, rather than a wrong environment.
                return;
            }
            owned.ptr = fresh;
            owned.cap = grown;
        }
        owned.ptr.add(owned.len).write(new);
        owned.len = owned.len.saturating_add(1);
    }
}

/// Put `s` into the list, replacing the entry for the same name if there is
/// one. `name_len` is the length of `s`'s name; `owned` says whether `s` is
/// ours (`setenv`) or the caller's (`putenv`). musl's `__putenv`.
///
/// # Safety
///
/// `s` must be a valid `NAME=VALUE` C string whose `=` is at `name_len`, and it
/// must stay valid while it is in the list.
unsafe fn put(s: *mut u8, name_len: usize, owned: bool) -> i32 {
    let list = env_list();
    let mut count = 0usize;
    if !list.is_null() {
        // SAFETY: `list` is a NULL-terminated array of C strings.
        unsafe {
            loop {
                let slot = list.add(count);
                let e = slot.read();
                if e.is_null() {
                    break;
                }
                if defines(e, s, name_len) {
                    slot.write(s);
                    owned_replace(e, if owned { s } else { core::ptr::null_mut() });
                    return 0;
                }
                count = count.saturating_add(1);
            }
        }
    }

    // Not present: append, which needs `count + 2` slots.
    let bytes = count
        .saturating_add(2)
        .saturating_mul(core::mem::size_of::<*const u8>());
    // SAFETY: a plain read of a pointer-sized slot.
    let ours = unsafe { owned_array().read() };
    let grown = if !ours.is_null() && list == ours {
        // Still our array: grow it where it is.
        // SAFETY: `ours` came from this allocator and has not been freed.
        unsafe { crate::malloc::realloc(ours.cast::<u8>(), bytes) }.cast::<*const u8>()
    } else {
        let fresh = crate::malloc::malloc(bytes).cast::<*const u8>();
        if !fresh.is_null() && count > 0 {
            // SAFETY: `list` holds `count` entries and `fresh` room for more.
            unsafe { core::ptr::copy_nonoverlapping(list, fresh, count) };
        }
        fresh
    };
    if grown.is_null() {
        if owned {
            // SAFETY: `s` is ours and never reached the list.
            unsafe { crate::malloc::free(s) };
        }
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    }
    if list != ours && !ours.is_null() {
        // The old array of ours was abandoned when the program assigned
        // `environ` elsewhere; this is the first chance to free it.
        // SAFETY: allocated here, and nothing points at it any more.
        unsafe { crate::malloc::free(ours.cast::<u8>()) };
    }
    // SAFETY: `grown` has `count + 2` slots; `owned_array()` is this
    // module's own slot.
    unsafe {
        grown.add(count).write(s);
        grown.add(count.saturating_add(1)).write(core::ptr::null());
        owned_array().write(grown);
    }
    set_env_list(grown);
    if owned {
        owned_replace(core::ptr::null(), s);
    }
    0
}

/// Get the value of an environment variable.
///
/// Returns a pointer to the value (after the `=`), or NULL if `name` is not
/// set — or is not a valid name at all: empty, or containing `=`. glibc would
/// look `A=B` up as a prefix and answer with the tail of some `A=B=…`; there
/// is no variable by that name, so there is nothing to answer.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getenv(name: *const u8) -> *const u8 {
    if name.is_null() {
        return core::ptr::null();
    }
    // SAFETY: `name` is a valid C string (the caller's contract).
    let Some(len) = (unsafe { valid_name_len(name) }) else {
        return core::ptr::null();
    };
    let list = env_list();
    if list.is_null() {
        return core::ptr::null();
    }
    // SAFETY: `environ` is a NULL-terminated array of C strings.
    unsafe {
        let mut i = 0usize;
        loop {
            let e = list.add(i).read();
            if e.is_null() {
                return core::ptr::null();
            }
            if defines(e, name, len) {
                return e.add(len.saturating_add(1));
            }
            i = i.saturating_add(1);
        }
    }
}

/// Set an environment variable.
///
/// If `overwrite` is non-zero and the variable exists, it is replaced.
/// Returns 0 on success, -1 on error.
///
/// # Errors
///
/// * `EINVAL` — `name` is NULL, empty or contains `=`, or `value` is NULL.
/// * `ENOMEM` — the new entry or the list could not be allocated. There is no
///   length or count limit beyond memory.
///
/// # Safety
///
/// `name` and `value` must be valid null-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setenv(name: *const u8, value: *const u8, overwrite: i32) -> i32 {
    if name.is_null() || value.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: `name` is a valid C string (the caller's contract).
    let Some(name_len) = (unsafe { valid_name_len(name) }) else {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    };
    // SAFETY: as above.
    if overwrite == 0 && !unsafe { getenv(name) }.is_null() {
        return 0;
    }
    // SAFETY: `value` is a valid C string (the caller's contract).
    let value_len = unsafe { string::strlen(value) };
    let Some(total) = name_len
        .checked_add(value_len)
        .and_then(|n| n.checked_add(2))
    else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    };
    let s = crate::malloc::malloc(total);
    if s.is_null() {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return -1;
    }
    // SAFETY: `s` holds `total = name_len + 1 + value_len + 1` bytes, and the
    // sources are readable for the lengths just measured.
    unsafe {
        core::ptr::copy_nonoverlapping(name, s, name_len);
        s.add(name_len).write(b'=');
        core::ptr::copy_nonoverlapping(value, s.add(name_len.saturating_add(1)), value_len);
        s.add(total.saturating_sub(1)).write(0);
        put(s, name_len, true)
    }
}

/// Remove an environment variable — every entry for it, as POSIX requires,
/// since a program that edits `environ` directly can make duplicates.
///
/// Returns 0 on success, including when the variable was not set.
///
/// # Errors
///
/// `EINVAL` — `name` is NULL, empty or contains `=`.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn unsetenv(name: *const u8) -> i32 {
    if name.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: `name` is a valid C string (the caller's contract).
    let Some(len) = (unsafe { valid_name_len(name) }) else {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    };
    let list = env_list();
    if list.is_null() {
        return 0;
    }
    // Compact in place, as glibc and musl both do — including in an array the
    // program supplied, which POSIX permits.
    // SAFETY: `environ` is a NULL-terminated array of C strings; `keep` never
    // passes `read`, so every write is to a slot already visited.
    unsafe {
        let mut read = 0usize;
        let mut keep = 0usize;
        loop {
            let e = list.add(read).read();
            if e.is_null() {
                break;
            }
            if defines(e, name, len) {
                owned_replace(e, core::ptr::null_mut());
            } else {
                list.add(keep).write(e);
                keep = keep.saturating_add(1);
            }
            read = read.saturating_add(1);
        }
        if keep != read {
            list.add(keep).write(core::ptr::null());
        }
    }
    0
}

// ---------------------------------------------------------------------------
// putenv
// ---------------------------------------------------------------------------

/// Insert or modify an environment variable.
///
/// `string` must be of the form `"NAME=VALUE"`, and — unlike `setenv` — it is
/// **not copied**: POSIX says it "shall become part of the environment, so
/// altering the string shall change the environment". The caller must keep it
/// alive while it is there. A string without `=` unsets that name (a glibc
/// extension musl shares).
///
/// Returns 0 on success, -1 on error.
///
/// # Errors
///
/// * `EINVAL` — `string` is NULL, or its name is empty (`"=value"`).
/// * `ENOMEM` — the list could not grow.
///
/// # Safety
///
/// `string` must be a valid null-terminated C string that outlives its entry.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putenv(string: *mut u8) -> i32 {
    if string.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: `string` is a valid C string (the caller's contract).
    let total = unsafe { string::strlen(string) };
    // SAFETY: readable for `total` bytes, just measured.
    let bytes = unsafe { core::slice::from_raw_parts(string, total) };
    let Some(name_len) = bytes.iter().position(|&b| b == b'=') else {
        // SAFETY: as above; a string without `=` is a name.
        return unsafe { unsetenv(string) };
    };
    if name_len == 0 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: `string`'s `=` is at `name_len`; the caller keeps it alive.
    unsafe { put(string, name_len, false) }
}

// ---------------------------------------------------------------------------
// clearenv
// ---------------------------------------------------------------------------

/// Clear the entire environment.
///
/// `environ` is left pointing at an empty list rather than NULL — glibc and
/// musl leave NULL, and a program that iterates `environ` afterwards without
/// checking then crashes; an empty list is equally conforming and harms
/// nobody. Frees every string `setenv` allocated and the list array if it is
/// ours. Returns 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clearenv() -> i32 {
    // Point `environ` away from everything first, so nothing reachable from
    // it is freed while it still points there.
    set_env_list(empty_env());
    // SAFETY: the bookkeeping is touched only in this module, its array holds
    // `len` initialised slots, and POSIX makes a concurrent caller the
    // caller's problem.
    unsafe {
        let owned = &mut *owned_strings();
        for i in 0..owned.len {
            let s = owned.ptr.add(i).read();
            if !s.is_null() {
                crate::malloc::free(s);
            }
        }
        if !owned.ptr.is_null() {
            crate::malloc::free(owned.ptr.cast::<u8>());
        }
        *owned = OwnedStrings {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        };

        let ours = owned_array().read();
        if !ours.is_null() {
            crate::malloc::free(ours.cast::<u8>());
        }
        owned_array().write(core::ptr::null_mut());
    }
    0
}

/// The shared empty list.
fn empty_env() -> *mut *const u8 {
    (&raw mut EMPTY_ENV).cast::<*const u8>()
}

// ---------------------------------------------------------------------------
// secure_getenv
// ---------------------------------------------------------------------------

/// Get an environment variable (security-aware).
///
/// In a real libc, `secure_getenv` returns null if the process is
/// running with elevated privileges (setuid/setgid).  Since our OS
/// doesn't have privilege escalation, this is identical to `getenv`.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn secure_getenv(name: *const u8) -> *const u8 {
    // No privilege escalation in our OS — just delegate.
    unsafe { getenv(name) }
}

/// Look up an environment variable from Rust, by name bytes.
///
/// The safe counterpart to [`getenv`] for callers inside this crate: it takes
/// the name as a byte slice (no NUL terminator needed, and no temporary
/// buffer) and hands back the value as a slice rather than a bare pointer, so
/// the caller never has to reconstruct the length with `strlen`.
///
/// The returned slice borrows the environment directly and carries the same
/// lifetime contract as C's `getenv`: a later `setenv`/`putenv`/`unsetenv`
/// touching this variable may free or rewrite the bytes. Copy the value out if
/// you need to hold it across such a call.
///
/// Returns `None` when the variable is unset; note that a variable set to the
/// empty string returns `Some(&[])`, which is a different thing — `TZ=""`
/// means "UTC" while an unset `TZ` means "use the system default".
#[must_use]
pub fn getenv_bytes(name: &[u8]) -> Option<&'static [u8]> {
    if name.is_empty() || name.contains(&b'=') {
        return None;
    }
    let list = env_list();
    if list.is_null() {
        return None;
    }
    // SAFETY: `environ` is a NULL-terminated array of C strings; the lifetime
    // is the C `getenv` contract described above.
    unsafe {
        let mut i = 0usize;
        loop {
            let e = list.add(i).read();
            if e.is_null() {
                return None;
            }
            let len = string::strlen(e);
            let entry = core::slice::from_raw_parts(e, len);
            if entry.get(..name.len()) == Some(name) && entry.get(name.len()) == Some(&b'=') {
                return entry.get(name.len().saturating_add(1)..);
            }
            i = i.saturating_add(1);
        }
    }
}

/// Initialize `environ` early in process startup.
///
/// `__libc_start_main` calls this after [`adopt_initial_envp`] (or instead of
/// it, when the kernel handed this process no environment). It only makes sure
/// `environ` is never NULL — a program may iterate it without checking.
pub fn init_environ() {
    if env_list().is_null() {
        set_env_list(empty_env());
    }
}

/// Make the start-up environment array `crt.rs` built — pointers into the
/// argument block the kernel handed this process, both of which live for the
/// whole process — the environment.
///
/// No copying and no limits: the list the kernel delivered is the list the
/// program sees. The table this replaced silently dropped any variable over
/// 255 bytes, and every one past the 128th.
///
/// # Safety
///
/// `envp` must be NULL or a NULL-terminated array of C strings that stays valid
/// (and is not otherwise written) for the life of the process.
pub unsafe fn adopt_initial_envp(envp: *mut *const u8) {
    if !envp.is_null() {
        set_env_list(envp);
    }
}

/// The current value of `environ`, read at the moment of the call.
///
/// This is what the exec forms without an `envp` parameter — `execv`,
/// `execvp`, `execl`, `execlp` — must hand the new image.  POSIX: "the
/// environment for the new process image shall be taken from the external
/// variable `environ` in the calling process."  *The variable*: `env -i`,
/// privilege-dropping launchers and Rust's `std` assign `environ` to a fresh
/// array and then call `execvp`, and they expect that array to be what the
/// child sees.
///
/// Those four forms passed NULL until 2026-09-24, which the kernel stores
/// faithfully as "no environment", so every program they started ran without
/// `PATH`, `HOME` or anything else its parent had set.
#[must_use]
pub(crate) fn current_environ() -> *const *const u8 {
    env_list().cast_const()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Cross-test serialisation lock for the process-global environment.  Tests
/// (in this file and others, e.g. `wordexp`, `tz`, `time`) that read or mutate
/// the environment must hold this lock for their duration.  Without it,
/// cargo's parallel test runner interleaves `clearenv()` / `setenv()` calls
/// from different tests and produces intermittent failures (see
/// `wordexp::tests::tilde_*` flakes).
#[cfg(test)]
pub static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire the env test lock, recovering from poison.
#[cfg(test)]
#[must_use = "the returned guard serialises env-mutating tests; bind it to `_g`"]
pub fn lock_env_for_test() -> std::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    /// Reset environment state before each test.  Acquires the
    /// cross-test env lock and clears the store; bind the returned
    /// guard to `_g` so it lives for the test body.
    #[must_use = "the returned guard serialises env-mutating tests; bind it to `_g`"]
    fn reset() -> std::sync::MutexGuard<'static, ()> {
        let g = super::lock_env_for_test();
        clearenv();
        g
    }

    /// Helper: read a C string pointer into a `&[u8]` slice (without
    /// the terminating null).  Panics if `ptr` is null.
    unsafe fn cstr_bytes(ptr: *const u8) -> &'static [u8] {
        assert!(!ptr.is_null(), "unexpected null pointer");
        let len = unsafe { string::strlen(ptr) } as usize;
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }

    /// Every entry in `environ`, in order.
    fn listed() -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let list = env_list();
        assert!(!list.is_null(), "environ must never be NULL");
        let mut i = 0;
        loop {
            // SAFETY: `environ` is a NULL-terminated array of C strings.
            let e = unsafe { list.add(i).read() };
            if e.is_null() {
                return out;
            }
            out.push(unsafe { cstr_bytes(e) }.to_vec());
            i += 1;
        }
    }

    // -----------------------------------------------------------------------
    // getenv
    // -----------------------------------------------------------------------

    #[test]
    fn getenv_returns_null_for_missing_var() {
        let _g = reset();
        let ptr = unsafe { getenv(b"NOSUCH\0".as_ptr()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn getenv_returns_null_for_null_name() {
        let _g = reset();
        let ptr = unsafe { getenv(core::ptr::null()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn getenv_returns_null_for_empty_name() {
        let _g = reset();
        // Empty string = just a null terminator.
        let ptr = unsafe { getenv(b"\0".as_ptr()) };
        assert!(ptr.is_null());
    }

    /// `A=B` is not a variable name, so there is nothing to find — even when
    /// an entry `A=B=C` exists, which is the variable `A`.
    #[test]
    fn getenv_of_a_name_containing_equals_is_null() {
        let _g = reset();
        unsafe { setenv(b"A\0".as_ptr(), b"B=C\0".as_ptr(), 1) };
        assert!(unsafe { getenv(b"A=B\0".as_ptr()) }.is_null());
        assert_eq!(unsafe { cstr_bytes(getenv(b"A\0".as_ptr())) }, b"B=C");
    }

    // -----------------------------------------------------------------------
    // setenv / getenv round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn setenv_then_getenv() {
        let _g = reset();
        let rc = unsafe { setenv(b"HOME\0".as_ptr(), b"/root\0".as_ptr(), 1) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"HOME\0".as_ptr()) };
        assert!(!val.is_null());
        assert_eq!(unsafe { cstr_bytes(val) }, b"/root");
        assert_eq!(listed(), [b"HOME=/root".to_vec()]);
    }

    #[test]
    fn setenv_overwrite_replaces_value() {
        let _g = reset();
        unsafe { setenv(b"K\0".as_ptr(), b"old\0".as_ptr(), 1) };
        unsafe { setenv(b"K\0".as_ptr(), b"new\0".as_ptr(), 1) };

        let val = unsafe { getenv(b"K\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"new");
        assert_eq!(listed(), [b"K=new".to_vec()], "replaced, not appended");
    }

    #[test]
    fn setenv_no_overwrite_keeps_original() {
        let _g = reset();
        unsafe { setenv(b"K\0".as_ptr(), b"first\0".as_ptr(), 1) };
        let rc = unsafe { setenv(b"K\0".as_ptr(), b"second\0".as_ptr(), 0) };
        assert_eq!(rc, 0); // success, but value unchanged

        let val = unsafe { getenv(b"K\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"first");
    }

    #[test]
    fn setenv_empty_value() {
        let _g = reset();
        let rc = unsafe { setenv(b"EMPTY\0".as_ptr(), b"\0".as_ptr(), 1) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"EMPTY\0".as_ptr()) };
        assert!(!val.is_null());
        assert_eq!(unsafe { cstr_bytes(val) }, b"");
    }

    #[test]
    fn setenv_rejects_null_name() {
        let _g = reset();
        let rc = unsafe { setenv(core::ptr::null(), b"v\0".as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_null_value() {
        let _g = reset();
        let rc = unsafe { setenv(b"K\0".as_ptr(), core::ptr::null(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_empty_name() {
        let _g = reset();
        let rc = unsafe { setenv(b"\0".as_ptr(), b"v\0".as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn setenv_rejects_equals_in_name() {
        let _g = reset();
        let rc = unsafe { setenv(b"A=B\0".as_ptr(), b"v\0".as_ptr(), 1) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// There is no length limit. The old table refused anything over 255
    /// bytes with `ENOMEM` — and at start-up dropped it without a word.
    #[test]
    fn a_long_value_is_kept_whole() {
        let _g = reset();
        let mut long = std::vec![b'x'; 70_000];
        long.push(0);
        assert_eq!(
            unsafe { setenv(b"LS_COLORS\0".as_ptr(), long.as_ptr(), 1) },
            0
        );
        let got = unsafe { cstr_bytes(getenv(b"LS_COLORS\0".as_ptr())) };
        assert_eq!(got.len(), 70_000);
    }

    /// Nor a count limit. The old table held 128.
    #[test]
    fn many_variables_are_all_kept() {
        let _g = reset();
        for i in 0..500u32 {
            let name = std::format!("V{i}\0");
            let value = std::format!("{i}\0");
            assert_eq!(unsafe { setenv(name.as_ptr(), value.as_ptr(), 1) }, 0);
        }
        assert_eq!(listed().len(), 500);
        assert_eq!(unsafe { cstr_bytes(getenv(b"V0\0".as_ptr())) }, b"0");
        assert_eq!(unsafe { cstr_bytes(getenv(b"V499\0".as_ptr())) }, b"499");
    }

    // -----------------------------------------------------------------------
    // unsetenv
    // -----------------------------------------------------------------------

    #[test]
    fn unsetenv_removes_variable() {
        let _g = reset();
        unsafe { setenv(b"DEL\0".as_ptr(), b"yes\0".as_ptr(), 1) };
        let rc = unsafe { unsetenv(b"DEL\0".as_ptr()) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"DEL\0".as_ptr()) };
        assert!(val.is_null());
        assert!(listed().is_empty());
    }

    #[test]
    fn unsetenv_nonexistent_succeeds() {
        let _g = reset();
        let rc = unsafe { unsetenv(b"GHOST\0".as_ptr()) };
        assert_eq!(rc, 0);
    }

    #[test]
    fn unsetenv_rejects_null_name() {
        let _g = reset();
        let rc = unsafe { unsetenv(core::ptr::null()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn unsetenv_rejects_empty_name() {
        let _g = reset();
        let rc = unsafe { unsetenv(b"\0".as_ptr()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn unsetenv_rejects_equals_in_name() {
        let _g = reset();
        let rc = unsafe { unsetenv(b"A=B\0".as_ptr()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// POSIX: every entry for the name goes. A program that writes `environ`
    /// itself can create duplicates.
    #[test]
    fn unsetenv_removes_every_duplicate_and_keeps_the_order_of_the_rest() {
        let _g = reset();
        let mut list: [*const u8; 5] = [
            b"A=1\0".as_ptr(),
            b"B=2\0".as_ptr(),
            b"A=3\0".as_ptr(),
            b"C=4\0".as_ptr(),
            core::ptr::null(),
        ];
        set_env_list(list.as_mut_ptr());
        assert_eq!(unsafe { unsetenv(b"A\0".as_ptr()) }, 0);
        assert_eq!(listed(), [b"B=2".to_vec(), b"C=4".to_vec()]);
        clearenv(); // before `list` goes out of scope
    }

    // -----------------------------------------------------------------------
    // putenv
    // -----------------------------------------------------------------------

    #[test]
    fn putenv_sets_variable() {
        let _g = reset();
        let mut s = *b"LANG=en_US\0";
        let rc = unsafe { putenv(s.as_mut_ptr()) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"LANG\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"en_US");
        clearenv(); // `s` is borrowed by the list until this
    }

    /// POSIX: the string *becomes* the entry, so changing it changes the
    /// environment. The old implementation copied it.
    #[test]
    fn putenv_does_not_copy_its_argument() {
        let _g = reset();
        let mut s = *b"MODE=aa\0";
        assert_eq!(unsafe { putenv(s.as_mut_ptr()) }, 0);
        s[5] = b'z';
        s[6] = b'z';
        assert_eq!(unsafe { cstr_bytes(getenv(b"MODE\0".as_ptr())) }, b"zz");
        assert_eq!(unsafe { getenv(b"MODE\0".as_ptr()) }, unsafe {
            s.as_ptr().add(5)
        });
        clearenv();
    }

    #[test]
    fn putenv_overwrites_existing() {
        let _g = reset();
        let mut s1 = *b"Z=one\0";
        unsafe { putenv(s1.as_mut_ptr()) };

        let mut s2 = *b"Z=two\0";
        unsafe { putenv(s2.as_mut_ptr()) };

        let val = unsafe { getenv(b"Z\0".as_ptr()) };
        assert_eq!(unsafe { cstr_bytes(val) }, b"two");
        assert_eq!(listed().len(), 1);
        clearenv();
    }

    /// A `putenv` over a `setenv` entry frees the `setenv` string — and a
    /// `setenv` over a `putenv` entry must not free the caller's buffer, which
    /// this test would crash on if it did.
    #[test]
    fn ownership_follows_who_allocated_the_string() {
        let _g = reset();
        let before = crate::malloc::live_allocations::count();
        unsafe { setenv(b"OWN\0".as_ptr(), b"lib\0".as_ptr(), 1) };
        let mut caller = *b"OWN=caller\0";
        assert_eq!(unsafe { putenv(caller.as_mut_ptr()) }, 0);
        assert_eq!(
            unsafe { setenv(b"OWN\0".as_ptr(), b"lib-again\0".as_ptr(), 1) },
            0
        );
        assert_eq!(
            unsafe { cstr_bytes(getenv(b"OWN\0".as_ptr())) },
            b"lib-again"
        );
        assert_eq!(&caller, b"OWN=caller\0", "the caller's buffer is untouched");
        clearenv();
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "clearenv must free every string and array setenv allocated"
        );
    }

    #[test]
    fn putenv_no_equals_unsets() {
        let _g = reset();
        unsafe { setenv(b"REM\0".as_ptr(), b"v\0".as_ptr(), 1) };
        let mut s = *b"REM\0";
        let rc = unsafe { putenv(s.as_mut_ptr()) };
        assert_eq!(rc, 0);

        let val = unsafe { getenv(b"REM\0".as_ptr()) };
        assert!(val.is_null());
    }

    #[test]
    fn putenv_rejects_null() {
        let _g = reset();
        let rc = unsafe { putenv(core::ptr::null_mut()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn putenv_rejects_empty_name_with_equals() {
        let _g = reset();
        // "=value" has an empty name portion.
        let mut s = *b"=value\0";
        let rc = unsafe { putenv(s.as_mut_ptr()) };
        assert_eq!(rc, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // environ assigned by the program
    // -----------------------------------------------------------------------

    /// `getenv` reads whatever `environ` points at. It used to read a private
    /// table, so a program that assigned `environ` — as `env -i` and Rust's
    /// `std` do before `execvp` — was invisible to it.
    #[test]
    fn getenv_sees_an_environ_the_program_assigned() {
        let _g = reset();
        let mut list: [*const u8; 3] = [
            b"PATH=/opt/bin\0".as_ptr(),
            b"X=1\0".as_ptr(),
            core::ptr::null(),
        ];
        set_env_list(list.as_mut_ptr());
        assert_eq!(
            unsafe { cstr_bytes(getenv(b"PATH\0".as_ptr())) },
            b"/opt/bin"
        );
        assert_eq!(getenv_bytes(b"X"), Some(&b"1"[..]));
        assert_eq!(current_environ(), list.as_ptr());
        clearenv();
    }

    /// `setenv` over an array the program owns copies it rather than
    /// resizing it — the program's array may be on its stack.
    #[test]
    fn setenv_does_not_resize_an_array_it_does_not_own() {
        let _g = reset();
        let mut list: [*const u8; 2] = [b"A=1\0".as_ptr(), core::ptr::null()];
        set_env_list(list.as_mut_ptr());
        assert_eq!(unsafe { setenv(b"B\0".as_ptr(), b"2\0".as_ptr(), 1) }, 0);
        assert_ne!(current_environ(), list.as_ptr());
        assert_eq!(listed(), [b"A=1".to_vec(), b"B=2".to_vec()]);
        assert_eq!(
            list[1],
            core::ptr::null(),
            "the program's array is unchanged"
        );
        clearenv();
    }

    // -----------------------------------------------------------------------
    // clearenv
    // -----------------------------------------------------------------------

    #[test]
    fn clearenv_removes_all() {
        let _g = reset();
        unsafe { setenv(b"A\0".as_ptr(), b"1\0".as_ptr(), 1) };
        unsafe { setenv(b"B\0".as_ptr(), b"2\0".as_ptr(), 1) };
        unsafe { setenv(b"C\0".as_ptr(), b"3\0".as_ptr(), 1) };

        let rc = clearenv();
        assert_eq!(rc, 0);

        assert!(unsafe { getenv(b"A\0".as_ptr()) }.is_null());
        assert!(unsafe { getenv(b"B\0".as_ptr()) }.is_null());
        assert!(unsafe { getenv(b"C\0".as_ptr()) }.is_null());
        assert!(listed().is_empty(), "an empty list, never NULL");
    }

    #[test]
    fn clearenv_on_empty_is_noop() {
        let _g = reset();
        let rc = clearenv();
        assert_eq!(rc, 0);
    }

    // -----------------------------------------------------------------------
    // secure_getenv
    // -----------------------------------------------------------------------

    #[test]
    fn secure_getenv_matches_getenv() {
        let _g = reset();
        unsafe { setenv(b"SEC\0".as_ptr(), b"val\0".as_ptr(), 1) };

        let a = unsafe { getenv(b"SEC\0".as_ptr()) };
        let b = unsafe { secure_getenv(b"SEC\0".as_ptr()) };
        assert_eq!(a, b);
        assert_eq!(unsafe { cstr_bytes(b) }, b"val");
    }

    // -----------------------------------------------------------------------
    // Interaction / multi-variable tests
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_variables_independent() {
        let _g = reset();
        unsafe { setenv(b"X\0".as_ptr(), b"10\0".as_ptr(), 1) };
        unsafe { setenv(b"Y\0".as_ptr(), b"20\0".as_ptr(), 1) };
        unsafe { setenv(b"XX\0".as_ptr(), b"30\0".as_ptr(), 1) };

        // "X" should not match "XX" or "Y".
        assert_eq!(unsafe { cstr_bytes(getenv(b"X\0".as_ptr())) }, b"10");
        assert_eq!(unsafe { cstr_bytes(getenv(b"Y\0".as_ptr())) }, b"20");
        assert_eq!(unsafe { cstr_bytes(getenv(b"XX\0".as_ptr())) }, b"30");
    }

    #[test]
    fn unsetenv_does_not_affect_others() {
        let _g = reset();
        unsafe { setenv(b"KEEP\0".as_ptr(), b"yes\0".as_ptr(), 1) };
        unsafe { setenv(b"DROP\0".as_ptr(), b"no\0".as_ptr(), 1) };

        unsafe { unsetenv(b"DROP\0".as_ptr()) };

        assert_eq!(unsafe { cstr_bytes(getenv(b"KEEP\0".as_ptr())) }, b"yes");
        assert!(unsafe { getenv(b"DROP\0".as_ptr()) }.is_null());
    }

    #[test]
    fn setenv_after_unsetenv() {
        let _g = reset();
        unsafe { setenv(b"REUSE\0".as_ptr(), b"a\0".as_ptr(), 1) };
        unsafe { unsetenv(b"REUSE\0".as_ptr()) };
        let rc = unsafe { setenv(b"REUSE\0".as_ptr(), b"b\0".as_ptr(), 1) };
        assert_eq!(rc, 0);
        assert_eq!(unsafe { cstr_bytes(getenv(b"REUSE\0".as_ptr())) }, b"b");
    }

    // -----------------------------------------------------------------------
    // Start-up adoption
    // -----------------------------------------------------------------------

    /// The start-up list is used as it is: every entry, whatever its length,
    /// in order. The old loader copied into the table and dropped anything
    /// over 255 bytes, and everything past the 128th.
    #[test]
    fn the_start_up_list_is_adopted_whole() {
        let _g = reset();
        let mut long = std::vec![b'p'; 400];
        long.splice(0..0, b"PATH=".iter().copied());
        long.push(0);
        let mut list: Vec<*const u8> = std::vec![long.as_ptr()];
        let names: Vec<std::string::String> = (0..200).map(|i| std::format!("E{i}=v\0")).collect();
        list.extend(names.iter().map(|n| n.as_ptr()));
        list.push(core::ptr::null());
        unsafe { adopt_initial_envp(list.as_mut_ptr()) };
        init_environ();
        assert_eq!(listed().len(), 201);
        assert_eq!(getenv_bytes(b"PATH").map(<[u8]>::len), Some(400));
        assert_eq!(getenv_bytes(b"E199"), Some(&b"v"[..]));
        clearenv();
    }

    #[test]
    fn no_start_up_list_leaves_an_empty_environ() {
        let _g = reset();
        unsafe { adopt_initial_envp(core::ptr::null_mut()) };
        init_environ();
        assert!(listed().is_empty());
    }
}
