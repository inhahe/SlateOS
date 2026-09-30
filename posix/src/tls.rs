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
pub(crate) const PT_TLS: u32 = 7;

/// Bytes reserved for the thread control block at and above the thread
/// pointer.  Only offsets 0 (TP self-reference) and 0x28 (stack-protector
/// canary) are architecturally used; 0x40 covers both with room to spare
/// and keeps TP 64-byte-friendly.
pub const TCB_SIZE: u64 = 0x40;

/// Byte offset of the stack-protector canary within the TCB.
///
/// GCC and Clang emit `mov %fs:0x28, %reg` in the prologue of every function
/// they protect, and compare against it in the epilogue. This is the value
/// that actually guards a stack frame on x86-64 — **not** the
/// `__stack_chk_guard` global, which is the fallback other architectures use.
pub const STACK_GUARD_OFFSET: u64 = 0x28;

/// Write the process's stack-protector canary into the TCB at `tp`.
///
/// Until 2026-09-13 nothing wrote this slot, and [`init_block`]'s own doc said
/// so: the TCB "is left at whatever the mapping already holds, which for a
/// fresh anonymous mapping is zero". **A zero canary is worse than a fixed
/// one.** Zero is the single likeliest value for an overflow to deposit — a
/// string terminator, a zeroed buffer, a short `memset` — so the check the
/// compiler emits passes for exactly the overflows it exists to catch.
///
/// Deliberately NOT done inside `init_block`: the canary value comes from
/// `AT_RANDOM`, whose fill path sets `errno` on failure, and `errno` lives in
/// TLS. Seeding from inside the routine that is *establishing* TLS would
/// touch `%fs` before `%fs` is valid. This is called by the caller, once TLS
/// is up.
///
/// Every thread in a process gets the **same** value, as glibc does by copying
/// the parent's guard into the child TCB. Re-rolling per thread would be worse
/// than useless: a thread that changed the value after another had already
/// loaded it into a live frame would abort a process that was never smashed.
///
/// # Safety
///
/// `[tp, tp + TCB_SIZE)` must be mapped and writable, and `tp` must be this
/// thread's thread pointer or a child's not-yet-running one.
#[cfg(target_os = "none")]
pub unsafe fn set_stack_guard(tp: u64, guard: u64) {
    // SAFETY: the caller guarantees the TCB is mapped and writable, and
    // STACK_GUARD_OFFSET (0x28) is inside TCB_SIZE (0x40).
    unsafe {
        (tp.wrapping_add(STACK_GUARD_OFFSET) as *mut u64).write(guard);
    }
}

/// Host build: there is no `%fs`-based TCB to write.
#[cfg(not(target_os = "none"))]
pub unsafe fn set_stack_guard(_tp: u64, _guard: u64) {}

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

/// `p_type` of a loadable segment.
pub(crate) const PT_LOAD: u32 = 1;
/// `p_type` of the program header table's own entry.
pub(crate) const PT_PHDR: u32 = 6;
/// `p_type` of `.eh_frame_hdr`'s segment: the index an unwinder searches.
pub(crate) const PT_GNU_EH_FRAME: u32 = 0x6474_e550;

/// Bytes in an `Elf64_Phdr`, the least an `e_phentsize` may say.
// Read only where there is an image to read: the target, and the tests'
// synthetic ones.
#[cfg_attr(not(any(test, target_os = "none")), allow(dead_code))]
const PHDR_SIZE: usize = 56;

/// One program header's fields (`Elf64_Phdr`), read out of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// The names are the ELF specification's own, prefix and all.
#[allow(clippy::struct_field_names)]
pub(crate) struct Phdr {
    /// `p_type`.
    pub p_type: u32,
    /// `p_offset`: where the segment starts in the file.
    pub p_offset: u64,
    /// `p_vaddr`: where it starts in memory, before the load bias.
    pub p_vaddr: u64,
    /// `p_filesz`: bytes of it the file holds.
    pub p_filesz: u64,
    /// `p_memsz`: bytes of it in memory.
    pub p_memsz: u64,
    /// `p_align`.
    pub p_align: u64,
}

/// The program's own ELF header and program header table, where they are
/// mapped: what a Linux program is told through `AT_PHDR`, `AT_PHNUM` and
/// `AT_PHENT`, and a native one finds through `__ehdr_start`
/// ([`program_headers`]). Read in place, never copied.
///
/// The TLS set-up here and `crate::dlfcn` -- `dl_iterate_phdr`, `dladdr`,
/// `_dl_find_object`, `dlinfo` -- all read the program's headers, and all
/// through this.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramHeaders {
    /// The ELF header.
    ehdr: *const u8,
    /// The first program header.
    phdr: *const u8,
    /// `e_phnum`.
    phnum: usize,
    /// `e_phentsize`: at least [`PHDR_SIZE`].
    phentsize: usize,
}

/// Read a `u64` at `p`, of no promised alignment.
///
/// # Safety
///
/// `p` must be valid for an 8-byte read.
unsafe fn read_u64(p: *const u8) -> u64 {
    // SAFETY: the caller's contract.
    unsafe { core::ptr::read_unaligned(p.cast::<u64>()) }
}

impl ProgramHeaders {
    /// The table the ELF header at `ehdr` describes; `None` when it has no
    /// entries, or entries too short to be `Elf64_Phdr`s.
    ///
    /// # Safety
    ///
    /// `ehdr` must point at an `Elf64_Ehdr`, and the program header table it
    /// describes must be mapped, both for as long as the result is used.
    // Called only where there is an image: the target, and the tests.
    #[cfg_attr(not(any(test, target_os = "none")), allow(dead_code))]
    pub(crate) unsafe fn of_ehdr(ehdr: *const u8) -> Option<Self> {
        // Elf64_Ehdr: e_phoff @0x20 (u64), e_phentsize @0x36 (u16), e_phnum
        // @0x38 (u16); read unaligned, the header being bytes at an address
        // of no promised alignment.
        //
        // SAFETY: the caller's contract; the offsets are inside the header's
        // 64 bytes.
        let (phoff, phentsize, phnum) = unsafe {
            (
                read_u64(ehdr.wrapping_add(0x20)),
                usize::from(core::ptr::read_unaligned(
                    ehdr.wrapping_add(0x36).cast::<u16>(),
                )),
                usize::from(core::ptr::read_unaligned(
                    ehdr.wrapping_add(0x38).cast::<u16>(),
                )),
            )
        };
        if phnum == 0 || phentsize < PHDR_SIZE {
            return None;
        }
        Some(Self {
            ehdr,
            phdr: ehdr.wrapping_add(usize::try_from(phoff).ok()?),
            phnum,
            phentsize,
        })
    }

    /// The ELF header.
    pub(crate) const fn ehdr(&self) -> *const u8 {
        self.ehdr
    }

    /// The first program header: `dl_phdr_info`'s `dlpi_phdr`.
    pub(crate) const fn phdr(&self) -> *const u8 {
        self.phdr
    }

    /// How many program headers there are.
    pub(crate) const fn phnum(&self) -> usize {
        self.phnum
    }

    /// Header `i`; `None` past the last.
    pub(crate) fn get(&self, i: usize) -> Option<Phdr> {
        if i >= self.phnum {
            return None;
        }
        let at = self.phdr.wrapping_add(i.checked_mul(self.phentsize)?);
        // Elf64_Phdr: p_type @0 (u32), p_offset @8, p_vaddr @16, p_filesz
        // @32, p_memsz @40, p_align @48 (u64s).
        //
        // SAFETY: `of_ehdr`'s contract maps the whole table, `phnum` entries
        // of `phentsize` >= 56 bytes, and `i < phnum`.
        unsafe {
            Some(Phdr {
                p_type: core::ptr::read_unaligned(at.cast::<u32>()),
                p_offset: read_u64(at.wrapping_add(8)),
                p_vaddr: read_u64(at.wrapping_add(16)),
                p_filesz: read_u64(at.wrapping_add(32)),
                p_memsz: read_u64(at.wrapping_add(40)),
                p_align: read_u64(at.wrapping_add(48)),
            })
        }
    }

    /// Every header, in order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = Phdr> + '_ {
        (0..self.phnum).filter_map(|i| self.get(i))
    }

    /// The first header of type `p_type`.
    pub(crate) fn find(&self, p_type: u32) -> Option<Phdr> {
        self.iter().find(|p| p.p_type == p_type)
    }

    /// The load bias: how far the program sits from the addresses its
    /// headers give. 0 for an `ET_EXEC`, which runs where it was linked --
    /// every program here -- and found as the C libraries find it for the
    /// rest: from `PT_PHDR`, the table's own entry, or else from the
    /// `PT_LOAD` mapping the file's first byte, where the ELF header is.
    pub(crate) fn load_bias(&self) -> u64 {
        if let Some(p) = self.find(PT_PHDR) {
            return (self.phdr.addr() as u64).wrapping_sub(p.p_vaddr);
        }
        if let Some(p) = self.iter().find(|p| p.p_type == PT_LOAD && p.p_offset == 0) {
            return (self.ehdr.addr() as u64).wrapping_sub(p.p_vaddr);
        }
        0
    }

    /// The program's `PT_TLS` segment, or [`TlsImage::EMPTY`] if it has
    /// none.
    pub(crate) fn tls_image(&self) -> TlsImage {
        let bias = self.load_bias();
        self.find(PT_TLS).map_or(TlsImage::EMPTY, |p| TlsImage {
            init_vaddr: bias.wrapping_add(p.p_vaddr),
            init_size: p.p_filesz,
            mem_size: p.p_memsz,
            align: normalise_align(p.p_align),
        })
    }
}

/// This program's headers; `None` if they are not in memory.
///
/// Found through the linker-defined `__ehdr_start` symbol, which in a static
/// link resolves to the load address of our own `Elf64_Ehdr` -- the same
/// information a Linux crt takes from `AT_PHDR`/`AT_PHNUM`. Native processes
/// get no auxiliary vector (`requests/d-a-native-processes-could-be-told-where-their-program-headers-are.md`),
/// so this is the only source.
///
/// ## When `__ehdr_start` is 0
///
/// The linker can only give the header an address if a loaded segment
/// contains it. A linker script that starts its one `PT_LOAD` at the first
/// section instead (`PHDRS { load PT_LOAD FLAGS(7); }` without `FILEHDR
/// PHDRS`, as `coreutils`, `oils` and `shell` were linked until 2026-09-25)
/// leaves the headers out of memory, and lld resolves `__ehdr_start` to 0.
/// This used to be read through anyway: every such program died on its first
/// instructions with a page fault at address 0x36, `e_phentsize`'s offset,
/// which said nothing about why
/// (`requests/a-bd-coreutils-cannot-start-two-link-faults.md`).
///
/// It now answers `None` -- for [`image`], "no TLS image", which is what
/// glibc (weak `__ehdr_start`, null-checked in `_dl_aux_init`) and musl (no
/// `AT_PHDR`, so its `PT_TLS` walk runs zero times) both do. The cost of that
/// answer is known and narrow: a program whose headers are unmapped *and*
/// which has a `PT_TLS` segment -- C `__thread`, since the slateos target
/// sets `has-thread-local` false -- runs with an empty TLS block, so its
/// thread-locals start at zero instead of their initialisers; and
/// `dl_iterate_phdr` has no program to report, so such a program cannot
/// unwind. Every program linked with lld's default layout maps its headers
/// and is unaffected.
#[cfg(target_os = "none")]
pub(crate) fn program_headers() -> Option<ProgramHeaders> {
    // Linker-defined: address of the ELF header of this executable.
    unsafe extern "C" {
        static __ehdr_start: u8;
    }

    let mut ehdr = core::ptr::addr_of!(__ehdr_start);
    // The compiler may assume the address of a (non-weak) static is never
    // null and fold the check below away, and the null case is exactly the
    // one it exists for. This empty `asm!` makes the value opaque: it claims
    // to be able to change `ehdr`, so nothing about it can be assumed after.
    //
    // SAFETY: the template is empty; the block reads and writes nothing but
    // the register it is handed, and touches no memory, stack or flags.
    #[allow(clippy::pointers_in_nomem_asm_block)]
    // the pointer is never dereferenced: only its value is laundered
    unsafe {
        core::arch::asm!(
            "/* {0} */",
            inout(reg) ehdr,
            options(pure, nomem, nostack, preserves_flags)
        );
    }
    if ehdr.is_null() {
        return None;
    }
    // SAFETY: `__ehdr_start` is non-null, so the linker gave it the load
    // address of our own ELF header, which it does only when a loaded
    // segment maps the header -- and the program header table follows it
    // in that segment, as every layout that maps the header puts it
    // (`FILEHDR PHDRS`). Both are the executable's, mapped for its life.
    unsafe { ProgramHeaders::of_ehdr(ehdr) }
}

/// Host build: no ELF image to inspect.
#[cfg(not(target_os = "none"))]
#[allow(clippy::missing_const_for_fn, clippy::unnecessary_wraps)]
pub(crate) fn program_headers() -> Option<ProgramHeaders> {
    None
}

/// Read this program's `PT_TLS` segment, or [`TlsImage::EMPTY`] if it has
/// none -- or if its program headers cannot be found ([`program_headers`]).
#[must_use]
pub fn image() -> TlsImage {
    program_headers().map_or(TlsImage::EMPTY, |ph| ph.tls_image())
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
    let installed = install(tp);
    if installed {
        // AFTER `install`, never before: the canary comes from `AT_RANDOM`,
        // whose fill path sets `errno` on failure, and `errno` lives in TLS.
        // Asking for it before `%fs` is valid would fault in the routine whose
        // whole job is making `%fs` valid.
        let guard = crate::crt::process_stack_guard();
        // SAFETY: the mapping above covers `[tp, tp + TCB_SIZE)` and this is
        // the only thread in the process at this point.
        unsafe {
            set_stack_guard(tp, guard);
        }
    }
    installed
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

    use super::{
        PT_GNU_EH_FRAME, PT_LOAD, PT_PHDR, PT_TLS, Phdr, ProgramHeaders, SyntheticImage,
        program_headers,
    };

    fn ph(
        p_type: u32,
        p_offset: u64,
        p_vaddr: u64,
        p_filesz: u64,
        p_memsz: u64,
        p_align: u64,
    ) -> Phdr {
        Phdr {
            p_type,
            p_offset,
            p_vaddr,
            p_filesz,
            p_memsz,
            p_align,
        }
    }

    /// A program linked at 0x400000, as lld lays one out: the headers in the
    /// first segment, a TLS segment, and `.eh_frame_hdr`.
    fn linked_at_4m(with_phdr: bool) -> std::vec::Vec<Phdr> {
        let mut v = std::vec::Vec::new();
        if with_phdr {
            v.push(ph(PT_PHDR, 64, 0x40_0040, 56 * 5, 56 * 5, 8));
        }
        v.push(ph(PT_LOAD, 0, 0x40_0000, 0x1000, 0x1000, 0x1000));
        v.push(ph(PT_LOAD, 0x1000, 0x40_1000, 0x800, 0x2000, 0x1000));
        v.push(ph(PT_TLS, 0x1400, 0x40_1400, 8, 24, 8));
        v.push(ph(PT_GNU_EH_FRAME, 0x900, 0x40_0900, 0x40, 0x40, 4));
        v
    }

    #[test]
    fn the_table_is_read_where_it_lies() {
        let img = SyntheticImage::new(&linked_at_4m(true));
        let h = img.headers();
        assert_eq!(h.ehdr().addr() as u64, img.addr());
        assert_eq!(h.phdr().addr() as u64, img.addr() + 64);
        assert_eq!(h.phnum(), 5);
        let all: std::vec::Vec<Phdr> = h.iter().collect();
        assert_eq!(all, linked_at_4m(true));
        assert_eq!(h.get(5), None);
        assert_eq!(h.find(PT_TLS), Some(linked_at_4m(true)[3]));
        assert_eq!(h.find(0x1234_5678), None);
    }

    /// `PT_PHDR` first, as musl and glibc read the bias; the `PT_LOAD` that
    /// maps the file's first byte without one; 0 with neither.
    #[test]
    fn the_load_bias_comes_from_pt_phdr_or_the_first_load() {
        let with = SyntheticImage::new(&linked_at_4m(true));
        assert_eq!(
            with.headers().load_bias(),
            with.addr().wrapping_sub(0x40_0000)
        );
        let without = SyntheticImage::new(&linked_at_4m(false));
        assert_eq!(
            without.headers().load_bias(),
            without.addr().wrapping_sub(0x40_0000)
        );
        let neither = SyntheticImage::new(&[ph(PT_LOAD, 0x1000, 0x40_1000, 8, 8, 8)]);
        assert_eq!(neither.headers().load_bias(), 0);
    }

    #[test]
    fn the_tls_image_is_pt_tls_moved_by_the_bias() {
        let img = SyntheticImage::new(&linked_at_4m(true));
        let bias = img.headers().load_bias();
        assert_eq!(
            img.headers().tls_image(),
            TlsImage {
                init_vaddr: bias.wrapping_add(0x40_1400),
                init_size: 8,
                mem_size: 24,
                align: 8,
            }
        );
        let none = SyntheticImage::new(&[ph(PT_LOAD, 0, 0x40_0000, 8, 8, 8)]);
        assert_eq!(none.headers().tls_image(), TlsImage::EMPTY);
    }

    #[test]
    fn a_table_that_is_not_one_is_refused() {
        let mut img = SyntheticImage::new(&linked_at_4m(true));
        img.set_u16(0x38, 0);
        // SAFETY: the block is an ELF header followed by its table.
        assert!(unsafe { ProgramHeaders::of_ehdr(img.ptr()) }.is_none());
        let mut img = SyntheticImage::new(&linked_at_4m(true));
        img.set_u16(0x36, 32);
        // SAFETY: as above.
        assert!(unsafe { ProgramHeaders::of_ehdr(img.ptr()) }.is_none());
    }

    /// On the host there is no image of our own to read: no headers, and no
    /// TLS image, which is what `image()` answered before it had headers to
    /// read at all.
    #[test]
    fn the_host_has_no_program_headers() {
        assert!(program_headers().is_none());
        assert_eq!(super::image(), TlsImage::EMPTY);
    }
}

/// Test builds: an ELF header and its program header table in one block, as
/// a program's are mapped -- the table at offset 64, 56 bytes an entry.
#[cfg(test)]
pub(crate) struct SyntheticImage {
    /// `u64`s so the block is 8-aligned, as the real one is.
    words: std::vec::Vec<u64>,
}

#[cfg(test)]
impl SyntheticImage {
    /// The image with these program headers.
    pub(crate) fn new(phdrs: &[Phdr]) -> Self {
        let mut b = std::vec![0u8; 64 + PHDR_SIZE * phdrs.len()];
        b[0x20..0x28].copy_from_slice(&64u64.to_le_bytes());
        b[0x36..0x38].copy_from_slice(&u16::try_from(PHDR_SIZE).unwrap().to_le_bytes());
        b[0x38..0x3a].copy_from_slice(&u16::try_from(phdrs.len()).unwrap().to_le_bytes());
        for (i, p) in phdrs.iter().enumerate() {
            let e = &mut b[64 + PHDR_SIZE * i..64 + PHDR_SIZE * (i + 1)];
            e[0..4].copy_from_slice(&p.p_type.to_le_bytes());
            e[8..16].copy_from_slice(&p.p_offset.to_le_bytes());
            e[16..24].copy_from_slice(&p.p_vaddr.to_le_bytes());
            e[24..32].copy_from_slice(&p.p_vaddr.to_le_bytes());
            e[32..40].copy_from_slice(&p.p_filesz.to_le_bytes());
            e[40..48].copy_from_slice(&p.p_memsz.to_le_bytes());
            e[48..56].copy_from_slice(&p.p_align.to_le_bytes());
        }
        let mut words = std::vec![0u64; b.len().div_ceil(8)];
        for (i, byte) in b.iter().enumerate() {
            words[i / 8] |= u64::from(*byte) << (8 * (i % 8));
        }
        Self { words }
    }

    /// Overwrite the `u16` at byte `offset` of the ELF header.
    pub(crate) fn set_u16(&mut self, offset: usize, v: u16) {
        for (k, byte) in v.to_le_bytes().iter().enumerate() {
            let i = offset + k;
            self.words[i / 8] &= !(0xff << (8 * (i % 8)));
            self.words[i / 8] |= u64::from(*byte) << (8 * (i % 8));
        }
    }

    /// The ELF header's address.
    pub(crate) fn ptr(&self) -> *const u8 {
        self.words.as_ptr().cast()
    }

    /// The same, as a number.
    pub(crate) fn addr(&self) -> u64 {
        self.ptr().addr() as u64
    }

    /// The headers, read from the block.
    pub(crate) fn headers(&self) -> ProgramHeaders {
        // SAFETY: the block is an ELF header followed by the table it
        // describes, and lives as long as `self`.
        unsafe { ProgramHeaders::of_ehdr(self.ptr()) }.expect("a synthetic image has a table")
    }
}
