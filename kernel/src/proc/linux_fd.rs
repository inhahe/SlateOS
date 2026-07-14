//! Per-process kernel-side file descriptor table for Linux-ABI processes.
//!
//! ## Why a kernel-side fd table?
//!
//! Native processes on this kernel keep their fd table in userspace
//! (see `posix/src/fdtable.rs`).  POSIX syscalls go to the userspace
//! POSIX layer, which translates the integer `fd` to a kernel handle
//! and then calls a kernel syscall taking that handle.
//!
//! A prebuilt Linux binary cannot use this scheme — it links against
//! glibc or musl, not our POSIX layer.  There is no userspace fd table
//! in the Linux process's address space for the kernel translator
//! ([`crate::syscall::linux`]) to consult.
//!
//! Linux itself solves this by keeping the fd table inside the kernel
//! (`task_struct->files`).  We follow that design for Linux-ABI
//! processes only — Native processes are unaffected.
//!
//! ## Scope (first cut)
//!
//! - In-PCB table of `MAX_FDS` slots holding `(HandleKind, raw_handle,
//!   fd_flags, status_flags)`.
//! - Pre-installed entries for fds 0, 1, 2 pointing at the kernel
//!   console (matching the existing stdio fast path in the Linux
//!   translator).
//! - `install_lowest` / `install_at` / `lookup` / `close` / `dup` /
//!   `dup2` / `dup3` operations.
//! - `MAX_FDS = 256` (matches the userspace table).  Sufficient for any
//!   realistic Linux startup sequence.
//!
//! ## Refcounting
//!
//! A single kernel handle may be referenced by multiple fds (after
//! `dup` / `dup2`).  Closing a fd entry only invokes the underlying
//! kernel close (e.g. `sys_fs_close`) when no other fd in this process
//! still references the same handle.  This matches POSIX semantics for
//! `dup`: dup'd fds share the open file description (offset + flags).
//!
//! ## Concurrency
//!
//! All accessors take the `PROCESS_TABLE` lock (via the PCB module's
//! existing locking discipline) — fd-table mutation is serialized
//! through the same mutex that guards the PCB itself.  Tests therefore
//! cannot run concurrent fd ops from multiple kernel threads on the
//! same process, which is exactly the Linux invariant.

#![allow(dead_code)] // Many entry points will be wired up incrementally.

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of open Linux fds per process.
pub const MAX_FDS: usize = 256;

/// Linux fd 0 / 1 / 2 reserved for stdin / stdout / stderr.
pub const STDIN_FD: i32 = 0;
pub const STDOUT_FD: i32 = 1;
pub const STDERR_FD: i32 = 2;

// fd_flags — bits returned by `fcntl(F_GETFD)` / set by `F_SETFD`.
pub const FD_CLOEXEC: u32 = 1;

// Common subset of Linux O_* flags we store in `status_flags`.
// Full list lives in [`crate::syscall::linux`]; we only persist a
// representative subset.  Access-mode bits (O_RDONLY = 0, O_WRONLY = 1,
// O_RDWR = 2) live in the low two bits and are immutable after open.
pub const O_ACCMODE: u32 = 0o0003;
pub const O_RDONLY: u32 = 0o0000;
pub const O_WRONLY: u32 = 0o0001;
pub const O_RDWR: u32 = 0o0002;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_CLOEXEC: u32 = 0o2_000_000;

// ---------------------------------------------------------------------------
// Handle kinds
// ---------------------------------------------------------------------------

/// What kind of kernel resource a Linux fd refers to.
///
/// Each variant determines which kernel close syscall to call when the
/// last fd referencing this handle is closed, and which subsystem
/// `read`/`write` should dispatch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    /// Pseudo-handle for the kernel console.  Read/write go through
    /// `SYS_CONSOLE_READ_CHAR` / `SYS_CONSOLE_WRITE`.  No kernel-side
    /// resource — close is a no-op.
    Console,
    /// VFS file handle.  Read/write via `SYS_FS_READ` / `SYS_FS_WRITE`;
    /// close via `SYS_FS_CLOSE`.
    File,
    /// Pipe endpoint.  Read/write via `SYS_PIPE_READ` / `SYS_PIPE_WRITE`;
    /// close via `SYS_PIPE_CLOSE`.
    Pipe,
    /// Eventfd counter — kernel-managed 64-bit u64 with read/write of
    /// exactly 8 bytes against the in-kernel `ipc::eventfd` table.
    /// Close releases the entry from that table.
    EventFd,
    /// pidfd — opaque reference to a target process.  `raw_handle`
    /// holds the target PID as a `u64`.  There is no kernel-side
    /// resource to release on close (the PCB lifetime is independent),
    /// so `needs_kernel_close()` returns `false`.  Read/write return
    /// `EINVAL`; poll returns POLLIN once the target process is gone.
    PidFd,
    /// memfd — anonymous in-memory file (Linux `memfd_create`).
    /// `raw_handle` holds the `ipc::memfd::MemFdHandle` raw u64.
    /// Read/write are byte-stream operations against the in-kernel
    /// `MEMFD_TABLE` data buffer; close releases one refcount.
    MemFd,
    /// epoll instance — Linux `epoll_create`/`epoll_create1`.
    /// `raw_handle` holds the `ipc::epoll::EpollHandle` raw u64.
    /// Read/write return `EINVAL`; the interest set is mutated via
    /// `epoll_ctl` and harvested via `epoll_wait`.  Close releases one
    /// refcount on the in-kernel `EPOLL_TABLE` entry.
    Epoll,
    /// signalfd instance — Linux `signalfd`/`signalfd4`.
    /// `raw_handle` holds the `ipc::signalfd::SignalFdHandle` raw u64.
    /// `read` drains masked pending signals into `signalfd_siginfo`
    /// records; `write` returns `EINVAL`.  `status_flags` carries
    /// `O_NONBLOCK` when created with `SFD_NONBLOCK`.  Close releases one
    /// refcount on the in-kernel `SIGNALFD_TABLE` entry.
    SignalFd,
    /// timerfd instance — Linux `timerfd_create`/`settime`/`gettime`.
    /// `raw_handle` holds the `ipc::timerfd::TimerFdHandle` raw u64.
    /// `read` returns the 8-byte expiration count (and resets it);
    /// `write` returns `EINVAL`.  `status_flags` carries `O_NONBLOCK`
    /// when created with `TFD_NONBLOCK`.  Close releases one refcount on
    /// the in-kernel `TIMERFD_TABLE` entry.
    Timerfd,
    /// inotify instance — Linux `inotify_init`/`inotify_init1`.
    /// `raw_handle` holds the `ipc::inotify::InotifyHandle` raw u64.
    /// `read` drains queued filesystem-change events into variable-length
    /// `struct inotify_event` records; `write` returns `EINVAL`.
    /// `status_flags` carries `O_NONBLOCK` when created with `IN_NONBLOCK`.
    /// Close releases one refcount on the in-kernel `INOTIFY_TABLE` entry
    /// (and, on final close, every native watch it owns).
    Inotify,
    /// ALSA PCM substream — Linux `/dev/snd/pcmC0D0p` (playback) /
    /// `pcmC0D0c` (capture).  `raw_handle` holds the
    /// `ipc::alsa_pcm::AlsaPcmHandle` raw u64.  The substream is driven via
    /// `ioctl(SNDRV_PCM_IOCTL_*)`; `write` pushes interleaved PCM frames to
    /// the substream's mixer slot (a later commit wires the routing).
    /// `status_flags` carries `O_NONBLOCK` when opened non-blocking.  Close
    /// releases one refcount on the in-kernel `ALSA_PCM_TABLE` entry (and, on
    /// final close, the mixer slot it holds).
    AlsaPcm,
    /// ALSA control device — Linux `/dev/snd/controlC0`.  Opened by
    /// ALSA-lib (`snd_ctl_open`, used by `alsamixer`/`amixer`/PulseAudio)
    /// to enumerate the card before touching any PCM substream.  Driven
    /// purely via `ioctl(SNDRV_CTL_IOCTL_*)`; `read`/`write` return
    /// `EINVAL`.  A control fd is a *stateless* window onto the global
    /// `audio_mixer` — there is no per-open kernel resource (unlike a PCM
    /// substream, which holds a scarce mixer slot), so `raw_handle` carries
    /// the card number (always 0) and `needs_kernel_close()` returns
    /// `false`.  This mirrors `Console`/`PidFd`.
    AlsaControl,
    /// DRM card / render node — Linux `/dev/dri/card0` (full KMS authority)
    /// or `/dev/dri/renderD128` (render-only).  Opened by Linux graphics
    /// clients (Mesa, libdrm, the X.Org modesetting driver, Wayland
    /// compositors, SDL/KMSDRM) and driven entirely via `ioctl(DRM_IOCTL_*)`.
    /// `raw_handle` holds the `drm::card_fd::DrmCardHandle` raw u64 — the
    /// per-open *client* object that records the target DRM device, whether
    /// this is a render node, and the per-fd `DRM_CLIENT_CAP_*` opt-ins.
    /// `read`/`write` return `EINVAL` (DRM is ioctl-driven).  Unlike the
    /// stateless `AlsaControl`, a DRM fd carries mutable per-open state
    /// shared across `dup`/`fork`, so it uses the refcounted instance model
    /// (like `AlsaPcm`) and `needs_kernel_close()` returns `true` (close
    /// releases one refcount on the in-kernel `DRM_CARD_TABLE` entry).
    DrmCard,
    /// AF_INET / AF_INET6 `SOCK_STREAM` socket backed by the userspace
    /// `net.stack` daemon (Path B userspace-netstack cutover).  `raw_handle`
    /// holds the `net::socket::SocketHandle` raw u64.  `read`/`write` map to
    /// stream `recv`/`send` against the daemon connection; the socket is
    /// driven `socket()` → `connect()` → `send`/`recv` → `close()`.  Shared
    /// across `dup`/`fork` via the refcounted `SOCKET_TABLE`, so
    /// `needs_kernel_close()` returns `true` (close releases one refcount and,
    /// on the last fd, tears the daemon connection down).  Only created when
    /// the `net.userspace` boot switch is set.
    Socket,
}

impl HandleKind {
    /// Does this kind own a real kernel-side resource that must be
    /// released when the last fd closes?
    #[must_use]
    pub const fn needs_kernel_close(self) -> bool {
        match self {
            Self::Console | Self::PidFd | Self::AlsaControl => false,
            Self::File
            | Self::Pipe
            | Self::EventFd
            | Self::MemFd
            | Self::Epoll
            | Self::SignalFd
            | Self::Timerfd
            | Self::Inotify
            | Self::AlsaPcm
            | Self::DrmCard
            | Self::Socket => true,
        }
    }
}

// ---------------------------------------------------------------------------
// FdEntry
// ---------------------------------------------------------------------------

/// One slot in the per-process Linux fd table.
#[derive(Debug, Clone, Copy)]
pub struct FdEntry {
    /// Kind of resource backing this fd.
    pub kind: HandleKind,
    /// Raw kernel handle value (interpretation depends on `kind`).
    pub raw_handle: u64,
    /// Per-fd flags (`FD_CLOEXEC`).  Set by `fcntl(F_SETFD)`.
    pub fd_flags: u32,
    /// File status flags (`O_APPEND`, `O_NONBLOCK`, ...).  Set at open
    /// time and modifiable via `fcntl(F_SETFL)` for non-access-mode
    /// bits.
    pub status_flags: u32,
    /// `fcntl(F_GETOWN)` / `F_SETOWN` value — the pid (positive)
    /// or pgid (negative) that should receive SIGIO when this fd
    /// signals readiness for async I/O.  Linux stores this on the
    /// `struct file` (open-file description); we attach it
    /// per-fd because we don't yet have a separate file-table
    /// layer.  Default 0 ("no delivery target", matches a fresh
    /// fd on Linux).
    ///
    /// We never actually deliver SIGIO — there is no signal
    /// machinery in the kernel — so this is a stored-only ABI
    /// round-trip.  Programs that set the owner and read it
    /// back observe what they wrote.
    pub f_owner: i32,
    /// `fcntl(F_GETSIG)` / `F_SETSIG` value — the signal number
    /// to deliver in lieu of SIGIO (default 0 means "use SIGIO").
    /// Linux validates 0 or [1, 64]; we mirror that range.
    /// Stored only; same delivery caveat as `f_owner`.
    pub f_owner_sig: i32,
}

impl FdEntry {
    /// Construct a console entry — used for the pre-installed
    /// stdin/stdout/stderr.
    #[must_use]
    pub const fn console(access: u32) -> Self {
        Self {
            kind: HandleKind::Console,
            raw_handle: 0,
            fd_flags: 0,
            status_flags: access,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for a freshly opened VFS file.
    #[must_use]
    pub const fn file(handle: u64, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::File,
            raw_handle: handle,
            fd_flags: 0,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for a pipe endpoint.
    #[must_use]
    pub const fn pipe(handle: u64, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::Pipe,
            raw_handle: handle,
            fd_flags: 0,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for an eventfd counter.  `raw_handle` is the
    /// `ipc::eventfd::EventFdHandle` raw u64.
    #[must_use]
    pub const fn eventfd(handle: u64, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::EventFd,
            raw_handle: handle,
            fd_flags: 0,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct a pidfd entry referencing process `target_pid`.
    /// `raw_handle` stores the PID as a u64 so the Linux dispatch
    /// layer can recover it for `pidfd_send_signal`, `pidfd_getfd`,
    /// poll-on-exit, and any future helpers.
    #[must_use]
    pub const fn pidfd(target_pid: u64, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::PidFd,
            raw_handle: target_pid,
            fd_flags: 0,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for a memfd.  `raw_handle` is the
    /// `ipc::memfd::MemFdHandle` raw u64.
    #[must_use]
    pub const fn memfd(handle: u64, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::MemFd,
            raw_handle: handle,
            fd_flags: 0,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for an epoll instance.  `raw_handle` is the
    /// `ipc::epoll::EpollHandle` raw u64.  `fd_flags` carries `FD_CLOEXEC`
    /// when the instance was created with `EPOLL_CLOEXEC`.
    #[must_use]
    pub const fn epoll(handle: u64, fd_flags: u32) -> Self {
        Self {
            kind: HandleKind::Epoll,
            raw_handle: handle,
            fd_flags,
            status_flags: 0,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for a signalfd instance.  `raw_handle` is the
    /// `ipc::signalfd::SignalFdHandle` raw u64.  `fd_flags` carries
    /// `FD_CLOEXEC` when created with `SFD_CLOEXEC`; `status_flags`
    /// carries `O_NONBLOCK` when created with `SFD_NONBLOCK`.
    #[must_use]
    pub const fn signalfd(handle: u64, fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::SignalFd,
            raw_handle: handle,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for a timerfd instance.  `raw_handle` is the
    /// `ipc::timerfd::TimerFdHandle` raw u64.  `fd_flags` carries
    /// `FD_CLOEXEC` when created with `TFD_CLOEXEC`; `status_flags`
    /// carries `O_NONBLOCK` when created with `TFD_NONBLOCK`.
    #[must_use]
    pub const fn timerfd(handle: u64, fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::Timerfd,
            raw_handle: handle,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for an inotify instance.  `raw_handle` is the
    /// `ipc::inotify::InotifyHandle` raw u64.  `fd_flags` carries
    /// `FD_CLOEXEC` when created with `IN_CLOEXEC`; `status_flags` carries
    /// `O_NONBLOCK` when created with `IN_NONBLOCK`.
    #[must_use]
    pub const fn inotify(handle: u64, fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::Inotify,
            raw_handle: handle,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for an ALSA PCM substream.  `raw_handle` is the
    /// `ipc::alsa_pcm::AlsaPcmHandle` raw u64.  `fd_flags` carries
    /// `FD_CLOEXEC` when opened with `O_CLOEXEC`; `status_flags` carries
    /// `O_NONBLOCK` when opened non-blocking.
    #[must_use]
    pub const fn alsa_pcm(handle: u64, fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::AlsaPcm,
            raw_handle: handle,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for an ALSA control device (`/dev/snd/controlC0`).
    /// A control fd is *stateless*: it carries no per-open kernel resource,
    /// so `raw_handle` holds the card number (always 0) and there is nothing
    /// to release on close.  `fd_flags` carries `FD_CLOEXEC` when opened with
    /// `O_CLOEXEC`; `status_flags` carries `O_NONBLOCK` when opened
    /// non-blocking.
    #[must_use]
    pub const fn alsa_control(fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::AlsaControl,
            raw_handle: 0,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for an AF_INET/AF_INET6 `SOCK_STREAM` socket.
    /// `raw_handle` is the `net::socket::SocketHandle` raw u64.  `fd_flags`
    /// carries `FD_CLOEXEC` when created with `SOCK_CLOEXEC`; `status_flags`
    /// carries `O_NONBLOCK` when created with `SOCK_NONBLOCK`.
    #[must_use]
    pub const fn socket(handle: u64, fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::Socket,
            raw_handle: handle,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }

    /// Construct an entry for a DRM card / render node (`/dev/dri/card0`,
    /// `/dev/dri/renderD128`).  `raw_handle` is the
    /// `drm::card_fd::DrmCardHandle` raw u64.  `fd_flags` carries
    /// `FD_CLOEXEC` when opened with `O_CLOEXEC`; `status_flags` carries
    /// `O_NONBLOCK` when opened non-blocking.
    #[must_use]
    pub const fn drm_card(handle: u64, fd_flags: u32, status_flags: u32) -> Self {
        Self {
            kind: HandleKind::DrmCard,
            raw_handle: handle,
            fd_flags,
            status_flags,
            f_owner: 0,
            f_owner_sig: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// KernelFdTable
// ---------------------------------------------------------------------------

/// Per-process Linux fd table.
///
/// Stored inside the PCB; allocated lazily when a process first needs
/// it (which is unconditionally true for Linux-ABI processes — the
/// table is built at the same point [`crate::proc::pcb::AbiMode::Linux`]
/// is stamped).
pub struct KernelFdTable {
    /// `entries[fd] = Some(...)` for open fds, `None` for free slots.
    entries: [Option<FdEntry>; MAX_FDS],
}

impl Default for KernelFdTable {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelFdTable {
    /// Build an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_FDS],
        }
    }

    /// Build a table with stdin/stdout/stderr pre-installed as
    /// console handles.
    ///
    /// This matches what glibc / musl expect at process startup: fds
    /// 0, 1, 2 already point at terminals (or whatever the parent set
    /// them to).  Without these, the very first `write(1, ...)` from a
    /// Linux binary would fail with `EBADF`.
    #[must_use]
    pub fn with_stdio() -> Self {
        let mut t = Self::new();
        // SAFETY-equivalent invariant: `entries` has length `MAX_FDS` >= 3.
        t.entries[STDIN_FD as usize] = Some(FdEntry::console(O_RDONLY));
        t.entries[STDOUT_FD as usize] = Some(FdEntry::console(O_WRONLY));
        t.entries[STDERR_FD as usize] = Some(FdEntry::console(O_WRONLY));
        t
    }

    /// Look up `fd`.  Returns `None` if `fd` is out of range or unused.
    #[must_use]
    pub fn lookup(&self, fd: i32) -> Option<FdEntry> {
        if fd < 0 {
            return None;
        }
        let idx = fd as usize;
        if idx >= MAX_FDS {
            return None;
        }
        self.entries[idx]
    }

    /// Snapshot every open fd as an ascending `(fd_number, entry)` list.
    ///
    /// Used to render `/proc/<pid>/fd/` — the kernel-visible fd table is
    /// the only truthful source for a Linux-ABI process's open
    /// descriptors (native processes keep theirs in userspace and are not
    /// visible here).
    #[must_use]
    pub fn list_open(&self) -> alloc::vec::Vec<(i32, FdEntry)> {
        let mut out = alloc::vec::Vec::new();
        for (idx, slot) in self.entries.iter().enumerate() {
            if let Some(entry) = slot {
                // `idx < MAX_FDS` (256) always fits in i32 without wrap.
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                out.push((idx as i32, *entry));
            }
        }
        out
    }

    /// Allocate the lowest-numbered free fd and store `entry` there.
    ///
    /// Returns the new fd number or `KernelError::TooManyOpenFiles` if
    /// the table is full.
    pub fn install_lowest(&mut self, entry: FdEntry) -> KernelResult<i32> {
        self.install_lowest_from(0, entry)
    }

    /// Allocate the lowest-numbered free fd `>= min_fd` and store
    /// `entry` there.  Used to implement `fcntl(F_DUPFD, min_fd)`.
    pub fn install_lowest_from(&mut self, min_fd: i32, entry: FdEntry) -> KernelResult<i32> {
        if min_fd < 0 {
            return Err(KernelError::InvalidArgument);
        }
        let start = min_fd as usize;
        if start >= MAX_FDS {
            return Err(KernelError::TooManyOpenFiles);
        }
        for (idx, slot) in self.entries[start..].iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(entry);
                // `start + idx` cannot overflow: bounded by MAX_FDS.
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                return Ok((start + idx) as i32);
            }
        }
        Err(KernelError::TooManyOpenFiles)
    }

    /// Install `entry` at a specific `fd`, overwriting any existing
    /// entry.  Caller is responsible for closing the previous handle
    /// (use [`Self::take`] first if it matters).
    pub fn install_at(&mut self, fd: i32, entry: FdEntry) -> KernelResult<()> {
        if fd < 0 {
            return Err(KernelError::InvalidArgument);
        }
        let idx = fd as usize;
        if idx >= MAX_FDS {
            return Err(KernelError::TooManyOpenFiles);
        }
        self.entries[idx] = Some(entry);
        Ok(())
    }

    /// Remove and return the entry at `fd`.  Returns `None` if the
    /// slot is unused or out of range.
    pub fn take(&mut self, fd: i32) -> Option<FdEntry> {
        if fd < 0 {
            return None;
        }
        let idx = fd as usize;
        if idx >= MAX_FDS {
            return None;
        }
        self.entries[idx].take()
    }

    /// Return `true` if any fd (other than `excluded_fd`) still
    /// references `(kind, raw_handle)`.  Used by `close` to decide
    /// whether to release the underlying kernel resource.
    #[must_use]
    pub fn is_handle_referenced(
        &self,
        kind: HandleKind,
        raw_handle: u64,
        excluded_fd: i32,
    ) -> bool {
        for (idx, slot) in self.entries.iter().enumerate() {
            if excluded_fd >= 0 && idx == excluded_fd as usize {
                continue;
            }
            if let Some(entry) = slot
                && entry.kind == kind
                && entry.raw_handle == raw_handle
            {
                return true;
            }
        }
        false
    }

    /// Update the `fd_flags` of an existing entry (for
    /// `fcntl(F_SETFD)`).
    pub fn set_fd_flags(&mut self, fd: i32, fd_flags: u32) -> KernelResult<()> {
        let entry = self.entry_mut(fd)?;
        entry.fd_flags = fd_flags;
        Ok(())
    }

    /// Update the `status_flags` of an existing entry (for
    /// `fcntl(F_SETFL)`).
    ///
    /// Access-mode bits (`O_ACCMODE`) are preserved from the original
    /// entry — Linux ignores attempts to change them via `F_SETFL`.
    pub fn set_status_flags(&mut self, fd: i32, new_flags: u32) -> KernelResult<()> {
        let entry = self.entry_mut(fd)?;
        let access = entry.status_flags & O_ACCMODE;
        entry.status_flags = (new_flags & !O_ACCMODE) | access;
        Ok(())
    }

    /// Read the `fcntl(F_GETOWN)` value for `fd`.
    ///
    /// Returns `Err(InvalidHandle)` if the fd is closed / out of range.
    pub fn get_owner(&self, fd: i32) -> KernelResult<i32> {
        self.lookup(fd)
            .map(|e| e.f_owner)
            .ok_or(KernelError::InvalidHandle)
    }

    /// Set the `fcntl(F_SETOWN)` value for `fd`.
    ///
    /// Linux stores this verbatim — positive = pid, negative = pgid,
    /// 0 = clear.  We do not validate that the pid/pgid actually
    /// exists, because Linux doesn't either (the value is consulted
    /// asynchronously at SIGIO delivery time, by which time the
    /// target may have exited).
    pub fn set_owner(&mut self, fd: i32, owner: i32) -> KernelResult<()> {
        let entry = self.entry_mut(fd)?;
        entry.f_owner = owner;
        Ok(())
    }

    /// Read the `fcntl(F_GETSIG)` value for `fd`.  0 means "use the
    /// default SIGIO".
    pub fn get_owner_sig(&self, fd: i32) -> KernelResult<i32> {
        self.lookup(fd)
            .map(|e| e.f_owner_sig)
            .ok_or(KernelError::InvalidHandle)
    }

    /// Set the `fcntl(F_SETSIG)` value for `fd`.
    ///
    /// Linux validates `sig == 0 || (1..=64).contains(&sig)` and
    /// returns `EINVAL` otherwise.  This helper enforces the same
    /// range; the surface syscall surfaces the error code.
    pub fn set_owner_sig(&mut self, fd: i32, sig: i32) -> KernelResult<()> {
        if sig != 0 && !(1..=64).contains(&sig) {
            return Err(KernelError::InvalidArgument);
        }
        let entry = self.entry_mut(fd)?;
        entry.f_owner_sig = sig;
        Ok(())
    }

    fn entry_mut(&mut self, fd: i32) -> KernelResult<&mut FdEntry> {
        if fd < 0 {
            return Err(KernelError::InvalidArgument);
        }
        let idx = fd as usize;
        if idx >= MAX_FDS {
            return Err(KernelError::InvalidHandle);
        }
        self.entries[idx].as_mut().ok_or(KernelError::InvalidHandle)
    }

    /// Duplicate `oldfd` onto the lowest free slot >= `min_fd`.
    /// Implements `dup` (min_fd=0) and `fcntl(F_DUPFD, min_fd)`.
    pub fn dup_lowest(&mut self, oldfd: i32, min_fd: i32) -> KernelResult<i32> {
        let mut src = self.lookup(oldfd).ok_or(KernelError::InvalidHandle)?;
        // POSIX: the duplicate clears FD_CLOEXEC.
        src.fd_flags = 0;
        self.install_lowest_from(min_fd, src)
    }

    /// Duplicate `oldfd` onto `newfd`, closing any prior occupant of
    /// `newfd`.  Returns the previous occupant (so the caller can
    /// close it after dropping the table lock if needed) and the
    /// newfd.
    pub fn dup2(&mut self, oldfd: i32, newfd: i32) -> KernelResult<(i32, Option<FdEntry>)> {
        let mut src = self.lookup(oldfd).ok_or(KernelError::InvalidHandle)?;
        if newfd < 0 {
            return Err(KernelError::InvalidArgument);
        }
        let idx = newfd as usize;
        if idx >= MAX_FDS {
            return Err(KernelError::TooManyOpenFiles);
        }
        // POSIX: when oldfd == newfd and oldfd is valid, dup2 returns
        // newfd without closing anything.
        if oldfd == newfd {
            return Ok((newfd, None));
        }
        let prev = self.entries[idx].take();
        // POSIX: the duplicate clears FD_CLOEXEC.
        src.fd_flags = 0;
        self.entries[idx] = Some(src);
        Ok((newfd, prev))
    }

    /// Iterate over `(fd, FdEntry)` for every open fd.  Used by
    /// teardown and `close-on-exec`.
    pub fn open_entries(&self) -> impl Iterator<Item = (i32, FdEntry)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| {
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                slot.map(|e| (idx as i32, e))
            })
    }

    /// Remove every open entry whose `FD_CLOEXEC` flag is set and
    /// return them in fd order so the caller can release any kernel
    /// resources they reference.
    ///
    /// This is the kernel side of POSIX `close-on-exec`: it does NOT
    /// itself close the underlying kernel handles (it can't — it does
    /// not know about `sys_fs_close` / `sys_pipe_close` etc., which
    /// live in higher layers).  The caller is responsible for invoking
    /// the appropriate native close on each returned entry whose
    /// [`HandleKind::needs_kernel_close`] is `true`, after first
    /// verifying via [`Self::is_handle_referenced`] that no
    /// non-cloexec fd still references it.
    pub fn take_cloexec_entries(&mut self) -> alloc::vec::Vec<FdEntry> {
        let mut out = alloc::vec::Vec::new();
        for slot in &mut self.entries {
            if let Some(entry) = *slot
                && entry.fd_flags & FD_CLOEXEC != 0
            {
                out.push(entry);
                *slot = None;
            }
        }
        out
    }

    /// Ensure stdin/stdout/stderr are present as `Console` entries,
    /// filling any missing slot.  Existing entries at fds 0/1/2 — even
    /// non-Console ones (e.g. an open file dup'd over stdout) — are
    /// preserved, matching POSIX semantics where exec only closes
    /// cloexec fds.
    ///
    /// Used by `exec` to guarantee a Linux image always boots with a
    /// usable stdio trio even if the previous image had closed one of
    /// the standard fds.
    pub fn ensure_stdio(&mut self) {
        const STDIO: [(i32, u32); 3] = [
            (STDIN_FD, O_RDONLY),
            (STDOUT_FD, O_WRONLY),
            (STDERR_FD, O_WRONLY),
        ];
        for &(fd, access) in &STDIO {
            #[allow(clippy::cast_sign_loss)]
            let idx = fd as usize;
            if self.entries[idx].is_none() {
                self.entries[idx] = Some(FdEntry::console(access));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run self-tests for the Linux fd table.
///
/// Exercises:
/// - empty + stdio construction
/// - install_lowest allocates 3 then 4 then 5 (skipping pre-installed 0/1/2)
/// - install_lowest_from honours min_fd
/// - install_at overwrites
/// - lookup out-of-range returns None
/// - take removes and lookup returns None afterwards
/// - is_handle_referenced semantics for File handles
/// - dup_lowest copies entry and clears FD_CLOEXEC
/// - dup2 closes the prior occupant and returns it
/// - dup2 with oldfd == newfd is a no-op
/// - set_fd_flags / set_status_flags preserve access-mode bits
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Empty table — fd 0 returns None.
    let empty = KernelFdTable::new();
    if empty.lookup(0).is_some() {
        serial_println!("[linux_fd] FAIL: empty table fd 0 should be None");
        return Err(KernelError::InternalError);
    }

    // with_stdio — fds 0/1/2 are Console.
    let mut t = KernelFdTable::with_stdio();
    for &fd in &[STDIN_FD, STDOUT_FD, STDERR_FD] {
        let entry = t.lookup(fd).ok_or_else(|| {
            serial_println!("[linux_fd] FAIL: stdio fd {} should be installed", fd);
            KernelError::InternalError
        })?;
        if entry.kind != HandleKind::Console {
            serial_println!("[linux_fd] FAIL: stdio fd {} kind = {:?}", fd, entry.kind);
            return Err(KernelError::InternalError);
        }
    }

    // install_lowest after stdio should give 3, 4, 5.
    let f3 = t.install_lowest(FdEntry::file(0x1111, O_RDONLY))?;
    let f4 = t.install_lowest(FdEntry::file(0x2222, O_RDWR))?;
    let f5 = t.install_lowest(FdEntry::file(0x3333, O_WRONLY))?;
    if (f3, f4, f5) != (3, 4, 5) {
        serial_println!(
            "[linux_fd] FAIL: install_lowest gave {}/{}/{}, want 3/4/5",
            f3, f4, f5,
        );
        return Err(KernelError::InternalError);
    }

    // install_lowest_from(10, ...) skips to 10.
    let f10 = t.install_lowest_from(10, FdEntry::file(0xAAAA, O_RDONLY))?;
    if f10 != 10 {
        serial_println!("[linux_fd] FAIL: install_lowest_from(10) = {}", f10);
        return Err(KernelError::InternalError);
    }

    // lookup at fd 5 reads back what we wrote.
    let e5 = t.lookup(5).ok_or(KernelError::InternalError)?;
    if e5.raw_handle != 0x3333 || e5.status_flags != O_WRONLY {
        serial_println!("[linux_fd] FAIL: lookup(5) returned wrong entry: {:?}", e5);
        return Err(KernelError::InternalError);
    }

    // Out-of-range and negative lookups return None.
    if t.lookup(-1).is_some() || t.lookup(MAX_FDS as i32).is_some() {
        serial_println!("[linux_fd] FAIL: out-of-range lookup should be None");
        return Err(KernelError::InternalError);
    }

    // is_handle_referenced for fd 4's handle: just the one ref.
    if t.is_handle_referenced(HandleKind::File, 0x2222, -1)
        && !t.is_handle_referenced(HandleKind::File, 0x2222, 4)
    {
        // Excluding fd 4 should drop the count to zero (only one ref).
    } else {
        serial_println!("[linux_fd] FAIL: is_handle_referenced should drop to 0 when excluding the sole reference");
        return Err(KernelError::InternalError);
    }

    // dup_lowest(4, 0) clones fd 4's File entry onto the next free slot.
    let dup_fd = t.dup_lowest(4, 0)?;
    if dup_fd != 6 {
        serial_println!("[linux_fd] FAIL: dup_lowest(4, 0) = {}, want 6", dup_fd);
        return Err(KernelError::InternalError);
    }
    let dup_entry = t.lookup(dup_fd).ok_or(KernelError::InternalError)?;
    if dup_entry.raw_handle != 0x2222 || dup_entry.fd_flags != 0 {
        serial_println!(
            "[linux_fd] FAIL: dup entry mismatch: {:?}",
            dup_entry,
        );
        return Err(KernelError::InternalError);
    }
    // Now there are two refs to handle 0x2222.
    if !t.is_handle_referenced(HandleKind::File, 0x2222, 4) {
        serial_println!("[linux_fd] FAIL: after dup, handle should still be referenced if we exclude fd 4");
        return Err(KernelError::InternalError);
    }

    // dup2(3, 5) — overwrites fd 5 (handle 0x3333) with fd 3 (handle 0x1111).
    let (new_fd, prev) = t.dup2(3, 5)?;
    if new_fd != 5 {
        serial_println!("[linux_fd] FAIL: dup2(3, 5) returned newfd {}", new_fd);
        return Err(KernelError::InternalError);
    }
    let prev_entry = prev.ok_or(KernelError::InternalError)?;
    if prev_entry.raw_handle != 0x3333 {
        serial_println!(
            "[linux_fd] FAIL: dup2 should return prior fd 5 entry (handle 0x3333), got {:?}",
            prev_entry,
        );
        return Err(KernelError::InternalError);
    }
    let new_entry = t.lookup(5).ok_or(KernelError::InternalError)?;
    if new_entry.raw_handle != 0x1111 {
        serial_println!(
            "[linux_fd] FAIL: dup2 destination should have source handle 0x1111, got {:?}",
            new_entry,
        );
        return Err(KernelError::InternalError);
    }

    // dup2(3, 3) — same fd, must not close anything and must succeed.
    let (same_fd, prev_same) = t.dup2(3, 3)?;
    if same_fd != 3 || prev_same.is_some() {
        serial_println!("[linux_fd] FAIL: dup2(3, 3) should be a no-op");
        return Err(KernelError::InternalError);
    }

    // take(3) removes the entry; subsequent lookup is None.
    if t.take(3).is_none() {
        serial_println!("[linux_fd] FAIL: take(3) returned None");
        return Err(KernelError::InternalError);
    }
    if t.lookup(3).is_some() {
        serial_println!("[linux_fd] FAIL: lookup(3) after take should be None");
        return Err(KernelError::InternalError);
    }

    // set_fd_flags + set_status_flags.
    t.set_fd_flags(4, FD_CLOEXEC)?;
    let e4 = t.lookup(4).ok_or(KernelError::InternalError)?;
    if e4.fd_flags != FD_CLOEXEC {
        serial_println!("[linux_fd] FAIL: set_fd_flags did not stick");
        return Err(KernelError::InternalError);
    }
    // set_status_flags preserves O_ACCMODE bits: fd 4 was opened O_RDWR.
    t.set_status_flags(4, O_NONBLOCK | O_RDONLY)?;
    let e4 = t.lookup(4).ok_or(KernelError::InternalError)?;
    if e4.status_flags & O_ACCMODE != O_RDWR {
        serial_println!(
            "[linux_fd] FAIL: set_status_flags clobbered O_ACCMODE: {:#o}",
            e4.status_flags,
        );
        return Err(KernelError::InternalError);
    }
    if e4.status_flags & O_NONBLOCK == 0 {
        serial_println!("[linux_fd] FAIL: set_status_flags did not set O_NONBLOCK");
        return Err(KernelError::InternalError);
    }

    // set_fd_flags / set_status_flags on a closed fd → EBADF.
    if !matches!(t.set_fd_flags(99, 0), Err(KernelError::InvalidHandle)) {
        serial_println!("[linux_fd] FAIL: set_fd_flags on closed fd should be EBADF");
        return Err(KernelError::InternalError);
    }

    // ------------------------------------------------------------------
    // take_cloexec_entries / ensure_stdio — exec close-on-exec semantics.
    // ------------------------------------------------------------------
    {
        let mut e = KernelFdTable::with_stdio();
        // Install three File handles; mark fds 3 and 5 cloexec, fd 4 not.
        let f3 = e.install_lowest(FdEntry::file(0xC0DE, O_RDONLY))?;
        let f4 = e.install_lowest(FdEntry::file(0xBEEF, O_RDWR))?;
        let f5 = e.install_lowest(FdEntry::file(0xFACE, O_WRONLY))?;
        if (f3, f4, f5) != (3, 4, 5) {
            serial_println!(
                "[linux_fd] FAIL: cloexec setup install_lowest gave {}/{}/{}",
                f3, f4, f5,
            );
            return Err(KernelError::InternalError);
        }
        e.set_fd_flags(3, FD_CLOEXEC)?;
        e.set_fd_flags(5, FD_CLOEXEC)?;
        // Also mark stderr (fd 2) cloexec — to verify ensure_stdio refills.
        e.set_fd_flags(STDERR_FD, FD_CLOEXEC)?;

        let taken = e.take_cloexec_entries();
        // Should have taken stderr + fd3 + fd5 = 3 entries.
        if taken.len() != 3 {
            serial_println!(
                "[linux_fd] FAIL: take_cloexec_entries took {} entries, want 3",
                taken.len(),
            );
            return Err(KernelError::InternalError);
        }
        // stderr slot is empty before ensure_stdio.
        if e.lookup(STDERR_FD).is_some() {
            serial_println!("[linux_fd] FAIL: cloexec stderr should be cleared before ensure_stdio");
            return Err(KernelError::InternalError);
        }
        // fd 4 (non-cloexec) survives.
        let surv = e.lookup(4).ok_or_else(|| {
            serial_println!("[linux_fd] FAIL: fd 4 (non-cloexec) should have survived");
            KernelError::InternalError
        })?;
        if surv.raw_handle != 0xBEEF {
            serial_println!("[linux_fd] FAIL: surviving fd 4 wrong handle: {:?}", surv);
            return Err(KernelError::InternalError);
        }

        e.ensure_stdio();
        // stdin (0) was already populated and not cloexec — must not be
        // overwritten.  stdout (1) ditto.  stderr (2) was cloexec'd —
        // must be refilled with a Console entry.
        let se = e.lookup(STDERR_FD).ok_or_else(|| {
            serial_println!("[linux_fd] FAIL: stderr should be refilled by ensure_stdio");
            KernelError::InternalError
        })?;
        if se.kind != HandleKind::Console {
            serial_println!("[linux_fd] FAIL: refilled stderr kind = {:?}", se.kind);
            return Err(KernelError::InternalError);
        }

        // ensure_stdio MUST NOT overwrite an existing non-Console fd
        // sitting at 0/1/2.  Plant a File at stdin and rerun.
        e.take(STDIN_FD);
        e.install_at(STDIN_FD, FdEntry::file(0xDEAD, O_RDWR))?;
        e.ensure_stdio();
        let s0 = e.lookup(STDIN_FD).ok_or(KernelError::InternalError)?;
        if s0.kind != HandleKind::File || s0.raw_handle != 0xDEAD {
            serial_println!(
                "[linux_fd] FAIL: ensure_stdio clobbered non-Console stdin: {:?}",
                s0,
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[linux_fd] Self-test PASSED");
    Ok(())
}
