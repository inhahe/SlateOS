//! The only part of scanout that cannot be tested off the target.
//!
//! Four system calls — `open`, `ioctl`, `mmap`, `close` — and the rule for
//! turning a kernel return value into an error. Everything else about talking
//! to a DRM device is protocol, lives in [`super`], and is exercised on the
//! build machine against [`super::tests::FakeCard`].
//!
//! ## Why the trait exists, and why it is shaped like this
//!
//! The obvious seam — "a function that does an ioctl" — does not work here,
//! because half the DRM ioctls are not self-contained. `GETRESOURCES`,
//! `GETCONNECTOR` and `GETENCODER` carry *pointers* to userspace arrays that
//! the kernel fills in, and a fake cannot honour a pointer: it has no way to
//! know how long the pointed-to buffer is, and writing through a `u64` a test
//! handed it would be exactly the unsafety this split exists to contain.
//!
//! So the seam is "an ioctl with out-of-line buffers": the caller passes the
//! payload *and* the buffers, saying only which payload field each buffer's
//! pointer belongs in ([`OutArray`]). Filling in the pointer is then the real
//! implementation's job and nobody else's — [`super`] never forms an address,
//! and a fake receives plain `&mut [u8]`s it can write into safely.
//!
//! ## Errors
//!
//! Failures are bare `errno` values rather than a rich enum, because every one
//! of them comes from the kernel already in that form and the caller's
//! decisions are made on a handful of specific numbers ([`EBUSY`], [`EAGAIN`],
//! [`EINTR`] mean "try again next frame"; anything else means the display is
//! gone). Wrapping them would add a translation layer whose only job would be
//! to be translated back.

/// A kernel error, as a positive `errno`.
pub type Errno = i32;

/// No such file — from `open`, means this `/dev/dri/cardN` does not exist.
/// The ordinary answer for every index past the machine's last card, so it is
/// the end of a search rather than a fault.
pub const ENOENT: Errno = 2;
/// Interrupted by a signal — the call did nothing and can be repeated.
pub const EINTR: Errno = 4;
/// Would block. From `PAGE_FLIP`, means the previous flip is still pending.
pub const EAGAIN: Errno = 11;
/// Busy. From `PAGE_FLIP`, means a flip is already queued on this CRTC.
pub const EBUSY: Errno = 16;
/// No such device — no DRM card on this machine, or none the kernel exposes.
pub const ENODEV: Errno = 19;

/// A buffer the kernel fills in, named by where its pointer lives.
///
/// DRM's enumeration ioctls do not return their arrays inline; the payload
/// holds a `u64` pointer per array and the kernel copies into it. This names
/// one such array without naming an address: `ptr_at` is a byte offset into the
/// payload — `ModeCardRes::CRTC_ID_PTR_AT`, and so on — and `buf` is the
/// storage. Writing the one into the other is [`KmsSys`]'s job.
pub struct OutArray<'a> {
    /// Byte offset within the payload of the `u64` pointer field for this
    /// array.
    pub ptr_at: usize,
    /// The storage the kernel writes into. Its length, divided by the element
    /// size, is the capacity the caller is advertising.
    pub buf: &'a mut [u8],
}

impl<'a> OutArray<'a> {
    /// An array whose pointer belongs at byte `ptr_at` of the payload.
    pub fn new(ptr_at: usize, buf: &'a mut [u8]) -> Self {
        Self { ptr_at, buf }
    }
}

/// Memory the display scans out of, mapped into this process.
///
/// A trait rather than a concrete type so a test can back it with a `Vec` and
/// the target can back it with an `mmap` — and so that unmapping is a `Drop`
/// impl on the real one, which is the only way a mapping survives every exit
/// path including a panic.
pub trait Mapped {
    /// The mapped bytes.
    ///
    /// Always at least as long as the `len` that was asked for.
    fn bytes(&mut self) -> &mut [u8];

    /// How many bytes are mapped.
    ///
    /// Separate from `bytes().len()` because the caller that needs to check the
    /// size — [`super::make_buffer`], verifying that the driver gave it what it
    /// asked for — holds the mapping behind a `Box` it has not yet stored, and
    /// asking for the length should not require a mutable borrow.
    fn bytes_len(&self) -> usize;
}

/// The four operations scanout needs from the operating system.
///
/// Deliberately not `Send`/`Sync`-bounded and deliberately `&mut self`
/// throughout: a DRM file descriptor has kernel-side per-fd state (which GEM
/// handles it owns, whether it is the DRM master) and two threads issuing
/// ioctls on one is a bug regardless of what Rust would allow.
pub trait KmsSys {
    /// Issue an ioctl.
    ///
    /// `payload` is the request structure, encoded by [`super::uapi`]. It is
    /// updated in place with whatever the kernel wrote back. For each entry in
    /// `arrays`, the implementation stores a pointer to that entry's buffer at
    /// the named payload offset before making the call.
    ///
    /// # Errors
    ///
    /// The kernel's `errno`.
    fn ioctl(
        &mut self,
        request: u32,
        payload: &mut [u8],
        arrays: &mut [OutArray<'_>],
    ) -> Result<(), Errno>;

    /// Map `len` bytes of the device at `offset` — the fake offset handed out
    /// by `MAP_DUMB` — into this process, readable and writable.
    ///
    /// # Errors
    ///
    /// The kernel's `errno`.
    fn map(&mut self, offset: u64, len: usize) -> Result<Box<dyn Mapped>, Errno>;
}

/// How many `/dev/dri/cardN` nodes are worth trying.
///
/// Linux's own minor-number range for the primary node is 0..64, but a machine
/// with more than a handful of GPUs is not a desktop, and every index costs an
/// `open` that will fail. Sixteen covers every real configuration and keeps a
/// no-display machine's failure path to sixteen failed syscalls.
pub const MAX_CARDS: u32 = 16;

/// The longest `/dev/dri/cardN\0` for `N < 100`.
const CARD_PATH_LEN: usize = 17;

/// A NUL-terminated `/dev/dri/cardN` path, without allocating.
///
/// Returned by value rather than as a `Vec<u8>` because [`CardSource::open`]
/// may be called on a path that must outlive nothing — and because the whole
/// point of this module is that the syscall layer does not depend on the
/// allocator being in a working state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardPath {
    /// The bytes, NUL-terminated at `len - 1`.
    bytes: [u8; CARD_PATH_LEN],
    /// How many bytes are used, including the NUL.
    len: usize,
}

impl CardPath {
    /// The path of `/dev/dri/card{index}`.
    ///
    /// Indices at or above 100 are clamped to 99, which cannot happen: the only
    /// caller bounds itself by [`MAX_CARDS`]. Clamping rather than returning an
    /// `Option` keeps this infallible, since a path that cannot be formed is
    /// not a case any caller has a sensible answer for.
    #[must_use]
    pub fn card(index: u32) -> Self {
        const PREFIX: &[u8] = b"/dev/dri/card";
        let mut bytes = [0u8; CARD_PATH_LEN];
        let mut len = 0usize;
        for &b in PREFIX {
            if let Some(slot) = bytes.get_mut(len) {
                *slot = b;
                len = len.saturating_add(1);
            }
        }
        let index = index.min(99);
        if index >= 10 {
            if let Some(slot) = bytes.get_mut(len) {
                *slot = b'0'.saturating_add(u8::try_from(index / 10).unwrap_or(0));
                len = len.saturating_add(1);
            }
        }
        if let Some(slot) = bytes.get_mut(len) {
            *slot = b'0'.saturating_add(u8::try_from(index % 10).unwrap_or(0));
            len = len.saturating_add(1);
        }
        // The NUL. `bytes` is zeroed, so this only advances the length.
        len = len.saturating_add(1);
        Self { bytes, len }
    }

    /// The path including its trailing NUL, as the kernel wants it.
    #[must_use]
    pub fn as_c_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }

    /// The path without its trailing NUL, for a diagnostic.
    #[must_use]
    pub fn as_display_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len.saturating_sub(1)).unwrap_or(&[])
    }
}

impl core::fmt::Display for CardPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Every byte this type can hold is ASCII, so the lossy conversion is
        // exact. It is used rather than `from_utf8` because a formatter cannot
        // return the error a `from_utf8` failure would produce, and there is no
        // input that could produce one.
        f.write_str(&String::from_utf8_lossy(self.as_display_bytes()))
    }
}

/// Something that can open a DRM card by index.
///
/// The seam that makes card *selection* testable. Opening a device node is a
/// syscall and cannot happen on the development machine, but "try each card in
/// turn and keep the first with a display attached" is policy, and policy with
/// no test is policy that is wrong on the machine nobody can run it on.
pub trait CardSource {
    /// What an opened card is.
    type Sys: KmsSys;

    /// Open `/dev/dri/card{index}`.
    ///
    /// # Errors
    ///
    /// The kernel's `errno` — `ENOENT` when there is no such node, which is the
    /// ordinary answer for every index past the last real card.
    fn open(&mut self, index: u32) -> Result<Self::Sys, Errno>;
}

#[cfg(target_os = "linux")]
pub use target::{Card, Cards};

/// The real thing: a `/dev/dri/cardN` file descriptor and raw system calls.
///
/// Gated on `target_os = "linux"` rather than on a SlateOS-specific cfg because
/// the SlateOS target *is* `target_os = "linux"` (see
/// `toolchain/x86_64-slateos.json`), and because the Linux ABI this speaks is
/// the real one — a build of the compositor for a Linux host drives a Linux
/// graphics card with this same code, which is a genuinely useful way to test
/// it against hardware the kernel does not yet support.
#[cfg(target_os = "linux")]
mod target {
    use super::{Errno, KmsSys, Mapped, OutArray};
    use std::arch::asm;

    /// `read`-ish syscall numbers, x86-64 Linux.
    const SYS_OPEN: u64 = 2;
    /// `close`.
    const SYS_CLOSE: u64 = 3;
    /// `mmap`.
    const SYS_MMAP: u64 = 9;
    /// `munmap`.
    const SYS_MUNMAP: u64 = 11;
    /// `ioctl`.
    const SYS_IOCTL: u64 = 16;

    /// `open` flags: read/write, and don't become the controlling terminal.
    const O_RDWR: u64 = 0o2;
    /// Don't leak the card into a child across `exec`. A compositor spawns
    /// applications; none of them should inherit the display device.
    const O_CLOEXEC: u64 = 0o2_000_000;

    /// `mmap` protection: readable and writable.
    const PROT_READ_WRITE: u64 = 0x1 | 0x2;
    /// `mmap` flags: shared, because the point is that the display sees the
    /// writes.
    const MAP_SHARED: u64 = 0x01;
    /// What `mmap` returns on failure, before the errno is decoded.
    const MAP_FAILED: i64 = -1;

    /// Issue a system call with six arguments.
    ///
    /// Returns the kernel's raw return value: negative values in `-4095..0`
    /// are `-errno`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the arguments are valid for the syscall named by
    /// `n` — in particular that any pointer argument points to memory of the
    /// size the kernel will read or write.
    #[inline]
    unsafe fn syscall6(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
        let ret: i64;
        // SAFETY: the `syscall` instruction clobbers `rcx` and `r11`, both
        // declared `lateout(_)` below, and returns its result in `rax`. The
        // register assignment (rdi, rsi, rdx, r10, r8, r9) is the x86-64 Linux
        // syscall ABI. Validity of the arguments themselves is this function's
        // documented precondition.
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") n as i64 => ret,
                in("rdi") a1,
                in("rsi") a2,
                in("rdx") a3,
                in("r10") a4,
                in("r8") a5,
                in("r9") a6,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        ret
    }

    /// Turn a raw syscall return into a `Result`.
    ///
    /// Linux signals failure by returning `-errno` in the range `-4095..0`;
    /// everything else is a successful result, including negative values that
    /// are genuinely large addresses.
    fn decode(ret: i64) -> Result<i64, Errno> {
        if (-4095..0).contains(&ret) {
            // `ret` is in -4095..0, so the negation is in 1..=4095 and fits in
            // an `i32`. `checked_neg` rather than `-` anyway: the one input
            // that would make the unary minus overflow is `i64::MIN`, and the
            // fact that the range check already excludes it is a fact about
            // two lines that could drift apart.
            Err(ret
                .checked_neg()
                .and_then(|v| Errno::try_from(v).ok())
                .unwrap_or(super::ENODEV))
        } else {
            Ok(ret)
        }
    }

    /// An open DRM card.
    ///
    /// Closes its file descriptor on drop, which also releases every GEM
    /// handle and framebuffer id created through it — so a compositor that
    /// panics does not leave a card holding its buffers.
    #[derive(Debug)]
    pub struct Card {
        /// The file descriptor. Always non-negative while this exists.
        fd: i32,
    }

    impl Card {
        /// Open a card by path.
        ///
        /// `path` must be NUL-terminated;
        /// [`CardPath::as_c_bytes`](super::CardPath::as_c_bytes) is.
        ///
        /// # Errors
        ///
        /// The kernel's `errno` — `ENOENT` when there is no such device node,
        /// `EACCES` when the compositor is not permitted to open it.
        pub fn open(path: &[u8]) -> Result<Self, Errno> {
            // SAFETY: `path.as_ptr()` is valid for `path.len()` bytes and the
            // caller's contract is that it is NUL-terminated within them, so
            // the kernel's read stops inside the slice. `open` writes nothing.
            let ret = unsafe {
                syscall6(
                    SYS_OPEN,
                    path.as_ptr() as u64,
                    O_RDWR | O_CLOEXEC,
                    0,
                    0,
                    0,
                    0,
                )
            };
            let fd = decode(ret)?;
            Ok(Self {
                fd: i32::try_from(fd).unwrap_or(-1),
            })
        }
    }

    /// The real [`CardSource`](super::CardSource): opens `/dev/dri/cardN`.
    ///
    /// A unit struct rather than a free function so that card *selection* can
    /// be written once, generically, and driven by a fake in a test.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Cards;

    impl super::CardSource for Cards {
        type Sys = Card;

        fn open(&mut self, index: u32) -> Result<Self::Sys, Errno> {
            Card::open(super::CardPath::card(index).as_c_bytes())
        }
    }

    impl Drop for Card {
        fn drop(&mut self) {
            if self.fd >= 0 {
                // SAFETY: `close` takes an integer and touches no memory. The
                // fd came from a successful `open` and is closed exactly once,
                // because `Card` is not `Clone` and this runs at most once.
                let _ = unsafe { syscall6(SYS_CLOSE, self.fd as u64, 0, 0, 0, 0, 0) };
            }
        }
    }

    impl KmsSys for Card {
        fn ioctl(
            &mut self,
            request: u32,
            payload: &mut [u8],
            arrays: &mut [OutArray<'_>],
        ) -> Result<(), Errno> {
            // Store each out-of-line buffer's address in the payload field
            // that names it. This is the whole reason the real implementation
            // and the protocol are different files: forming these addresses is
            // the one thing a fake cannot do.
            for array in arrays.iter_mut() {
                let addr = if array.buf.is_empty() {
                    // A zero-capacity array is a *probe* — the caller is
                    // asking "how many?" and the kernel must not copy. A null
                    // pointer is what Linux expects there, and is also what
                    // `Vec::as_mut_ptr` would not give us for an empty Vec.
                    0
                } else {
                    array.buf.as_mut_ptr() as u64
                };
                let end = array.ptr_at.saturating_add(8);
                if let Some(field) = payload.get_mut(array.ptr_at..end) {
                    field.copy_from_slice(&addr.to_le_bytes());
                }
            }
            // SAFETY: `payload` is a live slice for the duration of the call
            // and is at least as large as the size encoded in `request` — the
            // encoders in `super::uapi` produce exactly `SIZE` bytes and the
            // request number is derived from that same `SIZE`, which is the
            // invariant the `the_size_a_number_encodes_is_the_size_the_payload_declares`
            // test pins. The out-of-line pointers just written point into
            // `arrays`, which outlive this call by virtue of being borrowed.
            let ret = unsafe {
                syscall6(
                    SYS_IOCTL,
                    self.fd as u64,
                    u64::from(request),
                    payload.as_mut_ptr() as u64,
                    0,
                    0,
                    0,
                )
            };
            decode(ret).map(|_| ())
        }

        fn map(&mut self, offset: u64, len: usize) -> Result<Box<dyn Mapped>, Errno> {
            // SAFETY: a null hint lets the kernel choose the address, so this
            // cannot clobber an existing mapping. `len` and `offset` are just
            // integers to the kernel; an invalid offset is rejected with
            // EINVAL rather than mapping something else.
            let ret = unsafe {
                syscall6(
                    SYS_MMAP,
                    0,
                    len as u64,
                    PROT_READ_WRITE,
                    MAP_SHARED,
                    self.fd as u64,
                    offset,
                )
            };
            let addr = decode(ret)?;
            if addr == MAP_FAILED || addr == 0 {
                return Err(super::ENODEV);
            }
            Ok(Box::new(Mapping {
                addr: addr as usize,
                len,
            }))
        }
    }

    /// A live `mmap` of a dumb buffer.
    #[derive(Debug)]
    struct Mapping {
        /// The base address the kernel chose. Non-zero while this exists.
        addr: usize,
        /// How many bytes were mapped.
        len: usize,
    }

    impl Mapped for Mapping {
        fn bytes(&mut self) -> &mut [u8] {
            // SAFETY: `addr` came from a successful `mmap` of `len` bytes with
            // PROT_READ|PROT_WRITE and is unmapped only in `Drop`, so the
            // region is live, writable and exclusively ours for as long as the
            // returned borrow (which cannot outlive `self`). Scanout memory is
            // written by the display engine too, but only *read* by it — the
            // torn frames that causes are what double buffering is for, not a
            // data race in the Rust sense.
            unsafe { core::slice::from_raw_parts_mut(self.addr as *mut u8, self.len) }
        }

        fn bytes_len(&self) -> usize {
            self.len
        }
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: unmaps exactly the region this struct owns, once —
            // `Mapping` is not `Clone` and `bytes()` hands out a borrow that
            // cannot outlive it.
            let _ = unsafe { syscall6(SYS_MUNMAP, self.addr as u64, self.len as u64, 0, 0, 0, 0) };
        }
    }
}
