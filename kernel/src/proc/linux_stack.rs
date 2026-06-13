//! System V (Linux) initial process-stack construction — **Linux-ABI only**.
//!
//! A Linux x86_64 binary's `_start` (in glibc/musl, or in a hand-rolled
//! static binary) does **not** receive its arguments in registers.  It
//! expects the kernel to have laid out a *System V initial stack* and to
//! have set `%rsp` pointing at it.  The layout, from low address (where
//! `%rsp` points) to high address, is:
//!
//! ```text
//!   [ %rsp ] argc                     (u64)
//!            argv[0]                   (pointer)
//!            ...
//!            argv[argc-1]
//!            NULL                      (argv terminator)
//!            envp[0]                   (pointer)
//!            ...
//!            NULL                      (envp terminator)
//!            auxv[0].a_type            (u64)   ─┐
//!            auxv[0].a_val             (u64)    │ auxiliary vector
//!            ...                                │ (Elf64_auxv_t pairs)
//!            AT_NULL, 0                        ─┘ (auxv terminator)
//!            [ padding ]               (zero, for 16-byte %rsp alignment)
//!            ── info block ──          (the bytes the pointers above target)
//!            argv/envp strings (NUL-terminated)
//!            16 random bytes           (AT_RANDOM target)
//!   [ stack_top ]
//! ```
//!
//! `%rsp` (pointing at `argc`) must be **16-byte aligned** at process
//! entry, per the x86_64 System V ABI.
//!
//! ## Why this lives in its own module, gated to Linux-ABI processes
//!
//! This is a Linux/System V-ABI construct.  SlateOS *native* processes do
//! **not** get a System V stack at all — they receive argv/envp from the
//! kernel via `SYS_PROCESS_GET_ARGS` and have no auxiliary vector by
//! design (see `posix/src/crt.rs` and design-decision #4 in
//! `design-decisions.md`).  The native launch path
//! ([`crate::proc::spawn::setup_user_stack`]) is never modified to build
//! any of this; it only maps a bare zeroed stack.  This module is invoked
//! **exclusively** on the `AbiMode::Linux` branch of the spawn/exec path,
//! writing the System V layout *into* the already-mapped stack frames.
//! Keeping it isolated here is what prevents the Linux ABI from leaking
//! into the native process model.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::FRAME_SIZE;
use crate::mm::page_table::{self, VirtAddr};
use crate::proc::elf::ElfFile;

// ---------------------------------------------------------------------------
// Auxiliary-vector entry types (the subset the loader populates).
// ---------------------------------------------------------------------------
// Values are fixed by the Linux/SysV ABI (`elf.h`); do not renumber.

/// End-of-vector marker (`a_val` must be 0).
pub const AT_NULL: u64 = 0;
/// Address of the program headers in the process image.
pub const AT_PHDR: u64 = 3;
/// Size in bytes of one program-header entry (`e_phentsize`).
pub const AT_PHENT: u64 = 4;
/// Number of program-header entries (`e_phnum`).
pub const AT_PHNUM: u64 = 5;
/// System page size as seen across the Linux ABI.
pub const AT_PAGESZ: u64 = 6;
/// Base address the program interpreter (ld.so) was loaded at.
pub const AT_BASE: u64 = 7;
/// Flags (always 0 for our targets).
pub const AT_FLAGS: u64 = 8;
/// Entry point of the program (the ELF `e_entry`).
pub const AT_ENTRY: u64 = 9;
/// Real user ID of the process.
pub const AT_UID: u64 = 11;
/// Effective user ID of the process.
pub const AT_EUID: u64 = 12;
/// Real group ID of the process.
pub const AT_GID: u64 = 13;
/// Effective group ID of the process.
pub const AT_EGID: u64 = 14;
/// Hardware-capability bitmask (0 — we advertise no optional CPU features
/// through the aux vector; binaries probe CPUID directly).
pub const AT_HWCAP: u64 = 16;
/// Clock ticks per second (`sysconf(_SC_CLK_TCK)`).
pub const AT_CLKTCK: u64 = 17;
/// Non-zero if the binary should run "secure" (setuid-like). Always 0.
pub const AT_SECURE: u64 = 23;
/// Address of 16 random bytes seeded for the libc stack guard / PRNG.
pub const AT_RANDOM: u64 = 25;
/// Address of a NUL-terminated string naming the executed file.
pub const AT_EXECFN: u64 = 31;

/// Page size reported to Linux binaries through `AT_PAGESZ`.
///
/// This is **4096**, not the native 16 KiB [`FRAME_SIZE`]: the Linux ABI
/// boundary uniformly reports a 4 KiB page (see design-decision #1).  A
/// Linux binary that computes page-aligned addresses from `AT_PAGESZ`
/// must see the same value `sysconf(_SC_PAGESIZE)` reports.
pub const LINUX_ABI_PAGE_SIZE: u64 = 4096;

/// Clock-tick rate reported through `AT_CLKTCK` (100 Hz, matching the
/// `NS_PER_JIFFY` used by `/proc/stat`).
const LINUX_ABI_CLK_TCK: u64 = 100;

/// `PT_LOAD` program-header type (mirrors the private constant in
/// [`crate::proc::elf`]).
const PT_LOAD: u32 = 1;

/// One auxiliary-vector entry (`Elf64_auxv_t`): a type tag and its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxEntry {
    /// The `AT_*` type tag.
    pub a_type: u64,
    /// The associated value (an integer or a user-space pointer).
    pub a_val: u64,
}

impl AuxEntry {
    /// Construct an entry.
    #[must_use]
    pub const fn new(a_type: u64, a_val: u64) -> Self {
        Self { a_type, a_val }
    }
}

/// A fully-built initial-stack image, ready to copy into the user stack.
#[derive(Debug)]
pub struct SysvStackImage {
    /// Raw bytes occupying `[rsp .. stack_top)` of the user stack.
    pub image: Vec<u8>,
    /// Initial user `%rsp` (points at `argc`), 16-byte aligned.
    pub rsp: u64,
    /// The final auxiliary vector as a standalone little-endian
    /// `Elf64_auxv_t` byte stream (`(a_type, a_val)` `u64` pairs ending
    /// in `AT_NULL`), including the appended `AT_RANDOM`/`AT_EXECFN`
    /// entries.  This is the same auxv embedded in `image`, extracted
    /// here so the spawn/exec path can persist it for `PR_GET_AUXV` and
    /// `/proc/<pid>/auxv` without re-parsing the stack.
    pub auxv_bytes: Vec<u8>,
}

/// A byte blob to place in the info block, tracked until `%rsp` is known.
struct Placed {
    /// Absolute user virtual address the blob was placed at.
    addr: u64,
    /// The bytes (e.g. `b"PROG\0"` or the 16 AT_RANDOM bytes).
    bytes: Vec<u8>,
}

/// Build the System V initial-stack image for a Linux-ABI process.
///
/// * `stack_top` — highest stack address (exclusive); the info block is
///   packed downward from here.  For our processes this is
///   [`crate::proc::spawn::USER_STACK_TOP`].
/// * `stack_limit` — lowest writable stack address; the returned `rsp`
///   is guaranteed `>= stack_limit` or [`KernelError::OutOfMemory`] is
///   returned (the arguments do not fit in the mapped stack).
/// * `argv`, `envp` — argument and environment byte strings (no trailing
///   NUL; this function appends it).  Bytes, never `str` — argv/envp may
///   hold any non-NUL byte.
/// * `aux` — auxiliary entries whose values are already final (e.g.
///   `AT_PHDR`, `AT_ENTRY`, `AT_PAGESZ`).  Must **not** contain
///   `AT_RANDOM`, `AT_EXECFN`, or `AT_NULL`: this function appends those
///   (their values point into the info block it lays out).
/// * `random16` — 16 bytes of randomness for `AT_RANDOM`.
///
/// Returns the image bytes and the aligned initial `%rsp`.
///
/// # Errors
///
/// [`KernelError::Overflow`] on address arithmetic overflow (not
/// reachable for a sane stack) and [`KernelError::OutOfMemory`] if the
/// laid-out stack would underflow `stack_limit`.
pub fn build_sysv_stack(
    stack_top: u64,
    stack_limit: u64,
    argv: &[&[u8]],
    envp: &[&[u8]],
    aux: &[AuxEntry],
    random16: &[u8; 16],
) -> KernelResult<SysvStackImage> {
    // ---- Phase 1: place the info-block blobs, packing downward. ----
    //
    // `cursor` is the next address to place *below*.  We subtract the
    // blob length, then the blob occupies [cursor, cursor+len).
    let mut cursor = stack_top;
    let mut placed: Vec<Placed> = Vec::new();

    // place() returns the address the blob landed at.
    let mut place = |cursor: &mut u64, bytes: Vec<u8>| -> KernelResult<u64> {
        let len = bytes.len() as u64;
        let addr = cursor.checked_sub(len).ok_or(KernelError::Overflow)?;
        *cursor = addr;
        placed.push(Placed { addr, bytes });
        Ok(addr)
    };

    // 16 random bytes for AT_RANDOM, just below the top.
    let random_addr = place(&mut cursor, random16.to_vec())?;

    // argv strings (each NUL-terminated). Record their addresses for the
    // pointer array and for AT_EXECFN (argv[0]).
    let mut argv_addrs: Vec<u64> = Vec::with_capacity(argv.len());
    for arg in argv {
        let mut s = arg.to_vec();
        s.push(0);
        argv_addrs.push(place(&mut cursor, s)?);
    }

    // envp strings (each NUL-terminated).
    let mut envp_addrs: Vec<u64> = Vec::with_capacity(envp.len());
    for env in envp {
        let mut s = env.to_vec();
        s.push(0);
        envp_addrs.push(place(&mut cursor, s)?);
    }

    let info_bottom = cursor;

    // ---- Phase 2: assemble the auxiliary vector. ----
    //
    // Caller-supplied entries first, then the ones whose values point
    // into the info block we just built, then the AT_NULL terminator.
    let mut auxv: Vec<AuxEntry> = aux.to_vec();
    auxv.push(AuxEntry::new(AT_RANDOM, random_addr));
    if let Some(&argv0) = argv_addrs.first() {
        // AT_EXECFN names the executed file; argv[0] is the conventional
        // and sufficient target (glibc/musl accept it).
        auxv.push(AuxEntry::new(AT_EXECFN, argv0));
    }
    auxv.push(AuxEntry::new(AT_NULL, 0));

    // Serialize the final auxv as a standalone byte stream (same layout
    // as embedded in the stack image) so callers can persist it.
    let mut auxv_bytes: Vec<u8> = Vec::with_capacity(auxv.len().saturating_mul(16));
    for entry in &auxv {
        auxv_bytes.extend_from_slice(&entry.a_type.to_le_bytes());
        auxv_bytes.extend_from_slice(&entry.a_val.to_le_bytes());
    }

    // ---- Phase 3: size the fixed lower part and align %rsp. ----
    //
    //   argc                        : 8
    //   argv pointers + NULL        : (argc + 1) * 8
    //   envp pointers + NULL        : (envc + 1) * 8
    //   auxv entries (incl AT_NULL) : auxv.len() * 16
    let word = 8u64;
    let argc = argv.len() as u64;
    let envc = envp.len() as u64;
    let argv_words = argc.checked_add(1).ok_or(KernelError::Overflow)?;
    let envp_words = envc.checked_add(1).ok_or(KernelError::Overflow)?;
    let aux_words = (auxv.len() as u64)
        .checked_mul(2)
        .ok_or(KernelError::Overflow)?;
    let total_words = argv_words
        .checked_add(envp_words)
        .and_then(|w| w.checked_add(aux_words))
        .and_then(|w| w.checked_add(1)) // argc itself
        .ok_or(KernelError::Overflow)?;
    let fixed_size = total_words.checked_mul(word).ok_or(KernelError::Overflow)?;

    // rsp = align_down(info_bottom - fixed_size, 16).
    let unaligned = info_bottom
        .checked_sub(fixed_size)
        .ok_or(KernelError::Overflow)?;
    let rsp = unaligned & !0xFu64;
    if rsp < stack_limit {
        // The arguments/environment do not fit in the mapped stack.
        return Err(KernelError::OutOfMemory);
    }

    // ---- Phase 4: render the image covering [rsp, stack_top). ----
    let image_len = stack_top.checked_sub(rsp).ok_or(KernelError::Overflow)? as usize;
    let mut image = vec![0u8; image_len];

    // Helper: write `bytes` at absolute user address `addr`.
    let write_at = |image: &mut [u8], addr: u64, bytes: &[u8]| -> KernelResult<()> {
        let off = addr.checked_sub(rsp).ok_or(KernelError::Overflow)? as usize;
        let end = off.checked_add(bytes.len()).ok_or(KernelError::Overflow)?;
        let slot = image.get_mut(off..end).ok_or(KernelError::InvalidAddress)?;
        slot.copy_from_slice(bytes);
        Ok(())
    };
    let write_u64 = |image: &mut [u8], addr: u64, val: u64| -> KernelResult<()> {
        write_at(image, addr, &val.to_le_bytes())
    };

    // Info-block blobs.
    for p in &placed {
        write_at(&mut image, p.addr, &p.bytes)?;
    }

    // Fixed part, low → high, starting at rsp.
    let mut at = rsp;
    write_u64(&mut image, at, argc)?;
    at = at.checked_add(word).ok_or(KernelError::Overflow)?;
    for &a in &argv_addrs {
        write_u64(&mut image, at, a)?;
        at = at.checked_add(word).ok_or(KernelError::Overflow)?;
    }
    write_u64(&mut image, at, 0)?; // argv NULL terminator
    at = at.checked_add(word).ok_or(KernelError::Overflow)?;
    for &e in &envp_addrs {
        write_u64(&mut image, at, e)?;
        at = at.checked_add(word).ok_or(KernelError::Overflow)?;
    }
    write_u64(&mut image, at, 0)?; // envp NULL terminator
    at = at.checked_add(word).ok_or(KernelError::Overflow)?;
    for entry in &auxv {
        write_u64(&mut image, at, entry.a_type)?;
        at = at.checked_add(word).ok_or(KernelError::Overflow)?;
        write_u64(&mut image, at, entry.a_val)?;
        at = at.checked_add(word).ok_or(KernelError::Overflow)?;
    }
    // [at, info_bottom) remains zero padding (already zeroed).

    Ok(SysvStackImage { image, rsp, auxv_bytes })
}

/// Compute the in-memory virtual address of the program-header table.
///
/// glibc/musl read `AT_PHDR` to locate the program headers (for TLS,
/// `PT_GNU_RELRO`, etc.).  The headers live in the file at byte offset
/// `e_phoff`; whichever `PT_LOAD` segment maps that file range exposes
/// them at `p_vaddr + (e_phoff - p_offset)`.  Returns `None` if no loaded
/// segment covers the header table (then `AT_PHDR` is simply omitted).
fn phdr_vaddr(elf: &ElfFile<'_>) -> Option<u64> {
    let phoff = elf.header.e_phoff;
    let phdr_table_len = u64::from(elf.header.e_phentsize)
        .checked_mul(u64::from(elf.header.e_phnum))?;
    let phoff_end = phoff.checked_add(phdr_table_len)?;
    for i in 0..elf.program_header_count() {
        let ph = elf.program_header(i)?;
        if ph.p_type != PT_LOAD {
            continue;
        }
        let seg_end = ph.p_offset.checked_add(ph.p_filesz)?;
        if phoff >= ph.p_offset && phoff_end <= seg_end {
            return ph.p_vaddr.checked_add(phoff.checked_sub(ph.p_offset)?);
        }
    }
    None
}

/// Assemble the standard auxiliary vector for a loaded Linux executable.
///
/// Does not include `AT_RANDOM`/`AT_EXECFN`/`AT_NULL` — [`build_sysv_stack`]
/// appends those.  Identity is reported as uid/gid 0 for now (we have no
/// per-process credential model exposed to the Linux ABI yet); revisit
/// when real credentials land.
///
/// `interp_base` is the base address the program interpreter (`ld.so`)
/// was loaded at, or `None` for a statically-linked executable (no
/// interpreter).  When `Some`, an `AT_BASE` entry is emitted so the
/// loader can relocate itself; `AT_ENTRY` always remains the
/// **executable's** entry (the real program entry the loader jumps to
/// once relocation is done), never the interpreter's.
///
/// `exec_load_bias` is the address the executable image itself was
/// loaded at relative to its link-time vaddrs.  It is `0` for an
/// `ET_EXEC` (fixed-address) binary and the chosen load base for an
/// `ET_DYN`/PIE executable.  Both `AT_ENTRY` (= `e_entry + bias`) and
/// `AT_PHDR` (= program-header vaddr + bias) must report the *runtime*
/// address so glibc/musl and the dynamic loader find the headers and
/// entry where they were actually mapped.
fn base_auxv(elf: &ElfFile<'_>, interp_base: Option<u64>, exec_load_bias: u64) -> Vec<AuxEntry> {
    // Auxv entry order is irrelevant to libc (it scans by `a_type`), so the
    // unconditional entries are built as one literal and the optional
    // `AT_PHDR` is appended afterwards.
    //
    // saturating_add for the bias: the loaded image was already validated
    // to satisfy `bias + p_vaddr + p_memsz <= USER_SPACE_END` when its
    // segments were mapped, so neither sum can actually overflow; saturate
    // rather than panic to stay clippy `arithmetic_side_effects`-clean.
    let mut aux = vec![
        AuxEntry::new(AT_PAGESZ, LINUX_ABI_PAGE_SIZE),
        AuxEntry::new(AT_PHENT, u64::from(elf.header.e_phentsize)),
        AuxEntry::new(AT_PHNUM, u64::from(elf.header.e_phnum)),
        AuxEntry::new(AT_ENTRY, elf.header.e_entry.saturating_add(exec_load_bias)),
        AuxEntry::new(AT_FLAGS, 0),
        AuxEntry::new(AT_HWCAP, 0),
        AuxEntry::new(AT_CLKTCK, LINUX_ABI_CLK_TCK),
        AuxEntry::new(AT_SECURE, 0),
        AuxEntry::new(AT_UID, 0),
        AuxEntry::new(AT_EUID, 0),
        AuxEntry::new(AT_GID, 0),
        AuxEntry::new(AT_EGID, 0),
    ];
    if let Some(phdr) = phdr_vaddr(elf) {
        aux.push(AuxEntry::new(AT_PHDR, phdr.saturating_add(exec_load_bias)));
    }
    // AT_BASE: where the program interpreter (ld.so) was mapped.  Omitted
    // entirely for static binaries — glibc/musl treat a missing AT_BASE
    // as "no interpreter", which is correct for a fully-static image.
    if let Some(base) = interp_base {
        aux.push(AuxEntry::new(AT_BASE, base));
    }
    aux
}

/// Copy `bytes` into the user address space at `vaddr`, page by page.
///
/// Walks the target page table to resolve each page's physical frame and
/// writes through the HHDM mapping.  The destination pages must already
/// be mapped present + writable (the stack frames mapped by
/// `setup_user_stack`).
///
/// # Safety
///
/// `pml4_phys` must be a valid PML4 for the target process, and
/// `[vaddr, vaddr + bytes.len())` must lie within present, writable,
/// user pages that no other CPU is concurrently mutating.
unsafe fn write_user_image(pml4_phys: u64, vaddr: u64, bytes: &[u8]) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    let frame_size = FRAME_SIZE as u64;
    let mut written: u64 = 0;
    let total = bytes.len() as u64;
    while written < total {
        let cur = vaddr.checked_add(written).ok_or(KernelError::Overflow)?;
        let phys = page_table::translate(pml4_phys, VirtAddr::new(cur))
            .ok_or(KernelError::InvalidAddress)?;
        let page_off = cur % frame_size;
        let in_page = frame_size.checked_sub(page_off).ok_or(KernelError::Overflow)?;
        let remaining = total.checked_sub(written).ok_or(KernelError::Overflow)?;
        let n = in_page.min(remaining);
        let src_off = written as usize;
        let src_end = src_off
            .checked_add(n as usize)
            .ok_or(KernelError::Overflow)?;
        let src = bytes.get(src_off..src_end).ok_or(KernelError::InvalidAddress)?;
        let dst = phys.checked_add(hhdm).ok_or(KernelError::Overflow)? as *mut u8;
        // SAFETY: `phys` is the physical address backing the mapped,
        // writable user page `cur`; `dst` is its HHDM alias.  `n` bytes
        // fit within this page (bounded by `in_page`).  `src` is a valid
        // slice of `bytes`.  Source and destination cannot overlap (one
        // is a kernel-built buffer, the other a user stack frame).
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst, n as usize);
        }
        written = written.checked_add(n).ok_or(KernelError::Overflow)?;
    }
    Ok(())
}

/// Outcome of installing a Linux SysV initial stack: the entry `%rsp`
/// and a standalone copy of the auxv to persist in the process's
/// Linux-ABI state (for `PR_GET_AUXV` / `/proc/<pid>/auxv`).
#[derive(Debug)]
pub struct InstalledLinuxStack {
    /// Aligned initial user `%rsp` (points at `argc`).
    pub rsp: u64,
    /// Raw `Elf64_auxv_t` byte stream — see [`SysvStackImage::auxv_bytes`].
    pub auxv_bytes: Vec<u8>,
}

/// Build the System V initial stack for a Linux-ABI process and install
/// it into the (already-mapped) user stack frames.
///
/// Invoked **only** on the `AbiMode::Linux` branch of the spawn/exec
/// path, after `setup_user_stack` has mapped the bare stack.  Returns the
/// aligned initial `%rsp` to hand to the entry trampoline, plus the
/// serialized auxv for the caller to persist.
///
/// * `stack_top` / `stack_limit` — bounds of the mapped user stack.
/// * `elf` — the parsed executable (for the program-header auxv values).
/// * `argv` / `envp` — byte-string arguments and environment.
/// * `random16` — 16 bytes of randomness for `AT_RANDOM`.
/// * `interp_base` — base address the program interpreter (`ld.so`) was
///   loaded at, or `None` for a static executable.  Forwarded to the
///   auxv as `AT_BASE`.
/// * `exec_load_bias` — load base of the executable image itself (`0`
///   for `ET_EXEC`, the chosen base for an `ET_DYN`/PIE executable).
///   Biases `AT_ENTRY` and `AT_PHDR` to their runtime addresses.
///
/// # Errors
///
/// Propagates [`build_sysv_stack`] errors and any failure resolving or
/// writing the stack pages.
#[allow(clippy::too_many_arguments)] // SysV stack inputs are irreducibly many.
pub fn install_linux_stack(
    pml4_phys: u64,
    stack_top: u64,
    stack_limit: u64,
    elf: &ElfFile<'_>,
    argv: &[&[u8]],
    envp: &[&[u8]],
    random16: &[u8; 16],
    interp_base: Option<u64>,
    exec_load_bias: u64,
) -> KernelResult<InstalledLinuxStack> {
    let aux = base_auxv(elf, interp_base, exec_load_bias);
    let built = build_sysv_stack(stack_top, stack_limit, argv, envp, &aux, random16)?;
    // SAFETY: the stack frames [stack_limit, stack_top) were just mapped
    // present + writable by `setup_user_stack`, and `built.image` spans
    // exactly [built.rsp, stack_top) with built.rsp >= stack_limit.  This
    // thread is the sole accessor of the freshly-created address space.
    unsafe {
        write_user_image(pml4_phys, built.rsp, &built.image)?;
    }
    Ok(InstalledLinuxStack {
        rsp: built.rsp,
        auxv_bytes: built.auxv_bytes,
    })
}

// ---------------------------------------------------------------------------
// Boot-time self-test
// ---------------------------------------------------------------------------
//
// The kernel is `#![no_std]`/`#![no_main]`, so host `#[test]` functions never
// run.  Verification happens at boot via `self_test()`, which returns
// `Err(KernelError::InternalError)` (after a `[linux_stack] FAIL: …` line)
// instead of panicking.

/// Read a little-endian u64 from the image at user address `addr`, or
/// `None` if `[addr, addr+8)` falls outside `[rsp, stack_top)`.
fn st_read_u64(img: &SysvStackImage, stack_top: u64, addr: u64) -> Option<u64> {
    if addr < img.rsp || addr.checked_add(8)? > stack_top {
        return None;
    }
    let off = (addr - img.rsp) as usize;
    let slice = img.image.get(off..off.checked_add(8)?)?;
    let mut b = [0u8; 8];
    b.copy_from_slice(slice);
    Some(u64::from_le_bytes(b))
}

/// Read `len` bytes from the image at user address `addr`, or `None` if
/// out of range.
fn st_read_bytes(img: &SysvStackImage, addr: u64, len: usize) -> Option<Vec<u8>> {
    if addr < img.rsp {
        return None;
    }
    let off = (addr - img.rsp) as usize;
    let end = off.checked_add(len)?;
    Some(img.image.get(off..end)?.to_vec())
}

/// Run the System V initial-stack builder self-tests.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if any invariant of
/// [`build_sysv_stack`] is violated.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    const TOP: u64 = 0x0000_7FFF_FFFF_0000;
    const LIMIT: u64 = TOP - 0x10000; // 64 KiB stack

    /// Fail helper: log and return Err.
    macro_rules! fail {
        ($($arg:tt)*) => {{
            serial_println!($($arg)*);
            return Err(KernelError::InternalError);
        }};
    }
    /// Require a condition, failing with a message otherwise.
    macro_rules! require {
        ($cond:expr, $($arg:tt)*) => {
            if !($cond) {
                fail!($($arg)*);
            }
        };
    }
    /// Unwrap an Option from a stack read, failing on None.
    macro_rules! read_u64 {
        ($img:expr, $addr:expr) => {
            match st_read_u64($img, TOP, $addr) {
                Some(v) => v,
                None => fail!("[linux_stack] FAIL: out-of-range u64 read at {:#x}", $addr),
            }
        };
    }
    macro_rules! read_bytes {
        ($img:expr, $addr:expr, $len:expr) => {
            match st_read_bytes($img, $addr, $len) {
                Some(v) => v,
                None => fail!("[linux_stack] FAIL: out-of-range byte read at {:#x}", $addr),
            }
        };
    }

    // --- Test 1: rsp is 16-byte aligned and within the stack. ---
    {
        let rnd = [7u8; 16];
        let img = build_sysv_stack(TOP, LIMIT, &[b"a"], &[], &[], &rnd)?;
        require!(img.rsp % 16 == 0, "[linux_stack] FAIL: rsp not 16-byte aligned");
        require!(
            img.rsp >= LIMIT && img.rsp < TOP,
            "[linux_stack] FAIL: rsp outside stack bounds"
        );
    }

    // --- Test 2: argc and argv pointers. ---
    {
        let rnd = [0u8; 16];
        let argv: &[&[u8]] = &[b"prog", b"arg1", b"second"];
        let img = build_sysv_stack(TOP, LIMIT, argv, &[], &[], &rnd)?;
        require!(read_u64!(&img, img.rsp) == 3, "[linux_stack] FAIL: argc != 3");
        for (i, want) in argv.iter().enumerate() {
            let ptr = read_u64!(&img, img.rsp + 8 + (i as u64) * 8);
            let mut expect = want.to_vec();
            expect.push(0);
            require!(
                read_bytes!(&img, ptr, expect.len()) == expect,
                "[linux_stack] FAIL: argv[{}] string mismatch",
                i
            );
        }
        let null = read_u64!(&img, img.rsp + 8 + 3 * 8);
        require!(null == 0, "[linux_stack] FAIL: argv not NULL-terminated");
    }

    // --- Test 3: envp pointers and terminator. ---
    {
        let rnd = [0u8; 16];
        let argv: &[&[u8]] = &[b"p"];
        let envp: &[&[u8]] = &[b"HOME=/root", b"PATH=/bin"];
        let img = build_sysv_stack(TOP, LIMIT, argv, envp, &[], &rnd)?;
        // argc(1) + argv ptr(1) + argv NULL(1) => envp starts at rsp + 24.
        let envp_base = img.rsp + 24;
        for (i, want) in envp.iter().enumerate() {
            let ptr = read_u64!(&img, envp_base + (i as u64) * 8);
            let mut expect = want.to_vec();
            expect.push(0);
            require!(
                read_bytes!(&img, ptr, expect.len()) == expect,
                "[linux_stack] FAIL: envp[{}] string mismatch",
                i
            );
        }
        let null = read_u64!(&img, envp_base + 2 * 8);
        require!(null == 0, "[linux_stack] FAIL: envp not NULL-terminated");
    }

    // --- Test 4: auxv includes AT_PAGESZ, AT_RANDOM, AT_EXECFN, AT_NULL. ---
    {
        let rnd = [0xABu8; 16];
        let argv: &[&[u8]] = &[b"prog"];
        let aux = &[AuxEntry::new(AT_PAGESZ, LINUX_ABI_PAGE_SIZE)];
        let img = build_sysv_stack(TOP, LIMIT, argv, &[], aux, &rnd)?;
        // auxv starts at rsp + argc(8) + (1 argv + NULL)(16) + (0 envp + NULL)(8).
        let mut at = img.rsp + 8 + 16 + 8;
        let mut seen_pagesz = false;
        let mut seen_random = false;
        let mut seen_execfn = false;
        loop {
            let ty = read_u64!(&img, at);
            let val = read_u64!(&img, at + 8);
            at += 16;
            match ty {
                AT_PAGESZ => {
                    require!(
                        val == LINUX_ABI_PAGE_SIZE,
                        "[linux_stack] FAIL: AT_PAGESZ != 4096"
                    );
                    seen_pagesz = true;
                }
                AT_RANDOM => {
                    require!(
                        read_bytes!(&img, val, 16) == vec![0xABu8; 16],
                        "[linux_stack] FAIL: AT_RANDOM bytes mismatch"
                    );
                    seen_random = true;
                }
                AT_EXECFN => {
                    require!(
                        read_bytes!(&img, val, 5) == b"prog\0".to_vec(),
                        "[linux_stack] FAIL: AT_EXECFN string mismatch"
                    );
                    seen_execfn = true;
                }
                AT_NULL => {
                    // Reaching here means the AT_NULL terminator was found
                    // (the only way to break out of the loop), so its
                    // presence need not be tracked in a separate flag.
                    require!(val == 0, "[linux_stack] FAIL: AT_NULL a_val != 0");
                    break;
                }
                _ => {}
            }
            require!(at < TOP, "[linux_stack] FAIL: auxv missing AT_NULL");
        }
        require!(
            seen_pagesz && seen_random && seen_execfn,
            "[linux_stack] FAIL: missing required auxv entry"
        );
    }

    // --- Test 5: empty argv/envp. ---
    {
        let rnd = [1u8; 16];
        let img = build_sysv_stack(TOP, LIMIT, &[], &[], &[], &rnd)?;
        require!(read_u64!(&img, img.rsp) == 0, "[linux_stack] FAIL: argc != 0");
        require!(
            read_u64!(&img, img.rsp + 8) == 0,
            "[linux_stack] FAIL: argv NULL missing"
        );
        require!(
            read_u64!(&img, img.rsp + 16) == 0,
            "[linux_stack] FAIL: envp NULL missing"
        );
        // With empty argv there is no AT_EXECFN; first aux pair is AT_RANDOM.
        require!(
            read_u64!(&img, img.rsp + 24) == AT_RANDOM,
            "[linux_stack] FAIL: expected AT_RANDOM first with empty argv"
        );
    }

    // --- Test 6: oversized argument returns OutOfMemory. ---
    {
        let rnd = [0u8; 16];
        let huge = vec![b'x'; 0x20000];
        let argv: &[&[u8]] = &[&huge];
        match build_sysv_stack(TOP, LIMIT, argv, &[], &[], &rnd) {
            Err(KernelError::OutOfMemory) => {}
            other => fail!(
                "[linux_stack] FAIL: oversized args expected OutOfMemory, got {:?}",
                other.map(|_| ())
            ),
        }
    }

    // --- Test 7: all argv/envp pointers lie within [rsp, TOP). ---
    {
        let rnd = [0u8; 16];
        let argv: &[&[u8]] = &[b"prog", b"x"];
        let envp: &[&[u8]] = &[b"A=b"];
        let img = build_sysv_stack(TOP, LIMIT, argv, envp, &[], &rnd)?;
        for i in 0..2u64 {
            let p = read_u64!(&img, img.rsp + 8 + i * 8);
            require!(
                p >= img.rsp && p < TOP,
                "[linux_stack] FAIL: argv pointer outside stack"
            );
        }
    }

    // --- Test 8: AT_BASE (dynamic loader base) round-trips into the
    // stack image when present, and is absent when the executable is
    // static.  This is the auxv path the ld.so base travels: a Linux
    // loader reads AT_BASE to relocate itself, so the value the kernel
    // computes must land verbatim in the userspace auxv. ---
    // Arithmetic here walks fixed compile-time stack offsets that cannot
    // overflow; raw `+` matches the other tests in this harness.
    #[allow(clippy::arithmetic_side_effects)]
    {
        let rnd = [0u8; 16];
        let argv: &[&[u8]] = &[b"prog"];
        const FAKE_BASE: u64 = 0x0000_5555_5555_0000;
        let with_base = &[AuxEntry::new(AT_BASE, FAKE_BASE)];
        let img = build_sysv_stack(TOP, LIMIT, argv, &[], with_base, &rnd)?;
        // auxv starts at rsp + argc(8) + (1 argv + NULL)(16) + (0 envp + NULL)(8).
        let mut at = img.rsp + 8 + 16 + 8;
        let mut found_base = false;
        loop {
            let ty = read_u64!(&img, at);
            let val = read_u64!(&img, at + 8);
            at += 16;
            if ty == AT_BASE {
                require!(
                    val == FAKE_BASE,
                    "[linux_stack] FAIL: AT_BASE value mismatch"
                );
                found_base = true;
            }
            if ty == AT_NULL {
                break;
            }
            require!(at < TOP, "[linux_stack] FAIL: auxv missing AT_NULL (AT_BASE test)");
        }
        require!(found_base, "[linux_stack] FAIL: AT_BASE not present when supplied");

        // Without an AT_BASE input entry, none must appear.
        let img2 = build_sysv_stack(TOP, LIMIT, argv, &[], &[], &rnd)?;
        let mut at2 = img2.rsp + 8 + 16 + 8;
        loop {
            let ty = read_u64!(&img2, at2);
            at2 += 16;
            require!(ty != AT_BASE, "[linux_stack] FAIL: AT_BASE present for static image");
            if ty == AT_NULL {
                break;
            }
            require!(at2 < TOP, "[linux_stack] FAIL: auxv missing AT_NULL (static test)");
        }
    }

    // --- Test 9: exec_load_bias shifts AT_ENTRY and AT_PHDR by exactly
    // the bias.  This is the auxv path a PIE (ET_DYN) executable's load
    // base travels: when the kernel loads the image at a non-zero base,
    // glibc/musl and ld.so must see the program headers and entry at
    // their *runtime* addresses, or TLS setup and relocation jump to the
    // wrong place.  We compare base_auxv at bias 0 vs a fake non-zero
    // bias and require the AT_ENTRY/AT_PHDR deltas to equal the bias. ---
    {
        let elf_data = crate::proc::elf::build_test_elf_public();
        let elf = crate::proc::elf::ElfFile::parse(&elf_data)?;
        const FAKE_BIAS: u64 = 0x0000_5555_5555_4000;

        let aux0 = base_auxv(&elf, None, 0);
        let auxb = base_auxv(&elf, None, FAKE_BIAS);

        let find = |aux: &[AuxEntry], ty: u64| -> Option<u64> {
            aux.iter().find(|e| e.a_type == ty).map(|e| e.a_val)
        };

        // AT_ENTRY is always present (every executable has an entry).
        let (Some(entry0), Some(entryb)) =
            (find(&aux0, AT_ENTRY), find(&auxb, AT_ENTRY))
        else {
            fail!("[linux_stack] FAIL: AT_ENTRY missing from base_auxv");
        };
        require!(
            entryb.wrapping_sub(entry0) == FAKE_BIAS,
            "[linux_stack] FAIL: AT_ENTRY not shifted by exec_load_bias"
        );
        // The unbiased AT_ENTRY must equal the raw e_entry.
        require!(
            entry0 == elf.header.e_entry,
            "[linux_stack] FAIL: AT_ENTRY at bias 0 != e_entry"
        );

        // AT_PHDR is optional (only when a PT_LOAD covers the header
        // table), but the test ELF does carry it; if present in both, the
        // delta must also equal the bias.
        if let (Some(phdr0), Some(phdrb)) =
            (find(&aux0, AT_PHDR), find(&auxb, AT_PHDR))
        {
            require!(
                phdrb.wrapping_sub(phdr0) == FAKE_BIAS,
                "[linux_stack] FAIL: AT_PHDR not shifted by exec_load_bias"
            );
        }
    }

    serial_println!("[linux_stack] SysV initial-stack self-test PASSED");
    Ok(())
}
