//! Kernel symbol table — resolve addresses to function names.
//!
//! Provides address-to-symbol lookup for kernel backtraces, crash
//! diagnostics, and profiling.  Without this, backtraces show only
//! raw addresses like `0xffffffff80103456` which require manual
//! cross-referencing with the linker map.
//!
//! ## How Symbols Are Loaded
//!
//! The kernel ELF binary contains a `.symtab` section with all function
//! symbols (names, addresses, sizes).  During boot, we scan the kernel
//! ELF loaded by the bootloader (via Limine's kernel file response)
//! and extract function symbols into a sorted array.
//!
//! ## Symbol Lookup
//!
//! Given an address, binary search finds the symbol whose address range
//! contains it.  Returns `Some("function_name+0x<offset>")` or `None`
//! if the address doesn't fall within any known symbol.
//!
//! ## Memory Usage
//!
//! One heap allocation, sized exactly: 16 bytes (address, size, name index)
//! per indexed symbol, ~126,000 of them, so ~2 MiB.
//!
//! **Names are not copied.** `name_offset` indexes the kernel ELF's own
//! `.strtab`, which the table borrows in place. This is not a micro-
//! optimisation: names dominate a symbol table, and concatenating them into
//! an owned buffer asked the heap for ~19 MiB in a single allocation and
//! panicked the kernel outright
//! (`memory allocation of 20160000 bytes failed`).
//!
//! That request could never have succeeded, at any point in boot and with
//! any amount of free memory. `mm::heap` serves anything too big for a slab
//! straight from the buddy allocator, whose `MAX_ORDER` of 10 caps one
//! allocation at 2^10 x 16 KiB = **16 MiB**; 19 MiB rounds to order 11 and
//! is refused on arithmetic, not on availability. The frame allocator had
//! 2.9 GiB free at the time. Any future change here that reintroduces a
//! single multi-megabyte allocation must respect that ceiling.
//!
//! Borrowing is sound because Limine's kernel-file mapping is permanent —
//! see [`SymbolTable`].
//!
//! The old estimate here read "a typical kernel has ~2000-5000 functions …
//! total ~100-200 KiB". This kernel has ~124,000, and that 25-60x error is
//! precisely what hid the allocation problem until `.symtab` was restored to
//! the boot image and this module ran for the first time in a long while.
//!
//! Both function symbols (`STT_FUNC`) and data symbols (`STT_OBJECT`) are
//! indexed. Data symbols are what turn a reported lock *address* into a lock
//! *name*: most locks in the tree take `sync::Mutex::new`'s default name of
//! `"?"`, so `lockdep`'s violation reports and `sync`'s spin-stall reports
//! identify a lock by the address of the static it lives in, and without a
//! data symbol that address resolves to nothing. They are also cheap — this
//! kernel has ~124k function symbols and ~2.3k data symbols, so including
//! them costs under 2% more entries and, since names are borrowed rather
//! than copied, nothing at all beyond that.
//!
//! Mixing the two in one address-ordered table is safe because [`lookup`]
//! bounds each hit by the symbol's own `st_size`: an address inside `.text`
//! cannot land on a `.data` symbol, and one inside `.data` cannot land on
//! the last function of `.text`.
//!
//! [`lookup`]: SymbolTable::lookup
//!
//! ## Limitations
//!
//! - Symbols without a size are given a default size of 1 byte.
//! - If the kernel is stripped, no symbols will be available. Note that a
//!   bare `llvm-strip` does this: `scripts/boot-test.sh` must use
//!   `--strip-debug`, which drops DWARF but keeps `.symtab`.
//!
//! ## References
//!
//! - Linux `kernel/kallsyms.c` — compressed symbol table
//! - ELF specification §4.6 (Symbol Table)

#![allow(dead_code)]

use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// ELF definitions (minimal, just what we need for symbol parsing)
// ---------------------------------------------------------------------------

/// ELF64 header.
#[repr(C)]
struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// ELF64 section header.
#[repr(C)]
struct Elf64SectionHeader {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
}

/// ELF64 symbol table entry.
#[repr(C)]
struct Elf64Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

// ELF section types.
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;

// ELF symbol types (low 4 bits of st_info).
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;

// ---------------------------------------------------------------------------
// Symbol storage
// ---------------------------------------------------------------------------

/// A single kernel symbol entry.
#[derive(Clone)]
struct KernelSymbol {
    /// Virtual address of the symbol start.
    addr: u64,
    /// Size in bytes (0 if unknown).
    size: u32,
    /// Byte index of this symbol's name in the kernel ELF's `.strtab`.
    ///
    /// An index into the ELF itself, not into any copy of it — see
    /// [`SymbolTable::strtab`].
    name_offset: u32,
}

/// Global symbol table.
static SYMBOLS: Mutex<SymbolTable> = Mutex::new(SymbolTable::empty());

/// Lock-free view of [`SYMBOLS`], published once the table is filled and
/// valid for the rest of the kernel's life.
///
/// [`resolve`] takes `SYMBOLS.lock()` and returns an allocated `String`.
/// Neither is permissible in the places that most need a symbol name — the
/// panic handler, the spin-stall reporter, and lockdep's violation reporter
/// — because each of those runs *while a lock is held or contended*, so
/// both `Mutex::lock` and the heap's own lock re-enter the very machinery
/// being reported on.
///
/// lockdep is the sharpest case. `SYMBOLS.lock()` called from inside
/// `report_violation` would register a new acquisition in the dependency
/// graph currently being walked; if that acquisition itself raised a
/// violation, `report_violation` would recurse with `SYMBOLS` already held
/// and spin forever — a hang in the one code path whose entire job is to
/// diagnose hangs.
///
/// The pointer is safe to follow without a lock because the table is
/// **write-once**: [`parse_elf_symbols`] fills it exactly once and nothing
/// mutates it afterwards, so the `Vec` buffers never reallocate and the
/// pointer never dangles. It addresses the interior of a `static`, which
/// outlives every caller.
static SNAPSHOT: core::sync::atomic::AtomicPtr<SymbolTable> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Whether symbols have been loaded.
static LOADED: AtomicBool = AtomicBool::new(false);

/// Number of symbols loaded.
static SYMBOL_COUNT: AtomicUsize = AtomicUsize::new(0);

struct SymbolTable {
    /// Sorted by address (for binary search).
    entries: Vec<KernelSymbol>,
    /// The kernel ELF's `.strtab`, borrowed in place — **not** copied.
    ///
    /// Names are the overwhelming bulk of a symbol table: the kernel ELF's
    /// `.symtab` holds ~1,008,000 entries, and even the ~126,000 that survive
    /// this module's filter carry Rust-mangled names averaging well over a
    /// hundred bytes. Concatenating them into an owned `Vec<u8>` (which is
    /// what this used to do) asked the *early-boot* heap for ~19 MiB in one
    /// allocation and panicked the kernel with
    /// `memory allocation of 20160000 bytes failed`.
    ///
    /// Borrowing is sound because Limine's kernel-file mapping is permanent:
    /// the frame allocator seeds its free lists from `USABLE` regions only
    /// (see `mm::frame`), so `BOOTLOADER_RECLAIMABLE` — which is where the
    /// kernel file lives — is never handed out and never reused. `'static`
    /// is therefore honest, not a convenience cast.
    strtab: &'static [u8],
}

impl SymbolTable {
    const fn empty() -> Self {
        Self {
            entries: Vec::new(),
            strtab: &[],
        }
    }

    /// Get the name for a symbol entry.
    ///
    /// `name_offset` indexes the ELF's own `.strtab`, where names are
    /// NUL-terminated, so the terminator scan is over borrowed bytes.
    fn name_of(&self, sym: &KernelSymbol) -> &'static str {
        let start = sym.name_offset as usize;
        let Some(rest) = self.strtab.get(start..) else {
            return "<invalid>";
        };
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let Some(bytes) = rest.get(..end) else {
            return "<invalid>";
        };
        core::str::from_utf8(bytes).unwrap_or("<invalid utf8>")
    }

    /// Binary search for the symbol containing `addr`.
    fn lookup(&self, addr: u64) -> Option<(&KernelSymbol, u64)> {
        if self.entries.is_empty() {
            return None;
        }

        // Binary search: find the last symbol with addr <= target.
        let idx = match self.entries.binary_search_by_key(&addr, |s| s.addr) {
            Ok(i) => i,
            Err(0) => return None, // Address before first symbol.
            Err(i) => i - 1,
        };

        let sym = &self.entries[idx];
        let offset = addr.saturating_sub(sym.addr);

        // Check if the address falls within the symbol's range.
        if sym.size > 0 && offset >= u64::from(sym.size) {
            return None; // Past the end of this symbol.
        }

        // If size is 0, accept any offset up to the next symbol.
        if sym.size == 0 {
            if let Some(next) = self.entries.get(idx + 1) {
                if addr >= next.addr {
                    return None;
                }
            }
        }

        Some((sym, offset))
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the kernel symbol table from the loaded kernel ELF.
///
/// Scans the kernel binary (via Limine's kernel file response) for
/// the `.symtab` section and extracts function symbols.
///
/// This must be called after the heap is initialized (allocates Vec).
pub fn init() {
    let kernel_file = crate::boot::kernel_file_address();
    let (base, size) = match kernel_file {
        Some((b, s)) => (b, s),
        None => {
            serial_println!("[ksyms] No kernel file available — symbols unavailable");
            return;
        }
    };

    if size < core::mem::size_of::<Elf64Header>() {
        serial_println!("[ksyms] Kernel file too small for ELF header");
        return;
    }

    // SAFETY: Limine guarantees the kernel file mapping is valid, and it is
    // *permanent*: the mapping lives in a `BOOTLOADER_RECLAIMABLE` region,
    // and the frame allocator seeds its free lists from `USABLE` regions
    // only (`mm::frame`), so this memory is never handed out and never
    // overwritten. `'static` is therefore accurate — which matters, because
    // the symbol table borrows names out of this slice rather than copying
    // them.
    let elf_bytes: &'static [u8] = unsafe { core::slice::from_raw_parts(base as *const u8, size) };

    // Validate ELF magic.
    if elf_bytes.get(0..4) != Some(b"\x7fELF") {
        serial_println!("[ksyms] Kernel file is not a valid ELF");
        return;
    }

    match parse_elf_symbols(elf_bytes) {
        Some(count) => {
            LOADED.store(true, Ordering::Release);
            SYMBOL_COUNT.store(count, Ordering::Relaxed);
            serial_println!("[ksyms] Loaded {count} code and data symbols");
        }
        None => {
            serial_println!("[ksyms] No symbol table found in kernel ELF");
        }
    }
}

/// Resolve a kernel address to a symbol name + offset.
///
/// Returns `Some("function_name+0x<offset>")` or `None`.
/// This is O(log n) via binary search.
#[must_use]
pub fn resolve(addr: u64) -> Option<String> {
    if !LOADED.load(Ordering::Acquire) {
        return None;
    }

    let table = SYMBOLS.lock();
    table.lookup(addr).map(|(sym, offset)| {
        let name = table.name_of(sym);
        if offset == 0 {
            String::from(name)
        } else {
            alloc::format!("{}+{:#x}", name, offset)
        }
    })
}

/// Resolve a kernel address to a symbol name and offset **without taking a
/// lock and without allocating**.
///
/// This is the form to use from a panic handler, a spin-stall report, or a
/// lockdep violation report — anywhere that runs while a lock is held or
/// contended, where [`resolve`]'s `SYMBOLS.lock()` and `String` allocation
/// would re-enter the machinery being diagnosed. See [`SNAPSHOT`] for why
/// reading the table unlocked is sound.
///
/// Returns `(name, offset_from_symbol_start)`, or `None` if symbols were
/// never loaded or the address falls in no known symbol.
#[must_use]
pub fn resolve_static(addr: u64) -> Option<(&'static str, u64)> {
    if !LOADED.load(Ordering::Acquire) {
        return None;
    }
    let ptr = SNAPSHOT.load(Ordering::Acquire);
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `SNAPSHOT` is non-null only after `parse_elf_symbols` stored
    // the address of `SYMBOLS`' interior with Release ordering, which this
    // Acquire load synchronises with. `SYMBOLS` is a `static`, so the
    // pointee outlives `'static`; and the table is never mutated after that
    // single store, so no `&mut` alias can exist and the `Vec` buffers the
    // returned `&str` borrows from can never reallocate.
    let table: &'static SymbolTable = unsafe { &*ptr };
    table
        .lookup(addr)
        .map(|(sym, offset)| (table.name_of(sym), offset))
}

/// Resolve an address and format it for display.
///
/// Returns `"function+0xNN"` if resolved, or `"0xADDR"` if not.
#[must_use]
pub fn format_addr(addr: u64) -> String {
    resolve(addr).unwrap_or_else(|| alloc::format!("{:#018x}", addr))
}

/// Check if symbols are loaded.
#[must_use]
pub fn is_loaded() -> bool {
    LOADED.load(Ordering::Acquire)
}

/// Get the number of loaded symbols.
#[must_use]
pub fn count() -> usize {
    SYMBOL_COUNT.load(Ordering::Relaxed)
}

/// Find the symbol nearest to (at or before) an address.
///
/// Returns (name, base_address, offset) or None.
#[must_use]
pub fn nearest(_addr: u64) -> Option<(&'static str, u64, u64)> {
    // Can't return a reference to the Mutex-guarded data directly.
    // This function is a convenience that returns owned data via resolve().
    None // Use resolve() instead for now.
}

// ---------------------------------------------------------------------------
// ELF parsing
// ---------------------------------------------------------------------------

/// Parse function symbols from the kernel ELF.
fn parse_elf_symbols(elf: &'static [u8]) -> Option<usize> {
    // SAFETY (group — covers all ELF pointer casts below): each cast is
    // preceded by a bounds check ensuring offset + struct_size <= elf.len(),
    // so the pointer is within the valid slice.  ELF structs are repr(C)
    // with no alignment requirements beyond u8, and the data is read-only.
    let header = unsafe { &*(elf.as_ptr() as *const Elf64Header) };

    // Validate basic ELF fields.
    if header.e_shoff == 0 || header.e_shnum == 0 {
        return None;
    }

    let sh_offset = header.e_shoff as usize;
    let sh_count = header.e_shnum as usize;
    let sh_entsize = header.e_shentsize as usize;

    if sh_entsize < core::mem::size_of::<Elf64SectionHeader>() {
        return None;
    }

    // Find .symtab and its associated string table.
    let mut symtab_hdr: Option<&Elf64SectionHeader> = None;
    let mut strtab_offset: usize = 0;
    let mut strtab_size: usize = 0;

    for i in 0..sh_count {
        let offset = sh_offset + i * sh_entsize;
        if offset + sh_entsize > elf.len() {
            break;
        }
        let shdr = unsafe { &*(elf.as_ptr().add(offset) as *const Elf64SectionHeader) };

        if shdr.sh_type == SHT_SYMTAB {
            symtab_hdr = Some(shdr);
            // The linked section (sh_link) is the string table.
            let strtab_idx = shdr.sh_link as usize;
            if strtab_idx < sh_count {
                let strtab_hdr_offset = sh_offset + strtab_idx * sh_entsize;
                if strtab_hdr_offset + sh_entsize <= elf.len() {
                    let strtab_hdr = unsafe {
                        &*(elf.as_ptr().add(strtab_hdr_offset) as *const Elf64SectionHeader)
                    };
                    strtab_offset = strtab_hdr.sh_offset as usize;
                    strtab_size = strtab_hdr.sh_size as usize;
                }
            }
            break;
        }
    }

    let symtab = symtab_hdr?;
    let sym_offset = symtab.sh_offset as usize;
    let sym_size = symtab.sh_size as usize;
    let sym_entsize = symtab.sh_entsize as usize;

    if sym_entsize < core::mem::size_of::<Elf64Sym>() || sym_entsize == 0 {
        return None;
    }
    if sym_offset + sym_size > elf.len() {
        return None;
    }
    if strtab_offset + strtab_size > elf.len() {
        return None;
    }

    let sym_count = sym_size / sym_entsize;
    let strtab = elf.get(strtab_offset..strtab_offset + strtab_size)?;

    // Size `entries` by counting what will actually be kept, rather than
    // guessing from `sym_count`.
    //
    // The guess used to be `sym_count / 2`, on the reasoning that "roughly
    // half are functions". It is not close: `.symtab` is dominated by
    // SECTION, FILE and NOTYPE entries contributed by every object file, and
    // only ~12% of its ~1,008,000 entries survive the filter below. The old
    // guess reserved ~8 MiB for ~2 MiB of data, on the early-boot heap, for
    // no benefit. A counting pass is one linear scan of memory that is
    // already resident, and it also avoids the doubling reallocations a
    // too-small guess would cause — which transiently need the old buffer
    // and the new one at once, the worst case for a heap this young.
    let mut keep = 0usize;
    for i in 0..sym_count {
        let entry_offset = sym_offset + i * sym_entsize;
        if entry_offset + sym_entsize > elf.len() {
            break;
        }
        let sym = unsafe { &*(elf.as_ptr().add(entry_offset) as *const Elf64Sym) };
        let sym_type = sym.st_info & 0xF;
        if (sym_type == STT_FUNC || sym_type == STT_OBJECT) && sym.st_value != 0 {
            keep = keep.saturating_add(1);
        }
    }

    let mut entries = Vec::with_capacity(keep);

    for i in 0..sym_count {
        let entry_offset = sym_offset + i * sym_entsize;
        if entry_offset + sym_entsize > elf.len() {
            break;
        }
        let sym = unsafe { &*(elf.as_ptr().add(entry_offset) as *const Elf64Sym) };

        // Include code and data, but nothing else: SECTION and FILE symbols
        // carry addresses that would shadow real ones in the table, and
        // NOTYPE covers assembler labels whose extent is unknown.
        let sym_type = sym.st_info & 0xF;
        if (sym_type != STT_FUNC && sym_type != STT_OBJECT) || sym.st_value == 0 {
            continue;
        }

        // Reference the name where it already lives, in the ELF's own
        // string table. `name_offset` is an index into `.strtab`, not into
        // a copy of it; skipping the copy is what keeps this parse within
        // the early heap's means.
        let name_idx = sym.st_name as usize;
        let Some(rest) = strtab.get(name_idx..) else {
            continue;
        };
        // An empty name (the entry points straight at a NUL) names nothing
        // and would only pad the table.
        if rest.first().is_none_or(|&b| b == 0) {
            continue;
        }

        #[allow(clippy::cast_possible_truncation)]
        entries.push(KernelSymbol {
            addr: sym.st_value,
            size: sym.st_size as u32,
            name_offset: name_idx as u32,
        });
    }

    if entries.is_empty() {
        return None;
    }

    // Sort by address for binary search.
    entries.sort_by_key(|e| e.addr);

    let count = entries.len();

    // Store in the global table.
    let mut table = SYMBOLS.lock();
    table.entries = entries;
    table.strtab = strtab;

    // Publish the lock-free view. This is the only write the table ever
    // receives, so from here on the pointer is stable and the data behind
    // it immutable — the invariant `resolve_static` relies on. Released
    // before the guard drops so no reader can observe a half-filled table.
    SNAPSHOT.store(
        core::ptr::from_ref::<SymbolTable>(&table).cast_mut(),
        Ordering::Release,
    );

    Some(count)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel symbol table.
pub fn self_test() {
    serial_println!("[ksyms] Running self-test...");

    // Test 1: Check if symbols loaded.
    let loaded = is_loaded();
    let sym_count = count();
    serial_println!("[ksyms]   Loaded: {} ({} symbols)", loaded, sym_count);

    if !loaded {
        serial_println!("[ksyms]   (symbols not available — skipping lookup tests)");
        serial_println!("[ksyms] Self-test PASSED (no symbols)");
        return;
    }

    // Test 2: Resolve the address of this function.
    // We know kmain exists because we're executing from it.
    // Get the address of a known function via a function pointer.
    let self_test_addr = self_test as *const () as u64;
    let resolved = resolve(self_test_addr);
    if let Some(ref name) = resolved {
        serial_println!("[ksyms]   self_test resolved: {}", name);
        // Should contain "self_test" somewhere.
        assert!(
            name.contains("self_test") || name.contains("ksyms"),
            "Expected self_test symbol, got: {}",
            name
        );
    } else {
        serial_println!("[ksyms]   self_test not resolved (may be inlined/optimized)");
    }

    // Test 3: format_addr produces output.
    let formatted = format_addr(self_test_addr);
    assert!(!formatted.is_empty());
    serial_println!("[ksyms]   format_addr: {}", formatted);

    // Test 4: Null address returns None.
    assert!(resolve(0).is_none(), "null address should not resolve");
    serial_println!("[ksyms]   Null address: OK (None)");

    // Test 5: Very high address returns None.
    assert!(resolve(0xFFFF_FFFF_FFFF_FFFE).is_none());
    serial_println!("[ksyms]   Invalid high address: OK (None)");

    serial_println!("[ksyms] Self-test PASSED");
}
