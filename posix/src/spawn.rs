// Indexing and arithmetic in this file operate on:
//
//  - Fixed-size packed argv/envp byte buffers (`EXEC_PACKED_MAX`) with
//    each write preceded by a `len + needed <= EXEC_PACKED_MAX` check.
//  - File-action arrays and fd-map slots bounded by `MAX_FD_MAP`.
//  - Resolved-path buffers of length `PATH_MAX` written by
//    `resolve_path` which itself returns the validated length.
//
// Bounds are established locally but clippy cannot see across the
// check.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

//! POSIX process spawning functions.
//!
//! Implements `posix_spawn`, `posix_spawnp`, `execve`, `execvp`, and
//! `execv`.
//!
//! ## How It Works
//!
//! Our kernel's `SYS_PROCESS_SPAWN_EX` and `SYS_PROCESS_EXEC` take raw
//! ELF data in memory, not file paths.  This module bridges the gap:
//!
//! 1. Open the file once, read-only (`SYS_FS_OPEN`), and `fstat` that
//!    handle for its type and size — see `load_elf` for why one handle
//!    rather than a `stat` and a read by path
//! 2. Allocate a buffer via mmap
//! 3. Read the ELF binary through the same handle (`SYS_FS_READ`)
//! 4. Pass the raw bytes to `SYS_PROCESS_SPAWN_EX` (with argv/envp)
//!    or `SYS_PROCESS_EXEC`
//! 5. Free the buffer via munmap
//!
//! ## PATH Search
//!
//! `posix_spawnp` and `execvp` support PATH-based executable lookup.
//! If the filename contains a `/`, it is used directly.  Otherwise,
//! each directory in the `PATH` environment variable (or the default
//! `/bin:/usr/bin`) is tried with the filename appended.  The first
//! path that exists (per `SYS_FS_STAT`) is used.
//!
//! ## Argument and Environment Passing
//!
//! `posix_spawn` packs `argv` and `envp` C string arrays into contiguous
//! null-terminated buffers and passes them to the kernel via the
//! `SpawnExArgs` struct.  The child retrieves them during startup via
//! `SYS_PROCESS_GET_ARGS` (handled in `crt.rs`).
//!
//! `execve` passes argv/envp via `SYS_PROCESS_EXEC` args 2–5.
//!
//! ## File Descriptor Inheritance
//!
//! `posix_spawn` builds an fd_map from the parent's fd table and
//! file_actions, then passes it to the kernel via `SYS_PROCESS_SPAWN_EX`.
//! The child retrieves inherited fds during startup via
//! `SYS_PROCESS_GET_INITIAL_FDS` (handled in `crt.rs`) and reinitializes
//! its fd table accordingly.
//!
//! File actions are applied in order against a virtual fd table seeded
//! from the parent's inheritable (non-`FD_CLOEXEC`) fds:
//! - **close**: removes the fd from the child's view
//! - **dup2**: copies a handle from one fd to another
//! - **open**: opens the file in the parent's context (raw syscall) and
//!   records the kernel handle for inheritance.  The handles are closed
//!   in the parent after the spawn syscall completes.
//!
//! ## Limitations
//! - `posix_spawnattr` flags are stored but only `POSIX_SPAWN_SETPGROUP`
//!   is meaningfully supported (spawn attributes are recorded for
//!   forward compatibility).

use crate::errno;
use crate::mman;
#[cfg(target_os = "none")]
use crate::printf::{VaList, va_trampoline};
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// ABI types (must match kernel's proc/spawn.rs layout)
// ---------------------------------------------------------------------------

/// Extended spawn arguments struct passed to `SYS_PROCESS_SPAWN_EX`.
///
/// A single pointer to this struct is passed in arg0.  All pointer
/// fields must point to valid memory for the duration of the syscall.
/// Layout must match kernel's `SpawnExArgs` exactly (C ABI, all u64).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnExArgs {
    /// Pointer to ELF data in memory.
    pub elf_ptr: u64,
    /// Length of ELF data in bytes.
    pub elf_len: u64,
    /// Pointer to process name string (UTF-8).
    pub name_ptr: u64,
    /// Length of name string in bytes.
    pub name_len: u64,
    /// Pointer to `FdMapEntry` array (0 = no fd inheritance).
    pub fd_map_ptr: u64,
    /// Number of `FdMapEntry` entries.
    pub fd_map_count: u64,
    /// Pointer to packed null-terminated argv string data.
    pub argv_ptr: u64,
    /// Total byte length of the packed argv data.
    pub argv_len: u64,
    /// Number of arguments.
    pub argc: u64,
    /// Pointer to packed null-terminated envp string data.
    pub envp_ptr: u64,
    /// Total byte length of the packed envp data.
    pub envp_len: u64,
    /// Number of environment variables.
    pub envc: u64,
}

/// Extended spawn arguments, **version 2**, passed to
/// [`SYS_PROCESS_SPAWN_EX2`].
///
/// [`SpawnExArgs`] plus a leading `struct_size` and a capability policy.
/// Layout must match the kernel's `SpawnEx2Args` (`kernel/src/proc/spawn.rs`)
/// exactly: C ABI, eighteen `u64`s, 144 bytes.
///
/// # Why the size field
///
/// So a *third* syscall number is never needed. `struct_size` is field 0, so
/// the kernel can accept a struct shorter than it expects (an older caller —
/// the missing tail is zero-filled, and **every field's zero value is its
/// version-1 behaviour**) and reject a longer one whose extra bytes are
/// non-zero (a newer caller asking for something this kernel cannot do).
///
/// That refusal is the point, not pedantry: the fields most likely to be added
/// to a spawn struct are *restrictions* — `no_new_privs`, a seccomp filter, a
/// namespace. A kernel that silently ignored one would turn a sandbox request
/// into a no-op with no way for the caller to find out.
///
/// Always set `struct_size` to `size_of::<SpawnEx2Args>()` and the right thing
/// happens whichever side is newer. [`spawn_ex2_args`] does that for you.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnEx2Args {
    /// `size_of::<SpawnEx2Args>()` as *this* build knows it, in bytes.
    pub struct_size: u64,
    /// Pointer to ELF data in memory.
    pub elf_ptr: u64,
    /// Length of ELF data in bytes.
    pub elf_len: u64,
    /// Pointer to process name string (UTF-8).
    pub name_ptr: u64,
    /// Length of name string in bytes.
    pub name_len: u64,
    /// Pointer to `FdMapEntry` array (0 = no fd inheritance).
    pub fd_map_ptr: u64,
    /// Number of `FdMapEntry` entries.
    pub fd_map_count: u64,
    /// Pointer to packed null-terminated argv string data.
    pub argv_ptr: u64,
    /// Total byte length of the packed argv data.
    pub argv_len: u64,
    /// Number of arguments.
    pub argc: u64,
    /// Pointer to packed null-terminated envp string data.
    pub envp_ptr: u64,
    /// Total byte length of the packed envp data.
    pub envp_len: u64,
    /// Number of environment variables.
    pub envc: u64,
    /// How much of the caller's capability table the child receives:
    /// [`SPAWN_CAP_MODE_INHERIT_ALL`] or [`SPAWN_CAP_MODE_SUBSET`].
    ///
    /// Any other value is `InvalidArgument` — not clamped and not defaulted.
    /// A caller who asked for a policy this kernel does not implement must not
    /// be handed a *wider* one.
    pub cap_mode: u64,
    /// Pointer to a [`CapEntryInfo`] array. Read only when `cap_mode` is
    /// [`SPAWN_CAP_MODE_SUBSET`].
    pub cap_ptr: u64,
    /// Number of entries at `cap_ptr`.
    pub cap_count: u64,
    /// The directory the child starts in, for `posix_spawn_file_actions_addchdir_np`:
    /// a pointer to the path bytes, no NUL. Read only when `cwd_len != 0`.
    ///
    /// Zero length -- which is also what a kernel reads for a caller whose
    /// `struct_size` stops before these two fields -- means the child starts
    /// in its parent's directory, as POSIX requires of `posix_spawn`
    /// (design-decisions.md §960). Otherwise the path must be canonical: this
    /// libc applies the `chdir` actions in order, against the directory each
    /// leaves the child in, and checks the result is a directory before it
    /// gets here. A kernel older than these fields reads them as a non-zero
    /// tail and refuses the spawn, rather than starting the child somewhere
    /// its caller did not ask for.
    pub cwd_ptr: u64,
    /// Length of the path at `cwd_ptr`, at most [`crate::syscall::CWD_RECORD_MAX`].
    pub cwd_len: u64,
}

/// A mismatch here is an ABI break that would show up as the kernel reading a
/// pointer out of the wrong field, so fail the build instead of the spawn.
/// This struct's entire compatibility story rests on both sides agreeing on
/// the size, which is exactly the thing a `const` assertion can guarantee and
/// a test can only observe on a run somebody makes.
const _: () = {
    assert!(size_of::<SpawnEx2Args>() == 144);
    assert!(align_of::<SpawnEx2Args>() == 8);
    // The prefix through `envc` must be layout-identical to `SpawnExArgs`, or
    // "version 1 plus a size field" is not what we are sending.
    assert!(size_of::<SpawnExArgs>() == 96);
    assert!(SPAWN_EX2_MIN_SIZE as usize == size_of::<SpawnExArgs>() + 8);
};

/// `cap_mode`: the child inherits the parent's entire capability table.
///
/// Zero so that a zero-filled tail reproduces `SYS_PROCESS_SPAWN_EX`'s
/// behaviour exactly.
pub const SPAWN_CAP_MODE_INHERIT_ALL: u64 = 0;

/// `cap_mode`: the child inherits exactly the listed capabilities, and nothing
/// else. A count of zero is legal and means the child gets **nothing**.
pub const SPAWN_CAP_MODE_SUBSET: u64 = 1;

/// The shortest `struct_size` the kernel accepts: through `envc`.
///
/// `13 * 8` — `struct_size` plus the twelve fields [`SpawnExArgs`] carries.
/// A caller may stop here and get version-1 behaviour with a size field.
pub const SPAWN_EX2_MIN_SIZE: u64 = 13 * 8;

/// The largest `cap_count` the kernel will read: `kernel/src/cap/table.rs`'s
/// `MAX_ENTRIES`, via `kernel/src/proc/spawn.rs`'s `SPAWN_CAP_MAX`.
///
/// This is the capacity of a capability *table*, so a larger request could not
/// be satisfied even if every entry in it were legitimate — the child has
/// nowhere to put them. The kernel answers `InvalidArgument` (from
/// `read_user_items`, before it reads a single entry); we answer `EINVAL`
/// locally for the same reason, so a caller that built an oversized list finds
/// out without a syscall and without a partially-validated request.
pub const SPAWN_CAP_MAX: usize = 4096;

/// One capability as the kernel enumerates and accepts it.
///
/// Re-exported rather than redeclared so that building a subset is
/// enumerate → filter → pass back with **no transcription step**: the same
/// 24-byte struct `SYS_CAP_QUERY` writes out is the one
/// [`SYS_PROCESS_SPAWN_EX2`] reads. A transcription step between "what I hold"
/// and "what I delegate" is a place to get a field wrong.
///
/// `reserved` is validated by the kernel, not skipped — zero it.
pub use crate::sys_capability::kernel_view::CapEntryInfo;

/// Build a [`SpawnEx2Args`] with the size field and capability policy already
/// correct, leaving every other field zero for the caller to fill in.
///
/// Exists so no call site ever writes `struct_size` by hand. A literal there
/// would be a number that is right until someone adds a field, and wrong in
/// the direction that makes the kernel read past what was written.
///
/// `caps` of `None` means [`SPAWN_CAP_MODE_INHERIT_ALL`]; `Some(slice)` means
/// exactly that slice, including `Some(&[])` for "no capabilities at all".
#[must_use]
pub fn spawn_ex2_args(caps: Option<&[CapEntryInfo]>) -> SpawnEx2Args {
    let (cap_mode, cap_ptr, cap_count) = match caps {
        None => (SPAWN_CAP_MODE_INHERIT_ALL, 0, 0),
        // A null pointer is *not* "the empty list" here, unlike the fd map and
        // argv in version 1: the kernel rejects `cap_ptr == 0` with a non-zero
        // count, and an empty subset must still be an explicit request for
        // nothing. `[].as_ptr()` is a dangling-but-aligned non-null pointer,
        // which is what the kernel expects to never read.
        Some(list) => (
            SPAWN_CAP_MODE_SUBSET,
            list.as_ptr() as u64,
            list.len() as u64,
        ),
    };
    SpawnEx2Args {
        struct_size: size_of::<SpawnEx2Args>() as u64,
        elf_ptr: 0,
        elf_len: 0,
        name_ptr: 0,
        name_len: 0,
        fd_map_ptr: 0,
        fd_map_count: 0,
        argv_ptr: 0,
        argv_len: 0,
        argc: 0,
        envp_ptr: 0,
        envp_len: 0,
        envc: 0,
        cap_mode,
        cap_ptr,
        cap_count,
        cwd_ptr: 0,
        cwd_len: 0,
    }
}

/// Header returned by `SYS_PROCESS_GET_ARGS`.
///
/// Prefixed to the output buffer, followed by packed argv strings
/// then packed envp strings.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnArgsHeader {
    /// Number of argv entries.
    pub argc: u32,
    /// Number of envp entries.
    pub envc: u32,
    /// Total bytes of packed argv data (including null terminators).
    pub argv_data_len: u32,
    /// Total bytes of packed envp data (including null terminators).
    pub envp_data_len: u32,
}

// ---------------------------------------------------------------------------
// FdMapEntry — file descriptor inheritance ABI
// ---------------------------------------------------------------------------

/// Handle type constants for `FdMapEntry`.
///
/// Must match `kernel/src/proc/spawn.rs fd_handle_type`.
pub mod fd_handle_type {
    /// Regular file handle (kernel dups via `fs::handle::dup()`).
    pub const FILE: u8 = 0;
    /// Pipe handle (raw pass-through — no kernel-level dup yet).
    pub const PIPE: u8 = 1;
    /// TCP socket handle.
    pub const TCP_SOCKET: u8 = 2;
    /// UDP socket handle.
    pub const UDP_SOCKET: u8 = 3;
    /// Console I/O (stdin/stdout/stderr virtual handle).
    pub const CONSOLE: u8 = 4;
    /// Eventfd counter handle (raw pass-through — no kernel-level dup
    /// yet; closing from either side closes for both).
    pub const EVENTFD: u8 = 5;
    /// Unix-domain stream socket endpoint (`socketpair`).  The kernel
    /// dups the endpoint into the child via `stream_socket::dup()`,
    /// which bumps the endpoint refcount.
    pub const STREAM_SOCKET: u8 = 6;
    /// Either end of a pty.  One constant covers both because `PtyHandle` is
    /// `(tty_id << 1) | end` — the low bit already distinguishes them, and a
    /// second constant would only create a place for the two encodings to
    /// disagree.  The kernel dups through `pty::dup()`, which refcounts the
    /// *end*.
    ///
    /// **This entry is ownership-gated, uniquely in this table.**  Naming a pty
    /// handle the calling process does not hold fails the whole spawn with
    /// `InvalidHandle` — not silently dropped, not partially applied.  The
    /// asymmetry is deliberate on the kernel's part: a `PtyHandle` is guessable
    /// by construction, unlike every other handle family here, and a master is
    /// the authority to type arbitrary bytes into a stranger's shell.  Since
    /// this loop maps only the *parent's own* open descriptors, the gate never
    /// fires for a spawn built here; it exists for a hand-built `fd_map`.
    ///
    /// Nothing on this path calls `SYS_PTY_DUP` on the caller's behalf, and
    /// `dup` must keep not calling it: spawn takes exactly one reference per
    /// `fd_map` entry — deliberately *not* deduped the way `linux_fd_redirects`
    /// dedups aliases, since that path moves one handle into several
    /// descriptors and so registers once, while this one dups per entry.
    pub const PTY: u8 = 7;
}

/// A file descriptor mapping entry for `SYS_PROCESS_SPAWN_EX`.
///
/// Tells the kernel which of the parent's handles the child should
/// inherit and at which POSIX fd numbers.  Layout must match
/// `kernel/src/proc/spawn.rs FdMapEntry` exactly (16 bytes, C ABI).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FdMapEntry {
    /// Target POSIX fd number in the child.
    pub fd: i32,
    /// Handle type (see [`fd_handle_type`] constants).
    pub handle_type: u8,
    /// Reserved padding (set to 0).
    pub _pad: [u8; 3],
    /// Parent's kernel handle to dup into the child.
    pub handle: u64,
}

/// Maximum number of fd mappings we can build: one per slot of this libc's fd
/// table, which is also what the kernel accepts (`FD_MAP_MAX`, 256).
///
/// It was 32 until 2026-09-24 — "three standard fds + the file actions
/// limit" — and every descriptor from 32 up was therefore dropped from every
/// child, across `posix_spawn` and across `exec`, without a word: the loop
/// that seeds the child's table simply stopped at 32. A shell holding a
/// here-document or a saved descriptor at fd 40 lost it in every command it
/// ran. The child's receiving buffer (`crt.rs`'s `MAX_INIT_FDS`) had the same
/// number and was raised with it.
pub(crate) const MAX_FD_MAP: usize = crate::fdtable::MAX_FDS;

// ---------------------------------------------------------------------------
// posix_spawn_file_actions
// ---------------------------------------------------------------------------

/// How many action slots the first `add*` allocates; the array doubles each
/// time it fills. glibc's `__posix_spawn_file_actions_realloc` starts at 8 and
/// doubles too, and like glibc there is no limit but memory.
///
/// There was one, 16 actions with paths of at most 255 bytes, stored inline,
/// until 2026-09-25. Both caps were chosen when this libc's `malloc` was one
/// mmap per allocation, so every allocation, however small, cost a 16 KiB
/// region. Since the heap became dlmalloc (design-decisions.md §1101) a small
/// allocation costs a small allocation, and the caps were refusing real
/// programs for no remaining reason: a `posix_spawn` with a path longer than
/// 255 bytes failed with `ENAMETOOLONG` that glibc never gives.
const INITIAL_FILE_ACTIONS: usize = 8;

/// File actions object for `posix_spawn` — **exactly the 80 bytes every C
/// header declares**, with the actions themselves on the heap.
///
/// # This type's size is its ABI, and getting it wrong is silent
///
/// `posix_spawn_file_actions_t` is an *opaque* type, but it is not an
/// incomplete one: every libc header defines it as a struct with a fixed
/// size, and every C caller therefore allocates that many bytes — almost
/// always on the stack, since the object is scoped to one spawn.  Both
/// references agree on 80:
///
/// ```c
/// /* musl, bits/spawn.h */          /* glibc, bits/spawn_faction.h */
/// typedef struct {                  typedef struct {
///   int __pad0[2];                    int __allocated;
///   void *__actions;                  int __used;
///   int __pad[16];                    struct __spawn_action *__actions;
/// } posix_spawn_file_actions_t;       int __pad[16];
///                                   } posix_spawn_file_actions_t;
/// ```
///
/// This struct used to store its sixteen 288-byte action slots *inline*,
/// making it 4624 bytes.  Nothing in Rust noticed, because every Rust caller
/// used the same definition and agreed with itself.  What it broke was C:
/// `posix_spawn_file_actions_init` wrote 4616 bytes into an 80-byte stack
/// object, so the object's own frame — locals, saved registers, the return
/// address — was overwritten with zeroes on the *first* call, in a function
/// that had not done anything wrong.
///
/// GNU make hit this on every recipe (`child_execute_job`, job.c, which puts
/// the attr and the file-actions objects side by side on the stack), and the
/// symptom was a null-pointer read a hundred instructions later, of a
/// perfectly ordinary local that a memset had quietly cleared.  See
/// known-issues.md `B-POSIX-SPAWN-FILE-ACTIONS-WAS-4624-BYTES-OF-AN-80-BYTE-C-TYPE`.
///
/// The field names below are glibc's because glibc's are meaningful; the
/// layout is byte-identical to musl's, whose first two `int`s are exactly
/// these two.  Nothing outside this module interprets the bytes — a C caller
/// only ever passes the object back to us — so matching the *size* is what
/// is load-bearing and matching the *names* is documentation.
///
/// [`test_file_actions_matches_musl_layout`] asserts the size and every
/// offset, so a future field addition cannot silently break the ABI again.
/// [`PosixSpawnattrT`] had that guard from the start and was correct; this
/// type did not, and was wrong by 4544 bytes for as long as it existed.
#[repr(C)]
pub struct PosixSpawnFileActionsT {
    /// Slots behind `actions`.  Zero until the first `add*` allocates.
    allocated: i32,
    /// Slots in use; the count `posix_spawn` replays, in order.
    used: i32,
    /// Heap array of `allocated` slots, or null before the first `add*`.
    ///
    /// Heap rather than inline is not a preference: 16 × 288 bytes cannot be
    /// made to fit in 80 no matter how the slot is packed, and 80 is not
    /// ours to choose.  Both references do the same thing for the same
    /// reason.
    actions: *mut FileActionSlot,
    /// glibc/musl `__pad[16]`.  Never read.  Present so that `size_of` is 80
    /// and a C caller's stack frame is the size we think it is.
    _pad: [i32; 16],
}

/// The 80 bytes, asserted at compile time rather than at test time.
///
/// Lane A asked for this specifically when it filed the bug, and the argument
/// is `kernel/src/cap/rights.rs`'s: the two halves of this contract live in
/// different languages compiled from different headers, so **no single diff
/// ever contains both**. A `#[test]` can only catch the mistake on a run that
/// someone remembers to do; a `const` block makes the mistake impossible to
/// build. The `#[test]` below additionally pins the field offsets, which is
/// the part a const block cannot express.
const _: () = {
    assert!(
        size_of::<PosixSpawnFileActionsT>() == 80,
        "posix_spawn_file_actions_t is 80 bytes in musl and glibc alike; a \
         larger Rust definition makes posix_spawn_file_actions_init overrun \
         the C caller's stack object"
    );
    assert!(align_of::<PosixSpawnFileActionsT>() == 8);
};

impl PosixSpawnFileActionsT {
    /// Actions recorded so far.
    ///
    /// `used` is `i32` to match the C layout, so it is narrowed here once
    /// rather than at each call site.  It is only ever advanced from 0 by
    /// `push`, which keeps it at most `allocated`, so the value is
    /// non-negative by construction.
    fn count(&self) -> usize {
        usize::try_from(self.used).unwrap_or(0)
    }

    /// Slots allocated behind `actions`; see [`Self::count`] on the narrowing.
    fn capacity(&self) -> usize {
        usize::try_from(self.allocated).unwrap_or(0)
    }

    /// The recorded actions, in the order they were added.
    ///
    /// Empty — not a dangling slice — before the first `add*`, because
    /// `actions` is null then and `used` is 0.
    fn slots(&self) -> &[FileActionSlot] {
        if self.actions.is_null() {
            return &[];
        }
        // SAFETY: `actions` is non-null, so it came from `grow`, which
        // allocated `allocated` slots; `push` initialised the first `used` of
        // them and keeps `used <= allocated`.
        unsafe { core::slice::from_raw_parts(self.actions, self.count()) }
    }

    /// Append one action, growing the slot array when it is full.
    ///
    /// Takes ownership of `slot`, its path included: the object owns it from
    /// here until [`Self::release`], and if the append fails the path is freed
    /// now. Returns 0, or `ENOMEM` if the array could not grow -- the error
    /// POSIX gives the `posix_spawn_file_actions_add*` functions for running
    /// out of memory.
    fn push(&mut self, slot: FileActionSlot) -> i32 {
        if (self.actions.is_null() || self.count() >= self.capacity()) && self.grow().is_none() {
            slot.free_path();
            return errno::ENOMEM;
        }
        let idx = self.count();
        // SAFETY: `actions` holds `allocated` slots (`grow`), and
        // `idx = used < allocated`, checked just above. The slot at `idx` is
        // uninitialised memory, so it is written, not assigned.
        unsafe { self.actions.add(idx).write(slot) };
        self.used = self.used.saturating_add(1);
        0
    }

    /// Double the slot array (or allocate its first `INITIAL_FILE_ACTIONS`).
    ///
    /// `realloc` moves the recorded slots with the block. That is sound: a
    /// slot is plain data and a pointer to a path allocated separately, and
    /// nothing points *into* the array.
    fn grow(&mut self) -> Option<()> {
        let new_cap = match self.capacity() {
            0 => INITIAL_FILE_ACTIONS,
            cap => cap.checked_mul(2)?,
        };
        // `allocated` is a C `int`; an array that cannot be counted in one is
        // refused here as the allocation failure it would be anyway.
        let new_allocated = i32::try_from(new_cap).ok()?;
        let bytes = new_cap.checked_mul(size_of::<FileActionSlot>())?;
        // SAFETY: `actions` is null or this object's own block from an earlier
        // `grow`; `realloc(NULL, n)` is `malloc(n)`.
        let raw = unsafe { crate::malloc::realloc(self.actions.cast::<u8>(), bytes) };
        if raw.is_null() {
            return None;
        }
        self.actions = raw.cast::<FileActionSlot>();
        self.allocated = new_allocated;
        Some(())
    }

    /// Release the slot array and return to the just-initialised state.
    ///
    /// Idempotent, so a caller that destroys twice — or destroys an object it
    /// only ever `init`ed — does not double-free.
    fn release(&mut self) {
        for slot in self.slots() {
            slot.free_path();
        }
        if !self.actions.is_null() {
            // SAFETY: `actions` came from `crate::malloc::realloc` in `grow`
            // and is freed exactly once, because it is nulled immediately.
            unsafe { crate::malloc::free(self.actions.cast::<u8>()) };
        }
        self.actions = core::ptr::null_mut();
        self.allocated = 0;
        self.used = 0;
    }
}

/// One recorded file action.
///
/// `path` is the action's own copy of the caller's path -- a `malloc` block,
/// NUL-terminated, owned by the object holding the slot and freed by its
/// `release` -- or null for the actions that name no path.
#[repr(C)]
struct FileActionSlot {
    /// 1 = Close, 2 = Dup2, 3 = Open, 4 = Chdir, 5 = Closefrom, 6 = Fchdir.
    tag: u8,
    fd: Fd,
    newfd: Fd,
    oflag: i32,
    mode: ModeT,
    path: *mut u8,
    path_len: usize,
}

impl FileActionSlot {
    const fn empty() -> Self {
        Self {
            tag: 0,
            fd: 0,
            newfd: 0,
            oflag: 0,
            mode: 0,
            path: core::ptr::null_mut(),
            path_len: 0,
        }
    }

    /// The path, without its terminator; empty for an action with none.
    fn path_bytes(&self) -> &[u8] {
        if self.path.is_null() {
            return &[];
        }
        // SAFETY: a non-null `path` is `dup_path`'s block of `path_len + 1`
        // bytes, which lives until `release` frees it.
        unsafe { core::slice::from_raw_parts(self.path, self.path_len) }
    }

    /// The path with its terminator, as `open` takes it; `None` for an
    /// action with none.
    fn path_cstr(&self) -> Option<&[u8]> {
        if self.path.is_null() {
            return None;
        }
        let len = self.path_len.checked_add(1)?;
        // SAFETY: as in `path_bytes`; the block includes the terminator.
        Some(unsafe { core::slice::from_raw_parts(self.path, len) })
    }

    /// Free the path, if there is one. Only `release` and a failed `push`
    /// call this, and neither uses the slot again.
    fn free_path(&self) {
        if !self.path.is_null() {
            // SAFETY: a non-null `path` is `dup_path`'s `malloc` block, and
            // each slot's is freed once: by `release`, or by the `push` that
            // failed to take the slot.
            unsafe { crate::malloc::free(self.path) };
        }
    }
}

/// Copy the C string `path` into a `malloc` block of its own, terminator and
/// all, returning the block and the length without the terminator. `None` if
/// the allocation fails.
///
/// glibc's `add*` functions `strdup` their path the same way, and accept any
/// length: a path too long to use fails the spawn, with `ENAMETOOLONG`, when
/// the action is carried out.
///
/// # Safety
///
/// `path` must be a valid, NUL-terminated C string.
unsafe fn dup_path(path: *const u8) -> Option<(*mut u8, usize)> {
    // SAFETY: `path` is a C string (this function's contract).
    let len = unsafe { crate::file::c_strlen_pub(path) };
    let block = crate::malloc::malloc(len.checked_add(1)?);
    if block.is_null() {
        return None;
    }
    // SAFETY: `block` holds `len + 1` fresh bytes and cannot overlap `path`,
    // which is readable for `len` bytes (`c_strlen_pub`).
    unsafe {
        core::ptr::copy_nonoverlapping(path, block, len);
        block.add(len).write(0);
    }
    Some((block, len))
}

/// Is `fd` acceptable to a `posix_spawn_file_actions_add*` call?
///
/// glibc's `__spawn_valid_fd` (posix/spawn_valid_fd.c) is
/// `fd >= 0 && (maxfd < 0 || fd < maxfd)` where `maxfd` is
/// `sysconf (_SC_OPEN_MAX)`.  Two things follow that our earlier code got
/// wrong: the rejection is `EBADF`, not `EINVAL`, and it also covers a
/// *too-large* fd, not just a negative one.
///
/// It matters that this runs before the object and the path are touched —
/// every `add*` that takes an fd calls it as its first statement, ahead of
/// `__strdup (path)` and ahead of any read of `file_actions->__used`.  See
/// design-decisions.md §303 for why that ordering is the ABI.
fn spawn_valid_fd(fd: Fd) -> bool {
    let maxfd = crate::unistd::sysconf(crate::unistd::_SC_OPEN_MAX);
    fd >= 0 && (maxfd < 0 || i64::from(fd) < maxfd)
}

/// Initialize a file actions object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_init(acts: *mut PosixSpawnFileActionsT) -> i32 {
    if acts.is_null() {
        return errno::EFAULT;
    }
    // Writes exactly the 80 bytes the C caller allocated, and no more.  Both
    // references do the same: glibc's `__posix_spawn_file_actions_init` is a
    // `memset (file_actions, '\0', sizeof (*file_actions))` and musl's is
    // `*fa = (posix_spawn_file_actions_t){0}`.  Storage for the actions is
    // deferred to the first `add*`, so this cannot fail.
    //
    // SAFETY: `acts` is non-null and the caller guarantees it points to
    // writable memory of at least `size_of::<PosixSpawnFileActionsT>()`,
    // which is now the 80 bytes their header declares.
    unsafe {
        (*acts).allocated = 0;
        (*acts).used = 0;
        (*acts).actions = core::ptr::null_mut();
        (*acts)._pad = [0; 16];
    }
    0
}

/// Destroy a file actions object.
///
/// Frees every action's copy of its path, then the slot array.  A caller
/// that `init`ed and never added anything frees nothing, and a caller that
/// destroys twice is safe: `release` nulls the pointer and zeroes the count
/// as it frees them.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_destroy(acts: *mut PosixSpawnFileActionsT) -> i32 {
    if !acts.is_null() {
        // SAFETY: acts is non-null (checked above).
        unsafe { (*acts).release() };
    }
    0
}

/// Add a close action.
///
/// The fd will be closed in the child before exec.
///
/// `__posix_spawn_file_actions_addclose` (posix/spawn_faction_addclose.c)
/// opens with `if (!__spawn_valid_fd (fd)) return EBADF;` — before it reads
/// `file_actions->__used` — so the descriptor verdict outranks the (absent in
/// glibc, `EFAULT` here) NULL check on the object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addclose(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
) -> i32 {
    if !spawn_valid_fd(fd) {
        return errno::EBADF;
    }
    if acts.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    a.push(FileActionSlot {
        tag: 1,
        fd,
        ..FileActionSlot::empty()
    })
}

/// Add a dup2 action.
///
/// In the child, `dup2(fd, newfd)` will be called before exec.
///
/// `__posix_spawn_file_actions_adddup2` (posix/spawn_faction_adddup2.c:32)
/// tests `!__spawn_valid_fd (fd) || !__spawn_valid_fd (newfd)` first and
/// returns `EBADF` for either.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_adddup2(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
    newfd: Fd,
) -> i32 {
    if !spawn_valid_fd(fd) || !spawn_valid_fd(newfd) {
        return errno::EBADF;
    }
    if acts.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    a.push(FileActionSlot {
        tag: 2,
        fd,
        newfd,
        ..FileActionSlot::empty()
    })
}

/// Add an open action.
///
/// In the child, the file at `path` will be opened with `oflag`/`mode`
/// and the resulting fd will be dup2'd to `fd`.
///
/// `__posix_spawn_file_actions_addopen` (posix/spawn_faction_addopen.c) is
/// `if (!__spawn_valid_fd (fd)) return EBADF;` and only then
/// `__strdup (path)`, so a bad descriptor outranks a NULL path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addopen(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
    path: *const u8,
    oflag: i32,
    mode: ModeT,
) -> i32 {
    if !spawn_valid_fd(fd) {
        return errno::EBADF;
    }
    if acts.is_null() || path.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    // SAFETY: path is non-null (checked above) and a C string by the caller's
    // contract.
    let Some((stored, path_len)) = (unsafe { dup_path(path) }) else {
        return errno::ENOMEM;
    };
    a.push(FileActionSlot {
        tag: 3,
        fd,
        oflag,
        mode,
        path: stored,
        path_len,
        ..FileActionSlot::empty()
    })
}

// ---------------------------------------------------------------------------
// posix_spawn_file_actions_addchdir_np
// ---------------------------------------------------------------------------

/// Add a change-directory action to a spawn file actions object.
///
/// This is a glibc/macOS extension (`_np` = non-portable).  In the
/// child process, the working directory will be changed to `path`
/// before executing the program.
///
/// The child is a new process rather than a forked copy of this one, so the
/// action is carried out here, in order with the others: `path` is resolved
/// against the directory the earlier actions left the child in, must be a
/// directory, and becomes where later relative `open` actions are resolved.
/// The kernel is then told to start the child there (`SpawnEx2Args::cwd_ptr`).
/// A spawn whose `chdir` fails, fails -- `ENOENT`, `ENOTDIR` and the rest, as
/// `chdir` itself would.
///
/// Returns 0 on success, or a POSIX error code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addchdir_np(
    acts: *mut PosixSpawnFileActionsT,
    path: *const u8,
) -> i32 {
    if acts.is_null() || path.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    // SAFETY: path is non-null (checked above) and a C string by the caller's
    // contract.
    let Some((stored, path_len)) = (unsafe { dup_path(path) }) else {
        return errno::ENOMEM;
    };
    // Tag 4: carried out by `ChildCwd::change_to` when the spawn runs.
    a.push(FileActionSlot {
        tag: 4,
        fd: -1,
        path: stored,
        path_len,
        ..FileActionSlot::empty()
    })
}

// ---------------------------------------------------------------------------
// posix_spawn_file_actions_addfchdir_np
// ---------------------------------------------------------------------------

/// Add a change-directory action naming the directory by descriptor
/// (glibc 2.29's `posix_spawn_file_actions_addfchdir_np`).
///
/// Carried out in order with the others, as `addchdir_np` is: `fd` is the
/// child's descriptor as the earlier actions left it -- an `addopen` of a
/// directory just before is the usual source -- and the child starts in the
/// directory it names. The spawn fails with `EBADF` if `fd` is not open in the
/// child then, and `ENOTDIR` if it is not a directory.
///
/// `EBADF` now for an `fd` no descriptor can have, as glibc's
/// `__spawn_valid_fd` does before touching the object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addfchdir_np(
    acts: *mut PosixSpawnFileActionsT,
    fd: Fd,
) -> i32 {
    if !spawn_valid_fd(fd) {
        return errno::EBADF;
    }
    if acts.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: acts is non-null (checked above).
    let a = unsafe { &mut *acts };
    // Tag 6: carried out by `apply_file_actions`.
    a.push(FileActionSlot {
        tag: 6,
        fd,
        ..FileActionSlot::empty()
    })
}

// ---------------------------------------------------------------------------
// posix_spawn_file_actions_addclosefrom_np — close all fds >= lowfd
// ---------------------------------------------------------------------------

/// Record a "close all fds from `lowfd` upward" action.
///
/// Non-portable glibc/macOS extension.  During `posix_spawn`, all
/// file descriptors ≥ `lowfd` will be closed in the child.
///
/// We store this as tag 5 (closefrom), with `fd` set to `lowfd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn_file_actions_addclosefrom_np(
    acts: *mut PosixSpawnFileActionsT,
    lowfd: i32,
) -> i32 {
    // `__posix_spawn_file_actions_addclosefrom` (posix/spawn_faction_addclosefrom.c:31)
    // is `if (!__spawn_valid_fd (from)) return EBADF;` before anything else.
    if !spawn_valid_fd(lowfd) {
        return errno::EBADF;
    }
    if acts.is_null() {
        return errno::EFAULT;
    }
    let a = unsafe { &mut *acts };
    // Tag 5 = Closefrom action.
    a.push(FileActionSlot {
        tag: 5,
        fd: lowfd,
        ..FileActionSlot::empty()
    })
}

// ---------------------------------------------------------------------------
// posix_spawnattr
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Spawn attribute flag constants
//
// The values are fixed by POSIX.1-2008 / POSIX.1-2017 and the GNU
// extensions, and match the bit layout used by glibc, FreeBSD, and
// musl.  They are exposed publicly so callers (and our own tests)
// can compose flag words by name without hard-coding magic numbers.
// ---------------------------------------------------------------------------

/// Reset effective uid/gid to real uid/gid in the child.
pub const POSIX_SPAWN_RESETIDS: i16 = 0x01;
/// Place the child in the process group given by `pgroup`.
pub const POSIX_SPAWN_SETPGROUP: i16 = 0x02;
/// Reset signals listed in `sigdefault` to SIG_DFL in the child.
pub const POSIX_SPAWN_SETSIGDEF: i16 = 0x04;
/// Replace the child's signal mask with `sigmask`.
pub const POSIX_SPAWN_SETSIGMASK: i16 = 0x08;
/// Apply `schedparam` to the child (with the current scheduler).
pub const POSIX_SPAWN_SETSCHEDPARAM: i16 = 0x10;
/// Apply `schedpolicy` and `schedparam` to the child.
pub const POSIX_SPAWN_SETSCHEDULER: i16 = 0x20;
/// Use a vfork-style spawn for the child (GNU extension).
pub const POSIX_SPAWN_USEVFORK: i16 = 0x40;
/// Place the child in a new session (POSIX.1-2018).
pub const POSIX_SPAWN_SETSID: i16 = 0x80;

/// Union of every flag bit currently accepted by
/// `posix_spawnattr_setflags`.  Any bit outside this mask causes
/// `posix_spawnattr_setflags` to return `EINVAL`, matching glibc's
/// `__POSIX_SPAWN_MASK` check.
pub const POSIX_SPAWN_VALID_FLAGS: i16 = POSIX_SPAWN_RESETIDS
    | POSIX_SPAWN_SETPGROUP
    | POSIX_SPAWN_SETSIGDEF
    | POSIX_SPAWN_SETSIGMASK
    | POSIX_SPAWN_SETSCHEDPARAM
    | POSIX_SPAWN_SETSCHEDULER
    | POSIX_SPAWN_USEVFORK
    | POSIX_SPAWN_SETSID;

/// Spawn attributes object.
///
/// # Layout
///
/// The size and field order match musl's `posix_spawnattr_t`
/// (`include/spawn.h`), which is the ABI our cross-toolchain compiles
/// against:
///
/// ```c
/// typedef struct {
///     int __flags;  pid_t __pgrp;
///     sigset_t __def, __mask;
///     int __prio, __pol;  void *__fn;
///     char __pad[64-sizeof(void *)];
/// } posix_spawnattr_t;
/// ```
///
/// That is 336 bytes at 8-byte alignment, and so is this — the trailing
/// `_reserved` array absorbs musl's `__fn` and `__pad`, which no caller
/// may inspect.  `flags` is `i16` rather than musl's `int` because POSIX
/// types `posix_spawnattr_setflags`'s second parameter as `short`; the
/// field is private to this file, so the narrower storage is invisible
/// across the ABI and the following `pgroup` sits at offset 4 either way.
/// [`test_spawnattr_matches_musl_layout`] asserts the size and every
/// offset, so a future field addition cannot silently break the ABI.
///
/// # What is stored versus what is applied
///
/// Every field is *recorded* faithfully, and each `posix_spawnattr_get*`
/// returns exactly what the matching setter stored — a caller that
/// round-trips an attribute object gets its own value back.  Whether the
/// spawn path then *acts* on a field is a separate question, and today
/// `sigdefault`, `sigmask`, `schedpriority` and `schedpolicy` are
/// recorded but not applied: we have no POSIX signal delivery to reset
/// or block, and no per-process scheduler policy to install at spawn
/// time.
///
/// Recording them anyway is not busywork.  A `posix_spawnattr_setsigmask`
/// that returned `ENOSYS` would push every caller onto a `fork`/`exec`
/// fallback — CPython's `os.posix_spawn` does exactly that — which is a
/// worse outcome than a spawn that ignores a mask the child would have
/// inherited as empty regardless.  When signal delivery and scheduler
/// policies land, the spawn path reads these fields and no caller
/// changes.
#[repr(C)]
pub struct PosixSpawnattrT {
    /// Attribute flags (bitwise OR of POSIX_SPAWN_* constants).
    flags: i16,
    /// Process group ID (used if POSIX_SPAWN_SETPGROUP is set).
    pgroup: PidT,
    /// Signals to reset to `SIG_DFL` (used if POSIX_SPAWN_SETSIGDEF).
    sigdefault: crate::signal::SigsetT,
    /// Signal mask to install (used if POSIX_SPAWN_SETSIGMASK).
    sigmask: crate::signal::SigsetT,
    /// Scheduling priority (used if POSIX_SPAWN_SETSCHEDPARAM).
    schedpriority: i32,
    /// Scheduling policy (used if POSIX_SPAWN_SETSCHEDULER).
    schedpolicy: i32,
    /// Padding out to musl's 336-byte object.  Never read.
    _reserved: [u8; 64],
}

/// This type was already correct and already had a layout *test*.  It gets the
/// `const` too, because the test only helps on a run somebody makes; see the
/// note on `PosixSpawnFileActionsT`'s assertion for the argument.
const _: () = {
    assert!(
        size_of::<PosixSpawnattrT>() <= 336,
        "musl posix_spawnattr_t"
    );
    assert!(align_of::<PosixSpawnattrT>() <= 8);
};

/// Initialize a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_init(attr: *mut PosixSpawnattrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe {
        (*attr).flags = 0;
        (*attr).pgroup = 0;
        // POSIX leaves the *values* of unset attributes unspecified, but
        // an object whose signal sets are uninitialised stack garbage is
        // a trap for the getters: `posix_spawnattr_getsigmask` on a
        // freshly-`init`ed object would hand back whatever was on the
        // stack.  glibc's `__spawnattr_init` memsets the whole struct
        // for the same reason.
        (*attr).sigdefault = crate::signal::SigsetT::EMPTY;
        (*attr).sigmask = crate::signal::SigsetT::EMPTY;
        (*attr).schedpriority = 0;
        (*attr).schedpolicy = 0;
    }
    0
}

/// Destroy a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_destroy(_attr: *mut PosixSpawnattrT) -> i32 {
    0 // No resources to free.
}

/// Set flags on a spawn attributes object.
///
/// Returns `EINVAL` if any bit outside `POSIX_SPAWN_VALID_FLAGS` is set in
/// `flags`, or `EFAULT` if `attr` is null.  This matches POSIX:
///
/// > If the value of the attribute being set is not valid,
/// > posix_spawnattr_setflags() shall return [EINVAL].
///
/// **In that order.** `__posix_spawnattr_setflags`
/// (posix/spawnattr_setflags.c) is exactly two statements: the
/// `flags & ~ALL_FLAGS` rejection, then `attr->__flags = flags`.  It has no
/// NULL check at all — a NULL `attr` faults on the store — so the flag word
/// is decided while the pointer is still untouched.  An earlier version of
/// this function checked `attr` first and justified it as giving the caller
/// "the more informative EFAULT"; that reasoning was invented, and it made a
/// bogus flag word on a NULL attribute report the wrong argument.  See
/// design-decisions.md §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setflags(attr: *mut PosixSpawnattrT, flags: i16) -> i32 {
    // Reject any bit outside the accepted mask.  Using bitwise-AND
    // against the inverted mask avoids assumptions about sign — the
    // i16 cast preserves the bit pattern.
    if (flags & !POSIX_SPAWN_VALID_FLAGS) != 0 {
        return errno::EINVAL;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe {
        (*attr).flags = flags;
    }
    0
}

/// Get flags from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getflags(attr: *const PosixSpawnattrT, flags: *mut i16) -> i32 {
    if attr.is_null() || flags.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        *flags = (*attr).flags;
    }
    0
}

/// Set the process group in a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setpgroup(attr: *mut PosixSpawnattrT, pgroup: PidT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe {
        (*attr).pgroup = pgroup;
    }
    0
}

/// Get the process group from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getpgroup(
    attr: *const PosixSpawnattrT,
    pgroup: *mut PidT,
) -> i32 {
    if attr.is_null() || pgroup.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        *pgroup = (*attr).pgroup;
    }
    0
}

// ---------------------------------------------------------------------------
// posix_spawnattr signal-set and scheduling attributes
//
// These four setters are the ones CPython 3.12's `os.posix_spawn` calls
// (Modules/posixmodule.c `py_posix_spawn`), and their absence was 4 of the
// 13 symbols that stopped CPython linking against our libc — see
// scripts/cpython-spike/README.md.
//
// They report errors the way the rest of the `posix_spawnattr_*` family in
// this file does: a `0`/errno return value, never `errno` + `-1`.  That is
// POSIX's convention for this family and glibc's actual behaviour.
//
// Note the deliberate asymmetry with `posix_spawnattr_setflags` above: that
// function validates the *value* before the pointer because glibc's
// implementation has no NULL check at all and decides the flag word while
// the pointer is untouched (design-decisions.md §303).  The functions below
// have no value to validate — every `sigset_t`, every priority, and (per
// POSIX) every policy is accepted here, with an invalid policy surfacing
// later from the spawn itself — so the NULL check is the only check, and it
// comes first.
// ---------------------------------------------------------------------------

/// Set the signal set to reset to `SIG_DFL` in the child.
///
/// Takes effect only if `POSIX_SPAWN_SETSIGDEF` is among the attribute
/// flags.  The set is stored verbatim; see [`PosixSpawnattrT`] for why it
/// is recorded even though nothing applies it yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setsigdefault(
    attr: *mut PosixSpawnattrT,
    sigdefault: *const crate::signal::SigsetT,
) -> i32 {
    if attr.is_null() || sigdefault.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).  `SigsetT` is a
    // plain `[u64; 16]`, so a by-value read is a 128-byte copy with no
    // interior pointers; `read_unaligned` because the caller's object is
    // only guaranteed to satisfy the C ABI's alignment, not Rust's.
    unsafe {
        (*attr).sigdefault = core::ptr::read_unaligned(sigdefault);
    }
    0
}

/// Get the `SIG_DFL`-reset signal set from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getsigdefault(
    attr: *const PosixSpawnattrT,
    sigdefault: *mut crate::signal::SigsetT,
) -> i32 {
    if attr.is_null() || sigdefault.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        core::ptr::write_unaligned(sigdefault, (*attr).sigdefault);
    }
    0
}

/// Set the signal mask to install in the child.
///
/// Takes effect only if `POSIX_SPAWN_SETSIGMASK` is among the attribute
/// flags.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setsigmask(
    attr: *mut PosixSpawnattrT,
    sigmask: *const crate::signal::SigsetT,
) -> i32 {
    if attr.is_null() || sigmask.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).  See
    // `posix_spawnattr_setsigdefault` for why the read is unaligned.
    unsafe {
        (*attr).sigmask = core::ptr::read_unaligned(sigmask);
    }
    0
}

/// Get the child signal mask from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getsigmask(
    attr: *const PosixSpawnattrT,
    sigmask: *mut crate::signal::SigsetT,
) -> i32 {
    if attr.is_null() || sigmask.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        core::ptr::write_unaligned(sigmask, (*attr).sigmask);
    }
    0
}

/// Set the scheduling policy to apply to the child.
///
/// Takes effect only if `POSIX_SPAWN_SETSCHEDULER` is among the attribute
/// flags.
///
/// The policy is **not** validated here.  POSIX specifies `EINVAL` for
/// `posix_spawnattr_setschedpolicy` only "if the value of the attribute
/// being set is not valid", and both glibc
/// (`sysdeps/posix/spawnattr_setschedpolicy.c`) and musl store the value
/// unconditionally, leaving an unsupported policy to surface from the
/// spawn as a failed `sched_setscheduler`.  Rejecting here would make us
/// stricter than the platform we are emulating, and — because our
/// scheduler does not yet consume the field at all — the rejection would
/// be based on a policy table we have not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setschedpolicy(attr: *mut PosixSpawnattrT, policy: i32) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr is non-null (checked above).
    unsafe {
        (*attr).schedpolicy = policy;
    }
    0
}

/// Get the scheduling policy from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getschedpolicy(
    attr: *const PosixSpawnattrT,
    policy: *mut i32,
) -> i32 {
    if attr.is_null() || policy.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        *policy = (*attr).schedpolicy;
    }
    0
}

/// Set the scheduling parameters to apply to the child.
///
/// Takes effect only if `POSIX_SPAWN_SETSCHEDPARAM` or
/// `POSIX_SPAWN_SETSCHEDULER` is among the attribute flags.
///
/// `struct sched_param` is a single `int sched_priority` on Linux, so only
/// that field is stored.  The priority is not range-checked for the same
/// reason the policy is not: the valid range depends on the policy, glibc
/// and musl both store it unconditionally, and an out-of-range value
/// surfaces from the spawn rather than from the attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_setschedparam(
    attr: *mut PosixSpawnattrT,
    schedparam: *const crate::sched::SchedParam,
) -> i32 {
    if attr.is_null() || schedparam.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        (*attr).schedpriority = core::ptr::read_unaligned(schedparam).sched_priority;
    }
    0
}

/// Get the scheduling parameters from a spawn attributes object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnattr_getschedparam(
    attr: *const PosixSpawnattrT,
    schedparam: *mut crate::sched::SchedParam,
) -> i32 {
    if attr.is_null() || schedparam.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers are non-null (checked above).
    unsafe {
        core::ptr::write_unaligned(
            schedparam,
            crate::sched::SchedParam {
                sched_priority: (*attr).schedpriority,
                // The reserved fields are written as zero rather than left
                // alone: this is a whole-struct store into the caller's
                // object, and musl's `sched_param` is 48 bytes.
                ..crate::sched::SchedParam::default()
            },
        );
    }
    0
}

// ---------------------------------------------------------------------------
// fd_map building from file_actions
// ---------------------------------------------------------------------------

/// Convert a `HandleKind` to the kernel's `fd_handle_type` constant.
fn kind_to_handle_type(kind: crate::fdtable::HandleKind) -> u8 {
    use crate::fdtable::HandleKind;
    match kind {
        HandleKind::File => fd_handle_type::FILE,
        HandleKind::Pipe => fd_handle_type::PIPE,
        HandleKind::Console => fd_handle_type::CONSOLE,
        HandleKind::TcpStream | HandleKind::TcpListener => fd_handle_type::TCP_SOCKET,
        HandleKind::UdpSocket => fd_handle_type::UDP_SOCKET,
        HandleKind::Eventfd => fd_handle_type::EVENTFD,
        HandleKind::UnixStream => fd_handle_type::STREAM_SOCKET,
        // A pty *slave* crosses as a console handle, and that is exact
        // rather than approximate.  `CONSOLE` does not name a specific
        // device: the kernel resolves every console read and write
        // through `current_tty()`, so a console fd means "my controlling
        // terminal".  A child reached through `login_tty` has *made* the
        // slave its controlling terminal (`setsid` + `TIOCSCTTY`) before
        // it execs, so console-kind stdio in the child lands in the pty,
        // with `OPOST`/`ONLCR` and the `TOSTOP` gate applied -- which is
        // the whole of what a shell running under a terminal emulator
        // needs.  See lane A's "Linux `write(1, ...)` is already
        // pty-aware" note in
        // `requests/a-b-pty-the-tty-layer-is-now-n-devices-and-a-pty-object-exists.md`.
        HandleKind::PtySlave => fd_handle_type::CONSOLE,
        // A pty *master* crosses as itself now that `fd_handle_type::PTY`
        // exists.  It used to be filtered out of `build_fd_map` entirely, for
        // want of any value that named a pty end — which meant the child
        // silently lost the master rather than being told it could not have
        // one, and which is what stopped `script -f`-shaped programs (where the
        // *child* is the master holder) from working at all.
        HandleKind::PtyMaster => fd_handle_type::PTY,
        // Epoll, Timerfd, and Inotify fds are per-process userspace
        // state and cannot be meaningfully transferred to a child.  Map
        // to FILE so the function is total; build_fd_map filters these
        // entries out before they reach this conversion.
        HandleKind::Epoll | HandleKind::Timerfd | HandleKind::Inotify => fd_handle_type::FILE,
    }
}

/// Convert the kernel's `fd_handle_type` constant back to a `HandleKind`.
///
/// This is the inverse of [`kind_to_handle_type`] and is used by the child's
/// crt0 (`crt::retrieve_initial_fds`) to rebuild its userspace fd table from
/// the entries the kernel hands back after `posix_spawn`/`execve`.
///
/// The two directions are deliberately kept adjacent: they were previously
/// maintained in separate files and silently drifted apart
/// (BUG-CRT0-STREAM-SOCKET-UNMAPPED — the send side learned `STREAM_SOCKET`
/// but the receive side never did, so inherited `AF_UNIX` endpoints were
/// mislabeled as `File`).  `round_trips_for_every_transferable_kind` below
/// pins the invariant so it cannot drift again.
///
/// Unknown/unrecognised types fall back to `File` so the function is total.
/// Note the round trip is exact for every *transferable* kind except
/// `TcpListener`, which shares the `TCP_SOCKET` wire type with `TcpStream`
/// and therefore comes back as `TcpStream`.
///
/// `handle` is needed only for `PTY`, whose single wire type covers both ends —
/// see [`handle_type_to_kind_for`].
pub fn handle_type_to_kind(handle_type: u8) -> crate::fdtable::HandleKind {
    handle_type_to_kind_for(handle_type, 0)
}

/// [`handle_type_to_kind`] with the handle available, which is what the child's
/// crt0 actually has.
///
/// One wire type, `PTY`, cannot be decoded from the type byte alone: it names
/// *either* end, because `PtyHandle` is `(tty_id << 1) | end` and so already
/// carries the distinction in its low bit.  Rebuilding both ends as one kind
/// would be worse than the `TcpListener` collapse — a master reconstructed as a
/// slave would send the emulator's own keystrokes back to itself — so the bit
/// is read rather than guessed.
pub fn handle_type_to_kind_for(handle_type: u8, handle: u64) -> crate::fdtable::HandleKind {
    use crate::fdtable::HandleKind;
    match handle_type {
        fd_handle_type::FILE => HandleKind::File,
        fd_handle_type::PIPE => HandleKind::Pipe,
        fd_handle_type::TCP_SOCKET => HandleKind::TcpStream,
        fd_handle_type::UDP_SOCKET => HandleKind::UdpSocket,
        fd_handle_type::CONSOLE => HandleKind::Console,
        fd_handle_type::EVENTFD => HandleKind::Eventfd,
        // Unix-domain stream endpoint (`socketpair`).  Reconstructing this
        // as `File` would route the child's read/write/recv through the
        // file path with the wrong semantics (EBADF or silent misbehaviour).
        fd_handle_type::STREAM_SOCKET => HandleKind::UnixStream,
        fd_handle_type::PTY => {
            if handle & 1 == 0 {
                HandleKind::PtyMaster
            } else {
                HandleKind::PtySlave
            }
        }
        _ => HandleKind::File,
    }
}

/// Descriptors the parent opened in its own table to carry out `open` file
/// actions, closed when this is dropped.
///
/// The kernel dups each one's handle into the child during the spawn, so the
/// parent's copies must go afterwards — whether the spawn succeeded or failed,
/// and on every path that gives up before it. That is why they are closed by
/// `Drop` rather than by a call each return has to remember.
struct OpenedHandles {
    /// Parent descriptors opened by `apply_file_actions`.
    fds: [Fd; MAX_FD_MAP],
    /// Number of valid entries.
    count: usize,
}

impl OpenedHandles {
    const fn new() -> Self {
        Self {
            fds: [-1; MAX_FD_MAP],
            count: 0,
        }
    }

    /// Record a descriptor to close after the spawn.
    ///
    /// One per `open` action, up to the size of the child's table -- more
    /// opens than a child has descriptors is past anything a spawn can mean.
    /// Past that the descriptor is closed now and the action fails with
    /// `ENOMEM`: its handle was about to be handed to the child, so it must
    /// not be closed while the spawn still goes ahead.
    fn push(&mut self, fd: Fd) -> Result<(), i32> {
        if let Some(slot) = self.fds.get_mut(self.count) {
            *slot = fd;
            self.count = self.count.wrapping_add(1);
            Ok(())
        } else {
            // Nothing to report from the close: the action is failing anyway.
            let _ = crate::file::close(fd);
            Err(errno::ENOMEM)
        }
    }
}

impl Drop for OpenedHandles {
    fn drop(&mut self) {
        // `close` can overwrite errno, and a spawn's caller may be about to
        // read the one that describes its failure.
        let saved = errno::get_errno();
        let mut i = 0usize;
        while i < self.count {
            // The child has its own reference by now, or never will; either
            // way there is nothing to do with a failed close of ours.
            let _ = crate::file::close(self.fds[i]);
            i = i.wrapping_add(1);
        }
        self.count = 0;
        errno::set_errno(saved);
    }
}

/// The child's descriptors before any file action: the parent's inheritable
/// ones, as `(wire handle type, kernel handle)` per fd number.
///
/// The second array holds the descriptors that are open but *not*
/// inheritable — `FD_CLOEXEC` — because one file action can still hand one of
/// them over: POSIX specifies that `adddup2(fd, fd)` clears `FD_CLOEXEC` on
/// `fd` in the child. It starts as a snapshot taken here, before any `open`
/// action can put a temporary descriptor of the parent's at the same number,
/// and an `O_CLOEXEC` open action adds to it. A number is in at most one of the
/// two arrays.
///
/// `from` is where each open slot's descriptor came from: the parent
/// descriptor whose table entry -- and so whose recorded path -- it shares. An
/// `addfchdir_np` action needs a path, since the kernel is told the child's
/// directory by name.
struct ChildFds {
    virt: [Option<(u8, u64)>; MAX_FD_MAP],
    cloexec: [Option<(u8, u64)>; MAX_FD_MAP],
    from: [Option<Fd>; MAX_FD_MAP],
}

impl ChildFds {
    /// Close `fd` in the child, whether or not it was close-on-exec.
    fn close(&mut self, fd: usize) {
        if let (Some(v), Some(c), Some(f)) = (
            self.virt.get_mut(fd),
            self.cloexec.get_mut(fd),
            self.from.get_mut(fd),
        ) {
            *v = None;
            *c = None;
            *f = None;
        }
    }

    /// Is `fd` open in the child at this point in the actions? A
    /// close-on-exec descriptor is: the child closes it only at `exec`.
    fn is_open(&self, fd: usize) -> bool {
        self.virt.get(fd).is_some_and(Option::is_some)
            || self.cloexec.get(fd).is_some_and(Option::is_some)
    }
}

fn inheritable_fds() -> ChildFds {
    use crate::fdtable;

    let mut out = ChildFds {
        virt: [None; MAX_FD_MAP],
        cloexec: [None; MAX_FD_MAP],
        from: [None; MAX_FD_MAP],
    };
    let mut idx = 0usize;
    while idx < MAX_FD_MAP {
        #[allow(clippy::cast_possible_wrap)]
        let fd = idx as i32;
        if let Some(entry) = fdtable::get_fd(fd) {
            // Skip epoll/timerfd/inotify fds — the instance state lives
            // in the parent's userspace memory and cannot be transferred
            // to the child.  For those three the filter is honest: there
            // is no kernel object to hand over.
            //
            // `PtyMaster` used to be in the same list for a different and worse
            // reason — it *does* have a kernel identity, but no
            // `fd_handle_type` named a pty end, so dropping it was a lie of
            // convenience.  `fd_handle_type::PTY` closed that gap, so a master
            // is inherited like anything else now and
            // `TD-B-PTY-MASTER-CANNOT-BE-INHERITED-ACROSS-SPAWN` is fixed.
            //
            // `PtySlave` deliberately is *not* skipped and deliberately does
            // *not* travel as `PTY`: `kind_to_handle_type` maps it to
            // `CONSOLE`, which the kernel resolves through `current_tty()`, and
            // `login_tty` has already made the pty the child's controlling
            // terminal — so that mapping is exact rather than approximate.
            let transferable = entry.kind != fdtable::HandleKind::Epoll
                && entry.kind != fdtable::HandleKind::Timerfd
                && entry.kind != fdtable::HandleKind::Inotify;
            if transferable {
                let wire = Some((kind_to_handle_type(entry.kind), entry.handle));
                // Close-on-exec fds are not inherited unless an action says so.
                if entry.flags & fdtable::FD_CLOEXEC == 0 {
                    out.virt[idx] = wire;
                } else {
                    out.cloexec[idx] = wire;
                }
                out.from[idx] = Some(fd);
            }
        }
        idx = idx.wrapping_add(1);
    }
    out
}

/// An fd number as a slot in the child's table, or `EBADF`.
///
/// The actions were range-checked when they were added, against `OPEN_MAX`;
/// this is the narrower range the kernel's fd map carries. Refusing a number
/// past it is the honest answer — the old code skipped the action silently,
/// so a `dup2` onto fd 40 simply did not happen.
fn child_slot(fd: Fd) -> Result<usize, i32> {
    usize::try_from(fd)
        .ok()
        .filter(|&n| n < MAX_FD_MAP)
        .ok_or(errno::EBADF)
}

/// The directory a spawned child will start in, as its `chdir` actions leave
/// it.
///
/// Starts as the parent's working directory. Each `addchdir_np` action moves
/// it, resolved against where the actions before it left it -- which is how
/// the child would see a relative path -- and checked to be a directory; an
/// `open` action after one resolves a relative path against it too, since in
/// the child that is where the file would be opened.
struct ChildCwd {
    path: [u8; crate::unistd::PATH_MAX],
    len: usize,
    /// Whether any `chdir` action ran. Only then is the kernel given a
    /// directory; otherwise the child inherits its parent's record.
    moved: bool,
}

impl ChildCwd {
    fn of_parent() -> Self {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = crate::unistd::current_cwd(&mut path);
        Self {
            path,
            len,
            moved: false,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        self.path.get(..self.len).unwrap_or(b"/")
    }

    /// `chdir(dir)`, as the child would do it.
    fn change_to(&mut self, dir: &[u8]) -> Result<(), i32> {
        if dir.is_empty() {
            return Err(errno::ENOENT);
        }
        let mut next = [0u8; crate::unistd::PATH_MAX];
        let n = crate::unistd::resolve_path_against(self.as_bytes(), dir, &mut next)
            .ok_or(errno::ENAMETOOLONG)?;
        let resolved = next
            .get(..n)
            .filter(|p| p.len() <= crate::unistd::CWD_RECORD_MAX)
            .ok_or(errno::ENAMETOOLONG)?;
        crate::unistd::check_directory(resolved)?;
        self.path = next;
        self.len = n;
        self.moved = true;
        Ok(())
    }

    /// `path_z` -- a path with its terminator -- as an `open` in the child
    /// would find it, NUL-terminated: resolved against the child's directory
    /// into `out` once a `chdir` has moved it, and passed through as it stands
    /// before that, when the child's directory and ours are the same one.
    fn open_path<'a>(
        &self,
        path_z: &'a [u8],
        out: &'a mut [u8; crate::unistd::PATH_MAX],
    ) -> Result<&'a [u8], i32> {
        // Every slot's path carries its terminator (`dup_path`).
        let given = path_z.strip_suffix(&[0]).ok_or(errno::EINVAL)?;
        if !self.moved || given.first() == Some(&b'/') {
            return Ok(path_z);
        }
        let mut resolved = [0u8; crate::unistd::PATH_MAX];
        let n = crate::unistd::resolve_path_against(self.as_bytes(), given, &mut resolved)
            .ok_or(errno::ENAMETOOLONG)?;
        // `n` bytes and a terminator must fit, as they must for any path.
        let dst = out.get_mut(..=n).ok_or(errno::ENAMETOOLONG)?;
        let (body, nul) = dst.split_at_mut_checked(n).ok_or(errno::ENAMETOOLONG)?;
        body.copy_from_slice(resolved.get(..n).ok_or(errno::ENAMETOOLONG)?);
        nul.fill(0);
        Ok(dst)
    }
}

/// Apply `acts` to the child's table in order, as the child would.
///
/// Every failure is reported, which is what POSIX requires of `posix_spawn`
/// ("if … any of the file actions fail, `posix_spawn()` shall fail") and what
/// glibc does. The old loop skipped a failed `open` with a comment saying so,
/// ignored `closefrom` altogether (so descriptors the caller asked to close
/// reached the child anyway), and turned a `dup2` from a closed descriptor
/// into a silent close of the target.
///
/// `chdir` actions (tag 4) move `cwd`, and later relative `open`s follow it.
/// They were ignored until the kernel could start a child in a directory
/// (design-decisions.md §960): the directory lived only in the child's own
/// libc, which a spawn starts from nothing.
fn apply_file_actions(
    child: &mut ChildFds,
    cwd: &mut ChildCwd,
    acts: &PosixSpawnFileActionsT,
    opened: &mut OpenedHandles,
) -> Result<(), i32> {
    use crate::fdtable;

    for slot in acts.slots() {
        match slot.tag {
            1 => {
                // Close. A descriptor that is not open in the child has nothing
                // to close, which glibc does not treat as an error either. A
                // close-on-exec one is closed too: it was still open until now,
                // and a later `adddup2(fd, fd)` must not bring it back.
                if let Ok(fd) = child_slot(slot.fd) {
                    child.close(fd);
                }
            }
            2 => {
                // Dup2(fd → newfd), against the table as the actions so far
                // have left it.
                let src = child_slot(slot.fd)?;
                let dst = child_slot(slot.newfd)?;
                let entry = if src == dst {
                    // Same number: POSIX makes this the way to hand over a
                    // close-on-exec descriptor, so look there as well.
                    child.virt[src].or(child.cloexec[src])
                } else {
                    child.virt[src]
                };
                child.virt[dst] = Some(entry.ok_or(errno::EBADF)?);
                // `dup2` clears `FD_CLOEXEC` on the new descriptor, and it now
                // shares `src`'s table entry and path.
                child.cloexec[dst] = None;
                child.from[dst] = child.from[src];
            }
            3 => {
                // Open, "as if `open(path, oflag, mode)`" — so it *is* `open`,
                // in this process, with everything that brings: the umask on a
                // create, `/dev/pts/<n>` and `/dev/ptmx`, the flag checks. The
                // descriptor is closed again once the kernel has copied its
                // handle into the child.
                let target = child_slot(slot.fd)?;
                if slot.path_len == 0 {
                    return Err(errno::ENOENT);
                }
                let mut in_child = [0u8; crate::unistd::PATH_MAX];
                let given = slot.path_cstr().ok_or(errno::ENOENT)?;
                let open_path = cwd.open_path(given, &mut in_child)?;
                let fd = crate::file::open(open_path.as_ptr(), slot.oflag, slot.mode);
                if fd < 0 {
                    return Err(errno::get_errno());
                }
                opened.push(fd)?;
                let entry = fdtable::get_fd(fd).ok_or(errno::EBADF)?;
                let wire = Some((kind_to_handle_type(entry.kind), entry.handle));
                // `O_CLOEXEC` on an action's open means the child's exec
                // closes it at once, so the kernel is not handed it; but it is
                // open until then, for a later `fchdir` or `dup2(fd, fd)`.
                child.close(target);
                if slot.oflag & crate::fcntl::O_CLOEXEC != 0 {
                    child.cloexec[target] = wire;
                } else {
                    child.virt[target] = wire;
                }
                child.from[target] = Some(fd);
            }
            4 => {
                // chdir: see `ChildCwd::change_to`.
                cwd.change_to(slot.path_bytes())?;
            }
            5 => {
                // closefrom(lowfd): every descriptor from lowfd up,
                // close-on-exec ones included.
                let low = usize::try_from(slot.fd).map_err(|_| errno::EBADF)?;
                for fd in low..MAX_FD_MAP {
                    child.close(fd);
                }
            }
            6 => {
                // fchdir(fd): the directory a descriptor open in the child
                // names, then as `chdir` -- see `ChildCwd::change_to`. The
                // path is the one the parent's `open` recorded for the
                // descriptor the slot came from; a pipe or a socket has none,
                // and is not a directory.
                let fd = child_slot(slot.fd)?;
                if !child.is_open(fd) {
                    return Err(errno::EBADF);
                }
                let parent_fd = child.from[fd].ok_or(errno::EBADF)?;
                let mut path = [0u8; crate::unistd::PATH_MAX];
                let n = fdtable::get_fd_path(parent_fd, &mut path);
                if n == 0 {
                    return Err(errno::ENOTDIR);
                }
                cwd.change_to(path.get(..n).ok_or(errno::ENAMETOOLONG)?)?;
            }
            _ => return Err(errno::EINVAL),
        }
    }
    Ok(())
}

/// Flatten the child's table into the `FdMapEntry` array the kernel takes,
/// returning how many entries were written.
fn flatten_fd_map(child: &ChildFds, out: &mut [FdMapEntry; MAX_FD_MAP]) -> usize {
    let mut count = 0usize;
    for (idx, slot) in child.virt.iter().enumerate() {
        if let Some((handle_type, handle)) = *slot
            && let Some(dst) = out.get_mut(count)
        {
            #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
            let fd = idx as i32;
            *dst = FdMapEntry {
                fd,
                handle_type,
                _pad: [0; 3],
                handle,
            };
            count = count.wrapping_add(1);
        }
    }
    count
}

/// What a spawned child starts with, once `file_actions` have been applied.
struct ChildPlan {
    /// How many entries of the fd map were written.
    fd_count: usize,
    /// The directory it starts in.
    cwd: ChildCwd,
}

/// Plan a spawned child's start: the parent's inheritable descriptors and
/// working directory, with `file_actions` applied to them in order. The fd
/// map is written to `out`.
///
/// `open` actions leave parent descriptors in `opened`, which closes them when
/// dropped — after the spawn syscall, since the kernel copies their handles.
///
/// # Errors
///
/// The first file action that fails, as its errno.
fn plan_child(
    file_actions: *const PosixSpawnFileActionsT,
    out: &mut [FdMapEntry; MAX_FD_MAP],
    opened: &mut OpenedHandles,
) -> Result<ChildPlan, i32> {
    let mut child = inheritable_fds();
    let mut cwd = ChildCwd::of_parent();
    if !file_actions.is_null() {
        // SAFETY: non-null, and the caller's contract is that it was
        // initialised by `posix_spawn_file_actions_init`.
        apply_file_actions(&mut child, &mut cwd, unsafe { &*file_actions }, opened)?;
    }
    Ok(ChildPlan {
        fd_count: flatten_fd_map(&child, out),
        cwd,
    })
}

/// The directory to hand the kernel for the child, if any.
///
/// Only a `chdir` action gives one; without one the kernel starts the child in
/// its parent's directory, which is what POSIX asks for. And only a kernel that
/// keeps the record (design-decisions.md §960) is given one: an older kernel
/// refuses the spawn outright over fields it does not know, so there the child
/// starts in its parent's directory, as every spawned child did before -- see
/// [`crate::unistd::kernel_keeps_cwd`] for why that beats refusing.
fn child_start_dir(plan: &ChildPlan) -> Option<&[u8]> {
    (plan.cwd.moved && crate::unistd::kernel_keeps_cwd()).then(|| plan.cwd.as_bytes())
}

/// [`plan_child`]'s fd map alone, for the tests that are about nothing else.
#[cfg(test)]
fn build_fd_map(
    file_actions: *const PosixSpawnFileActionsT,
    out: &mut [FdMapEntry; MAX_FD_MAP],
    opened: &mut OpenedHandles,
) -> Result<usize, i32> {
    plan_child(file_actions, out, opened).map(|plan| plan.fd_count)
}

// ---------------------------------------------------------------------------
// posix_spawn
// ---------------------------------------------------------------------------

/// Spawn a new process from a file path.
///
/// Reads the ELF binary at `path` and creates a new process via
/// `SYS_PROCESS_SPAWN_EX`.  On success, stores the child PID in
/// `*pid` (if non-null).
///
/// # Parameters
///
/// - `pid`: Output parameter for child PID (may be null).
/// - `path`: Path to the ELF binary (null-terminated C string).
/// - `file_actions`: File actions to apply in the child (close, dup2, open).
///   Applied to the parent's fd table to build the kernel fd_map.
///   The child retrieves inherited fds via `SYS_PROCESS_GET_INITIAL_FDS`
///   during startup.
/// - `attrp`: Spawn attributes (flags, process group).  Recorded but most
///   flags have no effect yet.
/// - `argv`: Null-terminated array of argument strings for the child.
///   Packed and passed to the kernel; the child retrieves them via
///   `SYS_PROCESS_GET_ARGS` during startup.  May be null.
/// - `envp`: Null-terminated array of environment strings for the child.
///   Packed and passed to the kernel.  May be null.
///
/// Returns 0 on success, or an error number (NOT -1) on failure.
/// This matches the POSIX spec: `posix_spawn` returns the error
/// directly, not via errno.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawn(
    pid: *mut PidT,
    path: *const u8,
    file_actions: *const PosixSpawnFileActionsT,
    _attrp: *const PosixSpawnattrT,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    // `None` — inherit everything. POSIX specifies `posix_spawn` as
    // fork+exec-equivalent, and `fork` hands the child the parent's whole
    // authority, so anything narrower here would be this libc inventing a
    // sandbox its callers never asked for. Narrowing is opt-in only, via
    // `slateos_spawn_caps`.
    unsafe { spawn_impl(pid, path, file_actions, argv, envp, None) }
}

/// Spawn a process holding **exactly** the capabilities in `caps`.
///
/// The SlateOS-native counterpart to [`posix_spawn`]. Identical in every other
/// respect — same fd inheritance, same file actions, same argv/envp packing —
/// but the child receives the listed capabilities instead of the parent's
/// whole table.
///
/// A `caps` of null with `cap_count` of 0 means the child gets **nothing**,
/// which is a legitimate request and not an error. Null with a non-zero count
/// is `EINVAL`: unlike the fd map and argv, this array is not optional, and
/// silently substituting the empty list would start a child holding nothing
/// when the caller asked for something.
///
/// # The refusal rule
///
/// If the caller names a capability it does not hold — **or rights wider than
/// it holds**, e.g. asking to delegate `WRITE` on something it holds only
/// `READ` on — the kernel fails the entire spawn and creates no process. It
/// does not trim the request to the intersection, and neither does this
/// function: **do not wrap this in a retry-with-fewer-caps loop.**
///
/// A refusal is reported as `EPERM`, distinct from the `EACCES` an unreadable
/// binary gives, so the two are never confused at the call site. (`EPERM` is
/// specific to this entry point; the shared kernel-error table maps
/// `PermissionDenied` to `EACCES` and is unchanged.)
///
/// That is deliberate, and the bug it exists to prevent is on record. A
/// quietly under-privileged child is how `make` came to parse its makefile
/// fine and then die inside `ld.so` with `libc.so.6: cannot open shared object
/// file: Permission denied` — a message naming nothing to do with the spawn,
/// read as a userspace bug for a day. The list is caller-written, so an
/// unsatisfiable entry is a caller bug and belongs at the call site.
///
/// Rights may be narrowed, never widened; the child gets the *requested*
/// rights, not the parent's.
///
/// # Building the list
///
/// Enumerate with `SYS_CAP_QUERY`, drop what the child should not have, pass
/// the remainder straight back — [`CapEntryInfo`] is the same struct on both
/// sides precisely so that there is no transcription step. Note the kernel
/// matches `resource_id` **exactly**, so a filtered enumeration round-trips
/// but a hand-built entry naming a different id will not.
///
/// Returns 0 on success or an error number (not -1), as [`posix_spawn`] does.
///
/// # Safety
///
/// Same contract as [`posix_spawn`], plus: `caps` must point to `cap_count`
/// initialised [`CapEntryInfo`] values, each with `reserved` zeroed — the
/// kernel validates that field rather than skipping it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn slateos_spawn_caps(
    pid: *mut PidT,
    path: *const u8,
    file_actions: *const PosixSpawnFileActionsT,
    _attrp: *const PosixSpawnattrT,
    argv: *const *const u8,
    envp: *const *const u8,
    caps: *const CapEntryInfo,
    cap_count: usize,
) -> i32 {
    if caps.is_null() && cap_count != 0 {
        return errno::EINVAL;
    }
    if cap_count > SPAWN_CAP_MAX {
        return errno::EINVAL;
    }
    // SAFETY: `caps` is non-null for any non-zero `cap_count` (checked above),
    // and the caller's contract is that it addresses `cap_count` initialised
    // entries. For a zero count we synthesise an empty slice from a dangling
    // aligned pointer rather than dereferencing `caps`, so a null with count 0
    // — the "child gets nothing" request — is well-defined here.
    let list: &[CapEntryInfo] = if cap_count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(caps, cap_count) }
    };
    unsafe { spawn_impl(pid, path, file_actions, argv, envp, Some(list)) }
}

/// The whole of `posix_spawn`, with the capability policy left open.
///
/// One body rather than two so the POSIX entry point and the native one cannot
/// drift: everything except `cap_mode` is identical between them, and a second
/// copy of ELF loading, fd-map construction and argv packing would be a second
/// place for the `munmap`/`close_all` cleanup to be got wrong.
///
/// `caps` of `None` selects [`SYS_PROCESS_SPAWN_EX`] (517) — not 559 with
/// `cap_mode == 0`. Both mean "inherit everything", but routing the untouched
/// path through the untouched syscall means adding this feature cannot regress
/// `posix_spawn`, which every existing caller uses. The one exception is a
/// `chdir` file action, which needs version 2's directory field.
///
/// # Safety
///
/// `path`, `file_actions`, `argv`, `envp` and `pid` carry `posix_spawn`'s
/// contract; `caps`, if `Some`, must remain valid for the syscall's duration.
// argc/envc and argv/envp pair on the canonical exec-family naming;
// the visual similarity is intentional and worth keeping.
#[allow(clippy::similar_names)]
unsafe fn spawn_impl(
    pid: *mut PidT,
    path: *const u8,
    file_actions: *const PosixSpawnFileActionsT,
    argv: *const *const u8,
    envp: *const *const u8,
    caps: Option<&[CapEntryInfo]>,
) -> i32 {
    if path.is_null() {
        return errno::EFAULT;
    }
    // The program, following any `#!` chain, and its packed argument list.
    // `E2BIG` for a list longer than `ARG_MAX`, which this used to answer by
    // quietly dropping the arguments that did not fit.
    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let (image, argv_len) = match load_program(path, argv, &[], &mut argv_buf) {
        Ok(loaded) => loaded,
        Err(err) => return err,
    };
    let argv_packed = argv_buf.get(..argv_len).unwrap_or(&[]);
    // SAFETY: forwarded from this function's own contract.
    unsafe { spawn_loaded(pid, path, &image, argv_packed, file_actions, envp, caps) }
}

/// The spawn syscall a request needs, with its argument struct.
enum SpawnRequest {
    /// [`SYS_PROCESS_SPAWN_EX`] (517): inherit every capability, start in the
    /// parent's directory.
    V1(SpawnExArgs),
    /// [`SYS_PROCESS_SPAWN_EX2`] (559): a capability subset, a starting
    /// directory, or both.
    V2(SpawnEx2Args),
}

/// Choose the syscall for a spawn whose version-1 fields are `v1`.
///
/// The untouched case -- no capability subset, no `chdir` action -- goes to
/// 517, not to 559 with `cap_mode == 0`. Both mean "inherit everything", but
/// sending it down the untouched syscall means neither feature can regress the
/// path every existing caller uses. Anything else needs version 2, the only
/// one with fields for a policy or a directory.
///
/// A pure function of its arguments so that the choice, and the copying of
/// every field into the struct it picks, can be tested on a host where no
/// spawn can reach the kernel.
fn spawn_request(
    v1: SpawnExArgs,
    caps: Option<&[CapEntryInfo]>,
    child_cwd: Option<&[u8]>,
) -> SpawnRequest {
    if caps.is_none() && child_cwd.is_none() {
        return SpawnRequest::V1(v1);
    }
    // `spawn_ex2_args` fills `struct_size` and the capability policy; writing
    // either by hand at a call site is how they go stale.
    let mut args = spawn_ex2_args(caps);
    args.elf_ptr = v1.elf_ptr;
    args.elf_len = v1.elf_len;
    args.name_ptr = v1.name_ptr;
    args.name_len = v1.name_len;
    args.fd_map_ptr = v1.fd_map_ptr;
    args.fd_map_count = v1.fd_map_count;
    args.argv_ptr = v1.argv_ptr;
    args.argv_len = v1.argv_len;
    args.argc = v1.argc;
    args.envp_ptr = v1.envp_ptr;
    args.envp_len = v1.envp_len;
    args.envc = v1.envc;
    if let Some(dir) = child_cwd {
        args.cwd_ptr = dir.as_ptr() as u64;
        args.cwd_len = dir.len() as u64;
    }
    SpawnRequest::V2(args)
}

/// Spawn an already-loaded program: apply the file actions, pack the
/// environment, make the syscall.
///
/// Split from [`spawn_impl`] so that `posix_spawnp` can search `PATH` by
/// *loading* each candidate and apply the file actions exactly once, to the
/// one it found. Applying them per candidate would repeat their side effects:
/// an `addopen` with `O_CREAT | O_EXCL` succeeds for the first candidate and
/// then fails with `EEXIST` for the second.
///
/// `name` is the path the caller named — the process's name, as Linux takes
/// `comm` from the script and not from its interpreter.
///
/// # Safety
///
/// As [`spawn_impl`]; `name` must be a valid C string.
#[allow(clippy::similar_names)]
unsafe fn spawn_loaded(
    pid: *mut PidT,
    name: *const u8,
    image: &ElfImage,
    argv_packed: &[u8],
    file_actions: *const PosixSpawnFileActionsT,
    envp: *const *const u8,
    caps: Option<&[CapEntryInfo]>,
) -> i32 {
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];
    let Some(envp_packed_len) = pack_cstring_array(envp, &mut envp_buf) else {
        errno::set_errno(errno::E2BIG);
        return errno::E2BIG;
    };
    let envc = count_cstring_array(envp);
    let argv_packed_len = argv_packed.len();
    let argc = crate::shebang::packed_count(argv_packed);

    // The process name, resolved as `load_program` resolved the file. It has
    // just been loaded through this path, so it resolves; the fallback is
    // only there because the types cannot say so.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: `name` is a valid C string (this function's contract).
    let resolved_len = unsafe { crate::unistd::resolve_path(name, &mut resolved) }.unwrap_or(0);

    // Build the fd_map from the parent's fd table + file_actions.  `open`
    // actions are carried out here, in this process, and the kernel dups
    // their handles into the child; `opened` closes the parent's copies when
    // it goes out of scope, after the syscall.  A failing action fails the
    // spawn, as POSIX requires.
    let mut fd_map = [FdMapEntry {
        fd: 0,
        handle_type: 0,
        _pad: [0; 3],
        handle: 0,
    }; MAX_FD_MAP];
    let mut opened = OpenedHandles::new();
    let plan = match plan_child(file_actions, &mut fd_map, &mut opened) {
        Ok(plan) => plan,
        Err(err) => {
            errno::set_errno(err);
            return err;
        }
    };
    let fd_map_count = plan.fd_count;
    let child_cwd = child_start_dir(&plan);

    // The fields both syscalls share. Computed once and copied into whichever
    // struct we send, so the two paths cannot disagree about what is being
    // spawned -- only about who the child is allowed to be, and where it
    // starts.
    let elf_ptr = image.ptr as u64;
    let elf_len = image.len as u64;
    let name_ptr = resolved.as_ptr() as u64;
    let name_len = resolved_len as u64;
    let fd_map_ptr = if fd_map_count > 0 {
        fd_map.as_ptr() as u64
    } else {
        0
    };
    let argv_ptr = if argv_packed_len > 0 {
        argv_packed.as_ptr() as u64
    } else {
        0
    };
    let envp_ptr = if envp_packed_len > 0 {
        envp_buf.as_ptr() as u64
    } else {
        0
    };

    let v1 = SpawnExArgs {
        elf_ptr,
        elf_len,
        name_ptr,
        name_len,
        fd_map_ptr,
        fd_map_count: fd_map_count as u64,
        argv_ptr,
        argv_len: argv_packed_len as u64,
        argc: argc as u64,
        envp_ptr,
        envp_len: envp_packed_len as u64,
        envc: envc as u64,
    };
    // Each struct lives in its arm until the syscall that reads it returns.
    let ret = match spawn_request(v1, caps, child_cwd) {
        SpawnRequest::V1(args) => syscall1(SYS_PROCESS_SPAWN_EX, (&raw const args) as u64),
        SpawnRequest::V2(args) => syscall1(SYS_PROCESS_SPAWN_EX2, (&raw const args) as u64),
    };

    // The parent's copies of any descriptors opened for `open` actions: the
    // kernel has duped their handles into the child's PCB, or failed to, and
    // either way they are done with. (The image is the caller's to drop.)
    drop(opened);

    if ret < 0 {
        // The delegation refusal must not arrive wearing the same errno as a
        // binary this process could not read. The shared table maps the
        // kernel's `PermissionDenied` to `EACCES`, and `load_elf` above can
        // return `EACCES` too — so on the subset path the caller would be left
        // unable to tell "you asked to delegate authority you do not hold"
        // from "I could not open the file", and would go and look at the file.
        //
        // That confusion is a smaller copy of the bug this syscall exists to
        // prevent (see `slateos_spawn_caps`), so the subset path reports
        // `EPERM` instead: from this entry point, `EPERM` means the kernel
        // refused on capability grounds and `EACCES` means the binary was
        // unreadable. POSIX `posix_spawn` keeps the shared mapping untouched —
        // it never takes this branch, because it never passes `Some`.
        if caps.is_some() && ret == errno::native::PERMISSION_DENIED {
            errno::set_errno(errno::EPERM);
            return errno::EPERM;
        }
        return native_to_posix_err(ret);
    }

    // Record the child PID for waitpid(-1, ...) to use later.
    let child_pid = ret as PidT;
    crate::process::record_child_pid(child_pid);

    // Store child PID if requested.
    if !pid.is_null() {
        unsafe {
            *pid = child_pid;
        }
    }

    0
}

/// Spawn a new process, searching the PATH for the executable.
///
/// Like `posix_spawn` but `file` is searched for in the directories
/// listed in the `PATH` environment variable.  If `file` contains a
/// `/`, it is used directly without PATH search.
///
/// The search follows glibc's `posix_spawnp`: each candidate is *loaded*,
/// failures in [`search_continues_after`]'s set move on to the next
/// directory, `EACCES` is remembered and reported if nothing is found, and
/// there is no shell fallback for a file that is not a program — that is
/// `execvp`'s rule, not this one's. The file actions run once, against the
/// program that was found (see [`spawn_loaded`] for why that matters).
///
/// Returns 0 on success, or an error number on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_spawnp(
    pid: *mut PidT,
    file: *const u8,
    file_actions: *const PosixSpawnFileActionsT,
    attrp: *const PosixSpawnattrT,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    if file.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: `file` is a valid C string (the caller's contract).
    let file_len = unsafe { crate::file::c_strlen_pub(file) };
    if file_len == 0 {
        return errno::ENOENT;
    }
    // SAFETY: readable for `file_len` bytes, just measured.
    let file_bytes = unsafe { core::slice::from_raw_parts(file, file_len) };

    // A name with a '/' is used as given (no PATH search).
    if file_bytes.contains(&b'/') {
        return posix_spawn(pid, file, file_actions, attrp, argv, envp);
    }
    if file_len > crate::linux_limits::NAME_MAX {
        return errno::ENAMETOOLONG;
    }

    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let mut candidates = PathCandidates::new(search_path_value(), file_bytes);
    let mut candidate = [0u8; crate::unistd::PATH_MAX];
    let mut denied = false;
    let mut last = errno::ENOENT;
    while let Some(next) = candidates.next_into(&mut candidate) {
        if let Err(err) = next {
            return err;
        }
        match load_program(candidate.as_ptr(), argv, &[], &mut argv_buf) {
            Ok((image, argv_len)) => {
                let argv_packed = argv_buf.get(..argv_len).unwrap_or(&[]);
                // SAFETY: `candidate` is the NUL-terminated path just loaded,
                // and the rest is forwarded from this function's contract.
                return unsafe {
                    spawn_loaded(
                        pid,
                        candidate.as_ptr(),
                        &image,
                        argv_packed,
                        file_actions,
                        envp,
                        None,
                    )
                };
            }
            Err(err) if search_continues_after(err) => {
                denied |= err == errno::EACCES;
                last = err;
            }
            Err(err) => return err,
        }
    }
    if denied { errno::EACCES } else { last }
}

// ---------------------------------------------------------------------------
// execve (proper implementation)
// ---------------------------------------------------------------------------

/// Maximum size for packed argv/envp buffers during exec.
const EXEC_PACKED_MAX: usize = 128 * 1024;

/// Replace the current process image with a new program.
///
/// Reads the program at `path` and calls `SYS_PROCESS_EXEC` to replace the
/// current process.  On success, this function does not return.  On
/// failure, returns -1 with errno set.
///
/// A file beginning `#!` is run by the interpreter it names, as on Linux —
/// see [`load_program`].  Anything that is neither that nor an ELF image is
/// `ENOEXEC`; unlike `execvp`, `execve` never falls back to a shell.
///
/// `argv` and `envp` are null-terminated arrays of null-terminated C
/// strings.  They are packed into contiguous buffers and passed to the
/// kernel so the new binary can read them via `SYS_PROCESS_GET_ARGS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];
    exec_with(path, argv, envp, &[], &mut argv_buf, &mut envp_buf)
}

/// The body of [`execve`]. Returns only on failure: -1, with errno set.
///
/// Separate so that `execvpe`'s PATH search can try candidate after candidate
/// with one pair of packing buffers — each 128 KiB, and on the stack — and so
/// that its shell fallback can put `/bin/sh file` in front of the caller's
/// arguments (`prefix`, empty for everyone else).
fn exec_with(
    path: *const u8,
    argv: *const *const u8,
    envp: *const *const u8,
    prefix: &[&[u8]],
    argv_buf: &mut [u8],
    envp_buf: &mut [u8],
) -> i32 {
    #[cfg(all(test, not(target_os = "none")))]
    exec_probe::record(envp);
    // The program, following any `#!` chain, and its packed argument list.
    // `E2BIG` rather than a silent truncation when either list is longer than
    // `ARG_MAX`.
    let (image, argv_len) = match load_program(path, argv, prefix, argv_buf) {
        Ok(loaded) => loaded,
        Err(err) => {
            errno::set_errno(err);
            return -1;
        }
    };
    let Some(envp_len) = pack_cstring_array(envp, envp_buf) else {
        errno::set_errno(errno::E2BIG);
        return -1;
    };

    // Preserve the current userspace fd table across the image
    // replacement.  A native process keeps its fd → kernel-handle map in
    // *userspace*, which `exec` wipes; without this the new image would
    // lose every `dup2()` redirection a shell set up before exec (the
    // `cmd > file` / `$(...)` / pipeline primitive).  We snapshot the
    // inheritable fds (`build_fd_map` with no file actions == "current
    // table minus cloexec/epoll/timerfd/inotify") and hand them to the
    // kernel, which stores them for the new image to read back via
    // `SYS_PROCESS_GET_INITIAL_FDS` during startup.  The handles are the
    // process's own (owned via the kernel's `ipc_handles`, which survive
    // exec) — this only rebuilds the userspace fd→handle mapping.
    //
    // Best-effort: a failure here costs only the redirection, not the
    // exec.  Done just before `SYS_PROCESS_EXEC` so it reflects the final
    // fd table; on exec failure it is harmlessly overwritten by the next
    // exec (or dropped at exit — the kernel never closes these aliases).
    {
        let mut fd_map = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        // No file actions, so nothing is opened and nothing can fail: the
        // inheritable descriptors, flattened.
        let fd_count = flatten_fd_map(&inheritable_fds(), &mut fd_map);
        let _ = syscall2(
            SYS_PROCESS_SET_EXEC_FDS,
            if fd_count > 0 {
                fd_map.as_ptr() as u64
            } else {
                0
            },
            fd_count as u64,
        );
    }

    // Replace the current process image with argv/envp.
    let ret = syscall6(
        SYS_PROCESS_EXEC,
        image.ptr as u64,
        image.len as u64,
        if argv_len > 0 {
            argv_buf.as_ptr() as u64
        } else {
            0
        },
        argv_len as u64,
        if envp_len > 0 {
            envp_buf.as_ptr() as u64
        } else {
            0
        },
        envp_len as u64,
    );

    // If we get here, exec failed. Unmap the image before setting errno, so
    // nothing the unmap does can disturb the value the caller will read.
    drop(image);
    let _ = errno::translate(ret);
    -1
}

/// Pack a null-terminated array of C strings into a contiguous buffer.
///
/// Each string is copied with its null terminator. `Some(total_bytes)`, or
/// `None` if the list does not fit — which is `E2BIG`'s case and the caller's
/// to report. A null `array` packs nothing and fits trivially.
///
/// IT USED TO TRUNCATE SILENTLY. `break; // Truncate silently if buffer is
/// full.` stood here, and the loss was invisible in both directions:
/// `count_cstring_array` counts the whole list regardless of any buffer, so an
/// oversized `argv` was packed short AND announced at its full `argc`. The
/// child started with fewer arguments than its parent passed and nothing
/// anywhere said so.
///
/// The buffer is `EXEC_PACKED_MAX` (128 KiB), which is also what
/// `sysconf(_SC_ARG_MAX)` advertises — so refusing here is the limit the
/// library already promises, not a new one.
fn pack_cstring_array(array: *const *const u8, buf: &mut [u8]) -> Option<usize> {
    if array.is_null() {
        return Some(0);
    }
    let mut pos = 0usize;
    let mut i = 0usize;
    loop {
        // SAFETY: Caller guarantees array is null-terminated.
        let ptr = unsafe { *array.add(i) };
        if ptr.is_null() {
            break;
        }
        let slen = unsafe { crate::file::c_strlen_pub(ptr) };
        // Need slen + 1 bytes (string + null terminator).
        let needed = slen + 1;
        if pos + needed > buf.len() {
            // The whole list or none of it. Packing what fits would hand the
            // kernel a buffer that disagrees with the `argc` beside it.
            return None;
        }
        // SAFETY: ptr points to a valid C string of length slen.
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr().add(pos), slen);
        }
        // Explicit null terminator.
        if let Some(b) = buf.get_mut(pos + slen) {
            *b = 0;
        }
        pos += needed;
        i += 1;
    }
    Some(pos)
}

/// Count the number of strings in a null-terminated C string array.
///
/// Used to determine `argc`/`envc` for `SpawnExArgs`.
/// Returns 0 if `array` is null.
fn count_cstring_array(array: *const *const u8) -> usize {
    if array.is_null() {
        return 0;
    }
    let mut count = 0usize;
    loop {
        // SAFETY: Caller guarantees array is null-terminated.
        let ptr = unsafe { *array.add(count) };
        if ptr.is_null() {
            break;
        }
        count = count.wrapping_add(1);
    }
    count
}

// ---------------------------------------------------------------------------
// execvp
// ---------------------------------------------------------------------------

/// Replace the current process image, searching PATH for the executable.
///
/// Exactly `execvpe(file, argv, environ)`, as glibc defines it: the new image
/// gets the calling process's current environment — see [`execvpe`] for the
/// search and for the shell fallback.
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execvp(file: *const u8, argv: *const *const u8) -> i32 {
    execvpe(file, argv, crate::environ::current_environ())
}

// ---------------------------------------------------------------------------
// execv
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program.
///
/// Like `execve` but inherits the current environment: POSIX says the new
/// image's environment "shall be taken from the external variable `environ`
/// in the calling process". It passed NULL until 2026-09-24, which the kernel
/// faithfully stored as an empty environment — see
/// [`crate::environ::current_environ`].
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execv(path: *const u8, argv: *const *const u8) -> i32 {
    execve(path, argv, crate::environ::current_environ())
}

// ---------------------------------------------------------------------------
// fexecve
// ---------------------------------------------------------------------------

/// Replace the current process image using an open file descriptor.
///
/// Like `execve` but takes an open fd instead of a path.  If the fd
/// has an associated path in the fd table, we resolve it and delegate
/// to `execve`.  Otherwise, returns -1 with `ENOENT`.
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fexecve(fd: i32, argv: *const *const u8, envp: *const *const u8) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // Try to resolve the fd to a path via the fd table's stored path.
    let mut path_buf = [0u8; crate::unistd::PATH_MAX];
    let path_len = crate::fdtable::get_fd_path(fd, &mut path_buf);
    if path_len == 0 {
        // No path associated with this fd.
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    execve(path_buf.as_ptr(), argv, envp)
}

// ---------------------------------------------------------------------------
// execl / execlp / execle — the variadic exec family
// ---------------------------------------------------------------------------
//
// These are the forms a C programmer writes when the argument list is known
// at the call site: `execl("/bin/sh", "sh", "-c", cmd, (char *)NULL)`.  We had
// the vector forms (`execv`/`execvp`/`execve`) and not the list forms, which
// is invisible until something real is linked: `scripts/coreutils-spike/run.sh`
// found `execl` and `execlp` among the nineteen symbols missing from
// `libc.a`.  zig's musl headers *declare* them, so `./configure` concluded
// they existed and compiled calls to them; the gap appeared only at link time.
//
// `execle` is not in the spike's list — coreutils happens not to call it — but
// it is the third member of the same POSIX family, it is missing for exactly
// the same reason, and it costs one extra line given the shared body below.
// Adding only the two the spike named would leave a hole of the same shape for
// the next program to fall into.
//
// The trampoline is the same `va_trampoline!` that `printf` uses: it spills
// the argument registers, builds a genuine System V `va_list` on its own
// frame, and tail-calls the `v*` worker.  One named integer parameter (`path`)
// means the list starts at `%rsi`, i.e. at the *first* argument — which is
// `argv[0]`, exactly as POSIX specifies.  (The C prototype names two
// parameters, `path` and `arg`, but that is a source-level detail: the caller
// places arguments identically either way, so counting `path` alone and
// letting `arg` fall inside the list is both correct and simpler.)

/// Argument vector slots assembled on the stack before falling back to `malloc`.
///
/// Sized for the realistic case — an `execl` call site is a literal argument
/// list written out in source, so this is generous — while the heap fallback
/// below means exceeding it costs an allocation rather than an error.  glibc
/// uses `alloca` here and so has no fixed point at all; we cannot, so we pick
/// a threshold instead of a limit.
#[cfg(target_os = "none")]
const EXECL_STACK_ARGV: usize = 64;

/// Which of the three list-form execs is being performed.
#[cfg(target_os = "none")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecLMode {
    /// `execl` — use `path` verbatim, inherit the environment.
    Direct,
    /// `execlp` — search `PATH` for `path`, inherit the environment.
    SearchPath,
    /// `execle` — use `path` verbatim; `envp` follows the terminating NULL.
    WithEnv,
}

/// Shared body of `execl`, `execlp` and `execle`.
///
/// # Safety
/// `ap` must be a valid, ABI-conformant `va_list` positioned at `argv[0]`,
/// whose remaining arguments are `char *` values terminated by a NULL — and,
/// for [`ExecLMode::WithEnv`], one further `char *const *` after that NULL.
#[cfg(target_os = "none")]
unsafe fn execl_body(path: *const u8, ap: *mut VaList, mode: ExecLMode) -> i32 {
    if path.is_null() || ap.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null per the check above, and valid per this function's
    // safety contract.
    let ap = unsafe { &mut *ap };

    // Pass 1 — count the arguments, on a *copy* of the list, so pass 2 starts
    // from the same place.  `VaList` is `Copy` and duplicating it is precisely
    // what C's `va_copy` does: the cursors are per-copy, while the register
    // save and overflow areas they index are only ever read.  Two passes are
    // needed because the vector has to be one contiguous NULL-terminated
    // array and we cannot know how long it is without walking it first.
    let mut argc = 0usize;
    {
        let mut probe = *ap;
        // SAFETY: `probe` is an independent cursor over the same conformant
        // save areas; `va_arg_int` reads one 8-byte slot per call.
        while !(unsafe { crate::printf::va_arg_int(&mut probe) } as *const u8).is_null() {
            argc = argc.saturating_add(1);
        }
    }

    // Storage for `argc` pointers plus the terminating NULL.
    let mut stack_argv = [core::ptr::null::<u8>(); EXECL_STACK_ARGV];
    let mut heap: *mut u8 = core::ptr::null_mut();
    let argv: *mut *const u8 = if argc < EXECL_STACK_ARGV {
        stack_argv.as_mut_ptr()
    } else {
        let slots = argc.saturating_add(1);
        let bytes = slots.saturating_mul(core::mem::size_of::<*const u8>());
        heap = crate::malloc::malloc(bytes);
        if heap.is_null() {
            errno::set_errno(errno::ENOMEM);
            return -1;
        }
        heap.cast()
    };

    // Pass 2 — collect.  This consumes the terminating NULL as well, which is
    // what leaves `ap` positioned at `envp` for `execle`.
    for i in 0..argc {
        // SAFETY: `ap` has the same contents pass 1 walked, so exactly `argc`
        // non-NULL slots precede the NULL; `i < argc`.
        let arg = unsafe { crate::printf::va_arg_int(ap) } as *const u8;
        // SAFETY: `argv` has room for `argc + 1` slots by construction.
        unsafe { *argv.add(i) = arg };
    }
    // SAFETY: as above — this reads the NULL that terminated pass 1.
    let _ = unsafe { crate::printf::va_arg_int(ap) };
    // SAFETY: slot `argc` is the last of the `argc + 1` allocated.
    unsafe { *argv.add(argc) = core::ptr::null() };

    let envp: *const *const u8 = if mode == ExecLMode::WithEnv {
        // SAFETY: per the safety contract, one `char *const *` follows the NULL.
        unsafe { crate::printf::va_arg_int(ap) as *const *const u8 }
    } else {
        core::ptr::null()
    };

    let argv = argv.cast_const();
    let ret = match mode {
        ExecLMode::Direct => execv(path, argv),
        ExecLMode::SearchPath => execvp(path, argv),
        ExecLMode::WithEnv => execve(path, argv, envp),
    };

    // Reached only when the exec failed, since a successful exec replaces the
    // process image.  `free` may itself touch errno, so save the failure
    // reason across it — the caller is about to read it.
    if !heap.is_null() {
        let saved = errno::get_errno();
        // SAFETY: `heap` came from `malloc` above and has not been freed.
        unsafe { crate::malloc::free(heap) };
        errno::set_errno(saved);
    }
    ret
}

/// `execl(path, arg0, ..., NULL)` — `execv` with a literal argument list.
///
/// # Safety
/// `ap` must be a conformant `va_list` of NUL-terminated `char *` values
/// terminated by a NULL pointer.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexecl(path: *const u8, ap: *mut VaList) -> i32 {
    // SAFETY: forwarded from the caller's contract.
    unsafe { execl_body(path, ap, ExecLMode::Direct) }
}

/// `execlp(file, arg0, ..., NULL)` — `execvp` with a literal argument list.
///
/// # Safety
/// As [`vexecl`].
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexeclp(file: *const u8, ap: *mut VaList) -> i32 {
    // SAFETY: forwarded from the caller's contract.
    unsafe { execl_body(file, ap, ExecLMode::SearchPath) }
}

/// `execle(path, arg0, ..., NULL, envp)` — `execve` with a literal argument list.
///
/// # Safety
/// As [`vexecl`], plus: one `char *const *` must follow the terminating NULL.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vexecle(path: *const u8, ap: *mut VaList) -> i32 {
    // SAFETY: forwarded from the caller's contract.
    unsafe { execl_body(path, ap, ExecLMode::WithEnv) }
}

#[cfg(target_os = "none")]
va_trampoline!("execl", "vexecl", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("execlp", "vexeclp", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("execle", "vexecle", "8", "rsi");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A raw kernel file handle, closed when dropped.
///
/// `load_elf` opens outside the fd table (it wants the bytes, not a
/// descriptor), so nothing else would ever close it; every early return below
/// must release it, and a guard makes that structural rather than a list of
/// `close` calls to keep in step with the returns.
struct KernelFileHandle(u64);

impl Drop for KernelFileHandle {
    fn drop(&mut self) {
        // Nothing useful can be done with a failed close of a read-only handle
        // on an error or success path that has already decided its result; the
        // kernel also reclaims it at process exit.
        let _ = syscall1(SYS_FS_CLOSE, self.0);
    }
}

/// Read the file at `path` (absolute, `path_len` bytes, no NUL needed) into
/// anonymous memory.
///
/// Returns the bytes as an [`ElfImage`], which unmaps itself when dropped, or a
/// POSIX error number. Nothing here checks what the bytes *are* —
/// [`load_program`] decides between an ELF image and a `#!` script.
///
/// # One handle, not two path lookups
///
/// The file is opened once, and its type, its size and its bytes are all read
/// through that one handle.  Until 2026-09-24 this was a `SYS_FS_STAT` by path
/// followed by a `SYS_FS_READ_FILE` by path, which had two defects:
///
/// * **It demanded a right that running a program does not need.**  Native
///   `SYS_FS_STAT` is gated on `(File, METADATA)`, while `SYS_FS_OPEN` and
///   `SYS_FS_READ` need `READ` and `SYS_FS_FSTAT` on a handle the caller owns
///   needs nothing further.  So a process holding exactly `READ | EXECUTE`
///   could not exec anything: `execve` failed in its first syscall, with
///   `EACCES`, before the kernel's exec path — which is the one that logs —
///   was ever reached.  That is `ctest-coreutils-runs`' exit 11 and
///   `ctest-python-repl`'s exit 8, both of which were reported as `execl`
///   losing its path (`requests/a-b-libc-execl-passes-a-null-path-to-execve.md`).
/// * **It raced.**  Two lookups can name two files: replacing the binary
///   between them sized the buffer from one file and filled it from the other.
///
/// # Errors
///
/// As `execve(2)` reports them: the open's own error (`ENOENT`, `EACCES`, …);
/// `EACCES` for anything that is not a regular file, a directory included;
/// `ENOEXEC` for an empty file; `ENOMEM` when the buffer cannot be mapped.
fn load_elf(path: *const u8, path_len: usize) -> Result<ElfImage, i32> {
    let opened = syscall3(
        SYS_FS_OPEN,
        path as u64,
        path_len as u64,
        crate::file::translate_open_flags(crate::fcntl::O_RDONLY),
    );
    if opened < 0 {
        // Linux's `execve` says `EACCES`, not `EISDIR`, for a directory, and a
        // kernel that refuses to open one read-only must not change the answer.
        let err = native_to_posix_err(opened);
        return Err(if err == errno::EISDIR {
            errno::EACCES
        } else {
            err
        });
    }
    #[allow(clippy::cast_sign_loss)] // `opened >= 0` was checked just above.
    let handle = KernelFileHandle(opened as u64);

    // SYS_FS_FSTAT writes the kernel's 80-byte FsStatResult, not a
    // `struct stat`, so translate it.
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let fstat_ret = syscall2(SYS_FS_FSTAT, handle.0, raw.as_mut_ptr() as u64);
    if fstat_ret < 0 {
        return Err(native_to_posix_err(fstat_ret));
    }
    let mut stat_buf = crate::stat::Stat::zeroed();
    crate::stat::fill_from_fsstat(&mut stat_buf, &raw);
    if !stat_buf.is_file() {
        return Err(errno::EACCES);
    }
    let Ok(file_size) = usize::try_from(stat_buf.st_size) else {
        return Err(errno::ENOMEM);
    };
    if file_size == 0 {
        return Err(errno::ENOEXEC);
    }

    let buf = mman::mmap(
        core::ptr::null_mut(),
        file_size,
        mman::PROT_READ | mman::PROT_WRITE,
        mman::MAP_PRIVATE | mman::MAP_ANONYMOUS,
        -1,
        0,
    );
    if buf == mman::MAP_FAILED {
        return Err(errno::ENOMEM);
    }
    // Owned from here: every return below unmaps it by dropping `image`.
    let mut image = ElfImage {
        ptr: buf.cast::<u8>(),
        alloc: file_size,
        len: 0,
    };

    // A read may return fewer bytes than asked; keep going until the size
    // `fstat` reported is filled or the file ends early (it shrank since).
    while image.len < file_size {
        let remaining = file_size.saturating_sub(image.len);
        // SAFETY: `image.len < file_size`, so the offset stays inside the
        // `file_size`-byte mapping made above.
        let dst = unsafe { image.ptr.add(image.len) };
        let got = syscall3(SYS_FS_READ, handle.0, dst as u64, remaining as u64);
        if got < 0 {
            return Err(native_to_posix_err(got));
        }
        if got == 0 {
            break;
        }
        #[allow(clippy::cast_sign_loss)] // `got > 0` here.
        let got = got as usize;
        image.len = image.len.saturating_add(got.min(remaining));
    }

    if image.len == 0 {
        return Err(errno::ENOEXEC);
    }
    Ok(image)
}

/// A program image read into anonymous memory by [`load_elf`], unmapped when
/// dropped.
///
/// A guard rather than the `(ptr, alloc, len)` triple this used to be, because
/// every caller had to `munmap` on each of its own failure paths with the
/// *allocation* size rather than the data size, and following a `#!` chain
/// adds more of those paths than it is reasonable to get right by hand. On a
/// successful `exec` the guard is never dropped — the whole address space it
/// lives in is replaced — which is correct: there is nothing left to free.
struct ElfImage {
    ptr: *mut u8,
    /// The mapping's size, which `munmap` needs.
    alloc: usize,
    /// How many bytes were actually read, which is what the kernel is given.
    len: usize,
}

impl ElfImage {
    fn bytes(&self) -> &[u8] {
        // SAFETY: `ptr` is a live mapping of `alloc >= len` bytes owned by this
        // guard, and the first `len` of them were written by `SYS_FS_READ`.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for ElfImage {
    fn drop(&mut self) {
        // A failed unmap of our own private anonymous mapping has no remedy
        // here; the region is reclaimed with the process in any case.
        let _ = mman::munmap(self.ptr.cast::<core::ffi::c_void>(), self.alloc);
    }
}

/// Load the program `path` names, following `#!` interpreter lines, and pack
/// the argument list it will run with.
///
/// This is the part of `execve(2)` that Linux does in the kernel and ours has
/// to do here, because the native exec takes bytes rather than a path: decide
/// what the file *is*. An ELF image is returned as it is. A script names its
/// interpreter, which is loaded in its place with the argument list rewritten
/// to `interpreter [argument] script arg1 …` — see [`crate::shebang`] for the
/// rules, all of them Linux's. Anything else is `ENOEXEC`, which is the error
/// `execvp`'s shell fallback keys on, so it is decided here rather than left
/// to whatever the kernel's loader happens to say.
///
/// `prefix`, when not empty, first replaces the caller's `argv[0]` — the
/// shell fallback's `/bin/sh file`.
///
/// Errors come in Linux's order: the file's own (`ENOENT`, `EACCES`, …) before
/// the argument list's `E2BIG`, and an interpreter's after both.
///
/// Returns the image and the packed length in `argv_buf`.
fn load_program(
    path: *const u8,
    argv: *const *const u8,
    prefix: &[&[u8]],
    argv_buf: &mut [u8],
) -> Result<(ElfImage, usize), i32> {
    // The path of the file being loaded, as a C string: the caller's first,
    // then each interpreter's as its script names it. Copied rather than
    // borrowed, because from the second level on it would otherwise point into
    // `argv_buf`, which the splice below rewrites.
    let mut current = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: `path` is a valid C string (the callers' contract, and they
    // reject null).
    let path_len = unsafe { crate::file::c_strlen_pub(path) };
    if path_len == 0 {
        return Err(errno::ENOENT);
    }
    if path_len >= current.len() {
        return Err(errno::ENAMETOOLONG);
    }
    // SAFETY: `path` is readable for `path_len` bytes, and `current` has room
    // for them plus the NUL that is already there.
    unsafe { core::ptr::copy_nonoverlapping(path, current.as_mut_ptr(), path_len) };
    let mut current_len = path_len;

    let mut image = load_by_name(&current)?;
    let mut argv_len = pack_cstring_array(argv, argv_buf).ok_or(errno::E2BIG)?;
    if !prefix.is_empty() {
        argv_len = crate::shebang::splice_argv(argv_buf, argv_len, prefix).ok_or(errno::E2BIG)?;
    }

    for _ in 0..crate::shebang::MAX_INTERP_DEPTH {
        let Some(parsed) = crate::shebang::parse(image.bytes()) else {
            return if image.bytes().starts_with(&ELF_MAGIC) {
                Ok((image, argv_len))
            } else {
                Err(errno::ENOEXEC)
            };
        };
        let script = parsed?;
        // The script's own bytes are not needed past this point, and the
        // interpreter is about to be read beside them.
        drop(image);

        let script_path = current.get(..current_len).ok_or(errno::ENAMETOOLONG)?;
        argv_len = match script.arg() {
            Some(arg) => crate::shebang::splice_argv(
                argv_buf,
                argv_len,
                &[script.interp(), arg, script_path],
            ),
            None => {
                crate::shebang::splice_argv(argv_buf, argv_len, &[script.interp(), script_path])
            }
        }
        .ok_or(errno::E2BIG)?;

        let interp = script.interp();
        let dst = current.get_mut(..interp.len()).ok_or(errno::ENAMETOOLONG)?;
        dst.copy_from_slice(interp);
        *current.get_mut(interp.len()).ok_or(errno::ENAMETOOLONG)? = 0;
        current_len = interp.len();

        image = load_by_name(&current)?;
    }

    // One load past the last permitted rewrite, as Linux's `depth > 5`: a
    // chain that is still a script here is a loop, or near enough to one.
    match crate::shebang::parse(image.bytes()) {
        None if image.bytes().starts_with(&ELF_MAGIC) => Ok((image, argv_len)),
        None => Err(errno::ENOEXEC),
        Some(_) => Err(errno::ELOOP),
    }
}

/// The four bytes every ELF image starts with.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// Resolve the NUL-terminated name in `name` against the working directory and
/// load it.
fn load_by_name(name: &[u8; crate::unistd::PATH_MAX]) -> Result<ElfImage, i32> {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: `name` holds a NUL within its `PATH_MAX` bytes — every writer
    // above leaves one — so it is a valid C string.
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(name.as_ptr(), &mut resolved) })
    else {
        // POSIX: an empty path is ENOENT; the only other refusal is length.
        return Err(if name.first() == Some(&0) {
            errno::ENOENT
        } else {
            errno::ENAMETOOLONG
        });
    };
    load_elf(resolved.as_ptr(), resolved_len)
}

/// Convert a native kernel error code to a POSIX errno value.
///
/// Unlike `errno::translate`, this doesn't set the global errno —
/// it just returns the POSIX error number.  Used by `posix_spawn`
/// which returns errors directly instead of via errno.
#[must_use]
fn native_to_posix_err(ret: i64) -> i32 {
    // Set errno via translate, then read it back.
    // This is slightly wasteful but keeps the mapping in one place.
    let _ = errno::translate(ret);
    errno::get_errno()
}

/// The directories a `PATH` search visits for `file`, as candidate paths.
///
/// POSIX's rules, which glibc and musl both follow: `PATH` is split on `:`;
/// an empty element — leading, trailing or doubled `:` — names the current
/// directory, so `PATH=":/bin"` searches `.` first ("a legacy feature", POSIX
/// says, but a real one); and an unset `PATH` means `confstr(_CS_PATH)`. The
/// old search skipped empty elements, so a program on a `PATH` that relied on
/// one was not found.
///
/// Pure, so the splitting is testable on the host; the callers do the loading.
struct PathCandidates<'a> {
    path: &'a [u8],
    file: &'a [u8],
    /// Where the next element starts; `None` once the last has been produced.
    next: Option<usize>,
}

impl<'a> PathCandidates<'a> {
    fn new(path: &'a [u8], file: &'a [u8]) -> Self {
        Self {
            path,
            file,
            next: Some(0),
        }
    }

    /// Write the next candidate, NUL-terminated, into `out`, and return its
    /// length. `None` when the list is exhausted; `Some(Err(ENAMETOOLONG))`
    /// for a candidate that does not fit, which ends a glibc search too.
    fn next_into(&mut self, out: &mut [u8; crate::unistd::PATH_MAX]) -> Option<Result<usize, i32>> {
        let start = self.next?;
        let rest = self.path.get(start..).unwrap_or(&[]);
        let (dir, more) = match rest.iter().position(|&b| b == b':') {
            Some(colon) => (rest.get(..colon).unwrap_or(&[]), Some(start + colon + 1)),
            None => (rest, None),
        };
        self.next = more;

        // "dir/file", or just "file" for the current directory.
        let sep = usize::from(!dir.is_empty());
        let len = dir.len() + sep + self.file.len();
        if len >= out.len() {
            return Some(Err(errno::ENAMETOOLONG));
        }
        let (head, tail) = out.split_at_mut(dir.len());
        head.copy_from_slice(dir);
        if sep == 1 {
            tail[0] = b'/';
        }
        tail[sep..sep + self.file.len()].copy_from_slice(self.file);
        tail[sep + self.file.len()] = 0;
        Some(Ok(len))
    }
}

/// The errors that mean "not here, try the next directory" in a `PATH` search.
///
/// glibc's list, and its reasoning: each says the file is missing or not
/// usable *by us* at this path. Anything else means a program was found and
/// could not be run, which is the caller's to hear about — `ENOEXEC` from a
/// corrupt binary, `E2BIG`, `ENOMEM` — so the search stops there. `EACCES`
/// continues but is remembered, so a search that finds nothing usable reports
/// "permission denied" rather than "not found" when it did find something.
fn search_continues_after(err: i32) -> bool {
    matches!(
        err,
        errno::EACCES
            | errno::ENOENT
            | errno::ESTALE
            | errno::ENOTDIR
            | errno::ENODEV
            | errno::ETIMEDOUT
    )
}

/// The `PATH` a search uses: the variable, or `confstr(_CS_PATH)` without it.
fn search_path_value() -> &'static [u8] {
    // SAFETY: "PATH\0" is a valid C string.
    let value = unsafe { crate::environ::getenv(c"PATH".as_ptr().cast::<u8>()) };
    if value.is_null() {
        return crate::unistd::CS_PATH;
    }
    // SAFETY: `getenv` returned a NUL-terminated string that lives in the
    // environment. `'static` is what the environment's lifetime is, as far as
    // any single call can know; this slice is consumed before this call
    // returns to a caller that could `setenv`.
    unsafe {
        let len = crate::string::strlen(value);
        core::slice::from_raw_parts(value, len)
    }
}

/// `execve`, and on `ENOEXEC` the shell fallback that POSIX requires of
/// `execvp` and `execlp`: "execute a command interpreter … as if the process
/// invoked the sh utility using `execl(<shell path>, arg0, file, arg1, …)`".
/// As glibc, `argv[0]` becomes the shell's own path. Returns only on failure,
/// with errno set — the shell's errno if the fallback ran and failed.
fn exec_or_shell(
    path: *const u8,
    path_bytes: &[u8],
    argv: *const *const u8,
    envp: *const *const u8,
    argv_buf: &mut [u8],
    envp_buf: &mut [u8],
) {
    let _ = exec_with(path, argv, envp, &[], argv_buf, envp_buf);
    if errno::get_errno() == errno::ENOEXEC {
        let shell = crate::paths::_PATH_BSHELL;
        let shell_name = shell.get(..shell.len().saturating_sub(1)).unwrap_or(&[]);
        let _ = exec_with(
            shell.as_ptr(),
            argv,
            envp,
            &[shell_name, path_bytes],
            argv_buf,
            envp_buf,
        );
    }
}

// ---------------------------------------------------------------------------
// execvpe — exec with PATH search + custom environment
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program, searching `PATH`.
///
/// Like `execve`, with `file` looked for in each `PATH` directory in turn
/// when it contains no `/` — see [`PathCandidates`] for the splitting and
/// [`search_continues_after`] for which failures move on to the next
/// directory. A file found but not in an executable format is run by
/// `/bin/sh` (POSIX's rule for this family, not for `execve`).
///
/// The search *attempts* each candidate rather than checking that it exists
/// first. The old code tested existence with `SYS_FS_STAT`, which needs a
/// right (`METADATA`) that running a program does not, and then committed to
/// the first name that existed — a directory, an unreadable file — where glibc
/// would have moved on.
///
/// Returns -1 with errno set; on success it does not return.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execvpe(file: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32 {
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `file` is a valid C string (the caller's contract).
    let file_len = unsafe { crate::file::c_strlen_pub(file) };
    if file_len == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    // SAFETY: readable for `file_len` bytes, just measured.
    let file_bytes = unsafe { core::slice::from_raw_parts(file, file_len) };

    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];

    if file_bytes.contains(&b'/') {
        exec_or_shell(file, file_bytes, argv, envp, &mut argv_buf, &mut envp_buf);
        return -1;
    }
    if file_len > crate::linux_limits::NAME_MAX {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }

    let mut candidates = PathCandidates::new(search_path_value(), file_bytes);
    let mut candidate = [0u8; crate::unistd::PATH_MAX];
    let mut denied = false;
    while let Some(next) = candidates.next_into(&mut candidate) {
        let len = match next {
            Ok(len) => len,
            Err(err) => {
                errno::set_errno(err);
                return -1;
            }
        };
        let name = candidate.get(..len).unwrap_or(&[]);
        exec_or_shell(
            candidate.as_ptr(),
            name,
            argv,
            envp,
            &mut argv_buf,
            &mut envp_buf,
        );
        let err = errno::get_errno();
        if !search_continues_after(err) {
            return -1;
        }
        denied |= err == errno::EACCES;
    }
    if denied {
        errno::set_errno(errno::EACCES);
    }
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test builds only: the `envp` the latest [`exec_with`] on this thread was
/// handed.
///
/// No exec can succeed on the host -- `load_program` meets `ENOSYS` first -- so
/// without this nothing can observe which environment an `exec*` call passes
/// on. That is how `execv` and `execvp` passing none at all went unnoticed
/// until a CPython rung lost `PYTHONHOME` on the target
/// (`requests/a-d-execv-execvp-execl-execlp-start-the-new-program-with-no-environment.md`).
/// Every exec entry point funnels through `exec_with`, so recording there
/// covers them all.
#[cfg(all(test, not(target_os = "none")))]
mod exec_probe {
    extern crate std;
    use core::cell::Cell;

    std::thread_local! {
        static LAST_ENVP: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn record(envp: *const *const u8) {
        LAST_ENVP.with(|c| c.set(envp as usize));
    }

    pub(super) fn last() -> *const *const u8 {
        LAST_ENVP.with(Cell::get) as *const *const u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `super::*` re-exports `CapEntryInfo` but not the modules of discriminants
    // beside it, and the ex2 tests build entries out of real `ResourceType` and
    // `Rights` values rather than invented ones — an entry whose type is 0 would
    // pass a shape test while being a request the kernel rejects.
    use crate::sys_capability::kernel_view;

    // -- posix_spawnattr_t ABI and round-tripping --

    /// The attribute object crosses the C ABI, so its size is a contract
    /// with every object our cross-toolchain compiled against musl's
    /// `<spawn.h>`.  Adding a field without shrinking `_reserved` would
    /// enlarge the struct and let `posix_spawnattr_init` write past the
    /// end of a caller's 336-byte stack slot — a stack smash that no
    /// compiler warning would catch, because the two sides are compiled
    /// from different headers.
    #[test]
    fn test_spawnattr_matches_musl_layout() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<PosixSpawnattrT>(), 336, "musl posix_spawnattr_t");
        assert_eq!(align_of::<PosixSpawnattrT>(), 8);
        // Field offsets, against musl's declaration order:
        //   __flags 0, __pgrp 4, __def 8, __mask 136, __prio 264, __pol 268
        let a = PosixSpawnattrT {
            flags: 0,
            pgroup: 0,
            sigdefault: crate::signal::SigsetT::EMPTY,
            sigmask: crate::signal::SigsetT::EMPTY,
            schedpriority: 0,
            schedpolicy: 0,
            _reserved: [0; 64],
        };
        let base = (&raw const a).cast::<u8>() as usize;
        let off = |p: usize| p - base;
        assert_eq!(off((&raw const a.flags).cast::<u8>() as usize), 0);
        assert_eq!(off((&raw const a.pgroup).cast::<u8>() as usize), 4);
        assert_eq!(off((&raw const a.sigdefault).cast::<u8>() as usize), 8);
        assert_eq!(off((&raw const a.sigmask).cast::<u8>() as usize), 136);
        assert_eq!(off((&raw const a.schedpriority).cast::<u8>() as usize), 264);
        assert_eq!(off((&raw const a.schedpolicy).cast::<u8>() as usize), 268);
    }

    /// The same contract for the file-actions object — and the reason this
    /// test exists at all is that its sibling above had it and this one did
    /// not.
    ///
    /// `PosixSpawnattrT` was guarded, and was right.  `PosixSpawnFileActionsT`
    /// sat beside it in this file, was reached by the same callers through the
    /// same header, and was **4624 bytes where C says 80** — because it stored
    /// its sixteen 288-byte action slots inline.  `posix_spawn_file_actions_init`
    /// therefore wrote 4544 bytes past the end of the caller's stack object on
    /// its first call.  GNU make hit it on every recipe and crashed a hundred
    /// instructions later reading a local that the overrun had zeroed.
    ///
    /// Nothing in Rust could have caught that: every Rust caller shares this
    /// definition, so the type is self-consistent and only disagrees with the
    /// C header it is supposed to be implementing.  A hardcoded 80 is the only
    /// thing that can state the other side of the contract, which is why the
    /// number is written out here rather than derived from the fields.
    ///
    /// musl:  `{ int __pad0[2];               void *__actions; int __pad[16]; }`
    /// glibc: `{ int __allocated; int __used; struct __spawn_action *__actions; int __pad[16]; }`
    ///
    /// Both are 80 bytes with the pointer at offset 8; we match glibc's naming
    /// because we use both leading ints for what glibc uses them for.
    #[test]
    fn test_file_actions_matches_musl_layout() {
        use core::mem::{align_of, size_of};
        assert_eq!(
            size_of::<PosixSpawnFileActionsT>(),
            80,
            "posix_spawn_file_actions_t is 80 bytes in every C header; a Rust \
             definition that is larger overruns the caller's stack slot"
        );
        assert_eq!(align_of::<PosixSpawnFileActionsT>(), 8);
        // SAFETY: the type is four plain-data fields (two i32, a raw pointer,
        // and an i32 array); an all-zero bit pattern is the same value
        // `posix_spawn_file_actions_init` writes, and a null `actions` is the
        // documented "nothing allocated yet" state.
        let a = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        let base = (&raw const a).cast::<u8>() as usize;
        let off = |p: usize| p - base;
        assert_eq!(off((&raw const a.allocated).cast::<u8>() as usize), 0);
        assert_eq!(off((&raw const a.used).cast::<u8>() as usize), 4);
        assert_eq!(off((&raw const a.actions).cast::<u8>() as usize), 8);
        assert_eq!(off((&raw const a._pad).cast::<u8>() as usize), 16);
    }

    /// Every setter's value must come back out of its getter unchanged.
    /// A setter that silently dropped its argument would still let CPython
    /// link and would still let `os.posix_spawn` "succeed" — the failure
    /// would only appear as a child running with the wrong signal mask,
    /// which is exactly the kind of bug that is impossible to attribute
    /// after the fact.
    #[test]
    fn test_spawnattr_attributes_round_trip() {
        let mut attr = core::mem::MaybeUninit::<PosixSpawnattrT>::uninit();
        assert_eq!(posix_spawnattr_init(attr.as_mut_ptr()), 0);
        // SAFETY: posix_spawnattr_init returned 0, so every field is
        // initialised.
        let attr = unsafe { attr.assume_init_mut() };

        let mut def = crate::signal::SigsetT::EMPTY;
        def.bits[0] = 0x0000_0000_0000_00ff;
        def.bits[15] = 0x8000_0000_0000_0000;
        let mut mask = crate::signal::SigsetT::EMPTY;
        mask.bits[1] = 0xdead_beef_cafe_f00d;

        assert_eq!(posix_spawnattr_setsigdefault(attr, &raw const def), 0);
        assert_eq!(posix_spawnattr_setsigmask(attr, &raw const mask), 0);
        assert_eq!(
            posix_spawnattr_setschedpolicy(attr, crate::sched::SCHED_RR),
            0
        );
        let param = crate::sched::SchedParam {
            sched_priority: 42,
            ..Default::default()
        };
        assert_eq!(posix_spawnattr_setschedparam(attr, &raw const param), 0);

        let mut got_def = crate::signal::SigsetT::EMPTY;
        let mut got_mask = crate::signal::SigsetT::EMPTY;
        let mut got_pol = 0_i32;
        let mut got_param = crate::sched::SchedParam {
            sched_priority: 0,
            ..Default::default()
        };
        assert_eq!(posix_spawnattr_getsigdefault(attr, &raw mut got_def), 0);
        assert_eq!(posix_spawnattr_getsigmask(attr, &raw mut got_mask), 0);
        assert_eq!(posix_spawnattr_getschedpolicy(attr, &raw mut got_pol), 0);
        assert_eq!(posix_spawnattr_getschedparam(attr, &raw mut got_param), 0);

        assert_eq!(got_def, def);
        assert_eq!(got_mask, mask);
        assert_eq!(got_pol, crate::sched::SCHED_RR);
        assert_eq!(got_param.sched_priority, 42);

        // Setting one attribute must not disturb its neighbours.  The
        // failure mode of a mis-declared struct is that two fields
        // overlap, and only a cross-check like this catches it: `flags`
        // and `pgroup` sit immediately before the signal sets, so a
        // 128-byte `sigdefault` written at the wrong offset lands on
        // them.
        let mut got_flags = -1_i16;
        let mut got_pgrp: PidT = -1;
        assert_eq!(posix_spawnattr_getflags(attr, &raw mut got_flags), 0);
        assert_eq!(posix_spawnattr_getpgroup(attr, &raw mut got_pgrp), 0);
        assert_eq!(got_flags, 0, "flags clobbered by a neighbouring setter");
        assert_eq!(got_pgrp, 0, "pgroup clobbered by a neighbouring setter");
    }

    /// A freshly-initialised object must read back as zeroed, not as
    /// whatever was on the caller's stack.
    #[test]
    fn test_spawnattr_init_clears_signal_sets() {
        // Fill the storage with a non-zero pattern first, so a missing
        // assignment in `posix_spawnattr_init` shows up as that pattern.
        // The buffer must be a `MaybeUninit<PosixSpawnattrT>` rather than
        // a `[u8; 336]`: the latter is 1-byte aligned, and casting it to
        // an 8-byte-aligned struct is UB that Rust's debug runtime traps.
        let mut storage = core::mem::MaybeUninit::<PosixSpawnattrT>::uninit();
        let attr = storage.as_mut_ptr();
        // SAFETY: `attr` points at 336 uninitialised but allocated bytes
        // with the struct's own alignment; writing a byte pattern over
        // them leaves the object initialised for every field type here
        // (integers and `[u64; 16]`, all valid for any bit pattern).
        unsafe {
            core::ptr::write_bytes(
                attr.cast::<u8>(),
                0xa5,
                core::mem::size_of::<PosixSpawnattrT>(),
            );
        }
        assert_eq!(posix_spawnattr_init(attr), 0);
        let mut got = crate::signal::SigsetT { bits: [0xdead; 16] };
        assert_eq!(posix_spawnattr_getsigdefault(attr, &raw mut got), 0);
        assert_eq!(got, crate::signal::SigsetT::EMPTY);
        assert_eq!(posix_spawnattr_getsigmask(attr, &raw mut got), 0);
        assert_eq!(got, crate::signal::SigsetT::EMPTY);
        let mut pol = -1_i32;
        assert_eq!(posix_spawnattr_getschedpolicy(attr, &raw mut pol), 0);
        assert_eq!(pol, 0);
    }

    /// NULL on either side is `EFAULT`, returned (not set in `errno`) —
    /// this family reports errors through its return value.
    #[test]
    fn test_spawnattr_null_arguments_report_efault() {
        let set = crate::signal::SigsetT::EMPTY;
        let param = crate::sched::SchedParam {
            sched_priority: 0,
            ..Default::default()
        };
        assert_eq!(
            posix_spawnattr_setsigmask(core::ptr::null_mut(), &raw const set),
            errno::EFAULT
        );
        assert_eq!(
            posix_spawnattr_setsigdefault(core::ptr::null_mut(), &raw const set),
            errno::EFAULT
        );
        assert_eq!(
            posix_spawnattr_setschedparam(core::ptr::null_mut(), &raw const param),
            errno::EFAULT
        );
        assert_eq!(
            posix_spawnattr_setschedpolicy(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
        // A valid object with a NULL value pointer is equally EFAULT: the
        // setter would otherwise read from address zero.
        let mut attr = core::mem::MaybeUninit::<PosixSpawnattrT>::uninit();
        assert_eq!(posix_spawnattr_init(attr.as_mut_ptr()), 0);
        assert_eq!(
            posix_spawnattr_setsigmask(attr.as_mut_ptr(), core::ptr::null()),
            errno::EFAULT
        );
        assert_eq!(
            posix_spawnattr_getsigmask(attr.as_ptr(), core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    // -- fd handle-type <-> HandleKind mapping --

    /// Every kind that can actually be transferred to a child must survive
    /// the serialise/reconstruct round trip, so the parent's
    /// `kind_to_handle_type` and the child crt0's `handle_type_to_kind`
    /// cannot drift apart again (BUG-CRT0-STREAM-SOCKET-UNMAPPED).
    #[test]
    fn round_trips_for_every_transferable_kind() {
        use crate::fdtable::HandleKind;
        for kind in [
            HandleKind::File,
            HandleKind::Pipe,
            HandleKind::Console,
            HandleKind::TcpStream,
            HandleKind::UdpSocket,
            HandleKind::Eventfd,
            HandleKind::UnixStream,
        ] {
            assert_eq!(
                handle_type_to_kind(kind_to_handle_type(kind)),
                kind,
                "{kind:?} did not survive the fd-inheritance round trip"
            );
        }
    }

    /// The regression itself: an inherited `AF_UNIX` endpoint must come back
    /// as `UnixStream`, not silently degrade to `File`.
    #[test]
    fn unix_stream_is_not_rebuilt_as_a_plain_file() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::UnixStream),
            fd_handle_type::STREAM_SOCKET
        );
        assert_eq!(
            handle_type_to_kind(fd_handle_type::STREAM_SOCKET),
            HandleKind::UnixStream
        );
    }

    /// `TcpListener` shares the `TCP_SOCKET` wire type with `TcpStream`, so it
    /// is the one documented lossy case — pinned so the asymmetry stays
    /// deliberate rather than becoming a surprise.
    #[test]
    fn tcp_listener_collapses_to_tcp_stream() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::TcpListener),
            fd_handle_type::TCP_SOCKET
        );
        assert_eq!(
            handle_type_to_kind(fd_handle_type::TCP_SOCKET),
            HandleKind::TcpStream
        );
    }

    /// Unrecognised wire types must not panic — the child rebuilds them as
    /// `File` so a forward-compatible parent can't wedge an older crt0.
    #[test]
    fn unknown_handle_type_falls_back_to_file() {
        use crate::fdtable::HandleKind;
        assert_eq!(handle_type_to_kind(200), HandleKind::File);
        assert_eq!(handle_type_to_kind(u8::MAX), HandleKind::File);
    }

    /// `fd_handle_type::PTY` must equal the kernel's constant, which is the one
    /// number in this table that cannot be checked by a round trip: both sides
    /// of the round trip live here, while the value that matters lives in
    /// `kernel/src/proc/spawn.rs`.
    #[test]
    fn pty_wire_type_matches_the_kernel() {
        assert_eq!(fd_handle_type::PTY, 7);
    }

    /// A master inherited across a spawn must come back a *master*.
    ///
    /// One wire type covers both ends, so the end can only come from the
    /// handle's low bit — `PtyHandle` is `(id << 1) | end`, `Master` being 0.
    /// Rebuilding a master as a slave would be worse than any other confusion
    /// in this table: the emulator's own keystrokes would come back to it.
    #[test]
    fn both_pty_ends_survive_the_round_trip() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::PtyMaster),
            fd_handle_type::PTY
        );
        // (id 3, master) and (id 3, slave).
        assert_eq!(
            handle_type_to_kind_for(fd_handle_type::PTY, 3 << 1),
            HandleKind::PtyMaster
        );
        assert_eq!(
            handle_type_to_kind_for(fd_handle_type::PTY, (3 << 1) | 1),
            HandleKind::PtySlave
        );
    }

    /// A pty *slave* deliberately does not travel as `PTY`: it goes as
    /// `CONSOLE`, which the kernel resolves through `current_tty()`, and
    /// `login_tty` has already made it the child's controlling terminal.
    /// Pinned so the asymmetry stays deliberate rather than looking like an
    /// oversight next to the master's new arm.
    #[test]
    fn pty_slave_still_travels_as_console() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::PtySlave),
            fd_handle_type::CONSOLE
        );
    }

    // -- FileActionSlot --

    #[test]
    fn test_file_action_slot_empty() {
        let slot = FileActionSlot::empty();
        assert_eq!(slot.tag, 0);
        assert_eq!(slot.fd, 0);
        assert_eq!(slot.newfd, 0);
        assert_eq!(slot.oflag, 0);
        assert_eq!(slot.mode, 0);
        assert_eq!(slot.path_len, 0);
    }

    // -- posix_spawn_file_actions_init/destroy --

    #[test]
    fn test_file_actions_init() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        let ret = posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 0);
    }

    #[test]
    fn test_file_actions_init_null() {
        let ret = posix_spawn_file_actions_init(core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_file_actions_destroy() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_destroy(&raw mut acts);
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 0);
    }

    #[test]
    fn test_file_actions_destroy_null() {
        // Destroying null should not crash, returns 0.
        let ret = posix_spawn_file_actions_destroy(core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    // -- posix_spawn_file_actions_addclose --

    #[test]
    fn test_file_actions_addclose() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, 3);
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 1);
        assert_eq!(acts.slots()[0].tag, 1); // Close
        assert_eq!(acts.slots()[0].fd, 3);
    }

    #[test]
    fn test_file_actions_addclose_null() {
        let ret = posix_spawn_file_actions_addclose(core::ptr::null_mut(), 3);
        assert_eq!(ret, errno::EFAULT);
    }

    /// glibc rejects a bad fd with `EBADF`, not `EINVAL`:
    /// `__posix_spawn_file_actions_addclose` (posix/spawn_faction_addclose.c)
    /// opens with `if (!__spawn_valid_fd (fd)) return EBADF;`.
    #[test]
    fn test_file_actions_addclose_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, -1);
        assert_eq!(ret, errno::EBADF);
    }

    /// `__spawn_valid_fd` (posix/spawn_valid_fd.c) is
    /// `fd >= 0 && (maxfd < 0 || fd < maxfd)` — so it also rejects an fd at or
    /// above `sysconf (_SC_OPEN_MAX)`, which a `fd < 0` test misses entirely.
    #[test]
    fn test_file_actions_addclose_fd_at_open_max_is_ebadf() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let maxfd = crate::unistd::sysconf(crate::unistd::_SC_OPEN_MAX);
        assert!(maxfd > 0, "this test needs a finite _SC_OPEN_MAX");
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, maxfd as Fd);
        assert_eq!(ret, errno::EBADF);
    }

    /// And the descriptor verdict outranks the NULL-object one: glibc reaches
    /// `__spawn_valid_fd` before it reads `file_actions->__used`.
    #[test]
    fn test_file_actions_addclose_bad_fd_outranks_a_null_object() {
        let ret = posix_spawn_file_actions_addclose(core::ptr::null_mut(), -1);
        assert_eq!(ret, errno::EBADF);
    }

    /// There is no action cap, as there is none in glibc: the array grows.
    /// It stopped at 16 until 2026-09-25, with `ENOMEM` on the 17th.
    #[test]
    fn test_file_actions_grow_past_the_old_cap() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        for i in 0..200 {
            assert_eq!(
                posix_spawn_file_actions_addclose(&raw mut acts, i % 64),
                0,
                "action {i}"
            );
        }
        assert_eq!(acts.count(), 200);
        assert!(acts.capacity() >= 200);
        // Replayed in the order they were added, across every growth.
        for (i, slot) in acts.slots().iter().enumerate() {
            assert_eq!((slot.tag, slot.fd), (1, (i % 64) as Fd), "slot {i}");
        }
        posix_spawn_file_actions_destroy(&raw mut acts);
        assert_eq!(acts.count(), 0);
    }

    /// Each action's path is its own copy, freed by `destroy`, and an `open`
    /// or `chdir` path is no longer capped at 255 bytes.
    #[test]
    fn test_file_action_paths_are_owned_copies_of_any_length() {
        let before = crate::malloc::live_allocations::count();
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);

        let mut long = std::vec![b'/'];
        long.extend(std::iter::repeat_n(b'd', 1000));
        long.push(0);
        let mut short = b"/tmp/x\0".to_vec();
        assert_eq!(
            posix_spawn_file_actions_addopen(&raw mut acts, 3, long.as_ptr(), 0, 0),
            0
        );
        assert_eq!(
            posix_spawn_file_actions_addchdir_np(&raw mut acts, short.as_ptr()),
            0
        );
        // The caller's buffers can change or go; the object keeps its copies.
        long.fill(b'z');
        short.fill(b'z');
        let slots = acts.slots();
        assert_eq!(slots[0].path_len, 1001);
        assert_eq!(slots[0].path_bytes().first(), Some(&b'/'));
        assert!(slots[0].path_bytes()[1..].iter().all(|&b| b == b'd'));
        assert_eq!(slots[0].path_cstr().and_then(|z| z.last()), Some(&0));
        assert_eq!(slots[1].path_bytes(), b"/tmp/x");

        posix_spawn_file_actions_destroy(&raw mut acts);
        assert_eq!(
            crate::malloc::live_allocations::count(),
            before,
            "destroy frees the slot array and every path"
        );
    }

    // -- posix_spawn_file_actions_adddup2 --

    #[test]
    fn test_file_actions_adddup2() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, 3, 1);
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 1);
        assert_eq!(acts.slots()[0].tag, 2); // Dup2
        assert_eq!(acts.slots()[0].fd, 3);
        assert_eq!(acts.slots()[0].newfd, 1);
    }

    #[test]
    fn test_file_actions_adddup2_null() {
        let ret = posix_spawn_file_actions_adddup2(core::ptr::null_mut(), 3, 1);
        assert_eq!(ret, errno::EFAULT);
    }

    /// `spawn_faction_adddup2.c:32` tests
    /// `!__spawn_valid_fd (fd) || !__spawn_valid_fd (newfd)` and returns
    /// `EBADF` for either.
    #[test]
    fn test_file_actions_adddup2_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, -1, 1);
        assert_eq!(ret, errno::EBADF);
    }

    #[test]
    fn test_file_actions_adddup2_negative_newfd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, 1, -1);
        assert_eq!(ret, errno::EBADF);
    }

    /// `newfd` is checked in the same expression as `fd`, so an out-of-range
    /// `newfd` is `EBADF` too — not just a negative one.
    #[test]
    fn test_file_actions_adddup2_newfd_at_open_max_is_ebadf() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let maxfd = crate::unistd::sysconf(crate::unistd::_SC_OPEN_MAX);
        assert!(maxfd > 0, "this test needs a finite _SC_OPEN_MAX");
        let ret = posix_spawn_file_actions_adddup2(&raw mut acts, 1, maxfd as Fd);
        assert_eq!(ret, errno::EBADF);
    }

    // -- posix_spawn_file_actions_addopen --

    #[test]
    fn test_file_actions_addopen() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let path = b"/dev/null\0";
        let ret = posix_spawn_file_actions_addopen(&raw mut acts, 0, path.as_ptr(), 0, 0o644);
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 1);
        assert_eq!(acts.slots()[0].tag, 3); // Open
        assert_eq!(acts.slots()[0].fd, 0);
        assert_eq!(acts.slots()[0].oflag, 0);
        assert_eq!(acts.slots()[0].mode, 0o644);
        assert_eq!(acts.slots()[0].path_len, 9); // "/dev/null"
    }

    #[test]
    fn test_file_actions_addopen_null_acts() {
        let path = b"/dev/null\0";
        let ret = posix_spawn_file_actions_addopen(core::ptr::null_mut(), 0, path.as_ptr(), 0, 0);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_file_actions_addopen_null_path() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addopen(&raw mut acts, 0, core::ptr::null(), 0, 0);
        assert_eq!(ret, errno::EFAULT);
    }

    /// `spawn_faction_addopen.c` is `if (!__spawn_valid_fd (fd)) return EBADF;`
    /// before `__strdup (path)`.
    #[test]
    fn test_file_actions_addopen_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let path = b"/dev/null\0";
        let ret = posix_spawn_file_actions_addopen(&raw mut acts, -1, path.as_ptr(), 0, 0);
        assert_eq!(ret, errno::EBADF);
    }

    /// Because that check precedes the `__strdup`, a bad fd outranks a NULL
    /// path — glibc never reaches the string at all.
    #[test]
    fn test_file_actions_addopen_bad_fd_outranks_a_null_path() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addopen(&raw mut acts, -1, core::ptr::null(), 0, 0);
        assert_eq!(ret, errno::EBADF);
    }

    // -- posix_spawn_file_actions ordering --

    #[test]
    fn test_file_actions_ordering() {
        // POSIX requires actions to be applied in order.
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);

        posix_spawn_file_actions_addclose(&raw mut acts, 3);
        posix_spawn_file_actions_adddup2(&raw mut acts, 4, 1);
        posix_spawn_file_actions_addclose(&raw mut acts, 5);

        assert_eq!(acts.count(), 3);
        // Verify order preserved.
        assert_eq!(acts.slots()[0].tag, 1); // Close(3)
        assert_eq!(acts.slots()[0].fd, 3);
        assert_eq!(acts.slots()[1].tag, 2); // Dup2(4, 1)
        assert_eq!(acts.slots()[1].fd, 4);
        assert_eq!(acts.slots()[1].newfd, 1);
        assert_eq!(acts.slots()[2].tag, 1); // Close(5)
        assert_eq!(acts.slots()[2].fd, 5);
    }

    // -- posix_spawnattr_init/destroy --

    #[test]
    fn test_spawnattr_init() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        let ret = posix_spawnattr_init(&raw mut attr);
        assert_eq!(ret, 0);
        assert_eq!(attr.flags, 0);
        assert_eq!(attr.pgroup, 0);
    }

    #[test]
    fn test_spawnattr_init_null() {
        let ret = posix_spawnattr_init(core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_destroy() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_destroy(&raw mut attr);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_spawnattr_destroy_null() {
        let ret = posix_spawnattr_destroy(core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    // -- posix_spawnattr_setflags/getflags --

    #[test]
    fn test_spawnattr_setflags() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_setflags(&raw mut attr, 0x02); // POSIX_SPAWN_SETPGROUP
        assert_eq!(ret, 0);
        assert_eq!(attr.flags, 0x02);
    }

    #[test]
    fn test_spawnattr_setflags_null() {
        let ret = posix_spawnattr_setflags(core::ptr::null_mut(), 0);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_getflags() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        posix_spawnattr_setflags(&raw mut attr, 0x05);
        let mut flags: i16 = 0;
        let ret = posix_spawnattr_getflags(&raw const attr, &raw mut flags);
        assert_eq!(ret, 0);
        assert_eq!(flags, 0x05);
    }

    #[test]
    fn test_spawnattr_getflags_null_attr() {
        let mut flags: i16 = 0;
        let ret = posix_spawnattr_getflags(core::ptr::null(), &raw mut flags);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_getflags_null_out() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_getflags(&raw const attr, core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    // -- posix_spawnattr_setpgroup/getpgroup --

    #[test]
    fn test_spawnattr_setpgroup() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_setpgroup(&raw mut attr, 42);
        assert_eq!(ret, 0);
        assert_eq!(attr.pgroup, 42);
    }

    #[test]
    fn test_spawnattr_setpgroup_null() {
        let ret = posix_spawnattr_setpgroup(core::ptr::null_mut(), 42);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_getpgroup() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        posix_spawnattr_setpgroup(&raw mut attr, 99);
        let mut pg: PidT = 0;
        let ret = posix_spawnattr_getpgroup(&raw const attr, &raw mut pg);
        assert_eq!(ret, 0);
        assert_eq!(pg, 99);
    }

    #[test]
    fn test_spawnattr_getpgroup_null_attr() {
        let mut pg: PidT = 0;
        let ret = posix_spawnattr_getpgroup(core::ptr::null(), &raw mut pg);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn test_spawnattr_getpgroup_null_out() {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        let ret = posix_spawnattr_getpgroup(&raw const attr, core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    // -- Spawn flag constants --

    #[test]
    fn test_spawn_flag_constants() {
        // Verify flag values match POSIX.
        assert_eq!(POSIX_SPAWN_RESETIDS, 0x01);
        assert_eq!(POSIX_SPAWN_SETPGROUP, 0x02);
        assert_eq!(POSIX_SPAWN_SETSIGDEF, 0x04);
        assert_eq!(POSIX_SPAWN_SETSIGMASK, 0x08);
    }

    #[test]
    fn test_spawn_flags_no_overlap() {
        let all = POSIX_SPAWN_RESETIDS
            | POSIX_SPAWN_SETPGROUP
            | POSIX_SPAWN_SETSIGDEF
            | POSIX_SPAWN_SETSIGMASK;
        // Each flag should be a distinct bit.
        assert_eq!(all, 0x0F);
    }

    // -- SpawnExArgs struct layout --

    #[test]
    fn test_spawn_ex_args_size() {
        // SpawnExArgs has 12 u64 fields = 96 bytes.
        assert_eq!(core::mem::size_of::<SpawnExArgs>(), 96);
    }

    #[test]
    fn test_spawn_ex_args_alignment() {
        // Must be u64-aligned for proper ABI.
        assert_eq!(core::mem::align_of::<SpawnExArgs>(), 8);
    }

    #[test]
    fn test_spawn_ex_args_field_layout() {
        // Verify fields are at the expected offsets (all u64, sequential).
        let args = SpawnExArgs {
            elf_ptr: 0x1111_1111_1111_1111,
            elf_len: 0x2222_2222_2222_2222,
            name_ptr: 0x3333_3333_3333_3333,
            name_len: 0x4444_4444_4444_4444,
            fd_map_ptr: 0x5555_5555_5555_5555,
            fd_map_count: 6,
            argv_ptr: 0x7777_7777_7777_7777,
            argv_len: 128,
            argc: 3,
            envp_ptr: 0xAAAA_AAAA_AAAA_AAAA,
            envp_len: 64,
            envc: 2,
        };
        assert_eq!(args.elf_ptr, 0x1111_1111_1111_1111);
        assert_eq!(args.elf_len, 0x2222_2222_2222_2222);
        assert_eq!(args.name_ptr, 0x3333_3333_3333_3333);
        assert_eq!(args.name_len, 0x4444_4444_4444_4444);
        assert_eq!(args.fd_map_ptr, 0x5555_5555_5555_5555);
        assert_eq!(args.fd_map_count, 6);
        assert_eq!(args.argv_ptr, 0x7777_7777_7777_7777);
        assert_eq!(args.argv_len, 128);
        assert_eq!(args.argc, 3);
        assert_eq!(args.envp_ptr, 0xAAAA_AAAA_AAAA_AAAA);
        assert_eq!(args.envp_len, 64);
        assert_eq!(args.envc, 2);
    }

    // -- SpawnArgsHeader struct layout --

    #[test]
    fn test_spawn_args_header_size() {
        // SpawnArgsHeader has 4 u32 fields = 16 bytes.
        assert_eq!(core::mem::size_of::<SpawnArgsHeader>(), 16);
    }

    #[test]
    fn test_spawn_args_header_alignment() {
        assert_eq!(core::mem::align_of::<SpawnArgsHeader>(), 4);
    }

    #[test]
    fn test_spawn_args_header_field_values() {
        let header = SpawnArgsHeader {
            argc: 5,
            envc: 3,
            argv_data_len: 100,
            envp_data_len: 50,
        };
        assert_eq!(header.argc, 5);
        assert_eq!(header.envc, 3);
        assert_eq!(header.argv_data_len, 100);
        assert_eq!(header.envp_data_len, 50);
    }

    // -- count_cstring_array --

    #[test]
    fn test_count_cstring_array_null() {
        assert_eq!(count_cstring_array(core::ptr::null()), 0);
    }

    #[test]
    fn test_count_cstring_array_empty() {
        // A null-terminated array with just the NULL terminator.
        let ptrs: [*const u8; 1] = [core::ptr::null()];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 0);
    }

    #[test]
    fn test_count_cstring_array_one() {
        let s = b"hello\0";
        let ptrs: [*const u8; 2] = [s.as_ptr(), core::ptr::null()];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 1);
    }

    #[test]
    fn test_count_cstring_array_three() {
        let s1 = b"one\0";
        let s2 = b"two\0";
        let s3 = b"three\0";
        let ptrs: [*const u8; 4] = [s1.as_ptr(), s2.as_ptr(), s3.as_ptr(), core::ptr::null()];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 3);
    }

    // -- pack_cstring_array (existing, but add a round-trip test with count) --

    /// A list that does not fit is REFUSED, not packed short.
    ///
    /// It used to be packed short, and the loss was invisible from both
    /// sides: `count_cstring_array` counts the whole list regardless of any
    /// buffer, so the kernel was handed a truncated buffer AND the full
    /// `argc`. A child started with fewer arguments than its parent passed
    /// and nothing said so. `E2BIG` is what POSIX spells for this.
    #[test]
    fn an_oversized_list_is_refused_not_truncated() {
        let s = b"0123456789\0";
        let ptrs: [*const u8; 5] = [
            s.as_ptr(),
            s.as_ptr(),
            s.as_ptr(),
            s.as_ptr(),
            core::ptr::null(),
        ];
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 4);
        // Room for two of the four: refused outright.
        let mut buf = [0u8; 25];
        assert_eq!(
            pack_cstring_array(ptrs.as_ptr(), &mut buf),
            None,
            "a list that does not fit must be refused, not packed short"
        );
        // The controls. Exactly enough room still packs, so the check is not
        // simply refusing everything...
        let mut exact = [0u8; 44];
        assert_eq!(pack_cstring_array(ptrs.as_ptr(), &mut exact), Some(44));
        // ...and one byte short of exactly enough does not.
        let mut short = [0u8; 43];
        assert_eq!(pack_cstring_array(ptrs.as_ptr(), &mut short), None);
    }

    #[test]
    fn test_pack_and_count_consistency() {
        let s1 = b"alpha\0";
        let s2 = b"beta\0";
        let ptrs: [*const u8; 3] = [s1.as_ptr(), s2.as_ptr(), core::ptr::null()];

        // Count should match.
        assert_eq!(count_cstring_array(ptrs.as_ptr()), 2);

        // Pack and verify format.
        let mut buf = [0u8; 256];
        let packed_len = pack_cstring_array(ptrs.as_ptr(), &mut buf);

        // "alpha\0beta\0" = 6 + 5 = 11 bytes.
        assert_eq!(packed_len, Some(11));
        assert_eq!(&buf[..6], b"alpha\0");
        assert_eq!(&buf[6..11], b"beta\0");
    }

    // -- FdMapEntry ABI --

    #[test]
    fn test_fd_map_entry_size() {
        assert_eq!(core::mem::size_of::<FdMapEntry>(), 16);
    }

    #[test]
    fn test_fd_map_entry_align() {
        assert_eq!(core::mem::align_of::<FdMapEntry>(), 8);
    }

    #[test]
    fn test_fd_map_entry_field_offsets() {
        let entry = FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        };
        let base = &entry as *const _ as usize;
        assert_eq!(&entry.fd as *const _ as usize - base, 0);
        assert_eq!(&entry.handle_type as *const _ as usize - base, 4);
        assert_eq!(&entry.handle as *const _ as usize - base, 8);
    }

    // -- fd_handle_type constants --

    #[test]
    fn test_fd_handle_type_values() {
        assert_eq!(fd_handle_type::FILE, 0);
        assert_eq!(fd_handle_type::PIPE, 1);
        assert_eq!(fd_handle_type::TCP_SOCKET, 2);
        assert_eq!(fd_handle_type::UDP_SOCKET, 3);
        assert_eq!(fd_handle_type::CONSOLE, 4);
        assert_eq!(fd_handle_type::EVENTFD, 5);
        // The last two were added after this test was written and were not
        // added to it, which is how a wire constant drifts from the kernel's
        // unnoticed. Every value in the module belongs here.
        assert_eq!(fd_handle_type::STREAM_SOCKET, 6);
        assert_eq!(fd_handle_type::PTY, 7);
    }

    #[test]
    fn test_fd_handle_type_distinct() {
        let vals = [
            fd_handle_type::FILE,
            fd_handle_type::PIPE,
            fd_handle_type::TCP_SOCKET,
            fd_handle_type::UDP_SOCKET,
            fd_handle_type::CONSOLE,
            fd_handle_type::EVENTFD,
            fd_handle_type::STREAM_SOCKET,
            fd_handle_type::PTY,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "types {} and {} collide", i, j);
            }
        }
    }

    // -- kind_to_handle_type --

    #[test]
    fn test_kind_to_handle_type_file() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::File), fd_handle_type::FILE);
    }

    #[test]
    fn test_kind_to_handle_type_pipe() {
        use crate::fdtable::HandleKind;
        assert_eq!(kind_to_handle_type(HandleKind::Pipe), fd_handle_type::PIPE);
    }

    #[test]
    fn test_kind_to_handle_type_console() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::Console),
            fd_handle_type::CONSOLE
        );
    }

    #[test]
    fn test_kind_to_handle_type_tcp() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::TcpStream),
            fd_handle_type::TCP_SOCKET
        );
        assert_eq!(
            kind_to_handle_type(HandleKind::TcpListener),
            fd_handle_type::TCP_SOCKET
        );
    }

    #[test]
    fn test_kind_to_handle_type_udp() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::UdpSocket),
            fd_handle_type::UDP_SOCKET
        );
    }

    #[test]
    fn test_kind_to_handle_type_eventfd() {
        use crate::fdtable::HandleKind;
        assert_eq!(
            kind_to_handle_type(HandleKind::Eventfd),
            fd_handle_type::EVENTFD
        );
    }

    // -- build_fd_map --

    /// Ensure fds 0/1/2 are Console handles.
    ///
    /// Other tests may close or overwrite them; this restores the
    /// expected state before each build_fd_map test.
    fn ensure_std_fds() {
        use crate::fdtable::{HandleKind, install_fd};
        let _ = install_fd(0, HandleKind::Console, 0);
        let _ = install_fd(1, HandleKind::Console, 1);
        let _ = install_fd(2, HandleKind::Console, 2);
    }

    #[test]
    fn test_build_fd_map_no_actions() {
        ensure_std_fds();
        // With no file_actions (null), the fd_map should contain
        // the parent's inheritable fds.  In the test environment,
        // fds 0/1/2 are pre-initialized as Console handles.
        let mut out = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(core::ptr::null(), &mut out, &mut opened)
            .expect("no action here can fail");

        // Should have at least fds 0, 1, 2 (Console).
        assert!(count >= 3, "expected at least 3 fds, got {}", count);

        // Verify first three are Console type.
        assert_eq!(out[0].fd, 0);
        assert_eq!(out[0].handle_type, fd_handle_type::CONSOLE);
        assert_eq!(out[1].fd, 1);
        assert_eq!(out[1].handle_type, fd_handle_type::CONSOLE);
        assert_eq!(out[2].fd, 2);
        assert_eq!(out[2].handle_type, fd_handle_type::CONSOLE);
    }

    /// The regression this whole change exists for: a pty master open in the
    /// parent must appear in the child's `fd_map`.
    ///
    /// It used to be filtered out silently, so a `script -f`-shaped program —
    /// where the *child* is the master holder — got a child with no master and
    /// no error to explain it.
    #[test]
    fn a_pty_master_is_no_longer_dropped_from_the_fd_map() {
        use crate::fdtable::{HandleKind, install_fd};
        ensure_std_fds();
        // (id 9, master): low bit clear.
        let handle: u64 = 9 << 1;
        let fd = 7;
        let _ = install_fd(fd, HandleKind::PtyMaster, handle);

        let mut out = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(core::ptr::null(), &mut out, &mut opened)
            .expect("no action here can fail");

        let found = out
            .get(..count)
            .unwrap_or(&[])
            .iter()
            .find(|e| e.fd == fd)
            .copied();
        let _ = crate::fdtable::close_fd(fd);

        let entry = found.expect("the pty master was dropped from the fd_map");
        assert_eq!(
            entry.handle_type,
            fd_handle_type::PTY,
            "a master must cross as PTY, not as FILE — the file layer would \
             misread the handle number"
        );
        assert_eq!(entry.handle, handle);
        // And the child must rebuild it as a master, not a slave.
        assert_eq!(
            handle_type_to_kind_for(entry.handle_type, entry.handle),
            HandleKind::PtyMaster
        );
    }

    #[test]
    fn test_build_fd_map_with_close() {
        ensure_std_fds();
        // Create file_actions that close fd 1 (stdout).
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_addclose(&raw mut acts, 1);

        let mut out = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count =
            build_fd_map(&raw const acts, &mut out, &mut opened).expect("no action here can fail");

        // fd 1 should be gone.  We should have fd 0 and fd 2.
        let has_fd1 = out[..count].iter().any(|e| e.fd == 1);
        assert!(!has_fd1, "fd 1 should have been closed");

        let has_fd0 = out[..count].iter().any(|e| e.fd == 0);
        let has_fd2 = out[..count].iter().any(|e| e.fd == 2);
        assert!(has_fd0, "fd 0 should still exist");
        assert!(has_fd2, "fd 2 should still exist");
    }

    #[test]
    fn test_build_fd_map_with_dup2() {
        ensure_std_fds();
        // Create file_actions that dup2(2, 1) — redirect stdout to stderr.
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_adddup2(&raw mut acts, 2, 1);

        let mut out = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count =
            build_fd_map(&raw const acts, &mut out, &mut opened).expect("no action here can fail");

        // fd 1 should now have the same handle as fd 2.
        let fd1 = out[..count].iter().find(|e| e.fd == 1);
        let fd2 = out[..count].iter().find(|e| e.fd == 2);
        assert!(fd1.is_some(), "fd 1 should exist");
        assert!(fd2.is_some(), "fd 2 should exist");
        assert_eq!(
            fd1.unwrap().handle,
            fd2.unwrap().handle,
            "fd 1 and fd 2 should share the same handle after dup2",
        );
    }

    #[test]
    fn test_build_fd_map_close_then_dup2() {
        ensure_std_fds();
        // Close fd 1, then dup2(2, 1) — common shell pattern for
        // redirecting stdout to a pipe.
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_addclose(&raw mut acts, 1);
        posix_spawn_file_actions_adddup2(&raw mut acts, 2, 1);

        let mut out = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count =
            build_fd_map(&raw const acts, &mut out, &mut opened).expect("no action here can fail");

        // fd 1 should exist (recreated by dup2) with fd 2's handle.
        let fd1 = out[..count].iter().find(|e| e.fd == 1);
        let fd2 = out[..count].iter().find(|e| e.fd == 2);
        assert!(fd1.is_some(), "fd 1 should be recreated by dup2");
        assert!(fd2.is_some(), "fd 2 should still exist");
        assert_eq!(fd1.unwrap().handle, fd2.unwrap().handle);
    }

    #[test]
    fn test_build_fd_map_close_all_standard() {
        // Close all three standard fds.
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        posix_spawn_file_actions_addclose(&raw mut acts, 0);
        posix_spawn_file_actions_addclose(&raw mut acts, 1);
        posix_spawn_file_actions_addclose(&raw mut acts, 2);

        let mut out = [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP];
        let mut opened = OpenedHandles::new();
        let count =
            build_fd_map(&raw const acts, &mut out, &mut opened).expect("no action here can fail");

        // No standard fds should remain.
        let has_0_1_2 = out[..count].iter().any(|e| e.fd <= 2);
        assert!(!has_0_1_2, "all standard fds should be closed");
    }

    #[test]
    fn test_max_fd_map_constant() {
        // One slot per fd-table entry, so no open descriptor is ever out of
        // the child's reach. It was 32, which dropped fds 32..256 silently.
        assert_eq!(MAX_FD_MAP, crate::fdtable::MAX_FDS);
    }

    // -----------------------------------------------------------------------
    // fexecve
    // -----------------------------------------------------------------------

    #[test]
    fn test_fexecve_negative_fd() {
        crate::errno::set_errno(0);
        let ret = fexecve(-1, core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_fexecve_no_path_fd() {
        // fd 999 has no path stored → ENOENT.
        crate::errno::set_errno(0);
        let ret = fexecve(999, core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // -----------------------------------------------------------------------
    // posix_spawn_file_actions_addchdir_np
    // -----------------------------------------------------------------------

    #[test]
    fn test_addchdir_np_null_acts() {
        let ret = posix_spawn_file_actions_addchdir_np(core::ptr::null_mut(), b"/tmp\0".as_ptr());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_addchdir_np_null_path() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addchdir_np(&raw mut acts, core::ptr::null());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_addchdir_np_success() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addchdir_np(&raw mut acts, b"/tmp\0".as_ptr());
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 1);
        assert_eq!(acts.slots()[0].tag, 4, "chdir action tag should be 4");
    }

    /// A `chdir` after sixteen other actions is recorded like any other: the
    /// array grows (it was full, and refused with `ENOMEM`, until 2026-09-25).
    #[test]
    fn test_addchdir_np_after_sixteen_actions() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        for _ in 0..16 {
            assert_eq!(posix_spawn_file_actions_addclose(&raw mut acts, 0), 0);
        }
        let ret = posix_spawn_file_actions_addchdir_np(&raw mut acts, b"/tmp\0".as_ptr());
        assert_eq!(ret, 0);
        assert_eq!(acts.slots()[16].tag, 4);
        assert_eq!(acts.slots()[16].path_bytes(), b"/tmp");
        posix_spawn_file_actions_destroy(&raw mut acts);
    }

    // -----------------------------------------------------------------------
    // execvpe — exec with PATH search + custom environment
    // -----------------------------------------------------------------------

    #[test]
    fn test_execvpe_null_file() {
        crate::errno::set_errno(0);
        let ret = execvpe(core::ptr::null(), core::ptr::null(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_execvpe_nonexistent_path_search() {
        // A filename without '/' that doesn't exist in PATH.
        // On our OS this returns ENOENT; on the test host, search_path
        // may produce unpredictable results via SYS_FS_STAT.
        crate::errno::set_errno(0);
        let ret = execvpe(
            b"nonexistent_binary_xyz_12345\0".as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
        );
        // Either ENOENT (not found in PATH) or the exec itself fails.
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_execvpe_with_slash_delegates_to_execve() {
        // A filename with '/' is used directly, not searched in PATH.
        // Syscall result is unpredictable on test host.
        let ret = execvpe(
            b"/nonexistent/binary\0".as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
        );
        // Should return -1 (exec replaces process on success, so any
        // return means failure).
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // posix_spawn_file_actions_addclosefrom_np
    // -----------------------------------------------------------------------

    #[test]
    fn test_addclosefrom_np_null_acts() {
        let ret = posix_spawn_file_actions_addclosefrom_np(core::ptr::null_mut(), 3);
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_addclosefrom_np_negative_fd() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclosefrom_np(&raw mut acts, -1);
        assert_eq!(ret, crate::errno::EBADF);
    }

    #[test]
    fn test_addclosefrom_np_success() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        let ret = posix_spawn_file_actions_addclosefrom_np(&raw mut acts, 3);
        assert_eq!(ret, 0);
        assert_eq!(acts.count(), 1);
        assert_eq!(acts.slots()[0].tag, 5, "closefrom action tag should be 5");
        assert_eq!(acts.slots()[0].fd, 3);
    }

    #[test]
    fn test_addclosefrom_np_after_sixteen_actions() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        for _ in 0..16 {
            assert_eq!(posix_spawn_file_actions_addclose(&raw mut acts, 0), 0);
        }
        assert_eq!(
            posix_spawn_file_actions_addclosefrom_np(&raw mut acts, 3),
            0
        );
        assert_eq!((acts.slots()[16].tag, acts.slots()[16].fd), (5, 3));
        posix_spawn_file_actions_destroy(&raw mut acts);
    }

    // -----------------------------------------------------------------------
    // Phase 81 — posix_spawnattr_setflags flag-mask validation
    //
    // POSIX:
    //   If the value of the attribute being set is not valid,
    //   posix_spawnattr_setflags() shall return [EINVAL].
    //
    // glibc applies a mask check (`flags & ~__POSIX_SPAWN_MASK`) and
    // returns EINVAL on any unrecognised bit.  These tests pin that
    // behaviour for our implementation.
    // -----------------------------------------------------------------------

    fn fresh_attr() -> PosixSpawnattrT {
        let mut attr = unsafe { core::mem::zeroed::<PosixSpawnattrT>() };
        posix_spawnattr_init(&raw mut attr);
        attr
    }

    // ---- (a) Mask invariants --------------------------------------------

    #[test]
    fn test_posix_spawn_valid_flags_equals_union() {
        assert_eq!(
            POSIX_SPAWN_VALID_FLAGS,
            POSIX_SPAWN_RESETIDS
                | POSIX_SPAWN_SETPGROUP
                | POSIX_SPAWN_SETSIGDEF
                | POSIX_SPAWN_SETSIGMASK
                | POSIX_SPAWN_SETSCHEDPARAM
                | POSIX_SPAWN_SETSCHEDULER
                | POSIX_SPAWN_USEVFORK
                | POSIX_SPAWN_SETSID
        );
    }

    #[test]
    fn test_posix_spawn_valid_flags_value() {
        // Every flag from RESETIDS (0x01) through SETSID (0x80) =
        // 0xFF.  This catches accidental gaps in the constants.
        assert_eq!(POSIX_SPAWN_VALID_FLAGS, 0xFF);
    }

    #[test]
    fn test_posix_spawn_flags_are_distinct_bits() {
        for f in [
            POSIX_SPAWN_RESETIDS,
            POSIX_SPAWN_SETPGROUP,
            POSIX_SPAWN_SETSIGDEF,
            POSIX_SPAWN_SETSIGMASK,
            POSIX_SPAWN_SETSCHEDPARAM,
            POSIX_SPAWN_SETSCHEDULER,
            POSIX_SPAWN_USEVFORK,
            POSIX_SPAWN_SETSID,
        ] {
            assert_eq!(f.count_ones(), 1, "flag {f:#x} must be a single bit");
        }
    }

    #[test]
    fn test_new_flag_constants_have_expected_values() {
        assert_eq!(POSIX_SPAWN_SETSCHEDPARAM, 0x10);
        assert_eq!(POSIX_SPAWN_SETSCHEDULER, 0x20);
        assert_eq!(POSIX_SPAWN_USEVFORK, 0x40);
        assert_eq!(POSIX_SPAWN_SETSID, 0x80);
    }

    // ---- (b) Rejection of unknown bits ----------------------------------

    #[test]
    fn test_setflags_rejects_single_unknown_high_bit() {
        let mut attr = fresh_attr();
        // i16::MIN = -0x8000 — sets the sign bit only; outside mask.
        let ret = posix_spawnattr_setflags(&raw mut attr, i16::MIN);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejects_bit_just_above_setsid() {
        // First bit outside the mask = 0x100.
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, 0x100);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejects_unknown_bit_combined_with_valid() {
        // POSIX_SPAWN_SETSID | 0x100 — partially valid, must still fail.
        let mut attr = fresh_attr();
        let bad = POSIX_SPAWN_SETSID | 0x100;
        let ret = posix_spawnattr_setflags(&raw mut attr, bad);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejects_negative_one() {
        // -1 in i16 = 0xFFFF — has every high bit set, must fail.
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, -1);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn test_setflags_rejection_does_not_mutate_attr() {
        // Critical invariant: a failed setflags must leave the previous
        // flag word untouched, otherwise callers can be left with a
        // half-configured attr object.
        let mut attr = fresh_attr();
        let ok = posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_RESETIDS);
        assert_eq!(ok, 0);
        let bad = posix_spawnattr_setflags(&raw mut attr, 0x4000);
        assert_eq!(bad, errno::EINVAL);
        // attr.flags should still hold the previous value.
        let mut got: i16 = 0;
        let r = posix_spawnattr_getflags(&raw const attr, &raw mut got);
        assert_eq!(r, 0);
        assert_eq!(got, POSIX_SPAWN_RESETIDS);
    }

    // ---- (c) Acceptance of every valid bit ------------------------------

    #[test]
    fn test_setflags_accepts_each_valid_bit_individually() {
        for f in [
            POSIX_SPAWN_RESETIDS,
            POSIX_SPAWN_SETPGROUP,
            POSIX_SPAWN_SETSIGDEF,
            POSIX_SPAWN_SETSIGMASK,
            POSIX_SPAWN_SETSCHEDPARAM,
            POSIX_SPAWN_SETSCHEDULER,
            POSIX_SPAWN_USEVFORK,
            POSIX_SPAWN_SETSID,
        ] {
            let mut attr = fresh_attr();
            let ret = posix_spawnattr_setflags(&raw mut attr, f);
            assert_eq!(ret, 0, "flag {f:#x} should be accepted");
            let mut got: i16 = 0;
            assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
            assert_eq!(got, f);
        }
    }

    #[test]
    fn test_setflags_accepts_full_mask() {
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_VALID_FLAGS);
        assert_eq!(ret, 0);
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, POSIX_SPAWN_VALID_FLAGS);
    }

    #[test]
    fn test_setflags_accepts_zero() {
        // Zero (no flags) must succeed — it's the post-init default.
        let mut attr = fresh_attr();
        let ret = posix_spawnattr_setflags(&raw mut attr, 0);
        assert_eq!(ret, 0);
    }

    // ---- (d) Validation order -------------------------------------------

    /// Both errors apply (NULL attr AND a bad flag bit), and the *flag* wins.
    ///
    /// `__posix_spawnattr_setflags` (posix/spawnattr_setflags.c) is two
    /// statements — `if (flags & ~ALL_FLAGS) return EINVAL;` then
    /// `attr->__flags = flags;` — with no NULL check whatever, so the flag
    /// word is decided while the pointer is still untouched.
    ///
    /// This test previously asserted the opposite, under the name
    /// `test_setflags_null_attr_precedes_flag_check`, on the reasoning that
    /// `EFAULT` was "more informative". That reasoning was invented rather
    /// than read off upstream. See design-decisions.md §303.
    #[test]
    fn test_setflags_bad_flag_precedes_the_null_attr_check() {
        let ret = posix_spawnattr_setflags(core::ptr::null_mut(), 0x4000);
        assert_eq!(ret, errno::EINVAL);
    }

    /// With a valid flag word the NULL pointer is still reached and reported.
    #[test]
    fn test_setflags_null_attr_with_a_valid_flag_is_efault() {
        let ret = posix_spawnattr_setflags(core::ptr::null_mut(), 0);
        assert_eq!(ret, errno::EFAULT);
    }

    // ---- (e) Workflow / buggy-caller patterns ---------------------------

    #[test]
    fn test_setflags_then_getflags_roundtrip_full_mask() {
        let mut attr = fresh_attr();
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_VALID_FLAGS),
            0,
        );
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, POSIX_SPAWN_VALID_FLAGS);
    }

    #[test]
    fn test_setflags_replace_overwrites_prior_flags() {
        let mut attr = fresh_attr();
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETSID),
            0,
        );
        // Replace with a smaller value.  setflags() is whole-word, not
        // bitwise-OR, so the second call must overwrite, not merge.
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_USEVFORK),
            0,
        );
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, POSIX_SPAWN_USEVFORK);
    }

    #[test]
    fn test_setflags_init_clears_flags() {
        // After a successful setflags, a second init() must reset the
        // attr to zero flags.  Otherwise reuse of a stale attr object
        // would silently carry old flags into a fresh spawn.
        let mut attr = fresh_attr();
        assert_eq!(
            posix_spawnattr_setflags(&raw mut attr, POSIX_SPAWN_VALID_FLAGS),
            0,
        );
        posix_spawnattr_init(&raw mut attr);
        let mut got: i16 = 0;
        assert_eq!(posix_spawnattr_getflags(&raw const attr, &raw mut got), 0);
        assert_eq!(got, 0);
    }

    // -- SYS_PROCESS_SPAWN_EX2 mirror --
    //
    // These pin the half of the ABI this side owns: the layout we send and
    // the shape of the request we build.  What the *kernel* does with each
    // malformed shape is asserted from ring 3 by `spawn::self_test_spawn_ex2_abi`
    // (see `requests/a-b-spawn-ex2-capability-subset.md`); duplicating that here
    // would be asserting our own guess at another lane's behaviour.  What is
    // testable here — and is the part that actually breaks — is that we never
    // *send* one of those malformed shapes.

    /// Byte offset of `$f` within a zeroed `$t`, without constructing a
    /// reference to the field.
    macro_rules! offset_of_field {
        ($t:ty, $v:expr, $f:ident) => {{
            let v: &$t = &$v;
            ((&raw const v.$f).cast::<u8>() as usize) - ((&raw const *v).cast::<u8>() as usize)
        }};
    }

    fn zero_ex2() -> SpawnEx2Args {
        spawn_ex2_args(None)
    }

    fn zero_ex() -> SpawnExArgs {
        SpawnExArgs {
            elf_ptr: 0,
            elf_len: 0,
            name_ptr: 0,
            name_len: 0,
            fd_map_ptr: 0,
            fd_map_count: 0,
            argv_ptr: 0,
            argv_len: 0,
            argc: 0,
            envp_ptr: 0,
            envp_len: 0,
            envc: 0,
        }
    }

    /// 144 bytes of eighteen `u64`s, no padding.
    ///
    /// The `const` block beside the declaration already fails the build on a
    /// size change; this states the *field* offsets, which a reordering could
    /// break while leaving the size right.  A swapped `cap_ptr`/`cap_count`
    /// would hand the kernel a count where it expects a pointer — an
    /// `InvalidAddress` at best and a read of unrelated memory at worst.
    #[test]
    fn ex2_layout_is_eighteen_u64s() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<SpawnEx2Args>(), 144);
        assert_eq!(align_of::<SpawnEx2Args>(), 8);
        let a = zero_ex2();
        for (i, off) in [
            offset_of_field!(SpawnEx2Args, a, struct_size),
            offset_of_field!(SpawnEx2Args, a, elf_ptr),
            offset_of_field!(SpawnEx2Args, a, elf_len),
            offset_of_field!(SpawnEx2Args, a, name_ptr),
            offset_of_field!(SpawnEx2Args, a, name_len),
            offset_of_field!(SpawnEx2Args, a, fd_map_ptr),
            offset_of_field!(SpawnEx2Args, a, fd_map_count),
            offset_of_field!(SpawnEx2Args, a, argv_ptr),
            offset_of_field!(SpawnEx2Args, a, argv_len),
            offset_of_field!(SpawnEx2Args, a, argc),
            offset_of_field!(SpawnEx2Args, a, envp_ptr),
            offset_of_field!(SpawnEx2Args, a, envp_len),
            offset_of_field!(SpawnEx2Args, a, envc),
            offset_of_field!(SpawnEx2Args, a, cap_mode),
            offset_of_field!(SpawnEx2Args, a, cap_ptr),
            offset_of_field!(SpawnEx2Args, a, cap_count),
            offset_of_field!(SpawnEx2Args, a, cwd_ptr),
            offset_of_field!(SpawnEx2Args, a, cwd_len),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(off, i * 8, "field {i} of SpawnEx2Args");
        }
    }

    /// Everything through `envc` sits exactly 8 bytes later than in version 1.
    ///
    /// This is the claim that makes `struct_size` work at all: a short struct
    /// is "version 1 plus a size field", so the kernel can zero-fill the tail
    /// and get version-1 behaviour.  If the prefix ever stopped matching, the
    /// size field would still be accepted and the *contents* would be wrong —
    /// which is the failure that produces a spawn of the wrong binary rather
    /// than an error.
    #[test]
    fn ex2_prefix_matches_ex_shifted_by_the_size_field() {
        let a = zero_ex2();
        let b = zero_ex();
        macro_rules! same {
            ($($f:ident),+ $(,)?) => {$(
                assert_eq!(
                    offset_of_field!(SpawnEx2Args, a, $f),
                    offset_of_field!(SpawnExArgs, b, $f) + 8,
                    concat!("field ", stringify!($f), " must be SpawnExArgs' + 8"),
                );
            )+};
        }
        same!(
            elf_ptr,
            elf_len,
            name_ptr,
            name_len,
            fd_map_ptr,
            fd_map_count,
            argv_ptr,
            argv_len,
            argc,
            envp_ptr,
            envp_len,
            envc,
        );
    }

    /// The `struct_size` we send must land in the kernel's accepted range.
    ///
    /// Lane A's table rejects a size below 104, not a multiple of 8, or above
    /// 4096.  Growing this struct is legal; growing it to a size the kernel
    /// rejects outright is not, and the difference is invisible until a spawn
    /// fails with `InvalidArgument` naming nothing.
    #[test]
    fn ex2_struct_size_is_one_the_kernel_accepts() {
        let n = size_of::<SpawnEx2Args>();
        assert_eq!(zero_ex2().struct_size as usize, n, "we send our own size");
        assert!(n >= SPAWN_EX2_MIN_SIZE as usize, "{n} < min");
        assert_eq!(n % 8, 0, "{n} is not a multiple of 8");
        assert!(n <= 4096, "{n} > the kernel's 4096-byte ceiling");
        assert_eq!(SPAWN_EX2_MIN_SIZE, 104);
    }

    /// `None` is the untouched case and must be *entirely* zero apart from the
    /// size, so that a caller who fills in only the version-1 fields gets
    /// version-1 behaviour with no capability policy attached by accident.
    #[test]
    fn ex2_args_none_is_inherit_all_and_otherwise_zero() {
        let a = spawn_ex2_args(None);
        assert_eq!(a.cap_mode, SPAWN_CAP_MODE_INHERIT_ALL);
        assert_eq!(a.cap_mode, 0);
        assert_eq!(a.cap_ptr, 0);
        assert_eq!(a.cap_count, 0);
        // No directory: the child starts in its parent's.
        assert_eq!((a.cwd_ptr, a.cwd_len), (0, 0));
        // Every version-1 field left for the caller.
        assert_eq!(
            (
                a.elf_ptr,
                a.elf_len,
                a.name_ptr,
                a.name_len,
                a.fd_map_ptr,
                a.fd_map_count
            ),
            (0, 0, 0, 0, 0, 0)
        );
        assert_eq!(
            (
                a.argv_ptr, a.argv_len, a.argc, a.envp_ptr, a.envp_len, a.envc
            ),
            (0, 0, 0, 0, 0, 0)
        );
    }

    /// "Give the child nothing" is a **non-null** pointer with a zero count.
    ///
    /// The kernel accepts `cap_ptr == 0` with `cap_count == 0` as well, so this
    /// is not required — but it is the shape that stays correct if that ever
    /// tightens, and more importantly it is the shape that proves we are not
    /// treating a null pointer as "the array is absent".  Version 1 does treat
    /// null that way for its fd map and argv; carrying that habit over here is
    /// exactly how a request for *specific* capabilities would silently become
    /// a request for none.
    #[test]
    fn ex2_args_empty_subset_is_a_request_not_an_absence() {
        let a = spawn_ex2_args(Some(&[]));
        assert_eq!(a.cap_mode, SPAWN_CAP_MODE_SUBSET);
        assert_eq!(a.cap_mode, 1);
        assert_eq!(a.cap_count, 0);
        assert_ne!(a.cap_ptr, 0, "an empty slice still has a non-null pointer");
        assert_eq!(a.cap_ptr % 8, 0, "and it is aligned for CapEntryInfo");
    }

    /// A non-empty subset is passed through by address, with no copy.
    #[test]
    fn ex2_args_subset_points_at_the_callers_slice() {
        let list = [
            CapEntryInfo {
                resource_type: kernel_view::res::FILE,
                reserved: [0; 3],
                rights: kernel_view::rights::READ,
                resource_id: 7,
            },
            CapEntryInfo {
                resource_type: kernel_view::res::PROCESS,
                reserved: [0; 3],
                rights: kernel_view::rights::SIGNAL,
                resource_id: 9,
            },
        ];
        let a = spawn_ex2_args(Some(&list));
        assert_eq!(a.cap_mode, SPAWN_CAP_MODE_SUBSET);
        assert_eq!(a.cap_count, 2);
        assert_eq!(a.cap_ptr, list.as_ptr() as u64);
    }

    /// The re-export is the same 24-byte struct `SYS_CAP_QUERY` writes.
    ///
    /// If this ever became a separate declaration that merely looked alike,
    /// enumerate → filter → spawn would compile and the entries would be
    /// reinterpreted field-by-field.  Asserting the size and the offsets here
    /// costs nothing and states the property the re-export exists to give.
    #[test]
    fn cap_entry_info_is_the_query_struct() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<CapEntryInfo>(), 24);
        assert_eq!(align_of::<CapEntryInfo>(), 8);
        let e = CapEntryInfo {
            resource_type: 0,
            reserved: [0; 3],
            rights: 0,
            resource_id: 0,
        };
        assert_eq!(offset_of_field!(CapEntryInfo, e, resource_type), 0);
        assert_eq!(offset_of_field!(CapEntryInfo, e, reserved), 2);
        assert_eq!(offset_of_field!(CapEntryInfo, e, rights), 8);
        assert_eq!(offset_of_field!(CapEntryInfo, e, resource_id), 16);
        // Same type, not merely the same shape.
        let _: kernel_view::CapEntryInfo = e;
    }

    /// A null array with a non-zero count is rejected here, before the syscall.
    ///
    /// The kernel rejects it too, so this is not the only guard — but it is the
    /// one that fires without having loaded the ELF, opened the file actions,
    /// and mapped a buffer, all of which this call would otherwise do before
    /// finding out.  Checked ahead of the null-`path` test on purpose: a caller
    /// with both wrong should hear about the one it asked a question about.
    #[test]
    fn slateos_spawn_caps_rejects_a_null_list_with_a_count() {
        let r = unsafe {
            slateos_spawn_caps(
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                1,
            )
        };
        assert_eq!(r, errno::EINVAL);
    }

    /// More entries than a capability table can hold cannot succeed, so it is
    /// answered locally rather than after a 4096-entry copy into the kernel.
    #[test]
    fn slateos_spawn_caps_rejects_an_oversized_list() {
        let one = CapEntryInfo {
            resource_type: kernel_view::res::FILE,
            reserved: [0; 3],
            rights: kernel_view::rights::READ,
            resource_id: 1,
        };
        let r = unsafe {
            slateos_spawn_caps(
                core::ptr::null_mut(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
                &raw const one,
                SPAWN_CAP_MAX + 1,
            )
        };
        assert_eq!(r, errno::EINVAL);
        assert_eq!(SPAWN_CAP_MAX, 4096, "kernel CapTable::MAX_ENTRIES");
    }

    // -----------------------------------------------------------------------
    // PATH search (execvpe / posix_spawnp)
    // -----------------------------------------------------------------------

    fn candidates(path: &[u8], file: &[u8]) -> std::vec::Vec<std::vec::Vec<u8>> {
        let mut it = PathCandidates::new(path, file);
        let mut out = std::vec::Vec::new();
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        while let Some(next) = it.next_into(&mut buf) {
            let len = next.expect("fits");
            assert_eq!(buf[len], 0, "NUL-terminated");
            out.push(buf[..len].to_vec());
        }
        out
    }

    #[test]
    fn path_elements_are_tried_in_order() {
        assert_eq!(
            candidates(b"/bin:/usr/bin", b"ls"),
            [b"/bin/ls".to_vec(), b"/usr/bin/ls".to_vec()]
        );
    }

    /// POSIX: an empty element is the current directory. The old search
    /// skipped it, so `PATH=":/bin"` never looked in `.` at all.
    #[test]
    fn an_empty_path_element_is_the_current_directory() {
        assert_eq!(
            candidates(b":/bin::/sbin:", b"x"),
            [
                b"x".to_vec(),
                b"/bin/x".to_vec(),
                b"x".to_vec(),
                b"/sbin/x".to_vec(),
                b"x".to_vec(),
            ]
        );
        assert_eq!(candidates(b"", b"x"), [b"x".to_vec()], "PATH set but empty");
    }

    #[test]
    fn a_candidate_that_does_not_fit_is_enametoolong() {
        let long_dir = std::vec![b'd'; crate::unistd::PATH_MAX];
        let mut it = PathCandidates::new(&long_dir, b"x");
        let mut buf = [0u8; crate::unistd::PATH_MAX];
        assert_eq!(it.next_into(&mut buf), Some(Err(errno::ENAMETOOLONG)));
        assert_eq!(it.next_into(&mut buf), None);
    }

    #[test]
    fn the_search_moves_on_only_for_not_here_errors() {
        for e in [
            errno::ENOENT,
            errno::EACCES,
            errno::ENOTDIR,
            errno::ESTALE,
            errno::ENODEV,
            errno::ETIMEDOUT,
        ] {
            assert!(search_continues_after(e), "{e} should continue");
        }
        // A program that was found and could not run is the caller's news.
        for e in [
            errno::ENOEXEC,
            errno::E2BIG,
            errno::ENOMEM,
            errno::ELOOP,
            errno::ENAMETOOLONG,
        ] {
            assert!(!search_continues_after(e), "{e} should stop the search");
        }
    }

    #[test]
    fn the_default_search_path_is_confstr_cs_path() {
        let mut buf = [0u8; 64];
        let n = crate::unistd::confstr(crate::unistd::_CS_PATH, buf.as_mut_ptr(), buf.len());
        assert_eq!(&buf[..n - 1], crate::unistd::CS_PATH);
    }

    // -----------------------------------------------------------------------
    // File actions that used to fail silently
    // -----------------------------------------------------------------------

    fn empty_map() -> [FdMapEntry; MAX_FD_MAP] {
        [FdMapEntry {
            fd: 0,
            handle_type: 0,
            _pad: [0; 3],
            handle: 0,
        }; MAX_FD_MAP]
    }

    /// A `dup2` from a descriptor that is not open is `EBADF`, as in glibc.
    /// It used to copy the empty slot over the target — a silent close.
    #[test]
    fn dup2_from_a_closed_descriptor_fails_the_spawn() {
        ensure_std_fds();
        let _ = crate::fdtable::close_fd(9);
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(posix_spawn_file_actions_adddup2(&raw mut acts, 9, 1), 0);
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        assert_eq!(
            build_fd_map(&raw const acts, &mut out, &mut opened),
            Err(errno::EBADF)
        );
        posix_spawn_file_actions_destroy(&raw mut acts);
    }

    /// `closefrom` was recorded and never applied, so every descriptor the
    /// caller asked to keep from the child reached it anyway.
    #[test]
    fn closefrom_removes_every_descriptor_from_its_floor() {
        use crate::fdtable::{HandleKind, install_fd};
        ensure_std_fds();
        let _ = install_fd(5, HandleKind::File, 505);
        let _ = install_fd(9, HandleKind::File, 509);
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(
            posix_spawn_file_actions_addclosefrom_np(&raw mut acts, 3),
            0
        );
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        let count =
            build_fd_map(&raw const acts, &mut out, &mut opened).expect("closefrom cannot fail");
        let fds: std::vec::Vec<i32> = out[..count].iter().map(|e| e.fd).collect();
        let _ = crate::fdtable::close_fd(5);
        let _ = crate::fdtable::close_fd(9);
        posix_spawn_file_actions_destroy(&raw mut acts);
        assert_eq!(fds, [0, 1, 2]);
    }

    /// Every descriptor from 32 up used to be dropped from every child.
    #[test]
    fn a_descriptor_above_32_reaches_the_child() {
        use crate::fdtable::{HandleKind, install_fd};
        ensure_std_fds();
        let _ = install_fd(40, HandleKind::File, 4040);
        let mut out = empty_map();
        let count = flatten_fd_map(&inheritable_fds(), &mut out);
        let found = out[..count].iter().find(|e| e.fd == 40).copied();
        let _ = crate::fdtable::close_fd(40);
        assert_eq!(found.map(|e| e.handle), Some(4040));
    }

    /// POSIX: `adddup2(fd, fd)` hands over a close-on-exec descriptor, with
    /// `FD_CLOEXEC` cleared in the child.
    #[test]
    fn dup2_onto_itself_hands_over_a_close_on_exec_descriptor() {
        use crate::fdtable::{FD_CLOEXEC, HandleKind, install_fd, set_fd_flags};
        ensure_std_fds();
        let _ = install_fd(6, HandleKind::File, 606);
        assert!(set_fd_flags(6, FD_CLOEXEC));

        // Without the action it is not inherited...
        let mut out = empty_map();
        let count = flatten_fd_map(&inheritable_fds(), &mut out);
        assert!(!out[..count].iter().any(|e| e.fd == 6));

        // ...and with it, it is.
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(posix_spawn_file_actions_adddup2(&raw mut acts, 6, 6), 0);
        let mut opened = OpenedHandles::new();
        let count = build_fd_map(&raw const acts, &mut out, &mut opened).expect("fd 6 is open");
        let found = out[..count].iter().find(|e| e.fd == 6).copied();
        let _ = crate::fdtable::close_fd(6);
        posix_spawn_file_actions_destroy(&raw mut acts);
        assert_eq!(found.map(|e| e.handle), Some(606));
    }

    // -- chdir actions (design-decisions.md §960) --

    /// The parent's directory and a clean host model of the filesystem's
    /// directories: set rather than assumed, since tests may share a thread.
    fn fresh_spawn_cwd(parent: &[u8]) {
        crate::unistd::host_dirs::clear();
        crate::unistd::model_kernel_without_cwd_record(false);
        crate::unistd::set_cwd_for_test(parent);
    }

    /// Plan a child whose file actions are `chdir`s to `dirs`, in order.
    fn plan_chdirs(dirs: &[&[u8]]) -> Result<ChildPlan, i32> {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        for dir in dirs {
            let mut z = dir.to_vec();
            z.push(0);
            assert_eq!(
                posix_spawn_file_actions_addchdir_np(&raw mut acts, z.as_ptr()),
                0
            );
        }
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        let plan = plan_child(&raw const acts, &mut out, &mut opened);
        posix_spawn_file_actions_destroy(&raw mut acts);
        plan
    }

    #[test]
    fn no_chdir_action_leaves_the_child_where_its_parent_is() {
        fresh_spawn_cwd(b"/home");
        let plan = plan_chdirs(&[]).expect("nothing can fail");
        assert!(!plan.cwd.moved, "the kernel should be told nothing");
        assert_eq!(plan.cwd.as_bytes(), b"/home");
    }

    /// A relative `chdir` is resolved against the parent's directory, as the
    /// child -- which starts there -- would resolve it.
    #[test]
    fn a_chdir_action_moves_the_child() {
        fresh_spawn_cwd(b"/home");
        crate::unistd::host_dirs::add(b"/home/proj");
        let plan = plan_chdirs(&[b"proj"]).expect("/home/proj exists");
        assert!(plan.cwd.moved);
        assert_eq!(plan.cwd.as_bytes(), b"/home/proj");
    }

    /// The directory goes to the kernel only when there was a `chdir`, and
    /// only when the kernel keeps the record; an older one would refuse the
    /// spawn over fields it does not know.
    #[test]
    fn the_kernel_is_given_a_directory_only_when_it_can_take_one() {
        fresh_spawn_cwd(b"/home");
        crate::unistd::host_dirs::add(b"/home/proj");

        crate::unistd::model_kernel_without_cwd_record(false);
        let plain = plan_chdirs(&[]).expect("nothing can fail");
        assert_eq!(child_start_dir(&plain), None, "no chdir: inherit");
        let moved = plan_chdirs(&[b"proj"]).expect("/home/proj exists");
        assert_eq!(child_start_dir(&moved), Some(&b"/home/proj"[..]));

        crate::unistd::model_kernel_without_cwd_record(true);
        assert_eq!(
            child_start_dir(&moved),
            None,
            "an older kernel is not asked"
        );
        crate::unistd::model_kernel_without_cwd_record(false);
    }

    /// `addfchdir_np` starts the child in the directory a descriptor names:
    /// here one the parent holds with a recorded path, as `open` records it.
    #[test]
    fn an_fchdir_action_moves_the_child_to_its_descriptors_directory() {
        use crate::fdtable::{HandleKind, close_fd, install_fd, store_fd_path};
        ensure_std_fds();
        fresh_spawn_cwd(b"/");
        crate::unistd::host_dirs::add(b"/srv/data");
        let _ = install_fd(41, HandleKind::File, 4141);
        store_fd_path(41, b"/srv/data".as_ptr(), 9);

        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(posix_spawn_file_actions_addfchdir_np(&raw mut acts, 41), 0);
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        let plan = plan_child(&raw const acts, &mut out, &mut opened);
        posix_spawn_file_actions_destroy(&raw mut acts);
        let _ = close_fd(41);
        let plan = plan.expect("fd 41 names /srv/data");
        assert!(plan.cwd.moved);
        assert_eq!(plan.cwd.as_bytes(), b"/srv/data");
    }

    /// Through a `dup2`, the new number names the same directory.
    #[test]
    fn an_fchdir_through_a_dup2_follows_the_original() {
        use crate::fdtable::{HandleKind, close_fd, install_fd, store_fd_path};
        ensure_std_fds();
        fresh_spawn_cwd(b"/");
        crate::unistd::host_dirs::add(b"/home/u");
        let _ = install_fd(42, HandleKind::File, 4242);
        store_fd_path(42, b"/home/u".as_ptr(), 7);

        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(posix_spawn_file_actions_adddup2(&raw mut acts, 42, 9), 0);
        assert_eq!(posix_spawn_file_actions_addclose(&raw mut acts, 42), 0);
        assert_eq!(posix_spawn_file_actions_addfchdir_np(&raw mut acts, 9), 0);
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        let plan = plan_child(&raw const acts, &mut out, &mut opened);
        posix_spawn_file_actions_destroy(&raw mut acts);
        let _ = close_fd(42);
        assert_eq!(plan.expect("9 is 42's copy").cwd.as_bytes(), b"/home/u");
    }

    /// A descriptor that is not open in the child is `EBADF`; one with no
    /// path -- a pipe, a console -- is not a directory.
    #[test]
    fn an_fchdir_to_nothing_or_to_a_non_directory_fails_the_spawn() {
        use crate::fdtable::{HandleKind, close_fd, install_fd};
        ensure_std_fds();
        fresh_spawn_cwd(b"/");
        let plan_of = |fd: Fd, close_first: bool| {
            let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
            posix_spawn_file_actions_init(&raw mut acts);
            if close_first {
                assert_eq!(posix_spawn_file_actions_addclose(&raw mut acts, fd), 0);
            }
            assert_eq!(posix_spawn_file_actions_addfchdir_np(&raw mut acts, fd), 0);
            let mut out = empty_map();
            let mut opened = OpenedHandles::new();
            let r = plan_child(&raw const acts, &mut out, &mut opened).map(|_| ());
            posix_spawn_file_actions_destroy(&raw mut acts);
            r
        };
        let _ = install_fd(43, HandleKind::Pipe, 4343);
        assert_eq!(
            plan_of(43, false),
            Err(errno::ENOTDIR),
            "a pipe has no path"
        );
        assert_eq!(
            plan_of(43, true),
            Err(errno::EBADF),
            "closed by the action before"
        );
        let _ = close_fd(43);
        assert_eq!(plan_of(44, false), Err(errno::EBADF), "never open");
        assert_eq!(
            posix_spawn_file_actions_addfchdir_np(core::ptr::null_mut(), -1),
            errno::EBADF
        );
    }

    /// A close action closes a close-on-exec descriptor too, so a later
    /// `adddup2(fd, fd)` cannot hand it over after all.
    #[test]
    fn a_closed_close_on_exec_descriptor_stays_closed() {
        use crate::fdtable::{FD_CLOEXEC, HandleKind, close_fd, install_fd, set_fd_flags};
        ensure_std_fds();
        let _ = install_fd(45, HandleKind::File, 4545);
        assert!(set_fd_flags(45, FD_CLOEXEC));
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(posix_spawn_file_actions_addclose(&raw mut acts, 45), 0);
        assert_eq!(posix_spawn_file_actions_adddup2(&raw mut acts, 45, 45), 0);
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        let r = build_fd_map(&raw const acts, &mut out, &mut opened);
        posix_spawn_file_actions_destroy(&raw mut acts);
        let _ = close_fd(45);
        assert_eq!(r, Err(errno::EBADF));
    }

    /// Each `chdir` starts from where the one before it left the child.
    #[test]
    fn chdir_actions_apply_in_order() {
        fresh_spawn_cwd(b"/");
        crate::unistd::host_dirs::add(b"/srv");
        crate::unistd::host_dirs::add(b"/srv/data");
        let plan = plan_chdirs(&[b"/srv", b"data"]).expect("both exist");
        assert_eq!(plan.cwd.as_bytes(), b"/srv/data");
    }

    /// POSIX: a file action that fails, fails the spawn -- with the error
    /// `chdir` itself would give.
    #[test]
    fn a_chdir_to_a_missing_directory_fails_the_spawn() {
        fresh_spawn_cwd(b"/");
        assert_eq!(plan_chdirs(&[b"/no/such"]).err(), Some(errno::ENOENT));
        assert_eq!(plan_chdirs(&[b""]).err(), Some(errno::ENOENT));
    }

    /// After a `chdir`, a relative `open` names a file in the child's
    /// directory, not in ours; before one, the path passes through as given.
    #[test]
    fn an_open_after_a_chdir_is_resolved_in_the_childs_directory() {
        fresh_spawn_cwd(b"/home");
        crate::unistd::host_dirs::add(b"/srv");
        let rel = b"log.txt\0";
        let mut out = [0u8; crate::unistd::PATH_MAX];

        let mut cwd = ChildCwd::of_parent();
        assert_eq!(
            cwd.open_path(rel, &mut out),
            Ok(&rel[..]),
            "no chdir yet: the child is where we are"
        );

        cwd.change_to(b"/srv").expect("/srv exists");
        let mut out = [0u8; crate::unistd::PATH_MAX];
        assert_eq!(cwd.open_path(rel, &mut out), Ok(&b"/srv/log.txt\0"[..]));

        let abs = b"/etc/foo\0";
        let mut out = [0u8; crate::unistd::PATH_MAX];
        assert_eq!(cwd.open_path(abs, &mut out), Ok(&abs[..]), "absolute");
    }

    fn v1_probe() -> SpawnExArgs {
        SpawnExArgs {
            elf_ptr: 1,
            elf_len: 2,
            name_ptr: 3,
            name_len: 4,
            fd_map_ptr: 5,
            fd_map_count: 6,
            argv_ptr: 7,
            argv_len: 8,
            argc: 9,
            envp_ptr: 10,
            envp_len: 11,
            envc: 12,
        }
    }

    fn assert_v1_fields_copied(a: &SpawnEx2Args) {
        assert_eq!(
            [
                a.elf_ptr,
                a.elf_len,
                a.name_ptr,
                a.name_len,
                a.fd_map_ptr,
                a.fd_map_count,
                a.argv_ptr,
                a.argv_len,
                a.argc,
                a.envp_ptr,
                a.envp_len,
                a.envc
            ],
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
        assert_eq!(a.struct_size as usize, size_of::<SpawnEx2Args>());
    }

    /// The untouched case stays on 517, which every existing caller uses.
    #[test]
    fn a_plain_spawn_stays_on_version_1() {
        assert!(matches!(
            spawn_request(v1_probe(), None, None),
            SpawnRequest::V1(_)
        ));
    }

    /// A directory needs version 2, with every capability inherited as
    /// before and the directory in the two new fields.
    #[test]
    fn a_chdir_action_goes_to_version_2_with_its_directory() {
        let dir = b"/srv/data";
        let SpawnRequest::V2(a) = spawn_request(v1_probe(), None, Some(dir)) else {
            panic!("a directory cannot travel in version 1");
        };
        assert_v1_fields_copied(&a);
        assert_eq!(a.cap_mode, SPAWN_CAP_MODE_INHERIT_ALL);
        assert_eq!((a.cwd_ptr, a.cwd_len), (dir.as_ptr() as u64, 9));
    }

    /// A capability subset alone leaves the directory fields zero: the child
    /// starts where its parent is.
    #[test]
    fn a_capability_subset_alone_sends_no_directory() {
        let SpawnRequest::V2(a) = spawn_request(v1_probe(), Some(&[]), None) else {
            panic!("a subset needs version 2");
        };
        assert_v1_fields_copied(&a);
        assert_eq!(a.cap_mode, SPAWN_CAP_MODE_SUBSET);
        assert_eq!((a.cwd_ptr, a.cwd_len), (0, 0));
    }

    /// An `open` action that fails fails the spawn with the open's errno. It
    /// used to be skipped, leaving the child without the descriptor and the
    /// caller without an error. (On the host every native syscall is stubbed,
    /// so any open fails; the point is that the failure is *reported*.)
    #[test]
    fn a_failing_open_action_fails_the_spawn() {
        ensure_std_fds();
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        assert_eq!(
            posix_spawn_file_actions_addopen(
                &raw mut acts,
                3,
                c"/definitely/not/here".as_ptr().cast::<u8>(),
                crate::fcntl::O_RDONLY,
                0
            ),
            0
        );
        let mut out = empty_map();
        let mut opened = OpenedHandles::new();
        let r = build_fd_map(&raw const acts, &mut out, &mut opened);
        posix_spawn_file_actions_destroy(&raw mut acts);
        assert!(
            r.is_err(),
            "the open failed and the spawn must say so: {r:?}"
        );
    }

    // -- The environment an exec without an `e` passes on --

    /// `execv` and `execvp` hand the new program the caller's environment --
    /// the list `environ` holds -- as POSIX requires of every `exec` without
    /// an `e` in its name. Both passed NULL until 2026-09-24, so a program
    /// started by `execv`, `execvp`, `execl` or `execlp` began with no
    /// environment at all: no `PATH`, no `HOME`, and for CPython no
    /// `PYTHONHOME`, so it could not find its own standard library.
    #[test]
    fn execv_and_execvp_pass_the_callers_environment() {
        let _env = crate::environ::lock_env_for_test();
        // SAFETY: NUL-terminated literals.
        let set =
            unsafe { crate::environ::setenv(b"SLATE_EXEC_PROBE\0".as_ptr(), b"1\0".as_ptr(), 1) };
        assert_eq!(set, 0);
        let env = crate::environ::current_environ();
        assert!(
            !env.is_null(),
            "a set variable means a non-empty environment"
        );
        let argv: [*const u8; 2] = [b"prog\0".as_ptr(), core::ptr::null()];

        exec_probe::record(core::ptr::null());
        assert_eq!(execv(b"/no/such/prog\0".as_ptr(), argv.as_ptr()), -1);
        assert_eq!(exec_probe::last(), env, "execv");

        exec_probe::record(core::ptr::null());
        assert_eq!(execvp(b"/no/such/prog\0".as_ptr(), argv.as_ptr()), -1);
        assert_eq!(exec_probe::last(), env, "execvp, given a path");

        exec_probe::record(core::ptr::null());
        assert_eq!(execvp(b"no-such-prog\0".as_ptr(), argv.as_ptr()), -1);
        assert_eq!(exec_probe::last(), env, "execvp, searching PATH");
    }

    /// `execve` and `execvpe` pass exactly the list they are given -- NULL
    /// included -- and never substitute the caller's.
    #[test]
    fn execve_and_execvpe_pass_their_own_envp() {
        let mine: [*const u8; 2] = [b"ONLY=this\0".as_ptr(), core::ptr::null()];
        let argv: [*const u8; 2] = [b"prog\0".as_ptr(), core::ptr::null()];

        exec_probe::record(core::ptr::null());
        assert_eq!(
            execve(b"/no/such/prog\0".as_ptr(), argv.as_ptr(), mine.as_ptr()),
            -1
        );
        assert_eq!(exec_probe::last(), mine.as_ptr());

        exec_probe::record(mine.as_ptr());
        assert_eq!(
            execve(
                b"/no/such/prog\0".as_ptr(),
                argv.as_ptr(),
                core::ptr::null()
            ),
            -1
        );
        assert!(
            exec_probe::last().is_null(),
            "an empty environment stays empty"
        );

        exec_probe::record(core::ptr::null());
        assert_eq!(
            execvpe(b"no-such-prog\0".as_ptr(), argv.as_ptr(), mine.as_ptr()),
            -1
        );
        assert_eq!(exec_probe::last(), mine.as_ptr());
    }
}
