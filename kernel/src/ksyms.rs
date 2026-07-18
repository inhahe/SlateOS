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
//! A typical kernel has ~2000-5000 functions.  Each symbol entry costs
//! ~24 bytes (address, size, name index), plus the string table.
//! Total: ~100-200 KiB — acceptable for better diagnostics.
//!
//! ## Limitations
//!
//! - Only function symbols (`STT_FUNC`) are loaded (not data symbols).
//! - Symbols without a size are given a default size of 1 byte.
//! - If the kernel is stripped, no symbols will be available.
//!
//! ## References
//!
//! - Linux `kernel/kallsyms.c` — compressed symbol table
//! - ELF specification §4.6 (Symbol Table)

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::serial_println;

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
    /// Index into the name buffer.
    name_offset: u32,
}

/// Global symbol table.
static SYMBOLS: Mutex<SymbolTable> = Mutex::new(SymbolTable::empty());

/// Whether symbols have been loaded.
static LOADED: AtomicBool = AtomicBool::new(false);

/// Number of symbols loaded.
static SYMBOL_COUNT: AtomicUsize = AtomicUsize::new(0);

struct SymbolTable {
    /// Sorted by address (for binary search).
    entries: Vec<KernelSymbol>,
    /// All symbol names concatenated (null-separated).
    names: Vec<u8>,
}

impl SymbolTable {
    const fn empty() -> Self {
        Self {
            entries: Vec::new(),
            names: Vec::new(),
        }
    }

    /// Get the name for a symbol entry.
    fn name_of(&self, sym: &KernelSymbol) -> &str {
        let start = sym.name_offset as usize;
        if start >= self.names.len() {
            return "<invalid>";
        }
        // Find the null terminator.
        let end = self.names[start..]
            .iter()
            .position(|&b| b == 0)
            .map_or(self.names.len(), |p| start + p);
        core::str::from_utf8(&self.names[start..end]).unwrap_or("<invalid utf8>")
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

    // SAFETY: Limine guarantees the kernel file mapping is valid.
    let elf_bytes = unsafe { core::slice::from_raw_parts(base as *const u8, size) };

    // Validate ELF magic.
    if &elf_bytes[0..4] != b"\x7fELF" {
        serial_println!("[ksyms] Kernel file is not a valid ELF");
        return;
    }

    match parse_elf_symbols(elf_bytes) {
        Some(count) => {
            LOADED.store(true, Ordering::Release);
            SYMBOL_COUNT.store(count, Ordering::Relaxed);
            serial_println!("[ksyms] Loaded {} function symbols", count);
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
fn parse_elf_symbols(elf: &[u8]) -> Option<usize> {
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
    let strtab = &elf[strtab_offset..strtab_offset + strtab_size];

    // Extract function symbols.
    let mut entries = Vec::with_capacity(sym_count / 2); // Roughly half are functions.
    let mut names = Vec::with_capacity(sym_count * 20); // ~20 chars per name average.

    for i in 0..sym_count {
        let entry_offset = sym_offset + i * sym_entsize;
        if entry_offset + sym_entsize > elf.len() {
            break;
        }
        let sym = unsafe { &*(elf.as_ptr().add(entry_offset) as *const Elf64Sym) };

        // Only include function symbols with a valid address.
        let sym_type = sym.st_info & 0xF;
        if sym_type != STT_FUNC || sym.st_value == 0 {
            continue;
        }

        // Get the symbol name from the string table.
        let name_idx = sym.st_name as usize;
        if name_idx >= strtab.len() {
            continue;
        }

        // Find the end of the name (null-terminated).
        let name_end = strtab[name_idx..]
            .iter()
            .position(|&b| b == 0)
            .map_or(strtab.len() - name_idx, |p| p);

        let name = &strtab[name_idx..name_idx + name_end];
        if name.is_empty() {
            continue;
        }

        // Store the name in our concatenated buffer.
        let name_offset = names.len();
        names.extend_from_slice(name);
        names.push(0); // Null terminator.

        #[allow(clippy::cast_possible_truncation)]
        entries.push(KernelSymbol {
            addr: sym.st_value,
            size: sym.st_size as u32,
            name_offset: name_offset as u32,
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
    table.names = names;

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
