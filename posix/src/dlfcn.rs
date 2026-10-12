//! `<dlfcn.h>` and `<link.h>`: the dynamic linker's interface, where every
//! program is one static executable.
//!
//! ## One object: the program
//!
//! Nothing is ever loaded into a program here -- there is no dynamic linker
//! and there are no shared objects -- so every question these calls answer
//! is about one object, the program itself. That is what glibc's answer in a
//! statically linked program too, and glibc 2.39's, so linked, are the model:
//! `posix/tools/oracle/dlfcn_harness.py` records them in `dlfcn_oracle.txt`,
//! which the tests replay.
//!
//! | Call | Answers |
//! |---|---|
//! | `dlopen(NULL)`, `dlopen("")` | the program's handle, the one POSIX promises for NULL -- which is the program's `struct link_map` |
//! | `dlopen` of the C library's own names (`libc.so.6`, `libm.so.6` ... -- `OWN_NAMES` below) | the program's handle too: every one of those libraries is linked into it |
//! | `dlopen(file)`, any other | NULL, `file: cannot open shared object file: ...`; with `RTLD_NOLOAD`, NULL and no error, a file not loaded being the answer asked for |
//! | `dlsym`, `dlvsym` | a symbol the program exports, if it was linked to export them (below); else NULL, `program: undefined symbol: name` |
//! | `dlclose` | 0 for the program's handle, which is never unloaded |
//! | `dlinfo` | the program's link map, namespace, directory, TLS module and block, program headers |
//! | `dladdr`, `dladdr1` | for an address inside the program, the program's name and ELF header, and the exported symbol holding it, if any |
//! | `dl_iterate_phdr` | one call, the program's: its headers, load bias and TLS |
//! | `_dl_find_object` | the program's segment holding an address, and its `.eh_frame_hdr` |
//! | `dlmopen` | the base namespace only, as glibc's static one |
//!
//! `dladdr` parts from glibc's static answer, which is 0 for every address:
//! glibc's static program map records no address range. Its dynamic one says
//! what POSIX asks for, that the program is an object like any other, and
//! that is what this says (design-decisions §1147).
//!
//! ## A program that exports its symbols
//!
//! A program linked with `--export-dynamic` (`-rdynamic`) carries its own
//! dynamic symbol table: every global symbol it defines, with a hash table to
//! find one by name, in a loaded segment that `PT_DYNAMIC` locates. glibc's
//! static `dlsym` never looks there -- it answers NULL for such a program as
//! for any other (measured: `gcc -static -Wl,--export-dynamic`, 2026-10-07)
//! -- and its dynamic one finds the symbol. This finds it, as POSIX's `dlsym`
//! of the global handle says it should and as dynamic glibc does, and
//! `dladdr` names it (design-decisions §1184). The rules are glibc's
//! (`do_lookup_x`, `determine_info`): a defined, global or weak symbol of
//! default or protected visibility; a TLS symbol at the calling thread's
//! copy; an IFUNC by calling its resolver; and a symbol's version, if the
//! program has versions at all, as `dlvsym` asks for it. A program linked
//! without exports is answered exactly as before, as glibc's static one.
//!
//! This is what lets a runtime that calls C by name work here. Mono's
//! `DllImport("libc")` opens `libc.so.6` and looks a function up in it.
//! CPython's `ctypes.CDLL(None)` does the same through the program's handle.
//!
//! ## Why `dl_iterate_phdr` matters
//!
//! It is how an unwinder finds the tables that unwind a stack. LLVM's
//! libunwind -- the one zig links a C++ program with -- asks it for each
//! object's `PT_GNU_EH_FRAME` and has no other way to find one. Until
//! 2026-09-29 it returned 0 without calling its callback, so libunwind found
//! no object at all and a C++ `throw` could never be caught: every one ended
//! in `std::terminate` (known-issues.md ->
//! `D-POSIX-DL-ITERATE-PHDR-NEVER-CALLED-BACK-SO-NO-CXX-THROW-COULD-BE-CAUGHT`).
//!
//! ## Errors
//!
//! Each thread has its own pending message, as glibc's does: a failing call
//! leaves one, `dlerror` returns the calling thread's and clears it, and the
//! string stays the caller's to read until the same thread's next `dlerror`.
//! It was one slot for the whole process until 2026-09-29, so one thread's
//! `dlerror` could take another's message, or hand it one it had not caused.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::tls::{PT_GNU_EH_FRAME, PT_LOAD, ProgramHeaders, TlsImage, program_headers};

// ---------------------------------------------------------------------------
// Constants, as glibc's and musl's headers give them
// ---------------------------------------------------------------------------

/// Resolve symbols lazily.
pub const RTLD_LAZY: i32 = 1;
/// Resolve symbols immediately.
pub const RTLD_NOW: i32 = 2;
/// The binding bits: one of [`RTLD_LAZY`] and [`RTLD_NOW`] must be set.
pub const RTLD_BINDING_MASK: i32 = 3;
/// Do not load the object: answer only if it already is.
pub const RTLD_NOLOAD: i32 = 4;
/// Look an object's own symbols up ahead of the global ones (glibc).
pub const RTLD_DEEPBIND: i32 = 8;
/// Make symbols available globally.
pub const RTLD_GLOBAL: i32 = 0x100;
/// Make symbols available only locally.
pub const RTLD_LOCAL: i32 = 0;
/// Never unload the object.
pub const RTLD_NODELETE: i32 = 0x1000;
/// Every bit `dlopen`'s mode may have; any other is "invalid mode
/// parameter", as in glibc.
const MODE_BITS: i32 =
    RTLD_BINDING_MASK | RTLD_NOLOAD | RTLD_DEEPBIND | RTLD_GLOBAL | RTLD_LOCAL | RTLD_NODELETE;

/// `dlsym`'s handle for the global scope.
pub const RTLD_DEFAULT: *mut c_void = core::ptr::null_mut();
/// `dlsym`'s handle for "the object after the caller's".
pub const RTLD_NEXT: *mut c_void = core::ptr::without_provenance_mut(usize::MAX);

/// `dlmopen`'s initial namespace.
pub const LM_ID_BASE: i64 = 0;
/// `dlmopen`'s "a new namespace".
pub const LM_ID_NEWLM: i64 = -1;

/// `dladdr1`: store the symbol's `ElfW(Sym)` entry.
pub const RTLD_DL_SYMENT: i32 = 1;
/// `dladdr1`: store the object's `struct link_map`.
pub const RTLD_DL_LINKMAP: i32 = 2;

/// `dlinfo`: the object's namespace (`Lmid_t`).
pub const RTLD_DI_LMID: i32 = 1;
/// `dlinfo`: the object's `struct link_map *`.
pub const RTLD_DI_LINKMAP: i32 = 2;
/// `dlinfo`: Solaris's configuration address; unsupported, as in glibc.
pub const RTLD_DI_CONFIGADDR: i32 = 3;
/// `dlinfo`: the library search path, into a `Dl_serinfo`.
pub const RTLD_DI_SERINFO: i32 = 4;
/// `dlinfo`: the size [`RTLD_DI_SERINFO`] needs.
pub const RTLD_DI_SERINFOSIZE: i32 = 5;
/// `dlinfo`: the object's directory, for `$ORIGIN`.
pub const RTLD_DI_ORIGIN: i32 = 6;
/// `dlinfo`: Solaris's profiling name; unsupported, as in glibc.
pub const RTLD_DI_PROFILENAME: i32 = 7;
/// `dlinfo`: Solaris's profiling output; unsupported, as in glibc.
pub const RTLD_DI_PROFILEOUT: i32 = 8;
/// `dlinfo`: the object's TLS module ID, 0 for none.
pub const RTLD_DI_TLS_MODID: i32 = 9;
/// `dlinfo`: the calling thread's block of the object's TLS.
pub const RTLD_DI_TLS_DATA: i32 = 10;
/// `dlinfo`: the object's program headers; returns their count.
pub const RTLD_DI_PHDR: i32 = 11;

/// The one module ID a program's own `PT_TLS` has: the first.
const PROGRAM_TLS_MODULE: u64 = 1;

// ---------------------------------------------------------------------------
// The C types
// ---------------------------------------------------------------------------

/// `Dl_info`: what `dladdr` found.
#[repr(C)]
pub struct DlInfo {
    /// Pathname of the object.
    pub dli_fname: *const u8,
    /// Where the object is loaded: its ELF header.
    pub dli_fbase: *mut c_void,
    /// Name of the nearest symbol, or NULL.
    pub dli_sname: *const u8,
    /// Its address, or NULL.
    pub dli_saddr: *mut c_void,
}

/// `struct link_map`'s public fields, as glibc's and musl's `<link.h>` give
/// them.
///
/// `l_addr` is atomic because it is the one field ever written, each time a
/// handle is given out; to C it is the plain `ElfW(Addr)` it has always
/// been, the two having one size and alignment.
#[repr(C)]
pub struct LinkMap {
    /// The load bias.
    pub l_addr: AtomicU64,
    /// The object's name: "" for the program, as in glibc.
    pub l_name: *const u8,
    /// Its dynamic section: none in a static program.
    pub l_ld: *mut c_void,
    /// The next object: none.
    pub l_next: *mut LinkMap,
    /// The one before: none.
    pub l_prev: *mut LinkMap,
}

/// `struct dl_phdr_info`: what `dl_iterate_phdr` tells its callback of one
/// object. 64 bytes, glibc's and musl's alike.
#[repr(C)]
pub struct DlPhdrInfo {
    /// The load bias.
    pub dlpi_addr: u64,
    /// The object's name: "" for the program.
    pub dlpi_name: *const u8,
    /// Its program headers.
    pub dlpi_phdr: *const c_void,
    /// How many.
    pub dlpi_phnum: u16,
    /// Objects added since the process began.
    pub dlpi_adds: u64,
    /// Objects removed since the process began.
    pub dlpi_subs: u64,
    /// The object's TLS module ID, 0 for none.
    pub dlpi_tls_modid: usize,
    /// The calling thread's block of the object's TLS, or NULL.
    pub dlpi_tls_data: *mut c_void,
}

/// glibc's `Dl_serpath`: one directory of a library search path.
#[repr(C)]
pub struct DlSerpath {
    /// The directory.
    pub dls_name: *mut u8,
    /// Where it came from.
    pub dls_flags: u32,
}

/// glibc's `Dl_serinfo`: a library search path, as `dlinfo` describes one.
#[repr(C)]
pub struct DlSerinfo {
    /// Bytes the whole description needs.
    pub dls_size: usize,
    /// Directories in it.
    pub dls_cnt: u32,
    /// The first of them; there are `dls_cnt`.
    pub dls_serpath: [DlSerpath; 1],
}

/// glibc 2.35's `struct dl_find_object`, as x86-64 lays it out (no
/// `dlfo_eh_dbase`, no `dlfo_eh_count`): 96 bytes.
#[repr(C)]
pub struct DlFindObject {
    /// Flags: none are defined.
    pub dlfo_flags: u64,
    /// Where the mapping holding the address starts.
    pub dlfo_map_start: *mut c_void,
    /// Where it ends.
    pub dlfo_map_end: *mut c_void,
    /// The object.
    pub dlfo_link_map: *mut LinkMap,
    /// Its `.eh_frame_hdr` (`PT_GNU_EH_FRAME`), or NULL.
    pub dlfo_eh_frame: *mut c_void,
    /// Room for fields to come (glibc's `__dflo_reserved`, so spelt); never
    /// written.
    pub reserved: [u64; 7],
}

// ---------------------------------------------------------------------------
// The program
// ---------------------------------------------------------------------------

/// The program's link map, whose address is its handle.
struct ProgramMap(LinkMap);

// SAFETY: every pointer in it is a constant that nothing writes, and
// `l_addr`, the one field written, is atomic.
unsafe impl Sync for ProgramMap {}

static PROGRAM: ProgramMap = ProgramMap(LinkMap {
    l_addr: AtomicU64::new(0),
    l_name: c"".as_ptr().cast(),
    l_ld: core::ptr::null_mut(),
    l_next: core::ptr::null_mut(),
    l_prev: core::ptr::null_mut(),
});

/// The program's handle -- its link map -- with the map's load bias brought
/// up to date.
fn program_handle() -> *mut c_void {
    if let Some(ph) = program_headers() {
        PROGRAM.0.l_addr.store(ph.load_bias(), Ordering::Relaxed);
    }
    core::ptr::addr_of!(PROGRAM.0).cast_mut().cast()
}

/// Is `handle` the program's?
fn is_program(handle: *mut c_void) -> bool {
    core::ptr::eq(handle.cast_const(), core::ptr::addr_of!(PROGRAM.0).cast())
}

/// The calling thread's block of the program's TLS: `block_size` below the
/// thread pointer, as `crate::tls` lays every thread's out; `None` without
/// a `PT_TLS` or a thread pointer.
fn tls_block(img: &TlsImage, tp: u64) -> Option<*mut c_void> {
    if img.mem_size == 0 || tp == 0 {
        return None;
    }
    let at = usize::try_from(tp.wrapping_sub(img.block_size())).ok()?;
    Some(core::ptr::with_exposed_provenance_mut(at))
}

/// Does the program have a `PT_TLS` segment? (One of no size still has a
/// module ID, as in glibc.)
fn has_tls(ph: &ProgramHeaders) -> bool {
    ph.find(crate::tls::PT_TLS).is_some()
}

/// The `PT_LOAD` segment holding `addr`, as `[start, end)` in memory.
fn segment_holding(ph: &ProgramHeaders, addr: usize) -> Option<(u64, u64)> {
    let bias = ph.load_bias();
    let a = addr as u64;
    ph.iter()
        .filter(|p| p.p_type == PT_LOAD)
        .map(|p| {
            let start = bias.wrapping_add(p.p_vaddr);
            (start, start.wrapping_add(p.p_memsz))
        })
        .find(|&(start, end)| start <= a && a < end)
}

/// A number as the pointer it is in memory the program's image or a TLS
/// block occupies, mapped by the kernel rather than allocated in Rust.
fn as_ptr(v: u64) -> *mut c_void {
    usize::try_from(v).map_or(
        core::ptr::null_mut(),
        core::ptr::with_exposed_provenance_mut,
    )
}

/// The names the C library's parts go by on a glibc system. Each of those
/// libraries is linked into every program here, so opening one by name opens
/// the program (design-decisions §1184). Not `pub`: it is no constant of any
/// C header, and `check-libc-abi.py` holds every public constant to one.
const OWN_NAMES: [&[u8]; 9] = [
    b"libc.so.6",
    b"libm.so.6",
    b"libdl.so.2",
    b"libpthread.so.0",
    b"librt.so.1",
    b"libutil.so.1",
    b"libcrypt.so.1",
    b"libresolv.so.2",
    b"libanl.so.1",
];

/// Is `file` -- by itself, or as the last part of a path -- one of the
/// [`OWN_NAMES`]?
fn is_own_name(file: &[u8]) -> bool {
    let base = file.rsplit(|&b| b == b'/').next().unwrap_or(file);
    OWN_NAMES.contains(&base)
}

// ---------------------------------------------------------------------------
// The program's exported symbols
// ---------------------------------------------------------------------------

/// `PT_DYNAMIC`: the segment holding the program's dynamic section.
const PT_DYNAMIC: u32 = 2;

// `Elf64_Dyn` tags: the section's end, and what locates the symbols.
const DT_NULL: u64 = 0;
const DT_HASH: u64 = 4;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_STRSZ: u64 = 10;
const DT_SYMENT: u64 = 11;
const DT_GNU_HASH: u64 = 0x6fff_fef5;
const DT_VERSYM: u64 = 0x6fff_fff0;
const DT_VERDEF: u64 = 0x6fff_fffc;
const DT_VERDEFNUM: u64 = 0x6fff_fffd;

/// `sizeof (Elf64_Dyn)`.
const DYN_SIZE: u64 = 16;
/// `sizeof (Elf64_Sym)`.
const SYM_SIZE: u64 = 24;

// What glibc's lookup weighs of a symbol: its binding, its kind, its
// section and its visibility.
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const STB_GNU_UNIQUE: u8 = 10;
const STT_NOTYPE: u8 = 0;
const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const STT_COMMON: u8 = 5;
const STT_TLS: u8 = 6;
const STT_GNU_IFUNC: u8 = 10;
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
const STV_DEFAULT: u8 = 0;
const STV_PROTECTED: u8 = 3;

/// A `.gnu.version` entry's version index.
const VERSYM_INDEX: u16 = 0x7fff;
/// The bit that hides a version from every lookup that does not name it.
const VERSYM_HIDDEN: u16 = 0x8000;

/// One `Elf64_Sym`, read out of the program's table.
#[derive(Clone, Copy, Debug)]
struct Sym {
    /// Its index in the table.
    index: u64,
    /// Where the entry is in memory: `dladdr1`'s `RTLD_DL_SYMENT` answer.
    at: u64,
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

impl Sym {
    const fn bind(&self) -> u8 {
        self.info.wrapping_shr(4)
    }

    const fn kind(&self) -> u8 {
        self.info & 0xf
    }

    const fn visibility(&self) -> u8 {
        self.other & 3
    }

    /// Does a lookup from outside the program's own code see it? glibc's
    /// `do_lookup_x` and `check_match`: defined, with a value (a TLS symbol's
    /// may be 0, an absolute one's is its own); of a kind that is code or
    /// data; global, weak or unique; and neither hidden nor internal.
    fn exported(&self) -> bool {
        let defined = self.shndx != SHN_UNDEF
            && (self.value != 0 || self.shndx == SHN_ABS || self.kind() == STT_TLS);
        let kind = matches!(
            self.kind(),
            STT_NOTYPE | STT_OBJECT | STT_FUNC | STT_COMMON | STT_TLS | STT_GNU_IFUNC
        );
        let bind = matches!(self.bind(), STB_GLOBAL | STB_WEAK | STB_GNU_UNIQUE);
        let seen = matches!(self.visibility(), STV_DEFAULT | STV_PROTECTED);
        defined && kind && bind && seen
    }
}

/// `dl_new_hash`, the GNU hash table's function.
fn gnu_hash(name: &[u8]) -> u32 {
    name.iter().fold(5381u32, |h, &c| {
        h.wrapping_mul(33).wrapping_add(u32::from(c))
    })
}

/// `_dl_elf_hash`, the SysV hash table's function.
fn elf_hash(name: &[u8]) -> u32 {
    name.iter().fold(0u32, |h, &c| {
        let h = h.wrapping_shl(4).wrapping_add(u32::from(c));
        let g = h & 0xf000_0000;
        (h ^ g.wrapping_shr(24)) & !g
    })
}

/// Where the program keeps the symbols it exports, read in place. Every read
/// is first checked to lie whole inside one of the program's loaded segments,
/// so a malformed table answers "not found" rather than faulting.
#[derive(Clone, Copy)]
struct Exports {
    ph: ProgramHeaders,
    bias: u64,
    // Run-time addresses of the tables, 0 where the program has none.
    symtab: u64,
    strtab: u64,
    strsz: u64,
    gnu_hash: u64,
    sysv_hash: u64,
    versym: u64,
    verdef: u64,
    verdefnum: u64,
}

impl Exports {
    /// The program's tables, if its headers show a dynamic section that
    /// locates a symbol table, its strings and a hash table to search them.
    fn of(ph: ProgramHeaders) -> Option<Self> {
        let dynamic = ph.find(PT_DYNAMIC)?;
        let bias = ph.load_bias();
        let mut e = Self {
            ph,
            bias,
            symtab: 0,
            strtab: 0,
            strsz: 0,
            gnu_hash: 0,
            sysv_hash: 0,
            versym: 0,
            verdef: 0,
            verdefnum: 0,
        };
        let mut syment = SYM_SIZE;
        let start = bias.wrapping_add(dynamic.p_vaddr);
        for i in 0..dynamic.p_memsz / DYN_SIZE {
            let at = start.checked_add(i.checked_mul(DYN_SIZE)?)?;
            let tag = e.u64_at(at)?;
            let val = e.u64_at(at.checked_add(8)?)?;
            // A pointer's run-time address is the link-time one plus the
            // bias -- 0 for an ET_EXEC, every program here -- no dynamic
            // linker having relocated the section in place.
            let addr = bias.wrapping_add(val);
            match tag {
                DT_NULL => break,
                DT_SYMTAB => e.symtab = addr,
                DT_STRTAB => e.strtab = addr,
                DT_STRSZ => e.strsz = val,
                DT_SYMENT => syment = val,
                DT_HASH => e.sysv_hash = addr,
                DT_GNU_HASH => e.gnu_hash = addr,
                DT_VERSYM => e.versym = addr,
                DT_VERDEF => e.verdef = addr,
                DT_VERDEFNUM => e.verdefnum = val,
                _ => {}
            }
        }
        let located =
            e.symtab != 0 && e.strtab != 0 && e.strsz != 0 && (e.gnu_hash != 0 || e.sysv_hash != 0);
        (located && syment == SYM_SIZE).then_some(e)
    }

    /// The memory `len` bytes at `addr` occupy, if one of the program's
    /// loaded segments holds them all.
    fn span(&self, addr: u64, len: u64) -> Option<*const u8> {
        let start = usize::try_from(addr).ok()?;
        let (_, end) = segment_holding(&self.ph, start)?;
        (addr.checked_add(len)? <= end).then(|| core::ptr::with_exposed_provenance(start))
    }

    fn u16_at(&self, addr: u64) -> Option<u16> {
        let p = self.span(addr, 2)?;
        // SAFETY: `span` found the two bytes in the program's image, mapped
        // for its life; read unaligned, alignment being no promise here.
        Some(unsafe { p.cast::<u16>().read_unaligned() })
    }

    fn u32_at(&self, addr: u64) -> Option<u32> {
        let p = self.span(addr, 4)?;
        // SAFETY: as `u16_at`'s, for four bytes.
        Some(unsafe { p.cast::<u32>().read_unaligned() })
    }

    fn u64_at(&self, addr: u64) -> Option<u64> {
        let p = self.span(addr, 8)?;
        // SAFETY: as `u16_at`'s, for eight bytes.
        Some(unsafe { p.cast::<u64>().read_unaligned() })
    }

    /// The string at `offset` in the string table, without its NUL; `None`
    /// if it does not end inside the table.
    fn string(&self, offset: u32) -> Option<&'static [u8]> {
        let offset = u64::from(offset);
        let room = self.strsz.checked_sub(offset).filter(|&r| r > 0)?;
        let p = self.span(self.strtab.checked_add(offset)?, room)?;
        // SAFETY: `span` found `room` bytes there, in the program's image,
        // which is mapped and never written for the life of the program.
        let bytes = unsafe { core::slice::from_raw_parts(p, usize::try_from(room).ok()?) };
        let n = bytes.iter().position(|&b| b == 0)?;
        bytes.get(..n)
    }

    /// Entry `index` of the symbol table.
    fn sym(&self, index: u64) -> Option<Sym> {
        let at = self.symtab.checked_add(index.checked_mul(SYM_SIZE)?)?;
        let p = self.span(at, SYM_SIZE)?;
        // SAFETY: `span` found the entry's 24 bytes there, in the program's
        // image; each field is read unaligned at its `Elf64_Sym` offset.
        unsafe {
            Some(Sym {
                index,
                at,
                name: p.cast::<u32>().read_unaligned(),
                info: p.add(4).read(),
                other: p.add(5).read(),
                shndx: p.add(6).cast::<u16>().read_unaligned(),
                value: p.add(8).cast::<u64>().read_unaligned(),
                size: p.add(16).cast::<u64>().read_unaligned(),
            })
        }
    }

    /// Call `each` with every entry named `name`, found through the hash
    /// table -- GNU's if the program has one, else the SysV one -- until it
    /// answers `true`.
    fn named(&self, name: &[u8], each: &mut dyn FnMut(Sym) -> bool) {
        if self.gnu_hash != 0 {
            // A table that is malformed partway answers what it found.
            let _ = self.gnu_named(name, each);
        } else {
            let _ = self.sysv_named(name, each);
        }
    }

    /// The GNU table: a Bloom filter that turns most names away at once,
    /// then a bucket per hash and a chain of hashes, its last marked by the
    /// low bit, in symbol order.
    fn gnu_named(&self, name: &[u8], each: &mut dyn FnMut(Sym) -> bool) -> Option<()> {
        let t = self.gnu_hash;
        let nbuckets = self.u32_at(t)?;
        let symoffset = u64::from(self.u32_at(t.checked_add(4)?)?);
        let bloom_size = self.u32_at(t.checked_add(8)?)?;
        let shift = self.u32_at(t.checked_add(12)?)?;
        if nbuckets == 0 || bloom_size == 0 {
            return None;
        }
        let h = gnu_hash(name);
        let bloom = t.checked_add(16)?;
        let word_at =
            bloom.checked_add(u64::from((h / 64).checked_rem(bloom_size)?).checked_mul(8)?)?;
        let word = self.u64_at(word_at)?;
        let mask =
            1u64.wrapping_shl(h % 64) | 1u64.wrapping_shl(h.checked_shr(shift).unwrap_or(0) % 64);
        if word & mask != mask {
            return None;
        }
        let buckets = bloom.checked_add(u64::from(bloom_size).checked_mul(8)?)?;
        let chain = buckets.checked_add(u64::from(nbuckets).checked_mul(4)?)?;
        let mut index = u64::from(
            self.u32_at(buckets.checked_add(u64::from(h.checked_rem(nbuckets)?).checked_mul(4)?)?)?,
        );
        if index < symoffset {
            return None;
        }
        loop {
            let ch =
                self.u32_at(chain.checked_add(index.checked_sub(symoffset)?.checked_mul(4)?)?)?;
            if ch | 1 == h | 1 {
                let s = self.sym(index)?;
                if self.string(s.name) == Some(name) && each(s) {
                    return Some(());
                }
            }
            if ch & 1 != 0 {
                return Some(());
            }
            index = index.checked_add(1)?;
        }
    }

    /// The SysV table: a bucket per hash, and a chain of symbol indexes
    /// ending at 0.
    fn sysv_named(&self, name: &[u8], each: &mut dyn FnMut(Sym) -> bool) -> Option<()> {
        let t = self.sysv_hash;
        let nbucket = self.u32_at(t)?;
        let nchain = self.u32_at(t.checked_add(4)?)?;
        if nbucket == 0 {
            return None;
        }
        let buckets = t.checked_add(8)?;
        let chain = buckets.checked_add(u64::from(nbucket).checked_mul(4)?)?;
        let h = elf_hash(name);
        let mut index =
            self.u32_at(buckets.checked_add(u64::from(h.checked_rem(nbucket)?).checked_mul(4)?)?)?;
        // At most `nchain` steps: a chain that loops is malformed, and is
        // left rather than followed forever.
        for _ in 0..nchain {
            if index == 0 || index >= nchain {
                return Some(());
            }
            let s = self.sym(u64::from(index))?;
            if self.string(s.name) == Some(name) && each(s) {
                return Some(());
            }
            index = self.u32_at(chain.checked_add(u64::from(index).checked_mul(4)?)?)?;
        }
        Some(())
    }

    /// How many entries the symbol table has: the SysV table says; the GNU
    /// one is followed from its highest bucket to the end of that chain.
    fn count(&self) -> Option<u64> {
        if self.sysv_hash != 0 {
            return self.u32_at(self.sysv_hash.checked_add(4)?).map(u64::from);
        }
        let t = self.gnu_hash;
        let nbuckets = u64::from(self.u32_at(t)?);
        let symoffset = u64::from(self.u32_at(t.checked_add(4)?)?);
        let bloom_size = u64::from(self.u32_at(t.checked_add(8)?)?);
        let buckets = t.checked_add(16)?.checked_add(bloom_size.checked_mul(8)?)?;
        let chain = buckets.checked_add(nbuckets.checked_mul(4)?)?;
        let mut last = 0u64;
        for b in 0..nbuckets {
            let v = u64::from(self.u32_at(buckets.checked_add(b.checked_mul(4)?)?)?);
            last = last.max(v);
        }
        if last < symoffset {
            return Some(symoffset);
        }
        loop {
            let ch =
                self.u32_at(chain.checked_add(last.checked_sub(symoffset)?.checked_mul(4)?)?)?;
            last = last.checked_add(1)?;
            if ch & 1 != 0 {
                return Some(last);
            }
        }
    }

    /// Symbol `index`'s `.gnu.version` entry; `None` if the program has no
    /// versions.
    fn versym(&self, index: u64) -> Option<u16> {
        if self.versym == 0 {
            return None;
        }
        self.u16_at(self.versym.checked_add(index.checked_mul(2)?)?)
    }

    /// The name of version `ndx`, from the program's version definitions.
    fn version_name(&self, ndx: u16) -> Option<&'static [u8]> {
        let mut at = self.verdef;
        if at == 0 {
            return None;
        }
        // Elf64_Verdef: vd_ndx @4 (u16), vd_aux @12 and vd_next @16 (u32s);
        // its first Elf64_Verdaux's vda_name @0 names it.
        for _ in 0..self.verdefnum.max(1) {
            let vd_ndx = self.u16_at(at.checked_add(4)?)?;
            let vd_aux = self.u32_at(at.checked_add(12)?)?;
            let vd_next = self.u32_at(at.checked_add(16)?)?;
            if vd_ndx == ndx {
                let name = self.u32_at(at.checked_add(u64::from(vd_aux))?)?;
                return self.string(name);
            }
            if vd_next == 0 {
                return None;
            }
            at = at.checked_add(u64::from(vd_next))?;
        }
        None
    }

    /// The entry `dlsym` (`version` `None`) or `dlvsym` answers `name` with:
    /// glibc's `check_match` and `do_lookup_x`. In a program without versions
    /// any exported definition answers either. With versions, `dlvsym` wants
    /// the one of that version, hidden or not; `dlsym` wants an unversioned
    /// definition, else the one version not hidden -- and none if there are
    /// several, which would be ambiguous.
    fn find(&self, name: &[u8], version: Option<&[u8]>) -> Option<Sym> {
        let mut found: Option<Sym> = None;
        let mut versioned: Option<Sym> = None;
        let mut versions = 0u32;
        self.named(name, &mut |s| {
            if !s.exported() {
                return false;
            }
            match (version, self.versym(s.index)) {
                (_, None) => {
                    found = Some(s);
                    true
                }
                (None, Some(v)) if v & VERSYM_INDEX <= 1 => {
                    found = Some(s);
                    true
                }
                (None, Some(v)) => {
                    if v & VERSYM_HIDDEN == 0 {
                        versions = versions.saturating_add(1);
                        versioned.get_or_insert(s);
                    }
                    false
                }
                (Some(want), Some(v)) => {
                    if self.version_name(v & VERSYM_INDEX) == Some(want) {
                        found = Some(s);
                        true
                    } else {
                        false
                    }
                }
            }
        });
        found.or(if versions == 1 { versioned } else { None })
    }

    /// The address entry `s` gives a caller whose thread pointer is `tp`: a
    /// TLS symbol's in the calling thread's block, an IFUNC's from its
    /// resolver, an absolute symbol's its value, any other's its value moved
    /// by the bias.
    fn address(&self, s: &Sym, tp: u64) -> Option<*mut c_void> {
        match s.kind() {
            STT_TLS => {
                let block = tls_block(&self.ph.tls_image(), tp);
                let p = tls_address(block, PROGRAM_TLS_MODULE, s.value);
                (!p.is_null()).then_some(p)
            }
            STT_GNU_IFUNC => {
                let at = as_ptr(self.bias.wrapping_add(s.value));
                if at.is_null() {
                    return None;
                }
                // SAFETY: the program's own code, which its link marked an
                // IFUNC: a function of no arguments returning the address to
                // use, called here as a dynamic linker calls it.
                let resolver: unsafe extern "C" fn() -> *mut c_void =
                    unsafe { core::mem::transmute::<*mut c_void, _>(at) };
                // SAFETY: as above.
                let p = unsafe { resolver() };
                (!p.is_null()).then_some(p)
            }
            _ => {
                let base = if s.shndx == SHN_ABS { 0 } else { self.bias };
                Some(as_ptr(base.wrapping_add(s.value)))
            }
        }
    }

    /// The exported symbol `dladdr` names for `addr`: glibc's
    /// `determine_info`. Global or weak, of default or protected visibility,
    /// neither TLS nor absolute, and holding `addr` -- inside its size, or at
    /// its address for a symbol of no size -- the one at the highest address
    /// among them.
    fn holding(&self, addr: u64) -> Option<Sym> {
        let count = self.count()?;
        let mut best: Option<Sym> = None;
        // Entry 0 is the null symbol.
        for i in 1..count {
            let Some(s) = self.sym(i) else { break };
            let eligible = matches!(s.bind(), STB_GLOBAL | STB_WEAK)
                && matches!(s.visibility(), STV_DEFAULT | STV_PROTECTED)
                && s.kind() != STT_TLS
                && (s.shndx != SHN_UNDEF || s.value != 0)
                && s.shndx != SHN_ABS
                && u64::from(s.name) < self.strsz;
            if !eligible {
                continue;
            }
            let start = self.bias.wrapping_add(s.value);
            let inside = if s.shndx == SHN_UNDEF || s.size == 0 {
                addr == start
            } else {
                addr.checked_sub(start).is_some_and(|d| d < s.size)
            };
            if inside && best.is_none_or(|b| b.value < s.value) {
                best = Some(s);
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// The calling thread's message
// ---------------------------------------------------------------------------

/// A thread's `dlerror` state, in its [`crate::perthread::PerThread`] block.
/// All-zero -- no message -- for a new thread. `Copy` as the block is; the
/// block is never copied but to make a new one, from [`DlErrorSlot::ZERO`].
#[derive(Clone, Copy)]
pub struct DlErrorSlot {
    /// The message the last failing call left, not yet read: `malloc`ed, or
    /// [`NO_MEMORY`]'s text, or NULL.
    pending: *mut u8,
    /// The message the last `dlerror` returned, kept until the next.
    shown: *mut u8,
}

impl DlErrorSlot {
    /// No message.
    pub const ZERO: Self = Self {
        pending: core::ptr::null_mut(),
        shown: core::ptr::null_mut(),
    };
}

/// The message when there is no memory for the real one.
static NO_MEMORY: [u8; 14] = *b"out of memory\0";

fn slot() -> *mut DlErrorSlot {
    // SAFETY: the calling thread's own block, which no other thread touches.
    unsafe { &raw mut (*crate::perthread::current()).dlerror }
}

/// Free a message, unless it is [`NO_MEMORY`]'s.
///
/// # Safety
///
/// `m` must be NULL, `NO_MEMORY`'s, or a block `malloc` gave and nothing
/// else frees.
unsafe fn release(m: *mut u8) {
    if !m.is_null() && !core::ptr::eq(m.cast_const(), NO_MEMORY.as_ptr()) {
        // SAFETY: the caller's contract.
        unsafe { crate::malloc::free(m) };
    }
}

/// Leave `parts`, joined, as the calling thread's pending message.
fn fail(parts: &[&[u8]]) {
    let len = parts.iter().fold(0usize, |n, p| n.saturating_add(p.len()));
    let m = crate::malloc::malloc(len.saturating_add(1));
    let message = if m.is_null() {
        NO_MEMORY.as_ptr().cast_mut()
    } else {
        let mut at = 0usize;
        for p in parts {
            // SAFETY: `m` has `len + 1` bytes, and the parts sum to `len`.
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), m.add(at), p.len()) };
            at = at.saturating_add(p.len());
        }
        // SAFETY: as above; `at == len`.
        unsafe { m.add(at).write(0) };
        m
    };
    let s = slot();
    // SAFETY: the calling thread's slot; the message replaced was the slot's
    // own and is referred to by nothing now.
    unsafe {
        release((*s).pending);
        (*s).pending = message;
    }
}

/// The C string at `s` as bytes, NUL not included; "" for NULL.
///
/// # Safety
///
/// `s` must be NULL or a C string that outlives the result.
unsafe fn bytes<'a>(s: *const u8) -> &'a [u8] {
    if s.is_null() {
        return b"";
    }
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) }
}

/// The program's short name, as the undefined-symbol message names it.
fn program_name<'a>() -> &'a [u8] {
    // SAFETY: a plain read of the pointer, which `__libc_start_main` sets
    // once to argv[0]'s last component, or leaves at a static string.
    let p = unsafe { crate::crt::progname_slot().read() };
    // SAFETY: the start-up string or the static one, both the process's.
    unsafe { bytes(p) }
}

/// Free the calling thread's messages: called as the thread exits.
pub(crate) fn thread_cleanup() {
    let s = slot();
    // SAFETY: the thread's own slot, its messages its own, and nothing can
    // read them after the thread is gone.
    unsafe {
        release((*s).pending);
        release((*s).shown);
        s.write(DlErrorSlot::ZERO);
    }
}

// ---------------------------------------------------------------------------
// dlopen, dlmopen, dlclose
// ---------------------------------------------------------------------------

/// `dlopen(file, mode)`: the program's handle for a NULL or empty `file`;
/// NULL for any other, there being nothing that could load it.
///
/// # Safety
///
/// `file` must be NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dlopen(file: *const u8, mode: i32) -> *mut c_void {
    if mode & !MODE_BITS != 0 {
        fail(&[b"invalid mode parameter"]);
        return core::ptr::null_mut();
    }
    // SAFETY: this function's contract.
    unsafe { open(file, mode) }
}

/// What `dlopen` and `dlmopen` share, after `dlopen`'s check of the mode's
/// bits (which glibc's `dlmopen` does not make).
///
/// # Safety
///
/// As for [`dlopen`].
unsafe fn open(file: *const u8, mode: i32) -> *mut c_void {
    // SAFETY: the caller's contract.
    let name = unsafe { bytes(file) };
    if mode & RTLD_BINDING_MASK == 0 {
        if name.is_empty() {
            fail(&[b"invalid mode for dlopen(): Invalid argument"]);
        } else {
            fail(&[name, b": invalid mode for dlopen(): Invalid argument"]);
        }
        return core::ptr::null_mut();
    }
    if name.is_empty() {
        return program_handle();
    }
    // One of the C library's own parts: inside the program, and so loaded --
    // RTLD_NOLOAD's question is answered yes too.
    if is_own_name(name) {
        return program_handle();
    }
    // Not loaded, and not to be: that is the answer, not an error.
    if mode & RTLD_NOLOAD != 0 {
        return core::ptr::null_mut();
    }
    fail(&[
        name,
        b": cannot open shared object file: dynamic loading is not supported",
    ]);
    core::ptr::null_mut()
}

/// `dlmopen(lmid, file, mode)`: [`dlopen`] in namespace `lmid`, of which
/// there is one, [`LM_ID_BASE`]; any other is refused as glibc's static
/// `dlmopen` refuses it.
///
/// # Safety
///
/// As for [`dlopen`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dlmopen(lmid: i64, file: *const u8, mode: i32) -> *mut c_void {
    if lmid != LM_ID_BASE {
        fail(&[b"invalid namespace: Invalid argument"]);
        return core::ptr::null_mut();
    }
    // SAFETY: this function's contract.
    unsafe { open(file, mode) }
}

/// `dlclose(handle)`: 0 for the program's handle, which is never unloaded;
/// -1 for anything else, which no `dlopen` gave.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dlclose(handle: *mut c_void) -> i32 {
    if is_program(handle) {
        return 0;
    }
    fail(&[b"shared object not open"]);
    -1
}

// ---------------------------------------------------------------------------
// dlsym, dlvsym, dlerror
// ---------------------------------------------------------------------------

/// Why a lookup through `handle` finds nothing: the message it leaves.
///
/// # Safety
///
/// `name` and `version` must be NULL or C strings.
unsafe fn lookup_fails(handle: *mut c_void, name: *const u8, version: *const u8) {
    if core::ptr::eq(handle.cast_const(), RTLD_NEXT.cast_const()) {
        fail(&[b"RTLD_NEXT used in code not dynamically loaded"]);
        return;
    }
    if !handle.is_null() && !is_program(handle) {
        fail(&[b"shared object not open"]);
        return;
    }
    // SAFETY: the caller's contract.
    let (name, version) = unsafe { (bytes(name), bytes(version)) };
    if version.is_empty() {
        fail(&[program_name(), b": undefined symbol: ", name]);
    } else {
        fail(&[
            program_name(),
            b": undefined symbol: ",
            name,
            b", version ",
            version,
        ]);
    }
}

/// What `dlsym` and `dlvsym` answer, the program's headers being `ph` and
/// the calling thread's pointer `tp`: the address of the symbol the program
/// exports by that name (and version), through [`RTLD_DEFAULT`] or the
/// program's handle; else NULL, with the reason `dlerror` gives.
///
/// # Safety
///
/// `name` and `version` must be NULL or C strings.
unsafe fn lookup(
    ph: Option<ProgramHeaders>,
    tp: u64,
    handle: *mut c_void,
    name: *const u8,
    version: *const u8,
) -> *mut c_void {
    if handle.is_null() || is_program(handle) {
        // SAFETY: the caller's contract.
        let (n, v) = unsafe { (bytes(name), bytes(version)) };
        let v = (!version.is_null()).then_some(v);
        let found = ph
            .and_then(Exports::of)
            .and_then(|e| e.find(n, v).and_then(|s| e.address(&s, tp)));
        if let Some(p) = found {
            return p;
        }
    }
    // SAFETY: the caller's contract.
    unsafe { lookup_fails(handle, name, version) };
    core::ptr::null_mut()
}

/// `dlsym(handle, name)`: the address of `name`, which the program exports
/// if it was linked to (`--export-dynamic`); else NULL, with the reason
/// `dlerror` gives, as for a program that exports nothing.
///
/// # Safety
///
/// `name` must be NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dlsym(handle: *mut c_void, name: *const u8) -> *mut c_void {
    // SAFETY: this function's contract.
    unsafe {
        lookup(
            program_headers(),
            crate::tls::thread_pointer(),
            handle,
            name,
            core::ptr::null(),
        )
    }
}

/// `dlvsym(handle, name, version)`: as [`dlsym`], naming the version.
///
/// # Safety
///
/// `name` and `version` must be NULL or C strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dlvsym(
    handle: *mut c_void,
    name: *const u8,
    version: *const u8,
) -> *mut c_void {
    // SAFETY: this function's contract.
    unsafe {
        lookup(
            program_headers(),
            crate::tls::thread_pointer(),
            handle,
            name,
            version,
        )
    }
}

/// `dlerror()`: the calling thread's pending message, which it clears; NULL
/// if there is none. The string lasts until the thread's next `dlerror`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dlerror() -> *mut u8 {
    let s = slot();
    // SAFETY: the calling thread's slot; the message shown last is the
    // caller's no longer, as `dlerror`'s contract says.
    unsafe {
        release((*s).shown);
        (*s).shown = core::mem::replace(&mut (*s).pending, core::ptr::null_mut());
        (*s).shown
    }
}

// ---------------------------------------------------------------------------
// dlinfo
// ---------------------------------------------------------------------------

/// `RTLD_DI_ORIGIN`'s answer: the program's directory, from
/// `/proc/self/exe`, into `out`; its length.
fn origin(out: &mut [u8]) -> Option<usize> {
    let n = crate::file::readlink(
        c"/proc/self/exe".as_ptr().cast(),
        out.as_mut_ptr(),
        out.len(),
    );
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n > 0 && n < out.len())?;
    let slash = out.get(..n)?.iter().rposition(|&b| b == b'/')?;
    // "/prog" is in "/".
    Some(slash.max(1))
}

/// `dlinfo(handle, request, arg)`: what `request` asks about the program,
/// through `arg`; 0 (or, for [`RTLD_DI_PHDR`], the header count), or -1
/// with the reason `dlerror` gives.
///
/// # Safety
///
/// `arg` must be valid for what `request` writes: an `Lmid_t`, a pointer,
/// a `size_t`, a `Dl_serinfo` of the size `RTLD_DI_SERINFOSIZE` gives, or
/// for `RTLD_DI_ORIGIN` `PATH_MAX` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dlinfo(handle: *mut c_void, request: i32, arg: *mut c_void) -> i32 {
    if !is_program(handle) {
        fail(&[b"shared object not open"]);
        return -1;
    }
    let supported = matches!(
        request,
        RTLD_DI_LMID
            | RTLD_DI_LINKMAP
            | RTLD_DI_SERINFO
            | RTLD_DI_SERINFOSIZE
            | RTLD_DI_ORIGIN
            | RTLD_DI_TLS_MODID
            | RTLD_DI_TLS_DATA
            | RTLD_DI_PHDR
    );
    if !supported {
        fail(&[b"unsupported dlinfo request"]);
        return -1;
    }
    if arg.is_null() {
        fail(&[b"dlinfo: no place to store the answer"]);
        return -1;
    }
    let ph = program_headers();
    // SAFETY: for each request, `arg` is what this function's contract says
    // it is.
    unsafe {
        match request {
            RTLD_DI_LMID => arg.cast::<i64>().write(LM_ID_BASE),
            RTLD_DI_LINKMAP => arg.cast::<*mut c_void>().write(program_handle()),
            // No directories are searched, there being nothing to load:
            // glibc's answer for an empty path, a `Dl_serinfo` with no
            // `Dl_serpath`s in it.
            RTLD_DI_SERINFOSIZE => {
                let si = arg.cast::<DlSerinfo>();
                (&raw mut (*si).dls_size).write(core::mem::offset_of!(DlSerinfo, dls_serpath));
                (&raw mut (*si).dls_cnt).write(0);
            }
            RTLD_DI_SERINFO => {}
            RTLD_DI_ORIGIN => {
                let mut buf = [0u8; crate::unistd::PATH_MAX];
                let Some(n) = origin(&mut buf) else {
                    fail(&[b"cannot determine the program's directory"]);
                    return -1;
                };
                let out = arg.cast::<u8>();
                core::ptr::copy_nonoverlapping(buf.as_ptr(), out, n);
                out.add(n).write(0);
            }
            RTLD_DI_TLS_MODID => {
                let id = if ph.is_some_and(|h| has_tls(&h)) {
                    PROGRAM_TLS_MODULE as usize
                } else {
                    0
                };
                arg.cast::<usize>().write(id);
            }
            RTLD_DI_TLS_DATA => {
                let data = ph
                    .filter(has_tls)
                    .and_then(|h| tls_block(&h.tls_image(), crate::tls::thread_pointer()))
                    .unwrap_or(core::ptr::null_mut());
                arg.cast::<*mut c_void>().write(data);
            }
            // RTLD_DI_PHDR, the one request left.
            _ => {
                let Some(h) = ph else {
                    arg.cast::<*const c_void>().write(core::ptr::null());
                    return 0;
                };
                arg.cast::<*const c_void>().write(h.phdr().cast());
                return i32::try_from(h.phnum()).unwrap_or(i32::MAX);
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// dladdr, dladdr1
// ---------------------------------------------------------------------------

/// What `dladdr` found for an address the program holds: the exported
/// symbol holding it, if there is one.
struct Held {
    sym: Option<Sym>,
}

/// `dladdr`'s answer for `addr` among these headers: if one of the
/// program's segments holds it, `*info` filled in -- the exported symbol
/// holding it too, where the program exports any -- and that symbol's entry,
/// if there is one; `None` for an address the program does not hold.
///
/// # Safety
///
/// `info` must be valid for a `Dl_info` write.
unsafe fn addr_info(
    ph: Option<ProgramHeaders>,
    addr: *const c_void,
    info: *mut DlInfo,
) -> Option<Held> {
    let ph = ph?;
    segment_holding(&ph, addr.addr())?;
    let exports = Exports::of(ph);
    let sym = exports.and_then(|e| e.holding(addr.addr() as u64));
    let (sname, saddr) = match (exports, sym) {
        (Some(e), Some(s)) => (
            e.string(s.name).map_or(core::ptr::null(), <[u8]>::as_ptr),
            as_ptr(e.bias.wrapping_add(s.value)),
        ),
        _ => (core::ptr::null(), core::ptr::null_mut()),
    };
    // SAFETY: a plain read of the pointer `__libc_start_main` set to argv[0].
    let name = unsafe { crate::crt::progname_full_slot().read() };
    // SAFETY: the caller's contract. `sname` is NUL-terminated where it
    // lies, in the program's string table, which `string` found to end
    // inside the table.
    unsafe {
        info.write(DlInfo {
            dli_fname: name,
            dli_fbase: ph.ehdr().cast_mut().cast(),
            dli_sname: sname,
            dli_saddr: saddr,
        });
    }
    Some(Held { sym })
}

/// `dladdr(addr, info)`: for an address inside the program, 1, with the
/// program's name (argv[0], as glibc gives it) and ELF header in `*info`,
/// and -- where the program exports its symbols -- the name and address of
/// the one holding it; 0 for any other address.
///
/// # Safety
///
/// `info` must be valid for a `Dl_info` write.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dladdr(addr: *const c_void, info: *mut DlInfo) -> i32 {
    if info.is_null() {
        return 0;
    }
    // SAFETY: this function's contract.
    i32::from(unsafe { addr_info(program_headers(), addr, info) }.is_some())
}

/// `dladdr1(addr, info, extra, flags)`: [`dladdr`], and, where it finds the
/// address, for [`RTLD_DL_LINKMAP`] the program's link map in `*extra` and
/// for [`RTLD_DL_SYMENT`] the entry of the symbol it names -- NULL if none.
/// Other flags are 0's, as in glibc.
///
/// # Safety
///
/// `info` as for [`dladdr`]; `extra` valid for a pointer write when `flags`
/// asks for one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dladdr1(
    addr: *const c_void,
    info: *mut DlInfo,
    extra: *mut *mut c_void,
    flags: i32,
) -> i32 {
    if info.is_null() {
        return 0;
    }
    // SAFETY: this function's contract.
    let Some(held) = (unsafe { addr_info(program_headers(), addr, info) }) else {
        return 0;
    };
    if !extra.is_null() {
        // SAFETY: this function's contract.
        unsafe {
            match flags {
                RTLD_DL_LINKMAP => extra.write(program_handle()),
                RTLD_DL_SYMENT => {
                    extra.write(held.sym.map_or(core::ptr::null_mut(), |s| as_ptr(s.at)));
                }
                _ => {}
            }
        }
    }
    1
}

// ---------------------------------------------------------------------------
// dl_iterate_phdr, _dl_find_object
// ---------------------------------------------------------------------------

/// A `dl_iterate_phdr` callback.
pub type PhdrCallback = unsafe extern "C" fn(*mut DlPhdrInfo, usize, *mut c_void) -> i32;

/// What `dl_iterate_phdr` tells its callback of the program, whose headers
/// these are, for a thread whose thread pointer is `tp`.
fn program_info(ph: &ProgramHeaders, tp: u64) -> DlPhdrInfo {
    let tls = has_tls(ph);
    DlPhdrInfo {
        dlpi_addr: ph.load_bias(),
        dlpi_name: c"".as_ptr().cast(),
        dlpi_phdr: ph.phdr().cast(),
        dlpi_phnum: u16::try_from(ph.phnum()).unwrap_or(u16::MAX),
        // One object added -- the program -- and none removed: glibc's
        // counts, less its vDSO.
        dlpi_adds: 1,
        dlpi_subs: 0,
        dlpi_tls_modid: if tls { PROGRAM_TLS_MODULE as usize } else { 0 },
        dlpi_tls_data: if tls {
            tls_block(&ph.tls_image(), tp).unwrap_or(core::ptr::null_mut())
        } else {
            core::ptr::null_mut()
        },
    }
}

/// `dl_iterate_phdr` over these headers.
///
/// # Safety
///
/// As for [`dl_iterate_phdr`].
unsafe fn iterate(
    ph: Option<ProgramHeaders>,
    tp: u64,
    callback: Option<PhdrCallback>,
    data: *mut c_void,
) -> i32 {
    let (Some(ph), Some(callback)) = (ph, callback) else {
        return 0;
    };
    let mut info = program_info(&ph, tp);
    // SAFETY: this function's contract; `info` lives across the call.
    unsafe { callback(&raw mut info, core::mem::size_of::<DlPhdrInfo>(), data) }
}

/// `dl_iterate_phdr(callback, data)`: `callback` once, for the program --
/// the only object there is -- and its answer; 0 without calling it if the
/// program's headers are not in memory ([`crate::tls::program_headers`]).
///
/// # Safety
///
/// `callback` must be NULL or a function of the C signature, safe to call
/// with the `data` given.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dl_iterate_phdr(callback: Option<PhdrCallback>, data: *mut c_void) -> i32 {
    // SAFETY: this function's contract.
    unsafe {
        iterate(
            program_headers(),
            crate::tls::thread_pointer(),
            callback,
            data,
        )
    }
}

/// `_dl_find_object` over these headers.
///
/// # Safety
///
/// As for [`_dl_find_object`].
unsafe fn find_object(
    ph: Option<ProgramHeaders>,
    pc: *mut c_void,
    result: *mut DlFindObject,
) -> i32 {
    let Some(ph) = ph else { return -1 };
    let Some((start, end)) = segment_holding(&ph, pc.addr()) else {
        return -1;
    };
    let eh = ph.find(PT_GNU_EH_FRAME).map_or(core::ptr::null_mut(), |p| {
        as_ptr(ph.load_bias().wrapping_add(p.p_vaddr))
    });
    // SAFETY: the caller's contract. The reserved words are not written,
    // as glibc does not write them.
    unsafe {
        (&raw mut (*result).dlfo_flags).write(0);
        (&raw mut (*result).dlfo_map_start).write(as_ptr(start));
        (&raw mut (*result).dlfo_map_end).write(as_ptr(end));
        (&raw mut (*result).dlfo_link_map).write(program_handle().cast());
        (&raw mut (*result).dlfo_eh_frame).write(eh);
    }
    0
}

/// `_dl_find_object(pc, result)` (glibc 2.35): if the program holds `pc`, 0,
/// with the segment holding it -- glibc's static answer, a segment rather
/// than the whole image -- and the program's `.eh_frame_hdr` (NULL for a
/// program linked without one) in `*result`; else -1.
///
/// # Safety
///
/// `result` must be valid for a `struct dl_find_object` write.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _dl_find_object(pc: *mut c_void, result: *mut DlFindObject) -> i32 {
    if result.is_null() {
        return -1;
    }
    // SAFETY: this function's contract.
    unsafe { find_object(program_headers(), pc, result) }
}

// ---------------------------------------------------------------------------
// __tls_get_addr
// ---------------------------------------------------------------------------

/// The address `__tls_get_addr` answers for `(module, offset)`, with the
/// calling thread's block of the program's TLS at `block`.
fn tls_address(block: Option<*mut c_void>, module: u64, offset: u64) -> *mut c_void {
    match (block, usize::try_from(offset)) {
        (Some(b), Ok(off)) if module == PROGRAM_TLS_MODULE => {
            b.cast::<u8>().wrapping_add(off).cast()
        }
        _ => core::ptr::null_mut(),
    }
}

/// `__tls_get_addr(ti)`: the address of a thread-local variable in the
/// general-dynamic model, `ti` being its `tls_index`, `{module, offset}`:
/// the calling thread's block of module 1 -- the program's, the only one --
/// and `offset` into it; NULL for any other module.
///
/// A static link rewrites these calls into direct `%fs`-relative accesses,
/// so a program here reaches this only through code that calls it by name.
///
/// # Safety
///
/// `ti` must be NULL or point at two `u64`s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __tls_get_addr(ti: *const u64) -> *mut c_void {
    if ti.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: this function's contract.
    let (module, offset) = unsafe { (ti.read_unaligned(), ti.add(1).read_unaligned()) };
    let block = program_headers()
        .filter(has_tls)
        .and_then(|h| tls_block(&h.tls_image(), crate::tls::thread_pointer()));
    tls_address(block, module, offset)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::{PT_PHDR, PT_TLS, Phdr, SyntheticImage};
    use std::ffi::CStr;
    use std::string::{String, ToString};
    use std::vec::Vec;

    /// The calling thread's pending message, read with `dlerror`.
    fn take() -> Option<String> {
        let m = dlerror();
        if m.is_null() {
            return None;
        }
        // SAFETY: `dlerror`'s string, valid until the next call.
        Some(
            unsafe { CStr::from_ptr(m.cast()) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn c(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// What the host's program is called in messages: `crt`'s default.
    fn prog() -> String {
        String::from_utf8(program_name().to_vec()).unwrap()
    }

    // -- the oracle: glibc 2.39's static dlfcn -----------------------------

    /// Run the probe the oracle line names, as the harness ran it: the
    /// answer (`handle`, `NULL` or a number) and the message `dlerror` left.
    fn run_probe(probe: &str) -> (String, Option<String>) {
        let _ = take();
        let program = program_handle();
        let answer = |p: *mut c_void| {
            if p.is_null() {
                "NULL".to_string()
            } else if is_program(p) {
                "handle".to_string()
            } else {
                "other".to_string()
            }
        };
        let x = c("x");
        let v = c("GLIBC_2.2.5");
        let file = c("libnonexistent.so");
        // SAFETY: every string passed is NUL-terminated and outlives its call.
        let r = unsafe {
            match probe {
                "dlopen(NULL,RTLD_NOW)" => answer(dlopen(core::ptr::null(), RTLD_NOW)),
                "dlopen(NULL,RTLD_LAZY)" => answer(dlopen(core::ptr::null(), RTLD_LAZY)),
                "dlopen(NULL,0)" => answer(dlopen(core::ptr::null(), 0)),
                "dlopen(NULL,0x1234)" => answer(dlopen(core::ptr::null(), 0x1234)),
                "dlopen(NULL,RTLD_NOLOAD|RTLD_NOW)" => {
                    answer(dlopen(core::ptr::null(), RTLD_NOLOAD | RTLD_NOW))
                }
                "dlopen(NULL,RTLD_NOW|RTLD_GLOBAL|RTLD_NODELETE|RTLD_DEEPBIND)" => answer(dlopen(
                    core::ptr::null(),
                    RTLD_NOW | RTLD_GLOBAL | RTLD_NODELETE | RTLD_DEEPBIND,
                )),
                "dlopen(\"\",RTLD_NOW)" => answer(dlopen(c"".as_ptr().cast(), RTLD_NOW)),
                "dlopen(file,0)" => answer(dlopen(file.as_ptr(), 0)),
                "dlsym(program,x)" => answer(dlsym(program, x.as_ptr())),
                "dlsym(RTLD_DEFAULT,x)" => answer(dlsym(RTLD_DEFAULT, x.as_ptr())),
                "dlsym(RTLD_NEXT,x)" => answer(dlsym(RTLD_NEXT, x.as_ptr())),
                "dlvsym(program,x,GLIBC_2.2.5)" => answer(dlvsym(program, x.as_ptr(), v.as_ptr())),
                "dlvsym(RTLD_DEFAULT,x,GLIBC_2.2.5)" => {
                    answer(dlvsym(RTLD_DEFAULT, x.as_ptr(), v.as_ptr()))
                }
                "dlclose(program)" => dlclose(program).to_string(),
                "dlinfo(program,RTLD_DI_LMID)" => {
                    let mut id: i64 = 99;
                    let r = dlinfo(program, RTLD_DI_LMID, (&raw mut id).cast());
                    std::format!("{r} {id}")
                }
                "dlinfo(program,RTLD_DI_CONFIGADDR)" => {
                    let mut p: *mut c_void = core::ptr::null_mut();
                    dlinfo(program, RTLD_DI_CONFIGADDR, (&raw mut p).cast()).to_string()
                }
                "dlinfo(program,RTLD_DI_PROFILENAME)" => {
                    let mut p: *mut c_void = core::ptr::null_mut();
                    dlinfo(program, RTLD_DI_PROFILENAME, (&raw mut p).cast()).to_string()
                }
                "dlinfo(program,RTLD_DI_PROFILEOUT)" => {
                    let mut p: *mut c_void = core::ptr::null_mut();
                    dlinfo(program, RTLD_DI_PROFILEOUT, (&raw mut p).cast()).to_string()
                }
                "dlinfo(program,999)" => {
                    let mut p: *mut c_void = core::ptr::null_mut();
                    dlinfo(program, 999, (&raw mut p).cast()).to_string()
                }
                "dlmopen(LM_ID_BASE,NULL,RTLD_NOW)" => {
                    answer(dlmopen(LM_ID_BASE, core::ptr::null(), RTLD_NOW))
                }
                "dlmopen(LM_ID_NEWLM,NULL,RTLD_NOW)" => {
                    answer(dlmopen(LM_ID_NEWLM, core::ptr::null(), RTLD_NOW))
                }
                "dlmopen(5,NULL,RTLD_NOW)" => answer(dlmopen(5, core::ptr::null(), RTLD_NOW)),
                "dlmopen(LM_ID_NEWLM,file,RTLD_NOW)" => {
                    answer(dlmopen(LM_ID_NEWLM, file.as_ptr(), RTLD_NOW))
                }
                "dlmopen(LM_ID_BASE,NULL,0x1234|RTLD_NOW)" => {
                    answer(dlmopen(LM_ID_BASE, core::ptr::null(), 0x1234 | RTLD_NOW))
                }
                other => panic!("the oracle has a probe the test does not know: {other}"),
            }
        };
        (r, take())
    }

    /// Every probe glibc's static dlfcn was asked, answered as it answered:
    /// the result and the message, the program's name standing for glibc's.
    #[test]
    fn dlfcn_is_glibcs_static_one() {
        let oracle = include_str!("dlfcn_oracle.txt");
        let mut n = 0;
        for line in oracle
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (probe, rest) = line.split_once(" = ").expect("probe = answer | message");
            let (want, want_msg) = rest.split_once(" | ").expect("answer | message");
            let (got, msg) = run_probe(probe);
            assert_eq!(got, want, "{probe}");
            let want_msg = (want_msg != "-").then(|| want_msg.replace("<prog>", &prog()));
            assert_eq!(msg, want_msg, "{probe}");
            n += 1;
        }
        assert!(n >= 24, "the oracle has {n} probes");
    }

    // -- dlopen, dlclose -----------------------------------------------------

    #[test]
    fn the_program_is_one_handle() {
        // SAFETY: NULL and a C string.
        let (a, b) = unsafe {
            (
                dlopen(core::ptr::null(), RTLD_NOW),
                dlopen(c"".as_ptr().cast(), RTLD_LAZY),
            )
        };
        assert!(!a.is_null());
        assert_eq!(a, b);
        assert!(is_program(a));
        // Closing it is always allowed, and never unloads it.
        for _ in 0..3 {
            assert_eq!(dlclose(a), 0);
        }
        assert_eq!(take(), None);
    }

    /// A file names an object nothing can load; with `RTLD_NOLOAD` that it is
    /// not loaded is the answer, not an error.
    #[test]
    fn a_file_is_never_loaded() {
        for name in [
            "libfoo.so.1",
            "/usr/lib/libz.so.1",
            "libc.so",
            "libc.so.7",
            "xlibc.so.6",
        ] {
            let f = c(name);
            // SAFETY: a C string.
            assert!(unsafe { dlopen(f.as_ptr(), RTLD_NOW) }.is_null(), "{name}");
            let m = take().unwrap();
            assert!(
                m.starts_with(&std::format!("{name}: cannot open shared object file: ")),
                "{m}"
            );
            // SAFETY: as above.
            assert!(unsafe { dlopen(f.as_ptr(), RTLD_NOW | RTLD_NOLOAD) }.is_null());
            assert_eq!(take(), None);
        }
    }

    /// The C library's own parts are linked into the program: opening one,
    /// by name or by a path ending in it, opens the program, loaded already
    /// -- so `RTLD_NOLOAD` finds it too -- and leaves no message.
    #[test]
    fn the_c_librarys_own_names_open_the_program() {
        for name in [
            "libc.so.6",
            "libm.so.6",
            "libdl.so.2",
            "libpthread.so.0",
            "librt.so.1",
            "libutil.so.1",
            "libcrypt.so.1",
            "libresolv.so.2",
            "libanl.so.1",
            "/lib/x86_64-linux-gnu/libc.so.6",
            "/usr/lib64/libm.so.6",
        ] {
            let f = c(name);
            for mode in [RTLD_NOW, RTLD_LAZY | RTLD_GLOBAL, RTLD_NOW | RTLD_NOLOAD] {
                // SAFETY: a C string.
                let h = unsafe { dlopen(f.as_ptr(), mode) };
                assert!(is_program(h), "{name} {mode:#x}");
                assert_eq!(take(), None, "{name}");
                assert_eq!(dlclose(h), 0);
            }
        }
        // The mode is checked first, as for any other name.
        let f = c("libc.so.6");
        // SAFETY: as above.
        assert!(unsafe { dlopen(f.as_ptr(), 0) }.is_null());
        assert_eq!(
            take().as_deref(),
            Some("libc.so.6: invalid mode for dlopen(): Invalid argument")
        );
    }

    #[test]
    fn a_handle_no_dlopen_gave_is_refused() {
        let bogus = core::ptr::without_provenance_mut::<c_void>(0x1234);
        assert_eq!(dlclose(bogus), -1);
        assert_eq!(take().as_deref(), Some("shared object not open"));
        assert_eq!(dlclose(core::ptr::null_mut()), -1);
        assert_eq!(take().as_deref(), Some("shared object not open"));
        let x = c("x");
        // SAFETY: a C string.
        assert!(unsafe { dlsym(bogus, x.as_ptr()) }.is_null());
        assert_eq!(take().as_deref(), Some("shared object not open"));
        let mut p: *mut c_void = core::ptr::null_mut();
        // SAFETY: `p` takes a pointer.
        assert_eq!(
            unsafe { dlinfo(bogus, RTLD_DI_LINKMAP, (&raw mut p).cast()) },
            -1
        );
        assert_eq!(take().as_deref(), Some("shared object not open"));
    }

    // -- dlerror ---------------------------------------------------------------

    /// Read once: the second `dlerror` has nothing to say.
    #[test]
    fn a_message_is_read_once() {
        let x = c("y");
        // SAFETY: a C string.
        unsafe { dlsym(RTLD_DEFAULT, x.as_ptr()) };
        assert_eq!(
            take(),
            Some(std::format!("{}: undefined symbol: y", prog()))
        );
        assert_eq!(take(), None);
    }

    /// Each thread its own message, as in glibc: a thread starts with none,
    /// and what it leaves is not the other's to read.
    #[test]
    fn each_thread_has_its_own_message() {
        let x = c("main_pending");
        // SAFETY: a C string.
        unsafe { dlsym(RTLD_DEFAULT, x.as_ptr()) };
        let seen = std::thread::spawn(|| {
            let started = take();
            let t = c("t_sym");
            // SAFETY: a C string.
            unsafe { dlsym(RTLD_DEFAULT, t.as_ptr()) };
            let own = take();
            let y = c("left_by_thread");
            // SAFETY: a C string.
            unsafe { dlsym(RTLD_DEFAULT, y.as_ptr()) };
            thread_cleanup();
            (started, own)
        })
        .join()
        .unwrap();
        assert_eq!(seen.0, None);
        assert_eq!(
            seen.1,
            Some(std::format!("{}: undefined symbol: t_sym", prog()))
        );
        assert_eq!(
            take(),
            Some(std::format!("{}: undefined symbol: main_pending", prog()))
        );
    }

    /// A message left unread is freed when the next replaces it, and the one
    /// shown when the next `dlerror` comes: nothing accumulates.
    #[test]
    fn messages_do_not_accumulate() {
        // Start from a thread with nothing pending or shown, so that every
        // block counted below is one of this loop's.
        std::thread::spawn(|| {
            let before = crate::malloc::live_allocations::count();
            let x = c("x");
            for i in 0..100 {
                // SAFETY: a C string.
                unsafe { dlsym(RTLD_DEFAULT, x.as_ptr()) };
                // Every other message is replaced unread.
                if i % 2 == 0 {
                    assert!(take().is_some());
                }
            }
            // One pending, one shown: two blocks, however many came before.
            assert_eq!(crate::malloc::live_allocations::count() - before, 2);
            thread_cleanup();
            assert_eq!(crate::malloc::live_allocations::count(), before);
        })
        .join()
        .unwrap();
    }

    // -- dlinfo ------------------------------------------------------------------

    #[test]
    fn the_link_map_is_the_handle() {
        // SAFETY: NULL.
        let h = unsafe { dlopen(core::ptr::null(), RTLD_NOW) };
        let mut lm: *mut LinkMap = core::ptr::null_mut();
        // SAFETY: `lm` takes a pointer.
        assert_eq!(
            unsafe { dlinfo(h, RTLD_DI_LINKMAP, (&raw mut lm).cast()) },
            0
        );
        assert_eq!(lm.cast::<c_void>(), h);
        // SAFETY: the program's map, a static.
        let m = unsafe { &*lm };
        // SAFETY: its name, a static C string.
        assert_eq!(unsafe { CStr::from_ptr(m.l_name.cast()) }.to_bytes(), b"");
        assert!(m.l_ld.is_null() && m.l_next.is_null() && m.l_prev.is_null());
        // The host has no headers: no bias was ever computed.
        assert_eq!(m.l_addr.load(Ordering::Relaxed), 0);
    }

    /// With no program headers -- the host -- there is no TLS module, no
    /// block, and no header table to give.
    #[test]
    fn without_headers_there_is_no_tls_and_no_table() {
        // SAFETY: NULL.
        let h = unsafe { dlopen(core::ptr::null(), RTLD_NOW) };
        let mut id = 99usize;
        let mut data: *mut c_void = core::ptr::dangling_mut();
        let mut phdr: *const c_void = core::ptr::dangling();
        // SAFETY: each `arg` is what its request writes.
        unsafe {
            assert_eq!(dlinfo(h, RTLD_DI_TLS_MODID, (&raw mut id).cast()), 0);
            assert_eq!(dlinfo(h, RTLD_DI_TLS_DATA, (&raw mut data).cast()), 0);
            assert_eq!(dlinfo(h, RTLD_DI_PHDR, (&raw mut phdr).cast()), 0);
        }
        assert_eq!(id, 0);
        assert!(data.is_null());
        assert!(phdr.is_null());
    }

    /// No directories are searched: an empty path, glibc's size for one --
    /// the header before the first `Dl_serpath` -- and no entries.
    #[test]
    fn serinfosize_is_an_empty_path() {
        // SAFETY: NULL.
        let h = unsafe { dlopen(core::ptr::null(), RTLD_NOW) };
        let mut si = DlSerinfo {
            dls_size: 99,
            dls_cnt: 99,
            dls_serpath: [DlSerpath {
                dls_name: core::ptr::null_mut(),
                dls_flags: 0,
            }],
        };
        // SAFETY: `si` is a `Dl_serinfo`.
        assert_eq!(
            unsafe { dlinfo(h, RTLD_DI_SERINFOSIZE, (&raw mut si).cast()) },
            0
        );
        assert_eq!((si.dls_size, si.dls_cnt), (16, 0));
    }

    #[test]
    fn serinfo_writes_nothing_for_no_directories() {
        // SAFETY: NULL.
        let h = unsafe { dlopen(core::ptr::null(), RTLD_NOW) };
        let mut si = DlSerinfo {
            dls_size: 7,
            dls_cnt: 7,
            dls_serpath: [DlSerpath {
                dls_name: core::ptr::null_mut(),
                dls_flags: 7,
            }],
        };
        // SAFETY: `si` is a `Dl_serinfo`.
        assert_eq!(
            unsafe { dlinfo(h, RTLD_DI_SERINFO, (&raw mut si).cast()) },
            0
        );
        assert_eq!(
            (si.dls_size, si.dls_cnt, si.dls_serpath[0].dls_flags),
            (7, 7, 7)
        );
    }

    #[test]
    fn a_null_arg_is_refused() {
        // SAFETY: NULL.
        let h = unsafe { dlopen(core::ptr::null(), RTLD_NOW) };
        // SAFETY: a NULL `arg` is refused before anything is written.
        assert_eq!(
            unsafe { dlinfo(h, RTLD_DI_LMID, core::ptr::null_mut()) },
            -1
        );
        assert!(take().is_some());
    }

    // -- the program's headers: dl_iterate_phdr, dladdr, _dl_find_object --

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

    /// A program as lld links one at 0x400000, with `__thread` data and
    /// `.eh_frame_hdr` -- but lying in a heap block, so its "bias" is the
    /// block's distance from 0x400000.
    fn program(with_tls: bool) -> SyntheticImage {
        let mut v = std::vec![
            ph(PT_PHDR, 64, 0x40_0040, 56 * 6, 56 * 6, 8),
            ph(PT_LOAD, 0, 0x40_0000, 0x1000, 0x1000, 0x1000),
            ph(PT_LOAD, 0x1000, 0x40_1000, 0x3000, 0x3000, 0x1000),
            ph(PT_LOAD, 0x4000, 0x40_4000, 0x100, 0x800, 0x1000),
            ph(PT_GNU_EH_FRAME, 0x900, 0x40_0900, 0x40, 0x40, 4),
        ];
        if with_tls {
            v.push(ph(PT_TLS, 0x4000, 0x40_4000, 8, 24, 8));
        }
        SyntheticImage::new(&v)
    }

    /// What one call of the callback was told.
    struct Reported {
        addr: u64,
        name: Vec<u8>,
        phdr: usize,
        phnum: u16,
        adds: u64,
        subs: u64,
        modid: usize,
        data: usize,
        size: usize,
    }

    struct Seen {
        calls: u32,
        info: Option<Reported>,
        answer: i32,
    }

    unsafe extern "C" fn record(info: *mut DlPhdrInfo, size: usize, data: *mut c_void) -> i32 {
        // SAFETY: `data` is the test's `Seen`; `info` the call's own.
        unsafe {
            let seen = &mut *data.cast::<Seen>();
            let i = &*info;
            seen.calls += 1;
            seen.info = Some(Reported {
                addr: i.dlpi_addr,
                name: CStr::from_ptr(i.dlpi_name.cast()).to_bytes().to_vec(),
                phdr: i.dlpi_phdr.addr(),
                phnum: i.dlpi_phnum,
                adds: i.dlpi_adds,
                subs: i.dlpi_subs,
                modid: i.dlpi_tls_modid,
                data: i.dlpi_tls_data.addr(),
                size,
            });
            seen.answer
        }
    }

    /// The program, once, as glibc's static `dl_iterate_phdr` reports it
    /// (the oracle's `iterate` lines): its name "", its headers where they
    /// lie, its bias, TLS module 1 and the calling thread's block of it.
    #[test]
    fn dl_iterate_phdr_reports_the_program() {
        let img = program(true);
        let h = img.headers();
        let tp = 0x7000_0000u64;
        for answer in [0, 7, -3] {
            let mut seen = Seen {
                calls: 0,
                info: None,
                answer,
            };
            // SAFETY: `record` and a `Seen`.
            let r = unsafe { iterate(Some(h), tp, Some(record), (&raw mut seen).cast()) };
            assert_eq!(r, answer, "the callback's answer is the call's");
            assert_eq!(seen.calls, 1, "one object: the program");
            let r = seen.info.unwrap();
            assert_eq!(r.addr, img.addr().wrapping_sub(0x40_0000));
            assert_eq!(r.name, b"");
            assert_eq!(r.phdr as u64, img.addr() + 64);
            assert_eq!(r.phnum, 6);
            assert_eq!((r.adds, r.subs), (1, 0));
            assert_eq!(r.modid, 1);
            // 24 bytes aligned to 8: the block starts 24 below the pointer.
            assert_eq!(r.data as u64, tp - 24);
            assert_eq!(r.size, 64);
        }
        // No TLS: module 0, no block.
        let plain = program(false);
        let mut seen = Seen {
            calls: 0,
            info: None,
            answer: 0,
        };
        // SAFETY: as above.
        unsafe {
            iterate(
                Some(plain.headers()),
                tp,
                Some(record),
                (&raw mut seen).cast(),
            )
        };
        let r = seen.info.unwrap();
        assert_eq!((r.modid, r.data), (0, 0));
    }

    /// Without headers there is no object to report; with no callback,
    /// nothing to call.
    #[test]
    fn dl_iterate_phdr_without_headers_or_callback_calls_nothing() {
        let mut seen = Seen {
            calls: 0,
            info: None,
            answer: 5,
        };
        // SAFETY: as above.
        assert_eq!(
            unsafe { dl_iterate_phdr(Some(record), (&raw mut seen).cast()) },
            0
        );
        assert_eq!(seen.calls, 0);
        let img = program(true);
        // SAFETY: no callback.
        assert_eq!(
            unsafe { iterate(Some(img.headers()), 0, None, core::ptr::null_mut()) },
            0
        );
    }

    /// Inside a segment of the program, the program; outside every one --
    /// the stack, the gaps between segments, one past the end -- nothing.
    #[test]
    fn dladdr_knows_the_program_and_nothing_else() {
        let img = program(true);
        let bias = img.addr().wrapping_sub(0x40_0000);
        let at = |v: u64| as_ptr(bias.wrapping_add(v)).cast_const();
        let mut info = DlInfo {
            dli_fname: core::ptr::null(),
            dli_fbase: core::ptr::null_mut(),
            dli_sname: c"stale".as_ptr().cast(),
            dli_saddr: core::ptr::dangling_mut(),
        };
        for inside in [
            0x40_0000, 0x40_0fff, 0x40_1000, 0x40_3fff, 0x40_4000, 0x40_47ff,
        ] {
            // SAFETY: `info` is a `Dl_info`.
            assert!(
                matches!(
                    unsafe { addr_info(Some(img.headers()), at(inside), &raw mut info) },
                    Some(Held { sym: None })
                ),
                "{inside:#x}: held, and no symbol in a program that exports none"
            );
            assert_eq!(info.dli_fbase.addr() as u64, img.addr());
            assert!(info.dli_sname.is_null() && info.dli_saddr.is_null());
            // SAFETY: `program_invocation_name`, a C string.
            assert_eq!(
                unsafe { CStr::from_ptr(info.dli_fname.cast()) }.to_bytes(),
                // SAFETY: the same pointer, read directly.
                unsafe { CStr::from_ptr(crate::crt::progname_full_slot().read().cast()) }
                    .to_bytes()
            );
        }
        for outside in [0x3f_ffff, 0x40_4800, 0x40_5000, 0x7fff_0000] {
            // SAFETY: as above.
            assert!(
                unsafe { addr_info(Some(img.headers()), at(outside), &raw mut info) }.is_none(),
                "{outside:#x}"
            );
        }
        // SAFETY: as above; on the host there are no headers at all.
        assert_eq!(unsafe { dladdr(core::ptr::null(), &raw mut info) }, 0);
    }

    // -- the program's exported symbols ---------------------------------------

    /// A symbol for [`exports_image`]: what its table entry says.
    #[derive(Clone, Copy)]
    struct TSym {
        name: &'static str,
        value: u64,
        size: u64,
        kind: u8,
        bind: u8,
        vis: u8,
        shndx: u16,
        /// Its `.gnu.version` entry, where the image has versions.
        versym: u16,
    }

    /// A global function of no size, defined, unversioned.
    const fn tsym(name: &'static str, value: u64) -> TSym {
        TSym {
            name,
            value,
            size: 0,
            kind: STT_FUNC,
            bind: STB_GLOBAL,
            vis: STV_DEFAULT,
            shndx: 1,
            versym: 1,
        }
    }

    /// The hash table an image has, and its bucket count.
    #[derive(Clone, Copy)]
    enum Hash {
        Gnu(u32),
        Sysv(u32),
    }

    /// A program linked at 0x400000 with `--export-dynamic`, lying in a heap
    /// block: its headers, dynamic section, symbol and string tables, hash
    /// table and -- given any -- version tables, all in one loaded segment.
    struct Built {
        words: Vec<u64>,
        /// Each input symbol's index in the table.
        index: Vec<u64>,
    }

    impl Built {
        fn addr(&self) -> u64 {
            self.words.as_ptr().addr() as u64
        }

        fn bias(&self) -> u64 {
            self.addr().wrapping_sub(0x40_0000)
        }

        fn headers(&self) -> ProgramHeaders {
            // SAFETY: the block starts with an ELF header followed by the
            // table it describes, and lives as long as `self`.
            unsafe { ProgramHeaders::of_ehdr(self.words.as_ptr().cast()) }.unwrap()
        }

        fn exports(&self) -> Exports {
            Exports::of(self.headers()).expect("the image locates its tables")
        }

        /// What `dlsym` (`version` `None`) or `dlvsym` answers through the
        /// program's handle, the thread pointer being `tp`.
        fn lookup(&self, name: &str, version: Option<&str>, tp: u64) -> *mut c_void {
            let n = c(name);
            let v = version.map(c);
            let _ = take();
            // SAFETY: C strings that outlive the call.
            unsafe {
                lookup(
                    Some(self.headers()),
                    tp,
                    program_handle(),
                    n.as_ptr(),
                    v.as_ref().map_or(core::ptr::null(), |v| v.as_ptr()),
                )
            }
        }
    }

    fn put(b: &mut [u8], at: usize, bytes: &[u8]) {
        b[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// The image of [`Built`]. `versions` are the version definitions,
    /// `(index, name)`; `tls_memsz` gives the image a `PT_TLS` of that size;
    /// `dyn_extra` adds dynamic entries ahead of the end.
    fn exports_image(
        syms: &[TSym],
        hash: Hash,
        versions: &[(u16, &str)],
        tls_memsz: u64,
        dyn_extra: &[(u64, u64)],
    ) -> Built {
        const BASE: u64 = 0x40_0000;
        let align8 = |n: usize| n.div_ceil(8) * 8;
        // The GNU table wants each bucket's symbols together, in bucket order.
        let mut order: Vec<usize> = (0..syms.len()).collect();
        if let Hash::Gnu(nb) = hash {
            order.sort_by_key(|&i| gnu_hash(syms[i].name.as_bytes()) % nb);
        }
        let mut strtab = std::vec![0u8];
        let mut name_off = std::vec![0u32; syms.len()];
        for &i in &order {
            name_off[i] = u32::try_from(strtab.len()).unwrap();
            strtab.extend_from_slice(syms[i].name.as_bytes());
            strtab.push(0);
        }
        let ver_name: Vec<u32> = versions
            .iter()
            .map(|(_, n)| {
                let o = u32::try_from(strtab.len()).unwrap();
                strtab.extend_from_slice(n.as_bytes());
                strtab.push(0);
                o
            })
            .collect();
        let nsyms = syms.len() + 1;
        let mut symtab = std::vec![0u8; nsyms * 24];
        let mut index = std::vec![0u64; syms.len()];
        let mut versym = std::vec![0u8; nsyms * 2];
        for (k, &i) in order.iter().enumerate() {
            let s = syms[i];
            let e = (k + 1) * 24;
            put(&mut symtab, e, &name_off[i].to_le_bytes());
            symtab[e + 4] = (s.bind << 4) | s.kind;
            symtab[e + 5] = s.vis;
            put(&mut symtab, e + 6, &s.shndx.to_le_bytes());
            put(&mut symtab, e + 8, &s.value.to_le_bytes());
            put(&mut symtab, e + 16, &s.size.to_le_bytes());
            put(&mut versym, (k + 1) * 2, &s.versym.to_le_bytes());
            index[i] = (k + 1) as u64;
        }
        let mut hashtab = Vec::new();
        match hash {
            Hash::Gnu(nb) => {
                let shift = 6u32;
                let mut bloom = 0u64;
                let mut buckets = std::vec![0u32; nb as usize];
                let mut chain = std::vec![0u32; syms.len()];
                for (k, &i) in order.iter().enumerate() {
                    let h = gnu_hash(syms[i].name.as_bytes());
                    bloom |= (1u64 << (h % 64)) | (1u64 << ((h >> shift) % 64));
                    let b = (h % nb) as usize;
                    if buckets[b] == 0 {
                        buckets[b] = u32::try_from(k + 1).unwrap();
                    }
                    let last = order
                        .get(k + 1)
                        .is_none_or(|&j| gnu_hash(syms[j].name.as_bytes()) % nb != h % nb);
                    chain[k] = if last { h | 1 } else { h & !1 };
                }
                for w in [nb, 1, 1, shift] {
                    hashtab.extend_from_slice(&w.to_le_bytes());
                }
                hashtab.extend_from_slice(&bloom.to_le_bytes());
                for w in buckets.iter().chain(chain.iter()) {
                    hashtab.extend_from_slice(&w.to_le_bytes());
                }
            }
            Hash::Sysv(nb) => {
                let mut bucket = std::vec![0u32; nb as usize];
                let mut chain = std::vec![0u32; nsyms];
                for (k, &i) in order.iter().enumerate() {
                    let idx = u32::try_from(k + 1).unwrap();
                    let b = (elf_hash(syms[i].name.as_bytes()) % nb) as usize;
                    chain[idx as usize] = bucket[b];
                    bucket[b] = idx;
                }
                for w in [nb, u32::try_from(nsyms).unwrap()] {
                    hashtab.extend_from_slice(&w.to_le_bytes());
                }
                for w in bucket.iter().chain(chain.iter()) {
                    hashtab.extend_from_slice(&w.to_le_bytes());
                }
            }
        }
        // Elf64_Verdef (20 bytes), its one Elf64_Verdaux (8) after it.
        let mut verdef = Vec::new();
        for (k, (ndx, _)) in versions.iter().enumerate() {
            let next = if k + 1 < versions.len() { 28u32 } else { 0 };
            verdef.extend_from_slice(&1u16.to_le_bytes());
            verdef.extend_from_slice(&0u16.to_le_bytes());
            verdef.extend_from_slice(&ndx.to_le_bytes());
            verdef.extend_from_slice(&1u16.to_le_bytes());
            verdef.extend_from_slice(&0u32.to_le_bytes());
            verdef.extend_from_slice(&20u32.to_le_bytes());
            verdef.extend_from_slice(&next.to_le_bytes());
            verdef.extend_from_slice(&ver_name[k].to_le_bytes());
            verdef.extend_from_slice(&0u32.to_le_bytes());
        }
        // PT_PHDR, the tables' PT_LOAD, the code's PT_LOAD, PT_DYNAMIC, and
        // PT_TLS if asked for.
        let nph = if tls_memsz > 0 { 5 } else { 4 };
        let phdrs = 64;
        let dyn_off = align8(phdrs + 56 * nph);
        let ndyn = 6 + dyn_extra.len() + if versions.is_empty() { 0 } else { 3 };
        let symtab_off = align8(dyn_off + 16 * ndyn);
        let strtab_off = align8(symtab_off + symtab.len());
        let hash_off = align8(strtab_off + strtab.len());
        let versym_off = align8(hash_off + hashtab.len());
        let verdef_off = align8(versym_off + versym.len());
        let tls_off = align8(verdef_off + verdef.len());
        let total = align8(tls_off + usize::try_from(tls_memsz).unwrap() + 8);
        let at = |o: usize| BASE + o as u64;
        let mut b = std::vec![0u8; total];
        put(&mut b, 0x20, &64u64.to_le_bytes());
        put(&mut b, 0x36, &56u16.to_le_bytes());
        put(&mut b, 0x38, &u16::try_from(nph).unwrap().to_le_bytes());
        // The symbols' code and data, 0x401000-0x403000, is a segment of its
        // own: `dladdr` asks only whether it holds an address, so it needs
        // no memory behind it. The tables must end below it.
        assert!(total <= 0x1000, "the tables fit below the code");
        let mut phs = std::vec![
            ph(PT_PHDR, 64, at(64), (56 * nph) as u64, (56 * nph) as u64, 8),
            ph(PT_LOAD, 0, BASE, total as u64, total as u64, 0x1000),
            ph(PT_LOAD, 0x1000, BASE + 0x1000, 0x2000, 0x2000, 0x1000),
            ph(
                PT_DYNAMIC,
                dyn_off as u64,
                at(dyn_off),
                (16 * ndyn) as u64,
                (16 * ndyn) as u64,
                8
            ),
        ];
        if tls_memsz > 0 {
            phs.push(ph(PT_TLS, tls_off as u64, at(tls_off), 0, tls_memsz, 8));
        }
        for (k, p) in phs.iter().enumerate() {
            let e = phdrs + 56 * k;
            put(&mut b, e, &p.p_type.to_le_bytes());
            put(&mut b, e + 8, &p.p_offset.to_le_bytes());
            put(&mut b, e + 16, &p.p_vaddr.to_le_bytes());
            put(&mut b, e + 24, &p.p_vaddr.to_le_bytes());
            put(&mut b, e + 32, &p.p_filesz.to_le_bytes());
            put(&mut b, e + 40, &p.p_memsz.to_le_bytes());
            put(&mut b, e + 48, &p.p_align.to_le_bytes());
        }
        let hash_tag = if matches!(hash, Hash::Gnu(_)) {
            DT_GNU_HASH
        } else {
            DT_HASH
        };
        let mut dyns = std::vec![
            (DT_SYMTAB, at(symtab_off)),
            (DT_STRTAB, at(strtab_off)),
            (DT_STRSZ, strtab.len() as u64),
            (DT_SYMENT, SYM_SIZE),
            (hash_tag, at(hash_off)),
        ];
        if !versions.is_empty() {
            dyns.push((DT_VERSYM, at(versym_off)));
            dyns.push((DT_VERDEF, at(verdef_off)));
            dyns.push((DT_VERDEFNUM, versions.len() as u64));
        }
        dyns.extend_from_slice(dyn_extra);
        dyns.push((DT_NULL, 0));
        for (k, (tag, val)) in dyns.iter().enumerate() {
            put(&mut b, dyn_off + 16 * k, &tag.to_le_bytes());
            put(&mut b, dyn_off + 16 * k + 8, &val.to_le_bytes());
        }
        put(&mut b, symtab_off, &symtab);
        put(&mut b, strtab_off, &strtab);
        put(&mut b, hash_off, &hashtab);
        if !versions.is_empty() {
            put(&mut b, versym_off, &versym);
            put(&mut b, verdef_off, &verdef);
        }
        let mut words = std::vec![0u64; total / 8];
        for (i, byte) in b.iter().enumerate() {
            words[i / 8] |= u64::from(*byte) << (8 * (i % 8));
        }
        Built { words, index }
    }

    /// Symbols linked at these addresses, as the tests below lay them out.
    fn basics() -> Vec<TSym> {
        std::vec![
            TSym {
                size: 0x10,
                ..tsym("f1", 0x40_1000)
            },
            TSym {
                size: 0x20,
                ..tsym("f2", 0x40_1010)
            },
            tsym("z", 0x40_1100),
            TSym {
                kind: STT_OBJECT,
                size: 8,
                ..tsym("counter", 0x40_2000)
            },
            TSym {
                bind: STB_WEAK,
                ..tsym("weakling", 0x40_1200)
            },
            TSym {
                vis: 2,
                ..tsym("hidden_one", 0x40_1300)
            },
            TSym {
                bind: 0,
                ..tsym("local_one", 0x40_1400)
            },
            TSym {
                shndx: SHN_UNDEF,
                value: 0,
                ..tsym("undefined_one", 0)
            },
            TSym {
                kind: 3,
                ..tsym("a_section", 0x40_1500)
            },
            TSym {
                shndx: SHN_ABS,
                value: 0x5555,
                ..tsym("absolute", 0)
            },
        ]
    }

    /// Through either hash table, any bucket count: an exported definition
    /// is found where its value says, moved by the bias; a hidden, local,
    /// undefined or section symbol, and a name that is not there, are not --
    /// and leave `dlsym`'s message.
    #[test]
    fn dlsym_finds_what_the_program_exports() {
        for hash in [
            Hash::Gnu(1),
            Hash::Gnu(3),
            Hash::Gnu(7),
            Hash::Sysv(1),
            Hash::Sysv(5),
        ] {
            let img = exports_image(&basics(), hash, &[], 0, &[]);
            let bias = img.bias();
            for (name, value) in [
                ("f1", 0x40_1000u64),
                ("f2", 0x40_1010),
                ("z", 0x40_1100),
                ("counter", 0x40_2000),
                ("weakling", 0x40_1200),
            ] {
                let p = img.lookup(name, None, 0);
                assert_eq!(p.addr() as u64, bias.wrapping_add(value), "{name}");
                assert_eq!(take(), None, "{name}: a found symbol leaves no message");
            }
            // An absolute symbol is its value, the bias not applying.
            assert_eq!(img.lookup("absolute", None, 0).addr(), 0x5555);
            for name in [
                "hidden_one",
                "local_one",
                "undefined_one",
                "a_section",
                "nope",
                "f",
                "f12",
                "",
            ] {
                assert!(img.lookup(name, None, 0).is_null(), "{name}");
                assert_eq!(
                    take(),
                    Some(std::format!("{}: undefined symbol: {name}", prog())),
                    "{name}"
                );
            }
        }
    }

    /// `RTLD_DEFAULT` searches the program as its handle does; `RTLD_NEXT`
    /// and a handle no `dlopen` gave find nothing, with their own messages.
    #[test]
    fn dlsym_through_each_handle() {
        let img = exports_image(&basics(), Hash::Gnu(3), &[], 0, &[]);
        let n = c("f2");
        let want = img.bias().wrapping_add(0x40_1010);
        // SAFETY: C strings.
        unsafe {
            let p = lookup(
                Some(img.headers()),
                0,
                RTLD_DEFAULT,
                n.as_ptr(),
                core::ptr::null(),
            );
            assert_eq!(p.addr() as u64, want);
            let p = lookup(
                Some(img.headers()),
                0,
                RTLD_NEXT,
                n.as_ptr(),
                core::ptr::null(),
            );
            assert!(p.is_null());
            assert_eq!(
                take().as_deref(),
                Some("RTLD_NEXT used in code not dynamically loaded")
            );
            let bogus = core::ptr::without_provenance_mut::<c_void>(0x1234);
            let p = lookup(Some(img.headers()), 0, bogus, n.as_ptr(), core::ptr::null());
            assert!(p.is_null());
            assert_eq!(take().as_deref(), Some("shared object not open"));
            // No headers -- the host, or a program whose headers are not
            // mapped -- and so no table: glibc's static answer.
            let p = lookup(None, 0, RTLD_DEFAULT, n.as_ptr(), core::ptr::null());
            assert!(p.is_null());
            assert_eq!(
                take(),
                Some(std::format!("{}: undefined symbol: f2", prog()))
            );
        }
    }

    /// A TLS symbol is the calling thread's copy: its block below the thread
    /// pointer, the symbol's value into it. With no thread pointer -- or no
    /// `PT_TLS` -- there is no copy to give.
    #[test]
    fn a_tls_symbol_is_the_calling_threads_copy() {
        let syms = [TSym {
            kind: STT_TLS,
            value: 16,
            size: 8,
            ..tsym("tls_var", 0)
        }];
        let img = exports_image(&syms, Hash::Gnu(1), &[], 24, &[]);
        let tp = 0x7000_0000u64;
        assert_eq!(img.lookup("tls_var", None, tp).addr() as u64, tp - 24 + 16);
        assert!(img.lookup("tls_var", None, 0).is_null());
        let _ = take();
        let no_tls = exports_image(&syms, Hash::Gnu(1), &[], 0, &[]);
        assert!(no_tls.lookup("tls_var", None, tp).is_null());
    }

    extern "C" fn chosen_one() {}

    extern "C" fn pick() -> *mut c_void {
        chosen_one as *mut c_void
    }

    /// An IFUNC is what its resolver answers, as a dynamic linker binds one.
    #[test]
    fn an_ifunc_is_what_its_resolver_answers() {
        let mut img = exports_image(
            &[TSym {
                kind: STT_GNU_IFUNC,
                ..tsym("chosen", 1)
            }],
            Hash::Gnu(1),
            &[],
            0,
            &[],
        );
        // The bias depends on where the block lies, so the entry's value --
        // the resolver's link-time address -- is written once it is known:
        // st_value, eight bytes into the entry.
        let resolver: extern "C" fn() -> *mut c_void = pick;
        let value = (resolver as usize as u64).wrapping_sub(img.bias());
        let e = img.exports();
        let off = usize::try_from(e.symtab.wrapping_sub(img.addr())).unwrap()
            + usize::try_from(SYM_SIZE * img.index[0]).unwrap()
            + 8;
        for (k, byte) in value.to_le_bytes().iter().enumerate() {
            let i = off + k;
            img.words[i / 8] &= !(0xff << (8 * (i % 8)));
            img.words[i / 8] |= u64::from(*byte) << (8 * (i % 8));
        }
        assert_eq!(img.lookup("chosen", None, 0), chosen_one as *mut c_void);
    }

    /// The versions a program defines: dlvsym wants the one it names,
    /// hidden or not; dlsym an unversioned definition, else the one default
    /// version -- and none when two would answer.
    #[test]
    fn versions_are_glibcs() {
        const HIDDEN: u16 = VERSYM_HIDDEN;
        let syms = [
            TSym {
                versym: 2 | HIDDEN,
                ..tsym("foo", 0x40_1000)
            },
            TSym {
                versym: 3,
                ..tsym("foo", 0x40_1100)
            },
            TSym {
                versym: 1,
                ..tsym("bar", 0x40_1200)
            },
            TSym {
                versym: 2,
                ..tsym("baz", 0x40_1300)
            },
            TSym {
                versym: 2,
                ..tsym("qux", 0x40_1400)
            },
            TSym {
                versym: 3,
                ..tsym("qux", 0x40_1500)
            },
            TSym {
                versym: 2 | HIDDEN,
                ..tsym("old", 0x40_1600)
            },
        ];
        let versions = [(1, "libtest"), (2, "V1"), (3, "V2")];
        for hash in [Hash::Gnu(1), Hash::Gnu(4), Hash::Sysv(3)] {
            let img = exports_image(&syms, hash, &versions, 0, &[]);
            let b = img.bias();
            let addr = |p: *mut c_void| p.addr() as u64;
            assert_eq!(
                addr(img.lookup("foo", None, 0)),
                b.wrapping_add(0x40_1100),
                "foo: the default"
            );
            assert_eq!(
                addr(img.lookup("foo", Some("V1"), 0)),
                b.wrapping_add(0x40_1000),
                "foo@V1: hidden, named"
            );
            assert_eq!(
                addr(img.lookup("foo", Some("V2"), 0)),
                b.wrapping_add(0x40_1100)
            );
            assert!(img.lookup("foo", Some("V3"), 0).is_null());
            assert_eq!(
                take(),
                Some(std::format!(
                    "{}: undefined symbol: foo, version V3",
                    prog()
                ))
            );
            assert_eq!(addr(img.lookup("bar", None, 0)), b.wrapping_add(0x40_1200));
            assert!(
                img.lookup("bar", Some("V1"), 0).is_null(),
                "an unversioned bar is no bar@V1"
            );
            assert_eq!(addr(img.lookup("baz", None, 0)), b.wrapping_add(0x40_1300));
            assert!(
                img.lookup("qux", None, 0).is_null(),
                "two default versions: ambiguous"
            );
            assert_eq!(
                addr(img.lookup("qux", Some("V2"), 0)),
                b.wrapping_add(0x40_1500)
            );
            assert!(
                img.lookup("old", None, 0).is_null(),
                "only a hidden version"
            );
            assert_eq!(
                addr(img.lookup("old", Some("V1"), 0)),
                b.wrapping_add(0x40_1600)
            );
        }
        // Without versions, a version asked for is no obstacle: glibc takes
        // an unversioned definition from an object that has none.
        let plain = exports_image(&basics(), Hash::Gnu(2), &[], 0, &[]);
        assert_eq!(
            plain.lookup("f1", Some("GLIBC_2.2.5"), 0).addr() as u64,
            plain.bias().wrapping_add(0x40_1000)
        );
    }

    /// `dladdr` names the exported symbol holding an address: inside its
    /// size, or at its address for a symbol of none, the highest such; TLS,
    /// absolute, hidden and local symbols are never names.
    #[test]
    fn dladdr_names_the_symbol() {
        for hash in [Hash::Gnu(1), Hash::Gnu(5), Hash::Sysv(2)] {
            let img = exports_image(&basics(), hash, &[], 0, &[]);
            let b = img.bias();
            let at = |v: u64| as_ptr(b.wrapping_add(v)).cast_const();
            let mut info = DlInfo {
                dli_fname: core::ptr::null(),
                dli_fbase: core::ptr::null_mut(),
                dli_sname: core::ptr::null(),
                dli_saddr: core::ptr::null_mut(),
            };
            for (addr, name, start) in [
                (0x40_1000u64, Some("f1"), 0x40_1000u64),
                (0x40_100f, Some("f1"), 0x40_1000),
                (0x40_1010, Some("f2"), 0x40_1010),
                (0x40_102f, Some("f2"), 0x40_1010),
                (0x40_1030, None, 0),
                (0x40_1100, Some("z"), 0x40_1100),
                (0x40_1101, None, 0),
                (0x40_2004, Some("counter"), 0x40_2000),
                (0x40_1300, None, 0),
                (0x40_1400, None, 0),
            ] {
                // SAFETY: `info` is a `Dl_info`.
                let held = unsafe { addr_info(Some(img.headers()), at(addr), &raw mut info) };
                assert!(held.is_some(), "{addr:#x} is in the program");
                match name {
                    Some(n) => {
                        // SAFETY: a name from the image's string table.
                        let got = unsafe { CStr::from_ptr(info.dli_sname.cast()) };
                        assert_eq!(got.to_bytes(), n.as_bytes(), "{addr:#x}");
                        assert_eq!(info.dli_saddr.addr() as u64, b.wrapping_add(start));
                        let s = held.unwrap().sym.unwrap();
                        assert_eq!(s.at, img.exports().symtab + SYM_SIZE * s.index);
                    }
                    None => {
                        assert!(
                            info.dli_sname.is_null() && info.dli_saddr.is_null(),
                            "{addr:#x}"
                        );
                        assert!(held.unwrap().sym.is_none());
                    }
                }
            }
        }
    }

    /// A table that is not what it should be is no table: the dynamic
    /// section outside the loaded segment, entries of the wrong size, no hash
    /// table; and names past the end of the string table are no names.
    #[test]
    fn a_malformed_table_finds_nothing() {
        // DT_SYMENT other than 24.
        let img = exports_image(&basics(), Hash::Gnu(1), &[], 0, &[(DT_SYMENT, 16)]);
        assert!(Exports::of(img.headers()).is_none());
        // A string table claimed smaller than it is: the later names run off
        // its end, and are not found; the earlier still are.
        let names = exports_image(&basics(), Hash::Gnu(1), &[], 0, &[]);
        let e = names.exports();
        let short = Exports { strsz: 4, ..e };
        assert!(short.find(b"counter", None).is_none());
        // The tables moved off the end of every segment.
        let mut far = names.exports();
        far.symtab = far.symtab.wrapping_add(0x10_0000);
        assert!(far.find(b"f1", None).is_none());
        assert!(far.holding(names.bias().wrapping_add(0x40_1000)).is_none());
    }

    /// The hash functions are glibc's: values checked against its
    /// `dl_new_hash` and `_dl_elf_hash` for these names.
    #[test]
    fn the_hash_functions_are_glibcs() {
        assert_eq!(gnu_hash(b""), 5381);
        assert_eq!(gnu_hash(b"printf"), 0x156b_2bb8);
        assert_eq!(gnu_hash(b"exit"), 0x7c96_7e3f);
        assert_eq!(elf_hash(b""), 0);
        assert_eq!(elf_hash(b"printf"), 0x0779_05a6);
        assert_eq!(elf_hash(b"exit"), 0x0006_cf04);
    }

    /// The segment holding the address, and `.eh_frame_hdr` -- glibc's
    /// static answer (the oracle's `find` lines) -- or -1.
    #[test]
    fn dl_find_object_finds_the_segment_and_the_unwind_index() {
        let img = program(true);
        let bias = img.addr().wrapping_sub(0x40_0000);
        let at = |v: u64| as_ptr(bias.wrapping_add(v));
        let mut r = DlFindObject {
            dlfo_flags: 9,
            dlfo_map_start: core::ptr::null_mut(),
            dlfo_map_end: core::ptr::null_mut(),
            dlfo_link_map: core::ptr::null_mut(),
            dlfo_eh_frame: core::ptr::null_mut(),
            reserved: [0x5555; 7],
        };
        for (pc, start, end) in [
            (0x40_1234, 0x40_1000, 0x40_4000),
            (0x40_0000, 0x40_0000, 0x40_1000),
            (0x40_47ff, 0x40_4000, 0x40_4800),
        ] {
            // SAFETY: `r` is a `struct dl_find_object`.
            assert_eq!(
                unsafe { find_object(Some(img.headers()), at(pc), &raw mut r) },
                0
            );
            assert_eq!(r.dlfo_flags, 0);
            assert_eq!(r.dlfo_map_start, at(start));
            assert_eq!(r.dlfo_map_end, at(end));
            assert!(is_program(r.dlfo_link_map.cast()));
            assert_eq!(r.dlfo_eh_frame, at(0x40_0900));
            assert_eq!(
                r.reserved, [0x5555; 7],
                "the reserved words are not written"
            );
        }
        for pc in [0x40_4800, 0x3f_ffff] {
            // SAFETY: as above.
            assert_eq!(
                unsafe { find_object(Some(img.headers()), at(pc), &raw mut r) },
                -1
            );
        }
        // Linked without `.eh_frame_hdr`, as glibc's static programs are.
        let bare = SyntheticImage::new(&[ph(PT_LOAD, 0, 0x40_0000, 0x1000, 0x1000, 0x1000)]);
        let b = bare.addr().wrapping_sub(0x40_0000);
        // SAFETY: as above.
        assert_eq!(
            unsafe { find_object(Some(bare.headers()), as_ptr(b + 0x40_0010), &raw mut r) },
            0
        );
        assert!(r.dlfo_eh_frame.is_null());
    }

    #[test]
    fn tls_get_addr_answers_module_one_only() {
        let block = Some(core::ptr::without_provenance_mut::<c_void>(0x9000));
        assert_eq!(tls_address(block, 1, 0x10).addr(), 0x9010);
        assert!(tls_address(block, 2, 0x10).is_null());
        assert!(tls_address(block, 0, 0).is_null());
        assert!(tls_address(None, 1, 0).is_null());
        // On the host: no program TLS, and a NULL index answers NULL.
        let ti = [1u64, 0];
        // SAFETY: two u64s.
        assert!(unsafe { __tls_get_addr(ti.as_ptr()) }.is_null());
        // SAFETY: NULL.
        assert!(unsafe { __tls_get_addr(core::ptr::null()) }.is_null());
    }

    #[test]
    fn the_tls_block_is_below_the_thread_pointer() {
        let img = TlsImage {
            init_vaddr: 0,
            init_size: 4,
            mem_size: 20,
            align: 16,
        };
        assert_eq!(tls_block(&img, 0x1_0000).unwrap().addr(), 0x1_0000 - 32);
        assert!(tls_block(&img, 0).is_none(), "no thread pointer, no block");
        assert!(tls_block(&TlsImage::EMPTY, 0x1_0000).is_none());
    }

    // -- the C types' sizes, as glibc's headers give them -------------------

    #[test]
    fn the_types_are_glibcs_size() {
        assert_eq!(core::mem::size_of::<DlInfo>(), 32);
        assert_eq!(core::mem::size_of::<LinkMap>(), 40);
        assert_eq!(core::mem::size_of::<DlPhdrInfo>(), 64);
        assert_eq!(core::mem::size_of::<DlSerpath>(), 16);
        assert_eq!(core::mem::size_of::<DlSerinfo>(), 32);
        assert_eq!(core::mem::offset_of!(DlSerinfo, dls_serpath), 16);
        assert_eq!(core::mem::size_of::<DlFindObject>(), 96);
    }
}
