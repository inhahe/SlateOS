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
//! | `dlopen(file)` | NULL, `file: cannot open shared object file: ...`; with `RTLD_NOLOAD`, NULL and no error, a file not loaded being the answer asked for |
//! | `dlsym`, `dlvsym` | NULL, `program: undefined symbol: name`: a static program has no symbol table in memory to look in |
//! | `dlclose` | 0 for the program's handle, which is never unloaded |
//! | `dlinfo` | the program's link map, namespace, directory, TLS module and block, program headers |
//! | `dladdr`, `dladdr1` | for an address inside the program, the program's name and ELF header, and no symbol |
//! | `dl_iterate_phdr` | one call, the program's: its headers, load bias and TLS |
//! | `_dl_find_object` | the program's segment holding an address, and its `.eh_frame_hdr` |
//! | `dlmopen` | the base namespace only, as glibc's static one |
//!
//! `dladdr` is the one place this parts from glibc's static answer, which is
//! 0 for every address: glibc's static program map records no address
//! range. Its dynamic one says what POSIX asks for, that the program is an
//! object like any other, and that is what this says (design-decisions
//! §1147).
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
    let p = unsafe { core::ptr::addr_of!(crate::crt::program_invocation_short_name).read() };
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

/// `dlsym(handle, name)`: NULL -- the program's symbol table is not in
/// memory -- with the reason `dlerror` gives.
///
/// # Safety
///
/// `name` must be NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn dlsym(handle: *mut c_void, name: *const u8) -> *mut c_void {
    // SAFETY: this function's contract.
    unsafe { lookup_fails(handle, name, core::ptr::null()) };
    core::ptr::null_mut()
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
    unsafe { lookup_fails(handle, name, version) };
    core::ptr::null_mut()
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

/// `dladdr`'s answer for `addr` among these headers: 1 with `*info` filled
/// in if one of the program's segments holds it, else 0.
///
/// # Safety
///
/// `info` must be valid for a `Dl_info` write.
unsafe fn addr_info(ph: Option<ProgramHeaders>, addr: *const c_void, info: *mut DlInfo) -> i32 {
    let Some(ph) = ph else { return 0 };
    if segment_holding(&ph, addr.addr()).is_none() {
        return 0;
    }
    // SAFETY: a plain read of the pointer `__libc_start_main` set to argv[0].
    let name = unsafe { core::ptr::addr_of!(crate::crt::program_invocation_name).read() };
    // SAFETY: the caller's contract.
    unsafe {
        info.write(DlInfo {
            dli_fname: name,
            dli_fbase: ph.ehdr().cast_mut().cast(),
            dli_sname: core::ptr::null(),
            dli_saddr: core::ptr::null_mut(),
        });
    }
    1
}

/// `dladdr(addr, info)`: for an address inside the program, 1, with the
/// program's name (argv[0], as glibc gives it) and ELF header in `*info`,
/// and no symbol, a static program's symbol table not being in memory; 0
/// for any other address.
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
    unsafe { addr_info(program_headers(), addr, info) }
}

/// `dladdr1(addr, info, extra, flags)`: [`dladdr`], and, where it finds the
/// address, for [`RTLD_DL_LINKMAP`] the program's link map in `*extra` and
/// for [`RTLD_DL_SYMENT`] the symbol's entry -- none, NULL. Other flags are
/// 0's, as in glibc.
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
    // SAFETY: this function's contract.
    let found = unsafe { dladdr(addr, info) };
    if found != 0 && !extra.is_null() {
        // SAFETY: this function's contract.
        unsafe {
            match flags {
                RTLD_DL_LINKMAP => extra.write(program_handle()),
                RTLD_DL_SYMENT => extra.write(core::ptr::null_mut()),
                _ => {}
            }
        }
    }
    found
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
        let f = c("libm.so.6");
        // SAFETY: a C string.
        assert!(unsafe { dlopen(f.as_ptr(), RTLD_NOW) }.is_null());
        let m = take().unwrap();
        assert!(
            m.starts_with("libm.so.6: cannot open shared object file: "),
            "{m}"
        );
        // SAFETY: as above.
        assert!(unsafe { dlopen(f.as_ptr(), RTLD_NOW | RTLD_NOLOAD) }.is_null());
        assert_eq!(take(), None);
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
            assert_eq!(
                unsafe { addr_info(Some(img.headers()), at(inside), &raw mut info) },
                1,
                "{inside:#x}"
            );
            assert_eq!(info.dli_fbase.addr() as u64, img.addr());
            assert!(info.dli_sname.is_null() && info.dli_saddr.is_null());
            // SAFETY: `program_invocation_name`, a C string.
            assert_eq!(
                unsafe { CStr::from_ptr(info.dli_fname.cast()) }.to_bytes(),
                // SAFETY: the same pointer, read directly.
                unsafe {
                    CStr::from_ptr(
                        core::ptr::addr_of!(crate::crt::program_invocation_name)
                            .read()
                            .cast(),
                    )
                }
                .to_bytes()
            );
        }
        for outside in [0x3f_ffff, 0x40_4800, 0x40_5000, 0x7fff_0000] {
            // SAFETY: as above.
            assert_eq!(
                unsafe { addr_info(Some(img.headers()), at(outside), &raw mut info) },
                0,
                "{outside:#x}"
            );
        }
        // SAFETY: as above; on the host there are no headers at all.
        assert_eq!(unsafe { dladdr(core::ptr::null(), &raw mut info) }, 0);
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
