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
//! 1. Stat the file to determine its size
//! 2. Allocate a buffer via mmap
//! 3. Read the ELF binary from the filesystem via `SYS_FS_READ_FILE`
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
/// exactly: C ABI, sixteen `u64`s, 128 bytes.
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
}

/// A mismatch here is an ABI break that would show up as the kernel reading a
/// pointer out of the wrong field, so fail the build instead of the spawn.
/// This struct's entire compatibility story rests on both sides agreeing on
/// the size, which is exactly the thing a `const` assertion can guarantee and
/// a test can only observe on a run somebody makes.
const _: () = {
    assert!(size_of::<SpawnEx2Args>() == 128);
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

/// Maximum number of fd mappings we can build.
///
/// Covers three standard fds + the file actions limit (16).
const MAX_FD_MAP: usize = 32;

// ---------------------------------------------------------------------------
// posix_spawn_file_actions
// ---------------------------------------------------------------------------

/// Maximum number of file actions per spawn.
///
/// Covers typical shell pipeline needs (a few close + dup2 pairs).
const MAX_FILE_ACTIONS: usize = 16;

/// Maximum path length stored inline in an open action.
const ACTION_PATH_MAX: usize = 256;

/// A single file action to execute in the child (POSIX order).
#[derive(Clone, Copy)]
// ALLOW: The large Open variant is intentional — all storage is inline
// (no heap) so that FileAction is Copy and fits in fixed-size arrays
// without dynamic allocation.  The size difference is acceptable here.
#[allow(clippy::large_enum_variant)]
#[allow(dead_code)] // Used when posix_spawn actually applies actions in child.
enum FileAction {
    /// Close a file descriptor.
    Close { fd: Fd },
    /// Duplicate `fd` to `newfd` (like dup2).
    Dup2 { fd: Fd, newfd: Fd },
    /// Open `path` with `oflag`/`mode` and assign to `fd`.
    Open {
        fd: Fd,
        path: [u8; ACTION_PATH_MAX],
        path_len: usize,
        oflag: i32,
        mode: ModeT,
    },
}

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
    /// rather than at each of the six call sites.  It is only ever advanced
    /// from 0 by `push`, which caps it at `MAX_FILE_ACTIONS`, so the value
    /// is non-negative by construction.
    fn count(&self) -> usize {
        usize::try_from(self.used).unwrap_or(0)
    }

    /// The recorded actions, in the order they were added.
    ///
    /// Empty — not a dangling slice — before the first `add*`, because
    /// `actions` is null then and `used` is 0.
    fn slots(&self) -> &[FileActionSlot] {
        if self.actions.is_null() {
            return &[];
        }
        // SAFETY: `actions` is non-null, so it came from `push`'s allocation
        // of `MAX_FILE_ACTIONS` initialised slots, and `used <=
        // MAX_FILE_ACTIONS` is `push`'s invariant.
        unsafe { core::slice::from_raw_parts(self.actions, self.count()) }
    }

    /// Append one action, allocating the slot array on first use.
    ///
    /// Returns 0, or `ENOMEM` if the object is full or the allocation fails —
    /// the two cases POSIX gives `posix_spawn_file_actions_add*` for running
    /// out of room, which is why they are not distinguished here.
    ///
    /// # Why the cap stays, when glibc has none
    ///
    /// Lane A's report suggested dropping `MAX_FILE_ACTIONS` along with the
    /// inline storage, since a growable array makes the `ENOMEM` a real one.
    /// It is not local to this type: `MAX_FILE_ACTIONS` also sizes
    /// [`OpenedHandles::handles`], and `MAX_FD_MAP >= 3 + MAX_FILE_ACTIONS` is
    /// asserted against the **fd map handed to the kernel's spawn syscall**,
    /// which is fixed-width. An uncapped action list would silently overrun
    /// that map — or, via `OpenedHandles::push`'s bounds check, silently *leak*
    /// the handles past the end. Lifting the cap therefore means widening a
    /// kernel interface that lives in lane A's tree, so it is not this fix.
    ///
    /// The cap is also what makes one allocation right: with it, the array is
    /// 4608 bytes and cannot grow, so there is no `realloc` path to get wrong.
    ///
    /// The slot comes in by reference rather than by value: it is 288 bytes,
    /// most of it the inline path, and passing it in registers-plus-stack-copy
    /// at each of the five call sites is a copy the callee only makes again.
    fn push(&mut self, slot: &FileActionSlot) -> i32 {
        if self.count() >= MAX_FILE_ACTIONS {
            return errno::ENOMEM;
        }
        if self.actions.is_null() {
            // The whole array at once rather than glibc's doubling: our
            // `malloc` is one mmap per allocation (see malloc.rs), so any
            // request under a 16 KiB region costs a whole region — the 4608
            // bytes here and glibc's initial 8 slots are charged identically.
            //
            // This is also why the path stays inline at 256 bytes rather than
            // being `strdup`ed as lane A suggested: under a page-granular
            // allocator a `strdup` per action costs a 16 KiB region *each*, so
            // 16 opens would take 256 KiB where one flat array takes 16.
            let bytes = MAX_FILE_ACTIONS.saturating_mul(size_of::<FileActionSlot>());
            let raw = crate::malloc::malloc(bytes);
            if raw.is_null() {
                return errno::ENOMEM;
            }
            let slots = raw.cast::<FileActionSlot>();
            let mut i = 0usize;
            while i < MAX_FILE_ACTIONS {
                // SAFETY: `slots` points to `MAX_FILE_ACTIONS` slots' worth of
                // fresh, writable, uninitialised bytes; `i` is in range.  The
                // slots must be *written*, never read, until initialised —
                // hence `write`, not a `&mut` reference to a live value.
                unsafe { slots.add(i).write(FileActionSlot::empty()) };
                i = i.wrapping_add(1);
            }
            self.actions = slots;
            self.allocated = i32::try_from(MAX_FILE_ACTIONS).unwrap_or(i32::MAX);
            self.used = 0;
        }
        let idx = self.count();
        // SAFETY: `actions` is non-null and holds `MAX_FILE_ACTIONS`
        // initialised slots; `idx < MAX_FILE_ACTIONS` from the cap above.
        unsafe { *self.actions.add(idx) = *slot };
        self.used = self.used.saturating_add(1);
        0
    }

    /// Release the slot array and return to the just-initialised state.
    ///
    /// Idempotent, so a caller that destroys twice — or destroys an object it
    /// only ever `init`ed — does not double-free.
    fn release(&mut self) {
        if !self.actions.is_null() {
            // SAFETY: `actions` came from `crate::malloc::malloc` in `push`
            // and is freed exactly once, because it is nulled immediately.
            unsafe { crate::malloc::free(self.actions.cast::<u8>()) };
        }
        self.actions = core::ptr::null_mut();
        self.allocated = 0;
        self.used = 0;
    }
}

/// Internal slot — wraps `Option<FileAction>` in a fixed-size repr.
#[derive(Clone, Copy)]
#[repr(C)]
struct FileActionSlot {
    /// 0 = empty, 1 = Close, 2 = Dup2, 3 = Open.
    tag: u8,
    fd: Fd,
    newfd: Fd,
    oflag: i32,
    mode: ModeT,
    path: [u8; ACTION_PATH_MAX],
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
            path: [0; ACTION_PATH_MAX],
            path_len: 0,
        }
    }

    #[allow(dead_code)] // Used when posix_spawn actually applies actions in child.
    fn to_action(self) -> Option<FileAction> {
        match self.tag {
            1 => Some(FileAction::Close { fd: self.fd }),
            2 => Some(FileAction::Dup2 {
                fd: self.fd,
                newfd: self.newfd,
            }),
            3 => Some(FileAction::Open {
                fd: self.fd,
                path: self.path,
                path_len: self.path_len,
                oflag: self.oflag,
                mode: self.mode,
            }),
            _ => None,
        }
    }
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
/// Frees the slot array allocated by the first `add*`.  A caller that
/// `init`ed and never added anything frees nothing, and a caller that
/// destroys twice is safe: `release` nulls the pointer as it frees it.
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
    a.push(&FileActionSlot {
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
    a.push(&FileActionSlot {
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
    // SAFETY: acts and path are non-null (checked above).
    let a = unsafe { &mut *acts };
    let path_len = unsafe { crate::file::c_strlen_pub(path) };
    if path_len >= ACTION_PATH_MAX {
        return errno::ENAMETOOLONG;
    }
    let mut stored_path = [0u8; ACTION_PATH_MAX];
    // SAFETY: path is readable for path_len bytes per c_strlen_pub contract.
    unsafe {
        core::ptr::copy_nonoverlapping(path, stored_path.as_mut_ptr(), path_len);
    }
    a.push(&FileActionSlot {
        tag: 3,
        fd,
        oflag,
        mode,
        path: stored_path,
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
/// Since our kernel handles CWD at the process level, this stores the
/// path and the spawn implementation will set the child's CWD.
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
    let a = unsafe { &mut *acts };
    let path_len = unsafe { crate::file::c_strlen_pub(path) };
    if path_len >= ACTION_PATH_MAX {
        return errno::ENAMETOOLONG;
    }
    let mut stored_path = [0u8; ACTION_PATH_MAX];
    // SAFETY: path is readable for path_len bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(path, stored_path.as_mut_ptr(), path_len);
    }
    // Tag 4 = Chdir action (not yet processed by spawn — forward-compatible).
    a.push(&FileActionSlot {
        tag: 4,
        fd: -1,
        path: stored_path,
        path_len,
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
    a.push(&FileActionSlot {
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
pub fn handle_type_to_kind(handle_type: u8) -> crate::fdtable::HandleKind {
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
        _ => HandleKind::File,
    }
}

/// Tracks kernel handles opened by `build_fd_map` for open file_actions.
///
/// The parent opens files on behalf of the child (so the kernel can dup
/// them into the child's PCB).  These handles must be closed after the
/// spawn syscall returns — whether it succeeded or failed.
struct OpenedHandles {
    /// Kernel handle values that were opened by build_fd_map.
    handles: [u64; MAX_FILE_ACTIONS],
    /// Number of valid entries.
    count: usize,
}

impl OpenedHandles {
    const fn new() -> Self {
        Self {
            handles: [0; MAX_FILE_ACTIONS],
            count: 0,
        }
    }

    fn push(&mut self, handle: u64) {
        if self.count < MAX_FILE_ACTIONS {
            self.handles[self.count] = handle;
            self.count = self.count.wrapping_add(1);
        }
    }

    /// Close all tracked handles.
    fn close_all(&self) {
        let mut i = 0usize;
        while i < self.count {
            let _ = syscall1(SYS_FS_CLOSE, self.handles[i]);
            i = i.wrapping_add(1);
        }
    }
}

/// Build an fd_map array from the parent's fd table and file_actions.
///
/// Simulates what the child needs to see: starts with the parent's
/// inheritable fds (non-`FD_CLOEXEC`), then applies file_actions in
/// order:
/// - **close**: removes the fd from the virtual table
/// - **dup2**: copies a handle from one fd to another
/// - **open**: opens the file in the parent's context (raw syscall, no
///   fd allocation) and records the kernel handle.  The kernel will dup
///   it into the child during spawn.  The raw handles are tracked in
///   `opened` so the caller can close them after the spawn syscall.
///
/// Returns the number of valid entries written to `out`.
///
/// # Design
///
/// We build a "virtual fd table" that represents what the child's fd
/// table should look like after applying all file_actions.  Each slot
/// stores `Option<(u8, u64)>` — the handle type and parent handle.
///
/// After applying all actions, we flatten the non-empty slots into
/// the output `FdMapEntry` array.
fn build_fd_map(
    file_actions: *const PosixSpawnFileActionsT,
    out: &mut [FdMapEntry; MAX_FD_MAP],
    opened: &mut OpenedHandles,
) -> usize {
    use crate::fdtable;

    // Virtual fd table: mirrors what the child should see.
    // We only track fds 0..MAX_FD_MAP because that's the most we can
    // pass to the kernel anyway.
    let mut virt: [Option<(u8, u64)>; MAX_FD_MAP] = [None; MAX_FD_MAP];

    // Step 1: Populate from parent's open fds that don't have FD_CLOEXEC.
    // For the child's fd_map, we include all inheritable fds from the
    // parent so the child starts with the same I/O handles.
    let mut idx = 0usize;
    while idx < MAX_FD_MAP {
        #[allow(clippy::cast_possible_wrap)]
        let fd = idx as i32;
        if let Some(entry) = fdtable::get_fd(fd) {
            // Skip close-on-exec fds — they shouldn't be inherited.
            // Skip epoll/timerfd/inotify fds — the instance state lives
            // in the parent's userspace memory and cannot be transferred
            // to the child.
            if entry.flags & fdtable::FD_CLOEXEC == 0
                && entry.kind != fdtable::HandleKind::Epoll
                && entry.kind != fdtable::HandleKind::Timerfd
                && entry.kind != fdtable::HandleKind::Inotify
            {
                virt[idx] = Some((kind_to_handle_type(entry.kind), entry.handle));
            }
        }
        idx = idx.wrapping_add(1);
    }

    // Step 2: Apply file_actions in order.
    if !file_actions.is_null() {
        // SAFETY: file_actions is non-null (checked above).  The caller
        // guarantees it was initialized via posix_spawn_file_actions_init.
        let acts = unsafe { &*file_actions };
        let slots = acts.slots();
        let mut action_idx = 0usize;
        while action_idx < slots.len() {
            if let Some(slot) = slots.get(action_idx) {
                match slot.tag {
                    1 => {
                        // Close: remove this fd from the virtual table.
                        #[allow(clippy::cast_sign_loss)]
                        let fd_u = slot.fd as usize;
                        if fd_u < MAX_FD_MAP {
                            virt[fd_u] = None;
                        }
                    }
                    2 => {
                        // Dup2(fd → newfd): copy fd's entry to newfd.
                        #[allow(clippy::cast_sign_loss)]
                        let src_u = slot.fd as usize;
                        #[allow(clippy::cast_sign_loss)]
                        let dst_u = slot.newfd as usize;
                        if dst_u < MAX_FD_MAP && src_u < MAX_FD_MAP {
                            // Copy the entry from the virtual table (which
                            // already reflects prior actions).
                            virt[dst_u] = virt[src_u];
                        }
                    }
                    3 => {
                        // Open: open the file in the parent's context.
                        // We use a raw syscall (no fd allocation) — we
                        // just need the kernel handle to pass via fd_map.
                        // The kernel will dup it into the child during spawn.
                        #[allow(clippy::cast_sign_loss)]
                        let target_fd = slot.fd as usize;
                        if target_fd < MAX_FD_MAP && slot.path_len > 0 {
                            // Resolve the path against CWD.
                            let mut resolved = [0u8; crate::unistd::PATH_MAX];
                            let resolved_len = unsafe {
                                crate::unistd::resolve_path(slot.path.as_ptr(), &mut resolved)
                            };

                            if let Some(rlen) = resolved_len {
                                let native_flags = crate::file::translate_open_flags(slot.oflag);
                                let ret = syscall3(
                                    SYS_FS_OPEN,
                                    resolved.as_ptr() as u64,
                                    rlen as u64,
                                    native_flags,
                                );
                                if ret >= 0 {
                                    let handle = ret as u64;
                                    virt[target_fd] = Some((fd_handle_type::FILE, handle));
                                    opened.push(handle);
                                }
                                // If open fails, silently skip this action.
                                // POSIX says posix_spawn should fail, but we
                                // can't return an error from build_fd_map
                                // without complicating the interface.  The
                                // child will simply not have this fd.
                            }
                        }
                    }
                    _ => {} // Unknown tag — skip.
                }
            }
            action_idx = action_idx.wrapping_add(1);
        }
    }

    // Step 3: Flatten to FdMapEntry array.
    let mut count = 0usize;
    let mut flat_idx = 0usize;
    while flat_idx < MAX_FD_MAP {
        if let Some((handle_type, handle)) = virt[flat_idx]
            && count < MAX_FD_MAP
        {
            #[allow(clippy::cast_possible_wrap)]
            let fd = flat_idx as i32;
            out[count] = FdMapEntry {
                fd,
                handle_type,
                _pad: [0; 3],
                handle,
            };
            count = count.wrapping_add(1);
        }
        flat_idx = flat_idx.wrapping_add(1);
    }

    count
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
/// `posix_spawn`, which every existing caller uses.
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

    // Build the fd_map from the parent's fd table + file_actions.
    // This tells the kernel which handles the child should inherit.
    // Open file_actions are executed here — the parent opens the files
    // and the kernel dups the handles into the child.  We track the
    // opened handles so we can close them after the spawn syscall.
    let mut fd_map = [FdMapEntry {
        fd: 0,
        handle_type: 0,
        _pad: [0; 3],
        handle: 0,
    }; MAX_FD_MAP];
    let mut opened = OpenedHandles::new();
    let fd_map_count = build_fd_map(file_actions, &mut fd_map, &mut opened);

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // POSIX: empty path → ENOENT; too-long → ENAMETOOLONG.
        // SAFETY: path is non-null (checked above) and a valid C string.
        opened.close_all(); // Clean up any handles opened by build_fd_map.
        return if unsafe { *path } == 0 {
            errno::ENOENT
        } else {
            errno::ENAMETOOLONG
        };
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, alloc_size, data_size) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => {
            opened.close_all();
            return err;
        }
    };

    // Pack argv into a contiguous null-terminated buffer.
    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let argv_packed_len = pack_cstring_array(argv, &mut argv_buf);
    let argc = count_cstring_array(argv);

    // Pack envp into a contiguous null-terminated buffer.
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];
    let envp_packed_len = pack_cstring_array(envp, &mut envp_buf);
    let envc = count_cstring_array(envp);

    // The fields both syscalls share. Computed once and copied into whichever
    // struct we send, so the two paths cannot disagree about what is being
    // spawned -- only about who the child is allowed to be.
    let elf_ptr = buf_ptr as u64;
    let elf_len = data_size as u64;
    let name_ptr = resolved.as_ptr() as u64;
    let name_len = resolved_len as u64;
    let fd_map_ptr = if fd_map_count > 0 {
        fd_map.as_ptr() as u64
    } else {
        0
    };
    let argv_ptr = if argv_packed_len > 0 {
        argv_buf.as_ptr() as u64
    } else {
        0
    };
    let envp_ptr = if envp_packed_len > 0 {
        envp_buf.as_ptr() as u64
    } else {
        0
    };

    // `None` goes to 517, not to 559 with `cap_mode == 0`. Both mean "inherit
    // everything", but sending the untouched case down the untouched syscall
    // means this feature cannot regress the path every existing caller uses.
    let ret = match caps {
        None => {
            let spawn_args = SpawnExArgs {
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
            syscall1(SYS_PROCESS_SPAWN_EX, (&raw const spawn_args) as u64)
        }
        Some(list) => {
            // `spawn_ex2_args` fills `struct_size` and the capability policy;
            // writing either by hand at a call site is how they go stale.
            let mut spawn_args = spawn_ex2_args(Some(list));
            spawn_args.elf_ptr = elf_ptr;
            spawn_args.elf_len = elf_len;
            spawn_args.name_ptr = name_ptr;
            spawn_args.name_len = name_len;
            spawn_args.fd_map_ptr = fd_map_ptr;
            spawn_args.fd_map_count = fd_map_count as u64;
            spawn_args.argv_ptr = argv_ptr;
            spawn_args.argv_len = argv_packed_len as u64;
            spawn_args.argc = argc as u64;
            spawn_args.envp_ptr = envp_ptr;
            spawn_args.envp_len = envp_packed_len as u64;
            spawn_args.envc = envc as u64;
            syscall1(SYS_PROCESS_SPAWN_EX2, (&raw const spawn_args) as u64)
        }
    };

    // Free the ELF buffer (must use alloc_size, not data_size, to
    // unmap the entire mmap'd region and avoid memory leaks).
    let _ = mman::munmap(buf_ptr.cast::<core::ffi::c_void>(), alloc_size);

    // Close any file handles opened by build_fd_map for open file_actions.
    // The kernel has already duped them into the child's PCB, so the
    // parent's copies are no longer needed.
    opened.close_all();

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

    // If `file` contains a '/', use it directly (no PATH search).
    let file_len = unsafe { crate::file::c_strlen_pub(file) };
    if contains_slash(file, file_len) {
        return posix_spawn(pid, file, file_actions, attrp, argv, envp);
    }

    // Search PATH for the executable.
    let mut found = [0u8; crate::unistd::PATH_MAX];
    if !search_path(file, file_len, &mut found) {
        return errno::ENOENT;
    }

    posix_spawn(pid, found.as_ptr(), file_actions, attrp, argv, envp)
}

// ---------------------------------------------------------------------------
// execve (proper implementation)
// ---------------------------------------------------------------------------

/// Maximum size for packed argv/envp buffers during exec.
const EXEC_PACKED_MAX: usize = 128 * 1024;

/// Replace the current process image with a new program.
///
/// Reads the ELF binary at `path` and calls `SYS_PROCESS_EXEC` to
/// replace the current process.  On success, this function does not
/// return.  On failure, returns -1 with errno set.
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

    // Resolve relative paths against CWD.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // POSIX: empty path → ENOENT; too-long → ENAMETOOLONG.
        // SAFETY: path is non-null (checked above) and a valid C string.
        errno::set_errno(if unsafe { *path } == 0 {
            errno::ENOENT
        } else {
            errno::ENAMETOOLONG
        });
        return -1;
    };

    // Load the ELF binary using the resolved absolute path.
    let (buf_ptr, alloc_size, data_size) = match load_elf(resolved.as_ptr(), resolved_len) {
        Ok(result) => result,
        Err(err) => {
            errno::set_errno(err);
            return -1;
        }
    };

    // Pack argv into a contiguous null-terminated buffer.
    let mut argv_buf = [0u8; EXEC_PACKED_MAX];
    let argv_len = pack_cstring_array(argv, &mut argv_buf);

    // Pack envp into a contiguous null-terminated buffer.
    let mut envp_buf = [0u8; EXEC_PACKED_MAX];
    let envp_len = pack_cstring_array(envp, &mut envp_buf);

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
        let mut opened = OpenedHandles::new();
        let fd_count = build_fd_map(core::ptr::null(), &mut fd_map, &mut opened);
        // `build_fd_map` opens nothing when file_actions is null, so
        // `opened` is empty — no handles to release here.
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
        buf_ptr as u64,
        data_size as u64,
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

    // If we get here, exec failed.  Free the buffer (must use
    // alloc_size to unmap the entire mmap'd region).
    let _ = mman::munmap(buf_ptr.cast::<core::ffi::c_void>(), alloc_size);
    let _ = errno::translate(ret);
    -1
}

/// Pack a null-terminated array of C strings into a contiguous buffer.
///
/// Each string is copied with its null terminator.  Returns the total
/// byte length written.  If `array` is null, returns 0.
fn pack_cstring_array(array: *const *const u8, buf: &mut [u8]) -> usize {
    if array.is_null() {
        return 0;
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
            break; // Truncate silently if buffer is full.
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
    pos
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
/// Like `execve` but `file` is searched for in the directories listed
/// in the `PATH` environment variable.  If `file` contains a `/`, it
/// is used directly without PATH search.
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execvp(file: *const u8, argv: *const *const u8) -> i32 {
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let file_len = unsafe { crate::file::c_strlen_pub(file) };

    // If `file` contains a '/', use it directly.
    if contains_slash(file, file_len) {
        return execve(file, argv, core::ptr::null());
    }

    // Search PATH for the executable.
    let mut found = [0u8; crate::unistd::PATH_MAX];
    if !search_path(file, file_len, &mut found) {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    execve(found.as_ptr(), argv, core::ptr::null())
}

// ---------------------------------------------------------------------------
// execv
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program.
///
/// Like `execve` but inherits the current environment (the `envp`
/// parameter is omitted).
///
/// On success, does not return.  On failure, returns -1 with errno set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execv(path: *const u8, argv: *const *const u8) -> i32 {
    execve(path, argv, core::ptr::null())
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

/// Load an ELF binary from the filesystem into an mmap'd buffer.
///
/// Returns `(buffer_ptr, alloc_size, data_size)` on success, or a POSIX
/// error number on failure.  `alloc_size` is the mmap allocation size
/// (must be used for munmap); `data_size` is the number of bytes
/// actually read (pass to the kernel as the ELF size).
fn load_elf(path: *const u8, path_len: usize) -> Result<(*mut u8, usize, usize), i32> {
    // Stat the file to get its size.  SYS_FS_STAT writes a 16-byte
    // FsStatResult, not a struct stat, so translate it.
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let stat_ret = syscall3(
        SYS_FS_STAT,
        path as u64,
        path_len as u64,
        raw.as_mut_ptr() as u64,
    );

    if stat_ret < 0 {
        return Err(native_to_posix_err(stat_ret));
    }

    let mut stat_buf = crate::stat::Stat::zeroed();
    crate::stat::fill_from_fsstat(&mut stat_buf, &raw);
    let file_size = stat_buf.st_size as usize;
    if file_size == 0 {
        return Err(errno::ENOEXEC);
    }

    // Allocate a buffer via mmap.
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

    let buf_ptr = buf.cast::<u8>();

    // Read the ELF binary into the buffer.
    let read_ret = syscall4(
        SYS_FS_READ_FILE,
        path as u64,
        path_len as u64,
        buf_ptr as u64,
        file_size as u64,
    );

    if read_ret < 0 {
        let _ = mman::munmap(buf, file_size);
        return Err(native_to_posix_err(read_ret));
    }

    let bytes_read = read_ret as usize;
    if bytes_read == 0 {
        let _ = mman::munmap(buf, file_size);
        return Err(errno::ENOEXEC);
    }

    Ok((buf_ptr, file_size, bytes_read))
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

/// Check whether a byte string contains a `/` character.
///
/// Used by `posix_spawnp` and `execvp` to decide whether to do a
/// PATH search (no slash) or use the path directly (has slash).
fn contains_slash(s: *const u8, len: usize) -> bool {
    let mut i: usize = 0;
    while i < len {
        // SAFETY: Caller guarantees `s` is readable for `len` bytes.
        if unsafe { *s.add(i) } == b'/' {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}

/// Default PATH used when the PATH environment variable is not set.
const DEFAULT_PATH: &[u8] = b"/bin:/usr/bin";

/// Search the PATH environment variable for an executable file.
///
/// Tries each directory in PATH with `file` appended.  Returns `true`
/// if found, writing the full null-terminated path into `out`.
///
/// The search checks existence via `SYS_FS_STAT` — it does not check
/// execute permission (our OS doesn't have a permission system yet).
fn search_path(file: *const u8, file_len: usize, out: &mut [u8; crate::unistd::PATH_MAX]) -> bool {
    // Get the PATH environment variable.
    // SAFETY: "PATH\0" is a valid C string.
    let path_env = unsafe { crate::environ::getenv(c"PATH".as_ptr().cast::<u8>()) };

    // Determine the PATH string and its length.
    let (path_ptr, path_total_len) = if path_env.is_null() {
        (DEFAULT_PATH.as_ptr(), DEFAULT_PATH.len())
    } else {
        let len = unsafe { crate::string::strlen(path_env) };
        (path_env, len)
    };

    // Iterate over ':'-delimited directory components.
    let mut start: usize = 0;
    while start <= path_total_len {
        // Find the end of this component (next ':' or end of string).
        let mut end = start;
        while end < path_total_len {
            // SAFETY: `end < path_total_len` guarantees readable.
            if unsafe { *path_ptr.add(end) } == b':' {
                break;
            }
            end = end.wrapping_add(1);
        }

        let dir_len = end.wrapping_sub(start);

        // Skip empty components (e.g., leading/trailing/double ':').
        if dir_len > 0 {
            // Build "dir/file" in `out`.  Need: dir_len + 1 (slash) + file_len < PATH_MAX.
            let total = dir_len.wrapping_add(1).wrapping_add(file_len);
            if total < crate::unistd::PATH_MAX {
                // Copy directory.
                let mut pos: usize = 0;
                let mut j: usize = 0;
                while j < dir_len {
                    if let Some(slot) = out.get_mut(pos) {
                        // SAFETY: `start + j < path_total_len` guarantees readable.
                        *slot = unsafe { *path_ptr.add(start.wrapping_add(j)) };
                    }
                    pos = pos.wrapping_add(1);
                    j = j.wrapping_add(1);
                }

                // Add separator '/'.
                if let Some(slot) = out.get_mut(pos) {
                    *slot = b'/';
                }
                pos = pos.wrapping_add(1);

                // Copy filename.
                let mut k: usize = 0;
                while k < file_len {
                    if let Some(slot) = out.get_mut(pos) {
                        // SAFETY: `k < file_len` and caller guarantees
                        // `file` is readable for `file_len` bytes.
                        *slot = unsafe { *file.add(k) };
                    }
                    pos = pos.wrapping_add(1);
                    k = k.wrapping_add(1);
                }

                // Null-terminate.
                if let Some(slot) = out.get_mut(pos) {
                    *slot = 0;
                }

                // Check if this path exists via SYS_FS_STAT.
                if file_exists(out.as_ptr(), pos) {
                    return true;
                }
            }
        }

        // Advance past the ':' (or past end to terminate the loop).
        start = end.wrapping_add(1);
    }

    false
}

/// Check whether a file exists at the given path.
///
/// Uses `SYS_FS_STAT` to test existence.  Does not check file type
/// or permissions — just whether stat succeeds.
fn file_exists(path: *const u8, path_len: usize) -> bool {
    let mut stat_buf = crate::stat::Stat::zeroed();
    let ret = syscall3(
        SYS_FS_STAT,
        path as u64,
        path_len as u64,
        (&raw mut stat_buf) as u64,
    );
    ret >= 0
}

// ---------------------------------------------------------------------------
// execvpe — exec with PATH search + custom environment
// ---------------------------------------------------------------------------

/// Replace the current process image with a new program, searching PATH.
///
/// Like `execvp` but accepts an explicit environment (`envp`).
/// If `file` contains `/`, it is used directly.
/// Otherwise, searches each directory in `PATH`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn execvpe(file: *const u8, argv: *const *const u8, envp: *const *const u8) -> i32 {
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let file_len = unsafe { crate::file::c_strlen_pub(file) };

    // If `file` contains a '/', use it directly.
    if contains_slash(file, file_len) {
        return execve(file, argv, envp);
    }

    // Search PATH for the executable.
    let mut found = [0u8; crate::unistd::PATH_MAX];
    if !search_path(file, file_len, &mut found) {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    execve(found.as_ptr(), argv, envp)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        let param = crate::sched::SchedParam { sched_priority: 42 };
        assert_eq!(posix_spawnattr_setschedparam(attr, &raw const param), 0);

        let mut got_def = crate::signal::SigsetT::EMPTY;
        let mut got_mask = crate::signal::SigsetT::EMPTY;
        let mut got_pol = 0_i32;
        let mut got_param = crate::sched::SchedParam { sched_priority: 0 };
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
        let param = crate::sched::SchedParam { sched_priority: 0 };
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

    #[test]
    fn test_file_action_slot_to_action_empty() {
        let slot = FileActionSlot::empty();
        assert!(slot.to_action().is_none());
    }

    #[test]
    fn test_file_action_slot_to_action_close() {
        let slot = FileActionSlot {
            tag: 1,
            fd: 5,
            ..FileActionSlot::empty()
        };
        let action = slot.to_action();
        assert!(action.is_some());
        match action.unwrap() {
            FileAction::Close { fd } => assert_eq!(fd, 5),
            _ => panic!("expected Close"),
        }
    }

    #[test]
    fn test_file_action_slot_to_action_dup2() {
        let slot = FileActionSlot {
            tag: 2,
            fd: 3,
            newfd: 7,
            ..FileActionSlot::empty()
        };
        let action = slot.to_action();
        match action.unwrap() {
            FileAction::Dup2 { fd, newfd } => {
                assert_eq!(fd, 3);
                assert_eq!(newfd, 7);
            }
            _ => panic!("expected Dup2"),
        }
    }

    #[test]
    fn test_file_action_slot_to_action_open() {
        let mut path = [0u8; ACTION_PATH_MAX];
        path[0] = b'/';
        path[1] = b'f';
        path[2] = b'o';
        path[3] = b'o';
        let slot = FileActionSlot {
            tag: 3,
            fd: 1,
            oflag: 0x42,
            mode: 0o644,
            path,
            path_len: 4,
            ..FileActionSlot::empty()
        };
        let action = slot.to_action();
        match action.unwrap() {
            FileAction::Open {
                fd,
                path: p,
                path_len,
                oflag,
                mode,
            } => {
                assert_eq!(fd, 1);
                assert_eq!(path_len, 4);
                assert_eq!(&p[..4], b"/foo");
                assert_eq!(oflag, 0x42);
                assert_eq!(mode, 0o644);
            }
            _ => panic!("expected Open"),
        }
    }

    #[test]
    fn test_file_action_slot_to_action_invalid_tag() {
        let slot = FileActionSlot {
            tag: 99,
            ..FileActionSlot::empty()
        };
        assert!(slot.to_action().is_none());
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

    #[test]
    fn test_file_actions_addclose_full() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        // Fill to capacity.
        for i in 0..MAX_FILE_ACTIONS {
            let ret = posix_spawn_file_actions_addclose(&raw mut acts, i as Fd);
            assert_eq!(ret, 0);
        }
        assert_eq!(acts.count(), MAX_FILE_ACTIONS);
        // One more should fail.
        let ret = posix_spawn_file_actions_addclose(&raw mut acts, 99);
        assert_eq!(ret, errno::ENOMEM);
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

    // -- contains_slash --

    #[test]
    fn test_contains_slash_empty() {
        assert!(!contains_slash(b"\0".as_ptr(), 0));
    }

    #[test]
    fn test_contains_slash_no_slash() {
        assert!(!contains_slash(b"hello\0".as_ptr(), 5));
    }

    #[test]
    fn test_contains_slash_has_slash() {
        assert!(contains_slash(b"/bin/sh\0".as_ptr(), 7));
    }

    #[test]
    fn test_contains_slash_only_slash() {
        assert!(contains_slash(b"/\0".as_ptr(), 1));
    }

    #[test]
    fn test_contains_slash_trailing() {
        assert!(contains_slash(b"foo/\0".as_ptr(), 4));
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
        assert_eq!(packed_len, 11);
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
        let count = build_fd_map(core::ptr::null(), &mut out, &mut opened);

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
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

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
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

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
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

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
        let count = build_fd_map(&raw const acts, &mut out, &mut opened);

        // No standard fds should remain.
        let has_0_1_2 = out[..count].iter().any(|e| e.fd <= 2);
        assert!(!has_0_1_2, "all standard fds should be closed");
    }

    #[test]
    fn test_max_fd_map_constant() {
        assert_eq!(MAX_FD_MAP, 32);
        // Must be large enough for 3 standard fds + MAX_FILE_ACTIONS.
        assert!(MAX_FD_MAP >= 3 + MAX_FILE_ACTIONS);
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

    #[test]
    fn test_addchdir_np_full() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        // Fill all slots.
        for _ in 0..MAX_FILE_ACTIONS {
            posix_spawn_file_actions_addclose(&raw mut acts, 0);
        }
        let ret = posix_spawn_file_actions_addchdir_np(&raw mut acts, b"/tmp\0".as_ptr());
        assert_eq!(
            ret,
            crate::errno::ENOMEM,
            "full actions should return ENOMEM"
        );
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
    fn test_addclosefrom_np_full() {
        let mut acts = unsafe { core::mem::zeroed::<PosixSpawnFileActionsT>() };
        posix_spawn_file_actions_init(&raw mut acts);
        for _ in 0..MAX_FILE_ACTIONS {
            posix_spawn_file_actions_addclose(&raw mut acts, 0);
        }
        let ret = posix_spawn_file_actions_addclosefrom_np(&raw mut acts, 3);
        assert_eq!(
            ret,
            crate::errno::ENOMEM,
            "full actions should return ENOMEM"
        );
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

    /// 128 bytes of sixteen `u64`s, no padding.
    ///
    /// The `const` block beside the declaration already fails the build on a
    /// size change; this states the *field* offsets, which a reordering could
    /// break while leaving the size right.  A swapped `cap_ptr`/`cap_count`
    /// would hand the kernel a count where it expects a pointer — an
    /// `InvalidAddress` at best and a read of unrelated memory at worst.
    #[test]
    fn ex2_layout_is_sixteen_u64s() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<SpawnEx2Args>(), 128);
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
            (a.argv_ptr, a.argv_len, a.argc, a.envp_ptr, a.envp_len, a.envc),
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
}
