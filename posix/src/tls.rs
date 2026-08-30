//! ELF thread-local storage (x86-64 "variant II") layout and installation.
//!
//! ## Why this exists
//!
//! A native static binary on this OS gets **no thread pointer** from the
//! kernel: `exec` leaves `fs_base` at 0 (userspace is expected to install
//! TLS itself) and there is no aux vector to discover `PT_TLS` from.  Any
//! compiler-generated `__thread` access — and the stack-protector canary
//! at `%fs:0x28`, which GCC/Clang emit by default at `-fstack-protector-*`
//! — dereferences `%fs:offset` and would fault on a null thread pointer.
//!
//! So the C runtime must reconstruct what a Linux crt would have read from
//! the aux vector: walk our own program headers, find `PT_TLS`, allocate a
//! block + TCB, copy the `.tdata` init image, write the TCB self-pointer,
//! and install the thread pointer with `SYS_SET_FS_BASE` (the counterpart
//! of Linux `arch_prctl(ARCH_SET_FS)`).
//!
//! ## Variant II layout
//!
//! x86-64 uses TLS variant II: the thread pointer (`%fs` base, "TP") sits
//! *above* the module's static TLS block, and the thread control block
//! begins exactly at TP:
//!
//! ```text
//!   lower addresses                                    higher addresses
//!   ┌───────────────────────────────┬──────────────────────────────────┐
//!   │  static TLS block (memsz)     │  TCB                             │
//!   │  .tdata image ‖ .tbss zeros   │  [TP+0]=TP  …  [TP+0x28]=canary  │
//!   └───────────────────────────────┴──────────────────────────────────┘
//!    TP - block_size()               TP
//! ```
//!
//! `__thread` variables are addressed as `%fs:-offset` (negative TP-relative
//! offsets, assigned by the linker), so the block must sit immediately below
//! TP — and at *exactly* the distance the linker assumed, which is
//! `round_up(p_memsz, p_align)` with the segment's own `p_align`.  Getting
//! that distance wrong is silent: every access lands a fixed number of bytes
//! away from the initialised data.  See [`normalise_align`].
//!
//! Two slots inside the TCB are architecturally meaningful: offset 0
//! must hold TP itself (the psABI's self-reference, used by
//! `__tls_get_addr`-style code and by debuggers), and offset `0x28` is where
//! GCC/Clang read the stack-protector canary.  We reserve [`TCB_SIZE`] bytes
//! to cover both; the rest is zero.
//!
//! ## Who allocates what
//!
//! - **Main thread**: [`setup_main_thread`] makes a dedicated anonymous
//!   mapping.  It is never freed (it lives as long as the process).
//! - **Child threads**: `pthread_create` carves the block out of the *top of
//!   the thread's stack mapping* (the musl approach) and hands the resulting
//!   TP to the new thread through its stack.  One mapping means one owner
//!   and one `munmap`, so the existing join/detach stack-reclaim protocol
//!   frees the TLS block too — there is no second lifetime to get wrong,
//!   and no window in which an exiting thread has already unmapped its TLS
//!   but still executes code that touches `%fs`.
//!
//!   Doing the allocation and initialisation in the *creating* thread is
//!   also what lets `pthread_create` report failure as `EAGAIN`: a child
//!   that discovered it had no memory for TLS could not report that to
//!   anyone, and could not safely run the start routine either.

/// `p_type` of the program header describing the TLS init image.
#[cfg(target_os = "none")]
const PT_TLS: u32 = 7;

/// Bytes reserved for the thread control block at and above the thread
/// pointer.  Only offsets 0 (TP self-reference) and 0x28 (stack-protector
/// canary) are architecturally used; 0x40 covers both with room to spare
/// and keeps TP 64-byte-friendly.
pub const TCB_SIZE: u64 = 0x40;

/// Round `v` up to a multiple of `a`, which must be a power of two.
const fn round_up(v: u64, a: u64) -> u64 {
    let mask = a.wrapping_sub(1);
    v.wrapping_add(mask) & !mask
}

/// Alignment of the thread pointer itself, as a floor on `p_align`.
///
/// The TCB must be at least 16-byte aligned (SysV ABI), independently of how
/// weakly the `PT_TLS` segment itself is aligned.
const MIN_TP_ALIGN: u64 = 16;

/// Sanitise a `p_align` value into a usable power-of-two alignment.
///
/// **This must reproduce the value the linker used**, because the offsets it
/// assigned to `__thread` variables are relative to
/// `TP - round_up(p_memsz, p_align)` (Drepper, *ELF Handling For
/// Thread-Local Storage*, §4.2 variant II).  Rounding `p_align` *up* here —
/// e.g. to the ABI's 16-byte TCB alignment — would move the whole block away
/// from where the linker expects it and every `__thread` access would read
/// the wrong address.  (That was a real bug: a C fixture with
/// `p_align == 4`, `p_memsz == 8` had its init image copied to `TP-16` while
/// the code read `TP-8`.)  The TCB's own 16-byte alignment requirement is
/// handled separately, by [`TlsImage::tp_align`].
///
/// The value must still be a power of two, because the layout arithmetic
/// masks with `align - 1`; 0 (meaning "no alignment constraint") becomes 1,
/// and a malformed or absurd value falls back to [`MIN_TP_ALIGN`] rather
/// than being trusted — no real `PT_TLS` requests more than 64 KiB.
#[must_use]
pub const fn normalise_align(raw: u64) -> u64 {
    if raw <= 1 {
        return 1;
    }
    if raw > 65536 {
        return MIN_TP_ALIGN;
    }
    if raw.is_power_of_two() {
        raw
    } else {
        raw.next_power_of_two()
    }
}

/// The program's `PT_TLS` segment: everything needed to build a per-thread
/// TLS block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsImage {
    /// Virtual address of the `.tdata` initialisation image (`p_vaddr`).
    pub init_vaddr: u64,
    /// Bytes of initialised data at `init_vaddr` (`p_filesz`).
    pub init_size: u64,
    /// Total per-thread size including the `.tbss` tail (`p_memsz`).
    pub mem_size: u64,
    /// The segment's own alignment (`p_align`), normalised to a power of two
    /// but **not** raised to the ABI's TCB alignment — see
    /// [`normalise_align`].  This is the value the linker used when it
    /// assigned TP-relative offsets to `__thread` variables.
    pub align: u64,
}

impl TlsImage {
    /// The image of a program with no `__thread` storage at all: no block,
    /// just a TCB (which is still required, for the canary slot and the
    /// self-pointer).
    pub const EMPTY: Self = Self {
        init_vaddr: 0,
        init_size: 0,
        mem_size: 0,
        align: 1,
    };

    /// Alignment required of the thread pointer itself.
    ///
    /// At least [`MIN_TP_ALIGN`] for the TCB, and at least the segment's own
    /// alignment so that an aligned TP implies an aligned block start (the
    /// block sits [`Self::block_size`] — a multiple of `align` — below TP).
    #[must_use]
    pub const fn tp_align(&self) -> u64 {
        if self.align > MIN_TP_ALIGN {
            self.align
        } else {
            MIN_TP_ALIGN
        }
    }

    /// Distance from the thread pointer down to the start of the TLS block —
    /// the linker's `tlsoffset` for this (single, static) module.
    ///
    /// Every `__thread` variable lives at `TP - block_size() + <offset the
    /// linker assigned>`, so this **must** use the segment's own `p_align`,
    /// not the TCB alignment.
    #[must_use]
    pub const fn block_size(&self) -> u64 {
        round_up(self.mem_size, self.align)
    }

    /// Bytes that must be reserved *above* a thread's usable stack to hold
    /// the TLS block, the TCB, the libc's own per-thread storage, and
    /// worst-case alignment slack.
    ///
    /// [`crate::perthread`] parks its block immediately above the TCB rather
    /// than allocating one, so its size is part of this reservation; see that
    /// module's docs for why.
    #[must_use]
    pub const fn reserve(&self) -> u64 {
        self.block_size()
            .wrapping_add(TCB_SIZE)
            .wrapping_add(crate::perthread::BLOCK_SIZE)
            .wrapping_add(self.tp_align())
    }

    /// Thread pointer for a region that starts at `base` and whose first
    /// `stack_size` bytes are reserved for something else (the thread's
    /// stack; 0 for a dedicated TLS mapping).
    ///
    /// The caller must have mapped at least `stack_size + self.reserve()`
    /// bytes at `base`; the returned TP then satisfies
    /// `base + stack_size <= tp - block_size()` and
    /// `tp + TCB_SIZE + perthread::BLOCK_SIZE <= base + stack_size +
    /// reserve()`.
    #[must_use]
    pub const fn thread_pointer(&self, base: u64, stack_size: u64) -> u64 {
        round_up(
            base.wrapping_add(stack_size)
                .wrapping_add(self.block_size()),
            self.tp_align(),
        )
    }
}

/// Read this program's `PT_TLS` segment, or [`TlsImage::EMPTY`] if it has
/// none.
///
/// Locates the program headers through the linker-defined `__ehdr_start`
/// symbol, which in a static non-PIE link resolves to the load address of
/// our own `Elf64_Ehdr` — the same information a Linux crt would take from
/// `AT_PHDR`/`AT_PHNUM`.
#[cfg(target_os = "none")]
#[must_use]
pub fn image() -> TlsImage {
    // Linker-defined: address of the ELF header of this executable.
    unsafe extern "C" {
        static __ehdr_start: u8;
    }

    let ehdr = core::ptr::addr_of!(__ehdr_start);
    // Elf64_Ehdr field offsets: e_phoff @0x20 (u64), e_phentsize @0x36
    // (u16), e_phnum @0x38 (u16).  Use unaligned reads — the header is a
    // packed byte layout at a symbol address of unknown alignment.
    //
    // SAFETY: `__ehdr_start` is the load address of our own ELF header,
    // always mapped read-only in a static executable, and the offsets read
    // here are within the 64-byte Elf64_Ehdr.
    let (e_phoff, e_phentsize, e_phnum) = unsafe {
        (
            core::ptr::read_unaligned(ehdr.add(0x20).cast::<u64>()),
            core::ptr::read_unaligned(ehdr.add(0x36).cast::<u16>()) as usize,
            core::ptr::read_unaligned(ehdr.add(0x38).cast::<u16>()) as usize,
        )
    };

    // SAFETY: e_phoff is our own header's program-header offset, so
    // `ehdr + e_phoff` is inside our mapped image.
    let phbase = unsafe { ehdr.add(e_phoff as usize) };
    for i in 0..e_phnum {
        // SAFETY: i < e_phnum, so this stays inside the program-header
        // table described by our own ELF header.
        let ph = unsafe { phbase.add(i.wrapping_mul(e_phentsize)) };
        // Elf64_Phdr: p_type@0 (u32), p_vaddr@16, p_filesz@32, p_memsz@40,
        // p_align@48 (all u64).
        //
        // SAFETY: `ph` points at a full Elf64_Phdr (56 bytes) inside the
        // mapped image; all reads are unaligned-safe.
        let p_type = unsafe { core::ptr::read_unaligned(ph.cast::<u32>()) };
        if p_type == PT_TLS {
            // SAFETY: as above — reads within this program header.
            return unsafe {
                TlsImage {
                    init_vaddr: core::ptr::read_unaligned(ph.add(16).cast::<u64>()),
                    init_size: core::ptr::read_unaligned(ph.add(32).cast::<u64>()),
                    mem_size: core::ptr::read_unaligned(ph.add(40).cast::<u64>()),
                    align: normalise_align(core::ptr::read_unaligned(ph.add(48).cast::<u64>())),
                }
            };
        }
    }
    TlsImage::EMPTY
}

/// Host build: no ELF image to inspect, and no thread pointer to install.
#[cfg(not(target_os = "none"))]
#[must_use]
pub fn image() -> TlsImage {
    TlsImage::EMPTY
}

/// Initialise the TLS block and TCB for thread pointer `tp`.
///
/// Copies the `.tdata` init image to the bottom of the block and writes the
/// psABI self-pointer at `[tp]`.  The `.tbss` tail and the rest of the TCB
/// (including the canary slot) are left at whatever the mapping already
/// holds, which for a fresh anonymous mapping is zero.
///
/// This is deliberately callable from *another* thread: `pthread_create`
/// initialises its child's block before the child ever runs.
///
/// # Safety
///
/// `[tp - img.block_size(), tp + TCB_SIZE)` must be mapped, writable, and
/// not concurrently in use as any other thread's TLS.
#[cfg(target_os = "none")]
pub unsafe fn init_block(tp: u64, img: &TlsImage) {
    if img.init_size > 0 {
        // SAFETY: the caller guarantees the destination block is mapped and
        // writable; the source is our own `.tdata` init image, `p_filesz`
        // bytes at `p_vaddr`, which is mapped read-only in the executable.
        // The two never overlap (one is the executable image, the other a
        // fresh mapping).
        unsafe {
            core::ptr::copy_nonoverlapping(
                img.init_vaddr as *const u8,
                (tp.wrapping_sub(img.block_size())) as *mut u8,
                img.init_size as usize,
            );
        }
    }
    // The x86-64 psABI requires %fs:0 to hold the thread pointer itself.
    //
    // SAFETY: `tp` is 16-byte aligned by construction and the caller
    // guarantees `[tp, tp + TCB_SIZE)` is mapped and writable.
    unsafe {
        (tp as *mut u64).write(tp);
    }
}

/// Host build: nothing to initialise (no thread pointer is ever installed,
/// and `pthread_create` cannot succeed on the host anyway).
#[cfg(not(target_os = "none"))]
#[allow(clippy::missing_const_for_fn)]
pub unsafe fn init_block(_tp: u64, _img: &TlsImage) {}

/// Whether *any* thread pointer has been installed in this process.
///
/// Reading `%fs:0` with `fs_base == 0` faults, so
/// [`thread_pointer`] needs a way to know that it is safe.  A single
/// process-global flag is enough — and is exactly right — because the
/// ordering is fixed: `__libc_start_main` installs the main thread's TP
/// before anything else runs, and `pthread_create` cannot be reached before
/// that.  So no thread can observe this flag set without having installed
/// its own TP first.  A program that never ran the crt (a bare-metal
/// `services/` binary) leaves it clear forever, which is the correct answer
/// for it.
#[cfg(target_os = "none")]
static TP_INSTALLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Install `tp` as the calling thread's thread pointer (`fs_base`).
///
/// Returns `false` if the kernel rejected the address as non-canonical,
/// which cannot happen for an address obtained from `mmap`.
///
/// This is the *only* step a new thread performs itself, and it must be the
/// first thing it does: everything after it may touch `%fs` (a canary, a
/// `__thread` variable).
#[cfg(target_os = "none")]
pub fn install(tp: u64) -> bool {
    use crate::syscall::{SYS_SET_FS_BASE, syscall6};
    let ok = syscall6(SYS_SET_FS_BASE, tp, 0, 0, 0, 0, 0) == 0;
    if ok {
        // Release: everything this thread wrote into its own TCB/TLS block
        // (the self-pointer, the `.tdata` image) must be visible to any
        // thread that later observes the flag.  In practice the writer is
        // this thread itself, but `pthread_create` initialises a *child's*
        // block from the parent, so the pairing is real.
        TP_INSTALLED.store(true, core::sync::atomic::Ordering::Release);
    }
    ok
}

/// This thread's thread pointer, or 0 if none has been installed.
///
/// Reads the psABI self-reference at `%fs:0` that [`init_block`] wrote —
/// the architecturally-guaranteed way to recover TP without a syscall.
/// Used by [`crate::perthread`] to find the libc's per-thread block.
#[cfg(target_os = "none")]
#[must_use]
pub fn thread_pointer() -> u64 {
    if !TP_INSTALLED.load(core::sync::atomic::Ordering::Acquire) {
        return 0;
    }
    let tp: u64;
    // SAFETY: the flag is only set after a successful `SYS_SET_FS_BASE`, and
    // `init_block` stored TP at `[TP+0]` before that, so `%fs:0` is mapped,
    // readable and holds the thread pointer.  The asm reads one word and
    // touches nothing else.
    unsafe {
        core::arch::asm!("mov {}, fs:[0]", out(reg) tp, options(nostack, readonly, preserves_flags));
    }
    tp
}

/// Host build: there is no `%fs` thread pointer, so callers fall back to
/// their own per-thread storage.
#[cfg(not(target_os = "none"))]
#[must_use]
#[allow(clippy::missing_const_for_fn)]
pub fn thread_pointer() -> u64 {
    0
}

/// Allocate, initialise and install the main thread's TLS.
///
/// Called once, as the very first thing `__libc_start_main` does, before
/// any code that might touch `%fs`.  The mapping is intentionally never
/// freed: it lives for the whole process, like the main thread's stack.
///
/// Returns `false` if no memory was available, in which case `fs_base`
/// stays 0.  That is survivable for a program that has neither `__thread`
/// storage nor stack protectors, and there is nothing better to do here —
/// startup itself must not depend on TLS.
///
/// # Safety
///
/// Must be called at most once per process, before any `__thread` access or
/// stack-protected function runs.
#[cfg(target_os = "none")]
pub unsafe fn setup_main_thread() -> bool {
    use crate::syscall::{SYS_MMAP, syscall6};

    let img = image();
    // PROT_READ|PROT_WRITE = 3, MAP_PRIVATE|MAP_ANONYMOUS = 0x22, fd = -1.
    // Anonymous pages are zero-filled, which is exactly what `.tbss` and
    // the unused TCB slots need.
    let base = syscall6(SYS_MMAP, 0, img.reserve(), 3, 0x22, u64::MAX, 0);
    if base < 0 {
        return false;
    }
    let tp = img.thread_pointer(base as u64, 0);
    // SAFETY: the mapping covers `img.reserve()` bytes at `base`, which by
    // `thread_pointer`'s contract contains `[tp - block_size, tp +
    // TCB_SIZE)`; no other thread exists yet, so nothing else uses it.
    unsafe {
        init_block(tp, &img);
    }
    install(tp)
}

#[cfg(test)]
mod tests {
    use super::{TCB_SIZE, TlsImage, normalise_align, round_up};

    /// Everything the reservation must fit *above* the thread pointer: the
    /// TCB itself plus the libc's per-thread block, which lives immediately
    /// above it (see [`crate::perthread`]).
    const ABOVE_TP: u64 = TCB_SIZE + crate::perthread::BLOCK_SIZE;

    #[test]
    fn round_up_is_identity_on_multiples() {
        assert_eq!(round_up(0, 16), 0);
        assert_eq!(round_up(16, 16), 16);
        assert_eq!(round_up(64, 64), 64);
    }

    #[test]
    fn round_up_rounds_partial_values() {
        assert_eq!(round_up(1, 16), 16);
        assert_eq!(round_up(15, 16), 16);
        assert_eq!(round_up(17, 16), 32);
        assert_eq!(round_up(65, 64), 128);
    }

    /// `p_align` must be preserved, *not* raised to the TCB's 16 bytes: the
    /// linker assigned `__thread` offsets relative to
    /// `TP - round_up(p_memsz, p_align)`.  Rounding up here would shift the
    /// whole block and silently mis-address every variable (the exact bug
    /// the `ctest-tls-thread` fixture caught, with `p_align == 4`).
    #[test]
    fn normalise_align_preserves_weak_alignments() {
        for raw in [2, 4, 8, 16] {
            assert_eq!(normalise_align(raw), raw, "raw={raw}");
        }
        // 0 means "no constraint"; 1 is the identity for `round_up`.
        assert_eq!(normalise_align(0), 1);
        assert_eq!(normalise_align(1), 1);
    }

    #[test]
    fn normalise_align_keeps_valid_powers_of_two() {
        for raw in [32, 64, 128, 4096, 65536] {
            assert_eq!(normalise_align(raw), raw, "raw={raw}");
        }
    }

    #[test]
    fn normalise_align_rejects_malformed_values() {
        // Not a power of two: rounded up, since the layout masks with
        // `align - 1`.
        assert_eq!(normalise_align(24), 32);
        assert_eq!(normalise_align(100), 128);
        // Absurdly large: a malformed header, replaced by the ABI minimum.
        assert_eq!(normalise_align(65537), 16);
        assert_eq!(normalise_align(u64::MAX), 16);
    }

    /// The thread pointer is always at least 16-byte aligned even when the
    /// segment asks for less, because the TCB (canary slot at `+0x28`) needs
    /// it.  That is independent of `block_size`, which tracks `p_align`.
    #[test]
    fn tp_align_floors_at_sixteen_without_disturbing_block_size() {
        let img = TlsImage {
            init_vaddr: 0,
            init_size: 4,
            mem_size: 8,
            align: 4,
        };
        assert_eq!(img.tp_align(), 16);
        // The linker's tlsoffset for p_memsz=8, p_align=4.
        assert_eq!(img.block_size(), 8);
        // A stronger request wins over the floor.
        let wide = TlsImage { align: 64, ..img };
        assert_eq!(wide.tp_align(), 64);
        assert_eq!(wide.block_size(), 64);
    }

    #[test]
    fn empty_image_still_reserves_a_tcb() {
        let img = TlsImage::EMPTY;
        assert_eq!(img.block_size(), 0);
        assert_eq!(img.reserve(), ABOVE_TP + 16);
    }

    #[test]
    fn block_size_rounds_to_alignment() {
        let img = TlsImage {
            init_vaddr: 0,
            init_size: 8,
            mem_size: 40,
            align: 32,
        };
        assert_eq!(img.block_size(), 64);
        assert_eq!(img.reserve(), 64 + ABOVE_TP + 32);
    }

    /// The core invariant: the computed thread pointer leaves room for the
    /// whole block below it and the whole TCB above it, without encroaching
    /// on the `stack_size` bytes at the bottom of the region.
    #[test]
    fn thread_pointer_fits_inside_the_reservation() {
        for align in [1u64, 2, 4, 8, 16, 32, 64, 4096] {
            for mem_size in [0u64, 1, 7, 8, 63, 64, 65, 1000] {
                for stack_size in [0u64, 16, 64 * 1024] {
                    for base in [0x1_0000u64, 0x1_0008, 0x2_0f00] {
                        let img = TlsImage {
                            init_vaddr: 0,
                            init_size: 0,
                            mem_size,
                            align,
                        };
                        let tp = img.thread_pointer(base, stack_size);
                        let end = base + stack_size + img.reserve();
                        assert_eq!(
                            tp % img.tp_align(),
                            0,
                            "tp not aligned: {tp:#x} tp_align={}",
                            img.tp_align()
                        );
                        assert_eq!(
                            (tp - img.block_size()) % align,
                            0,
                            "block start not p_align-aligned: tp={tp:#x} align={align}"
                        );
                        assert!(
                            tp - img.block_size() >= base + stack_size,
                            "block overlaps the stack: tp={tp:#x} block={} base={base:#x} stack={stack_size}",
                            img.block_size()
                        );
                        assert!(
                            tp + ABOVE_TP <= end,
                            "TCB/per-thread block past the mapping: tp={tp:#x} end={end:#x}"
                        );
                    }
                }
            }
        }
    }

    /// A dedicated (main-thread) mapping is the `stack_size == 0` case.
    #[test]
    fn thread_pointer_for_dedicated_mapping() {
        let img = TlsImage {
            init_vaddr: 0,
            init_size: 4,
            mem_size: 4,
            align: 16,
        };
        // block_size = 16, so TP is one alignment step above a 16-aligned
        // base.
        assert_eq!(img.thread_pointer(0x4000, 0), 0x4010);
    }
}
