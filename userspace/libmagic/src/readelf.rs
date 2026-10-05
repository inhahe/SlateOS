//! libmagic's `readelf.c`: what an ELF file's headers say that rules cannot
//! -- how it is linked, its interpreter, its notes (the OS it is for, its build
//! ID), whether it is stripped -- read from the file itself, and appended to the
//! rules' description.
//!
//! The reading is upstream's, quirks included where they show: a section's
//! name is read through the header *before* it (so `.debug_info` is noticed
//! one section late), a program header's zero `p_vaddr` counts as 4, and the
//! PIE test edits the mode the rules' `${x?...}` reads -- which is how the
//! same file is a "pie executable" or a "shared object".

use std::fs::File;

use crate::buffer::{Buffer, pread};
use crate::funcs::{Errno, Ms, file_printable};
use crate::magic::*;
use crate::printf::Arg;

const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;

const ET_REL: u16 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const ET_CORE: u16 = 4;

const EM_SPARC: u16 = 2;
const EM_386: u16 = 3;
const EM_SPARC32PLUS: u16 = 18;
const EM_SPARCV9: u16 = 43;
const EM_IA_64: u16 = 50;
const EM_AMD64: u16 = 62;

const SHT_SYMTAB: u32 = 2;
const SHT_NOTE: u32 = 7;
const SHT_SUNW_CAP: u32 = 0x6fff_fff5;

const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;

const NT_PRPSINFO: u32 = 3;
const NT_AUXV: u32 = 6;
const NT_NETBSD_CORE_PROCINFO: u32 = 1;
const NT_NETBSD_VERSION: u32 = 1;
const NT_NETBSD_EMULATION: u32 = 2;
const NT_FREEBSD_VERSION: u32 = 1;
const NT_OPENBSD_VERSION: u32 = 1;
const NT_DRAGONFLY_VERSION: u32 = 1;
const NT_GNU_VERSION: u32 = 1;
const NT_GNU_BUILD_ID: u32 = 3;
const NT_NETBSD_PAX: u32 = 3;
const NT_NETBSD_MARCH: u32 = 5;
const NT_NETBSD_CMODEL: u32 = 6;
const NT_GO_BUILD_ID: u32 = 4;

const GNU_OS_LINUX: u32 = 0;
const GNU_OS_HURD: u32 = 1;
const GNU_OS_SOLARIS: u32 = 2;
const GNU_OS_KFREEBSD: u32 = 3;
const GNU_OS_KNETBSD: u32 = 4;

const DT_NEEDED: u64 = 1;
const DT_FLAGS_1: u64 = 0x6fff_fffb;
const DF_1_PIE: u64 = 0x0800_0000;

const AT_LINUX_UID: u64 = 11;
const AT_LINUX_EUID: u64 = 12;
const AT_LINUX_GID: u64 = 13;
const AT_LINUX_EGID: u64 = 14;
const AT_LINUX_PLATFORM: u64 = 15;
const AT_LINUX_EXECFN: u64 = 31;

const CA_SUNW_NULL: u64 = 0;
const CA_SUNW_HW_1: u64 = 1;
const CA_SUNW_SF_1: u64 = 2;
const SF1_SUNW_FPKNWN: u64 = 0x01;
const SF1_SUNW_FPUSED: u64 = 0x02;
const SF1_SUNW_MASK: u64 = 0x03;

const FLAGS_CORE_STYLE: u32 = 0x0003;
const FLAGS_DID_CORE: u32 = 0x0004;
const FLAGS_DID_OS_NOTE: u32 = 0x0008;
const FLAGS_DID_BUILD_ID: u32 = 0x0010;
const FLAGS_DID_CORE_STYLE: u32 = 0x0020;
const FLAGS_DID_NETBSD_PAX: u32 = 0x0040;
const FLAGS_DID_NETBSD_MARCH: u32 = 0x0080;
const FLAGS_DID_NETBSD_CMODEL: u32 = 0x0100;
const FLAGS_DID_NETBSD_EMULATION: u32 = 0x0200;
const FLAGS_DID_NETBSD_UNKNOWN: u32 = 0x0400;
const FLAGS_IS_CORE: u32 = 0x0800;
const FLAGS_DID_AUXV: u32 = 0x1000;

const OS_STYLE_SVR4: u32 = 0;
const OS_STYLE_FREEBSD: u32 = 1;
const OS_STYLE_NETBSD: u32 = 2;
const OS_STYLE_NAMES: [&[u8]; 3] = [b"SVR4", b"FreeBSD", b"NetBSD"];

/// `BUFSIZ`: the note and interpreter buffers.
const BUFSIZ: usize = 8192;

/// `SIZE_UNKNOWN`.
const SIZE_UNKNOWN: i64 = -1;

/// `prpsoffsets32` and `prpsoffsets64`: where a core's program name may sit,
/// larger offsets first.
const PRPSOFFSETS32: [usize; 7] = [100, 84, 44, 28, 48, 32, 8];
const PRPSOFFSETS64: [usize; 5] = [136, 120, 56, 40, 16];

/// The SunOS hardware capability names, by bit.
const CAP_DESC_SPARC: &[(u64, &str)] = &[
    (0x0001, "MUL32"),
    (0x0002, "DIV32"),
    (0x0004, "FSMULD"),
    (0x0008, "V8PLUS"),
    (0x0010, "POPC"),
    (0x0020, "VIS"),
    (0x0040, "VIS2"),
    (0x0080, "ASI_BLK_INIT"),
    (0x0100, "FMAF"),
    (0x4000, "FJFMAU"),
    (0x8000, "IMA"),
];

const CAP_DESC_386: &[(u64, &str)] = &[
    (0x0000_0001, "FPU"),
    (0x0000_0002, "TSC"),
    (0x0000_0004, "CX8"),
    (0x0000_0008, "SEP"),
    (0x0000_0010, "AMD_SYSC"),
    (0x0000_0020, "CMOV"),
    (0x0000_0040, "MMX"),
    (0x0000_0080, "AMD_MMX"),
    (0x0000_0100, "AMD_3DNow"),
    (0x0000_0200, "AMD_3DNowx"),
    (0x0000_0400, "FXSR"),
    (0x0000_0800, "SSE"),
    (0x0000_1000, "SSE2"),
    (0x0000_2000, "PAUSE"),
    (0x0000_4000, "SSE3"),
    (0x0000_8000, "MON"),
    (0x0001_0000, "CX16"),
    (0x0002_0000, "AHF"),
    (0x0004_0000, "TSCP"),
    (0x0008_0000, "AMD_SSE4A"),
    (0x0010_0000, "POPCNT"),
    (0x0020_0000, "AMD_LZCNT"),
    (0x0040_0000, "SSSE3"),
    (0x0080_0000, "SSE4.1"),
    (0x0100_0000, "SSE4.2"),
];

/// How a file's words are read: its class and whether its byte order is the
/// host's (`swap` is upstream's test against a little-endian host).
#[derive(Clone, Copy)]
struct Elf {
    clazz: u8,
    swap: bool,
}

impl Elf {
    fn u16(self, b: &[u8], at: usize) -> u16 {
        let w = [byte(b, at), byte(b, at + 1)];
        if self.swap { u16::from_be_bytes(w) } else { u16::from_le_bytes(w) }
    }
    fn u32(self, b: &[u8], at: usize) -> u32 {
        let w = [byte(b, at), byte(b, at + 1), byte(b, at + 2), byte(b, at + 3)];
        if self.swap { u32::from_be_bytes(w) } else { u32::from_le_bytes(w) }
    }
    fn u64(self, b: &[u8], at: usize) -> u64 {
        let mut w = [0u8; 8];
        for (i, x) in w.iter_mut().enumerate() {
            *x = byte(b, at + i);
        }
        if self.swap { u64::from_be_bytes(w) } else { u64::from_le_bytes(w) }
    }
    /// A word that is 32 bits in one class and 64 in the other.
    fn addr(self, b: &[u8], at32: usize, at64: usize) -> u64 {
        if self.clazz == ELFCLASS32 { u64::from(self.u32(b, at32)) } else { self.u64(b, at64) }
    }
    fn is32(self) -> bool {
        self.clazz == ELFCLASS32
    }
    /// `xph_sizeof`, `xsh_sizeof`, `xnh_sizeof`, `xdh_sizeof`, `xcap_sizeof`,
    /// `xauxv_sizeof`.
    fn ph_sizeof(self) -> usize {
        if self.is32() { 32 } else { 56 }
    }
    fn sh_sizeof(self) -> usize {
        if self.is32() { 40 } else { 64 }
    }
    fn dh_sizeof(self) -> usize {
        if self.is32() { 8 } else { 16 }
    }
    fn cap_sizeof(self) -> usize {
        if self.is32() { 8 } else { 16 }
    }
    fn auxv_sizeof(self) -> usize {
        if self.is32() { 8 } else { 16 }
    }
}

fn byte(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}

/// A program header's fields, as the `xph_*` macros read them.
struct Phdr {
    typ: u32,
    offset: i64,
    align: u64,
    vaddr: u64,
    filesz: u64,
}

impl Phdr {
    #[allow(clippy::cast_possible_wrap)]
    fn read(e: Elf, b: &[u8]) -> Phdr {
        if e.is32() {
            let align = e.u32(b, 28);
            let vaddr = e.u32(b, 8);
            Phdr {
                typ: e.u32(b, 0),
                offset: i64::from(e.u32(b, 4)),
                align: if align != 0 { u64::from(align) } else { 4 },
                vaddr: if vaddr != 0 { u64::from(vaddr) } else { 4 },
                filesz: u64::from(e.u32(b, 16)),
            }
        } else {
            let align = e.u64(b, 48);
            let vaddr = e.u64(b, 16);
            Phdr {
                typ: e.u32(b, 0),
                offset: e.u64(b, 8) as i64,
                align: if align != 0 { align } else { 4 },
                vaddr: if vaddr != 0 { vaddr } else { 4 },
                filesz: e.u64(b, 32),
            }
        }
    }
}

/// A section header's fields, as the `xsh_*` macros read them.
#[derive(Default)]
struct Shdr {
    name: u32,
    typ: u32,
    offset: i64,
    size: u64,
}

impl Shdr {
    #[allow(clippy::cast_possible_wrap)]
    fn read(e: Elf, b: &[u8]) -> Shdr {
        if e.is32() {
            Shdr {
                name: e.u32(b, 0),
                typ: e.u32(b, 4),
                offset: i64::from(e.u32(b, 16)),
                size: u64::from(e.u32(b, 20)),
            }
        } else {
            Shdr {
                name: e.u32(b, 0),
                typ: e.u32(b, 4),
                offset: e.u64(b, 24) as i64,
                size: e.u64(b, 32),
            }
        }
    }
}

/// `pread`: `None` is -1; a negative offset is C's `EINVAL`.
fn pread_at(errno: &mut Option<Errno>, fd: &File, buf: &mut [u8], off: i64) -> Option<usize> {
    let Ok(off) = u64::try_from(off) else {
        *errno = Some(Errno::Kind(std::io::ErrorKind::InvalidInput));
        return None;
    };
    match pread(fd, buf, off) {
        Ok(n) => Some(n),
        Err(e) => {
            *errno = Some(Errno::of(&e));
            None
        }
    }
}

/// `errno` as a `file_error` argument: the error it would name, if any.
fn errno_error(errno: Option<Errno>) -> Option<std::io::Error> {
    errno.map(Errno::to_error)
}

/// C's `strcmp(&nbuf[at], s) == 0`, past the buffer reading zeros.
fn cstr_eq(nbuf: &[u8], at: usize, s: &[u8]) -> bool {
    for (i, &c) in s.iter().chain(std::iter::once(&0u8)).enumerate() {
        if byte(nbuf, at + i) != c {
            return false;
        }
    }
    true
}

/// `toomany`.
fn toomany(ms: &mut Ms, name: &str, num: u16) -> i32 {
    if ms.flags & MAGIC_MIME != 0 {
        return 1;
    }
    if ms.printf(b", too many %s (%u)", &[Arg::Str(name.as_bytes()), Arg::I32(u32::from(num))]) == -1 {
        return -1;
    }
    1
}

/// The state the note readers share: `flags` and the notes left to read.
struct NoteCx<'f> {
    e: Elf,
    flags: u32,
    notecount: u16,
    fd: &'f File,
    ph_off: i64,
    ph_num: u32,
    fsize: i64,
}

/// `dophn_core`: a core file's notes, through its program headers.
fn dophn_core(ms: &mut Ms, cx: &mut NoteCx<'_>, off: i64, num: u16, size: usize) -> i32 {
    let e = cx.e;
    if ms.flags & MAGIC_MIME != 0 {
        return 0;
    }
    if num == 0 {
        return if ms.print_str(b", no program header") == -1 { -1 } else { 0 };
    }
    if size != e.ph_sizeof() {
        return if ms.print_str(b", corrupted program header size") == -1 { -1 } else { 0 };
    }
    let mut off = off;
    let mut ph = vec![0u8; e.ph_sizeof()];
    for _ in 0..num {
        if pread_at(&mut ms.errno, cx.fd, &mut ph, off).is_none_or(|n| n < ph.len()) {
            return if ms.printf(b", can't read elf program headers at %jd", &[Arg::I64(off as u64)]) == -1 {
                -1
            } else {
                0
            };
        }
        off = off.wrapping_add(size as i64);
        let p = Phdr::read(e, &ph);
        if cx.fsize != SIZE_UNKNOWN && p.offset > cx.fsize {
            continue;
        }
        if p.typ != PT_NOTE {
            continue;
        }
        // A PT_NOTE: every note in it.
        let len = usize::try_from(p.filesz).map_or(BUFSIZ, |f| f.min(BUFSIZ));
        let mut nbuf = vec![0u8; len];
        let Some(bufsize) = pread_at(&mut ms.errno, cx.fd, &mut nbuf, p.offset) else {
            return if ms.printf(b" can't read note section at %jd", &[Arg::I64(p.offset as u64)]) == -1 {
                -1
            } else {
                0
            };
        };
        nbuf.truncate(bufsize);
        let mut offset = 0usize;
        loop {
            if offset >= bufsize {
                break;
            }
            offset = donote(ms, cx, &nbuf, offset, bufsize, 4, true);
            if offset == 0 {
                break;
            }
        }
    }
    0
}

/// `do_note_netbsd_version`.
fn do_note_netbsd_version(ms: &mut Ms, desc: u32) -> i32 {
    if ms.print_str(b", for NetBSD") == -1 {
        return -1;
    }
    // `MMmmrrpp00` since the stuck 199905.
    if desc > 100_000_000 {
        let mut ver_patch = (desc / 100) % 100;
        let mut ver_rel = (desc / 10000) % 100;
        let ver_min = (desc / 1_000_000) % 100;
        let ver_maj = desc / 100_000_000;
        if ms.printf(b" %u.%u", &[Arg::I32(ver_maj), Arg::I32(ver_min)]) == -1 {
            return -1;
        }
        if ver_maj >= 9 {
            ver_patch += 100 * ver_rel;
            ver_rel = 0;
        }
        if ver_rel == 0 && ver_patch != 0 {
            if ms.printf(b".%u", &[Arg::I32(ver_patch)]) == -1 {
                return -1;
            }
        } else if ver_rel != 0 {
            while ver_rel > 26 {
                if ms.print_str(b"Z") == -1 {
                    return -1;
                }
                ver_rel -= 26;
            }
            if ms.printf(b"%c", &[Arg::I32(u32::from(b'A') + ver_rel - 1)]) == -1 {
                return -1;
            }
        }
    }
    0
}

/// `do_note_freebsd_version`: `__FreeBSD_version`, by the Porter's
/// Handbook's scheme.
fn do_note_freebsd_version(ms: &mut Ms, desc: u32) -> i32 {
    if ms.print_str(b", for FreeBSD") == -1 {
        return -1;
    }
    let d = |v: u32| Arg::I32(v);
    if desc == 460_002 {
        if ms.print_str(b" 4.6.2") == -1 {
            return -1;
        }
    } else if desc < 460_100 {
        if ms.printf(b" %d.%d", &[d(desc / 100_000), d(desc / 10000 % 10)]) == -1 {
            return -1;
        }
        if !(desc / 1000).is_multiple_of(10) && ms.printf(b".%d", &[d(desc / 1000 % 10)]) == -1 {
            return -1;
        }
        if (!desc.is_multiple_of(1000) || desc.is_multiple_of(100_000)) && ms.printf(b" (%d)", &[d(desc)]) == -1 {
            return -1;
        }
    } else if desc < 500_000 {
        if ms.printf(b" %d.%d", &[d(desc / 100_000), d(desc / 10000 % 10 + desc / 1000 % 10)]) == -1 {
            return -1;
        }
        if !(desc / 100).is_multiple_of(10) {
            if ms.printf(b" (%d)", &[d(desc)]) == -1 {
                return -1;
            }
        } else if !(desc / 10).is_multiple_of(10) && ms.printf(b".%d", &[d(desc / 10 % 10)]) == -1 {
            return -1;
        }
    } else {
        if ms.printf(b" %d.%d", &[d(desc / 100_000), d(desc / 1000 % 100)]) == -1 {
            return -1;
        }
        if !(desc / 100).is_multiple_of(10) || desc % 100_000 / 100 == 0 {
            if ms.printf(b" (%d)", &[d(desc)]) == -1 {
                return -1;
            }
        } else if !(desc / 10).is_multiple_of(10) && ms.printf(b".%d", &[d(desc / 10 % 10)]) == -1 {
            return -1;
        }
    }
    0
}

/// The note being read: its type, sizes, and where its name and contents
/// are in the buffer.
#[derive(Clone, Copy)]
struct Note {
    typ: u32,
    namesz: u32,
    descsz: u32,
    noff: usize,
    doff: usize,
}

/// `do_bid_note`: a GNU or Go build ID.
fn do_bid_note(ms: &mut Ms, nbuf: &[u8], n: Note, flags: &mut u32) -> i32 {
    if n.namesz == 4 && cstr_eq(nbuf, n.noff, b"GNU") && n.typ == NT_GNU_BUILD_ID && (4..=20).contains(&n.descsz) {
        *flags |= FLAGS_DID_BUILD_ID;
        let btype: &[u8] = match n.descsz {
            8 => b"xxHash",
            16 => b"md5/uuid",
            20 => b"sha1",
            _ => b"unknown",
        };
        if ms.printf(b", BuildID[%s]=", &[Arg::Str(btype)]) == -1 {
            return -1;
        }
        for i in 0..n.descsz as usize {
            if ms.printf(b"%02x", &[Arg::I32(u32::from(byte(nbuf, n.doff + i)))]) == -1 {
                return -1;
            }
        }
        return 1;
    }
    if n.namesz == 4 && cstr_eq(nbuf, n.noff, b"Go") && n.typ == NT_GO_BUILD_ID && n.descsz < 128 {
        // `file_copystr`: `descsz` bytes, as a C string.
        let id: Vec<u8> = (0..n.descsz as usize).map(|i| byte(nbuf, n.doff + i)).collect();
        if ms.printf(b", Go BuildID=%s", &[Arg::Str(&id)]) == -1 {
            return -1;
        }
        return 1;
    }
    0
}

/// `do_os_note`: the OS a binary is for.
fn do_os_note(ms: &mut Ms, e: Elf, nbuf: &[u8], n: Note, flags: &mut u32) -> i32 {
    if n.namesz == 5 && cstr_eq(nbuf, n.noff, b"SuSE") && n.typ == NT_GNU_VERSION && n.descsz == 2 {
        *flags |= FLAGS_DID_OS_NOTE;
        let a = [Arg::I32(u32::from(byte(nbuf, n.doff))), Arg::I32(u32::from(byte(nbuf, n.doff + 1)))];
        return if ms.printf(b", for SuSE %d.%d", &a) == -1 { -1 } else { 1 };
    }
    if n.namesz == 4 && cstr_eq(nbuf, n.noff, b"GNU") && n.typ == NT_GNU_VERSION && n.descsz == 16 {
        *flags |= FLAGS_DID_OS_NOTE;
        if ms.print_str(b", for GNU/") == -1 {
            return -1;
        }
        let os: &[u8] = match e.u32(nbuf, n.doff) {
            GNU_OS_LINUX => b"Linux",
            GNU_OS_HURD => b"Hurd",
            GNU_OS_SOLARIS => b"Solaris",
            GNU_OS_KFREEBSD => b"kFreeBSD",
            GNU_OS_KNETBSD => b"kNetBSD",
            _ => b"<unknown>",
        };
        if ms.print_str(os) == -1 {
            return -1;
        }
        let v = [
            Arg::I32(e.u32(nbuf, n.doff + 4)),
            Arg::I32(e.u32(nbuf, n.doff + 8)),
            Arg::I32(e.u32(nbuf, n.doff + 12)),
        ];
        return if ms.printf(b" %d.%d.%d", &v) == -1 { -1 } else { 1 };
    }
    if n.namesz == 7 && cstr_eq(nbuf, n.noff, b"NetBSD") && n.typ == NT_NETBSD_VERSION && n.descsz == 4 {
        *flags |= FLAGS_DID_OS_NOTE;
        return if do_note_netbsd_version(ms, e.u32(nbuf, n.doff)) == -1 { -1 } else { 1 };
    }
    if n.namesz == 8 && cstr_eq(nbuf, n.noff, b"FreeBSD") && n.typ == NT_FREEBSD_VERSION && n.descsz == 4 {
        *flags |= FLAGS_DID_OS_NOTE;
        return if do_note_freebsd_version(ms, e.u32(nbuf, n.doff)) == -1 { -1 } else { 1 };
    }
    if n.namesz == 8 && cstr_eq(nbuf, n.noff, b"OpenBSD") && n.typ == NT_OPENBSD_VERSION && n.descsz == 4 {
        *flags |= FLAGS_DID_OS_NOTE;
        // The note's content is always 0.
        return if ms.print_str(b", for OpenBSD") == -1 { -1 } else { 1 };
    }
    if n.namesz == 10 && cstr_eq(nbuf, n.noff, b"DragonFly") && n.typ == NT_DRAGONFLY_VERSION && n.descsz == 4 {
        *flags |= FLAGS_DID_OS_NOTE;
        if ms.print_str(b", for DragonFly") == -1 {
            return -1;
        }
        let desc = e.u32(nbuf, n.doff);
        let v = [Arg::I32(desc / 100_000), Arg::I32(desc / 10000 % 10), Arg::I32(desc % 10000)];
        return if ms.printf(b" %d.%d.%d", &v) == -1 { -1 } else { 1 };
    }
    0
}

/// `do_pax_note`: NetBSD's PaX flags.
fn do_pax_note(ms: &mut Ms, e: Elf, nbuf: &[u8], n: Note, flags: &mut u32) -> i32 {
    const PAX: [&[u8]; 6] = [b"+mprotect", b"-mprotect", b"+segvguard", b"-segvguard", b"+ASLR", b"-ASLR"];
    if n.namesz == 4 && cstr_eq(nbuf, n.noff, b"PaX") && n.typ == NT_NETBSD_PAX && n.descsz == 4 {
        *flags |= FLAGS_DID_NETBSD_PAX;
        let desc = e.u32(nbuf, n.doff);
        if desc != 0 && ms.print_str(b", PaX: ") == -1 {
            return -1;
        }
        let mut did = false;
        for (i, p) in PAX.iter().enumerate() {
            if (1u32 << i) & desc == 0 {
                continue;
            }
            let sep: &[u8] = if did { b"," } else { b"" };
            did = true;
            if ms.printf(b"%s%s", &[Arg::Str(sep), Arg::Str(p)]) == -1 {
                return -1;
            }
        }
        return 1;
    }
    0
}

/// `do_core_note`: a core file's style, and the program that dropped it.
#[allow(clippy::too_many_lines)]
fn do_core_note(ms: &mut Ms, e: Elf, nbuf: &[u8], n: Note, flags: &mut u32, size: usize) -> i32 {
    let mut os_style: Option<u32> = None;
    // Linux 2.0.36 did not terminate the name; four bytes of CORE count.
    let name4 = (0..4).map(|i| byte(nbuf, n.noff + i)).collect::<Vec<u8>>();
    if (n.namesz == 4 && name4 == b"CORE") || (n.namesz == 5 && cstr_eq(nbuf, n.noff, b"CORE")) {
        os_style = Some(OS_STYLE_SVR4);
    }
    if n.namesz == 8 && cstr_eq(nbuf, n.noff, b"FreeBSD") {
        os_style = Some(OS_STYLE_FREEBSD);
    }
    if n.namesz >= 11 && (0..11).map(|i| byte(nbuf, n.noff + i)).eq(b"NetBSD-CORE".iter().copied()) {
        os_style = Some(OS_STYLE_NETBSD);
    }
    if let Some(style) = os_style {
        if *flags & FLAGS_DID_CORE_STYLE == 0 {
            if ms.printf(b", %s-style", &[Arg::Str(OS_STYLE_NAMES[style as usize])]) == -1 {
                return -1;
            }
            *flags |= FLAGS_DID_CORE_STYLE;
            *flags |= style;
        }
    }
    match os_style {
        Some(OS_STYLE_NETBSD) => {
            if n.typ == NT_NETBSD_CORE_PROCINFO {
                let mut pi = [0u8; 160];
                for (i, slot) in pi.iter_mut().enumerate().take((n.descsz as usize).min(160)) {
                    *slot = byte(nbuf, n.doff + i);
                }
                let name = file_printable(ms.flags & MAGIC_RAW != 0, 512, &pi[124..156], 32);
                let args = [
                    Arg::Str(&name),
                    Arg::I32(e.u32(&pi, 80)),
                    Arg::I32(e.u32(&pi, 100)),
                    Arg::I32(e.u32(&pi, 112)),
                    Arg::I32(e.u32(&pi, 120)),
                    Arg::I32(e.u32(&pi, 156)),
                    Arg::I32(e.u32(&pi, 8)),
                    Arg::I32(e.u32(&pi, 12)),
                ];
                if ms.printf(
                    b", from '%.31s', pid=%u, uid=%u, gid=%u, nlwps=%u, lwp=%u (signal %u/code %u)",
                    &args,
                ) == -1
                {
                    return -1;
                }
                *flags |= FLAGS_DID_CORE;
                return 1;
            }
        }
        Some(OS_STYLE_FREEBSD) => {
            if n.typ == NT_PRPSINFO && *flags & FLAGS_IS_CORE != 0 {
                let argoff = if e.is32() { 4 + 4 + 17 } else { 4 + 4 + 8 + 17 };
                let from = nbuf.get(n.doff + argoff..).unwrap_or_default();
                if ms.printf(b", from '%.80s'", &[Arg::Str(from)]) == -1 {
                    return -1;
                }
                let pidoff = argoff + 81 + 2;
                if n.doff + pidoff + 4 <= size {
                    if ms.printf(b", pid=%u", &[Arg::I32(e.u32(nbuf, n.doff + pidoff))]) == -1 {
                        return -1;
                    }
                }
                *flags |= FLAGS_DID_CORE;
            }
        }
        _ => {
            if n.typ == NT_PRPSINFO && *flags & FLAGS_IS_CORE != 0 {
                let offsets: &[usize] = if e.is32() { &PRPSOFFSETS32 } else { &PRPSOFFSETS64 };
                let isquote = |c: u8| b"'\"`".contains(&c);
                let mut i = 0usize;
                'offsets: while i < offsets.len() {
                    let mut reloffset = offsets[i];
                    let mut noffset = n.doff + reloffset;
                    let mut j = 0usize;
                    while j < 16 {
                        // Past the buffer, or past the contents: not here.
                        if noffset >= size || reloffset >= n.descsz as usize {
                            i += 1;
                            continue 'offsets;
                        }
                        let c = byte(nbuf, noffset);
                        if c == 0 {
                            // A NUL first is wrong; any other ends it.
                            if j == 0 {
                                i += 1;
                                continue 'offsets;
                            }
                            break;
                        }
                        if !crate::cstd::isprint(c) || isquote(c) {
                            i += 1;
                            continue 'offsets;
                        }
                        j += 1;
                        noffset += 1;
                        reloffset += 1;
                    }
                    // That worked. Try the next offsets, in case this match
                    // is the middle of a string.
                    let mut found = i;
                    for k in i + 1..offsets.len() {
                        if offsets[k] >= offsets[found] {
                            continue;
                        }
                        // `pr_fname == pr_psargs - 16` and an unterminated
                        // fname (qemu).
                        if offsets[k] + 16 == offsets[found] && j == 16 {
                            continue;
                        }
                        let adjust = (n.doff + offsets[k]..n.doff + offsets[found])
                            .all(|no| crate::cstd::isprint(byte(nbuf, no)));
                        if adjust {
                            found = k;
                        }
                    }
                    let cname = n.doff + offsets[found];
                    let mut cp = cname;
                    while cp < size && byte(nbuf, cp) != 0 && crate::cstd::isprint(byte(nbuf, cp)) {
                        cp += 1;
                    }
                    // Linux appends a space to the command line.
                    while cp > cname && crate::cstd::isspace(byte(nbuf, cp - 1)) {
                        cp -= 1;
                    }
                    // `file_copystr` into 256 bytes.
                    let name: Vec<u8> = (cname..cp.min(cname + 255)).map(|x| byte(nbuf, x)).collect();
                    if ms.printf(b", from '%s'", &[Arg::Str(&name)]) == -1 {
                        return -1;
                    }
                    *flags |= FLAGS_DID_CORE;
                    return 1;
                }
            }
        }
    }
    0
}

/// `get_offset_from_virtaddr`: the file offset of a virtual address, by the
/// program headers; 0 when none holds it.
fn get_offset_from_virtaddr(ms: &mut Ms, cx: &NoteCx<'_>, virtaddr: u64) -> i64 {
    let e = cx.e;
    let mut off = cx.ph_off;
    let mut ph = vec![0u8; e.ph_sizeof()];
    for _ in 0..cx.ph_num {
        if pread_at(&mut ms.errno, cx.fd, &mut ph, off).is_none_or(|n| n < ph.len()) {
            return if ms.printf(b", can't read elf program header at %jd", &[Arg::I64(off as u64)]) == -1 {
                -1
            } else {
                0
            };
        }
        off = off.wrapping_add(e.ph_sizeof() as i64);
        let p = Phdr::read(e, &ph);
        if cx.fsize != SIZE_UNKNOWN && p.offset > cx.fsize {
            continue;
        }
        if virtaddr >= p.vaddr && virtaddr < p.vaddr.wrapping_add(p.filesz) {
            #[allow(clippy::cast_possible_wrap)]
            return p.offset.wrapping_add(virtaddr.wrapping_sub(p.vaddr) as i64);
        }
    }
    0
}

/// `get_string_on_virtaddr`: a printable string at a virtual address, or
/// nothing.
fn get_string_on_virtaddr(ms: &mut Ms, cx: &NoteCx<'_>, virtaddr: u64) -> Option<Vec<u8>> {
    let offset = get_offset_from_virtaddr(ms, cx, virtaddr);
    let mut buf = vec![0u8; 256];
    let got = if offset < 0 { None } else { pread_at(&mut ms.errno, cx.fd, &mut buf, offset) };
    let n = match got {
        Some(n) if n > 0 => n,
        _ => {
            let _printed = ms.printf(b", can't read elf string at %jd", &[Arg::I64(offset as u64)]);
            return None;
        }
    };
    buf[n - 1] = 0;
    let s = crate::cstd::cstr(&buf);
    // Only printable characters, and something.
    if s.iter().all(|&c| crate::cstd::isprint(c)) && !s.is_empty() {
        Some(s.to_vec())
    } else {
        None
    }
}

/// `do_auxv_note`: from a core's auxiliary vector, the program's name, its
/// platform and its ids.
fn do_auxv_note(ms: &mut Ms, cx: &mut NoteCx<'_>, nbuf: &[u8], n: Note) -> i32 {
    let e = cx.e;
    if cx.flags & (FLAGS_IS_CORE | FLAGS_DID_CORE_STYLE) != (FLAGS_IS_CORE | FLAGS_DID_CORE_STYLE) {
        return 0;
    }
    if cx.flags & FLAGS_CORE_STYLE != OS_STYLE_SVR4 || n.typ != NT_AUXV {
        return 0;
    }
    cx.flags |= FLAGS_DID_AUXV;
    let elsize = e.auxv_sizeof();
    let mut nval = 0usize;
    let mut off = 0usize;
    while off + elsize <= n.descsz as usize {
        let at = n.doff + off;
        let (typ, val) = if e.is32() {
            (u64::from(e.u32(nbuf, at)), u64::from(e.u32(nbuf, at + 4)))
        } else {
            (e.u64(nbuf, at), e.u64(nbuf, at + 8))
        };
        off += elsize;
        // At most 50 entries, against a denial of service.
        let n0 = nval;
        nval += 1;
        if n0 >= 50 {
            ms.error(None, b"Too many ELF Auxv elements");
            return 1;
        }
        let (is_string, tag): (bool, &[u8]) = match typ {
            AT_LINUX_EXECFN => (true, b"execfn"),
            AT_LINUX_PLATFORM => (true, b"platform"),
            AT_LINUX_UID => (false, b"real uid"),
            AT_LINUX_GID => (false, b"real gid"),
            AT_LINUX_EUID => (false, b"effective uid"),
            AT_LINUX_EGID => (false, b"effective gid"),
            _ => continue,
        };
        if is_string {
            let Some(s) = get_string_on_virtaddr(ms, cx, val) else {
                continue;
            };
            if ms.printf(b", %s: '%s'", &[Arg::Str(tag), Arg::Str(&s)]) == -1 {
                return -1;
            }
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let v = val as u32;
            if ms.printf(b", %s: %d", &[Arg::Str(tag), Arg::I32(v)]) == -1 {
                return -1;
            }
        }
    }
    1
}

/// `dodynamic`: one dynamic entry -- a PIE flag, or a needed library.
fn dodynamic(ms: &mut Ms, e: Elf, dbuf: &[u8], offset: usize, size: usize, pie: &mut bool, need: &mut usize) -> usize {
    let dh = e.dh_sizeof();
    if dh + offset > size {
        // Out of entries.
        return dh + offset;
    }
    let (tag, val) = if e.is32() {
        (u64::from(e.u32(dbuf, offset)), u64::from(e.u32(dbuf, offset + 4)))
    } else {
        (e.u64(dbuf, offset), e.u64(dbuf, offset + 8))
    };
    match tag {
        DT_FLAGS_1 => {
            *pie = true;
            if val & DF_1_PIE != 0 {
                ms.mode |= 0o111;
            } else {
                ms.mode &= !0o111;
            }
        }
        DT_NEEDED => *need += 1,
        _ => {}
    }
    offset + dh
}

/// `donote`: one note -- the offset of the next, or 0 to stop.
#[allow(clippy::too_many_lines)]
fn donote(ms: &mut Ms, cx: &mut NoteCx<'_>, nbuf: &[u8], offset: usize, size: usize, align: usize, core: bool) -> usize {
    let e = cx.e;
    if cx.notecount == 0 {
        return 0;
    }
    cx.notecount -= 1;
    const NH: usize = 12;
    if NH + offset > size {
        // Out of note headers.
        return NH + offset;
    }
    let namesz = e.u32(nbuf, offset);
    let descsz = e.u32(nbuf, offset + 4);
    let typ = e.u32(nbuf, offset + 8);
    let mut offset = offset + NH;
    if namesz == 0 && descsz == 0 {
        // Out of note headers.
        return if offset >= size { offset } else { size };
    }
    if namesz & 0x8000_0000 != 0 {
        let _printed = ms.printf(b", bad note name size %#lx", &[Arg::I64(u64::from(namesz))]);
        return 0;
    }
    if descsz & 0x8000_0000 != 0 {
        let _printed = ms.printf(b", bad note description size %#lx", &[Arg::I64(u64::from(descsz))]);
        return 0;
    }
    // `ELF_ALIGN`, in C's `size_t` arithmetic: a huge alignment from a
    // program header wraps as it does there.
    let elf_align = |a: usize| (a.wrapping_add(align).wrapping_sub(1) / align).wrapping_mul(align);
    let noff = offset;
    let doff = elf_align(offset.wrapping_add(namesz as usize));
    if offset.wrapping_add(namesz as usize) > size {
        // Past the end of the buffer.
        return doff;
    }
    offset = elf_align(doff.wrapping_add(descsz as usize));
    if doff.wrapping_add(descsz as usize) > size {
        return if offset >= size { offset } else { size };
    }
    let n = Note {
        typ,
        namesz,
        descsz,
        noff,
        doff,
    };
    if cx.flags & FLAGS_DID_OS_NOTE == 0 && do_os_note(ms, e, nbuf, n, &mut cx.flags) != 0 {
        return offset;
    }
    if cx.flags & FLAGS_DID_BUILD_ID == 0 && do_bid_note(ms, nbuf, n, &mut cx.flags) != 0 {
        return offset;
    }
    if cx.flags & FLAGS_DID_NETBSD_PAX == 0 && do_pax_note(ms, e, nbuf, n, &mut cx.flags) != 0 {
        return offset;
    }
    if cx.flags & FLAGS_DID_CORE == 0 && do_core_note(ms, e, nbuf, n, &mut cx.flags, size) != 0 {
        return offset;
    }
    if cx.flags & FLAGS_DID_AUXV == 0 {
        // Only a core's notes have program headers to read strings through.
        let r = if core {
            do_auxv_note(ms, cx, nbuf, n)
        } else {
            let saved = (cx.ph_off, cx.ph_num, cx.fsize);
            cx.ph_off = 0;
            cx.ph_num = 0;
            cx.fsize = 0;
            let r = do_auxv_note(ms, cx, nbuf, n);
            (cx.ph_off, cx.ph_num, cx.fsize) = saved;
            r
        };
        if r != 0 {
            return offset;
        }
    }
    if namesz == 7 && cstr_eq(nbuf, noff, b"NetBSD") {
        let descsz = descsz.min(100);
        let (flag, tag): (u32, &[u8]) = match typ {
            NT_NETBSD_VERSION => return offset,
            NT_NETBSD_MARCH => (FLAGS_DID_NETBSD_MARCH, b"compiled for"),
            NT_NETBSD_CMODEL => (FLAGS_DID_NETBSD_CMODEL, b"compiler model"),
            NT_NETBSD_EMULATION => (FLAGS_DID_NETBSD_EMULATION, b"emulation:"),
            _ => {
                if cx.flags & FLAGS_DID_NETBSD_UNKNOWN != 0 {
                    return offset;
                }
                cx.flags |= FLAGS_DID_NETBSD_UNKNOWN;
                let _printed = ms.printf(b", note=%u", &[Arg::I32(typ)]);
                return offset;
            }
        };
        if cx.flags & flag != 0 {
            return offset;
        }
        cx.flags |= flag;
        let s: Vec<u8> = (0..descsz as usize).map(|i| byte(nbuf, doff + i)).collect();
        let _printed = ms.printf(b", %s: %s", &[Arg::Str(tag), Arg::Str(&s)]);
        return offset;
    }
    offset
}

/// `doshn`: the section headers -- notes, SunOS capabilities, and whether a
/// symbol table or debug information is left.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn doshn(ms: &mut Ms, cx: &mut NoteCx<'_>, off: i64, num: u16, size: usize, mach: u16, strtab: i32) -> i32 {
    let e = cx.e;
    let fd = cx.fd;
    let fsize = cx.fsize;
    if ms.flags & MAGIC_MIME != 0 {
        return 0;
    }
    if num == 0 {
        return if ms.print_str(b", no section header") == -1 { -1 } else { 0 };
    }
    if size != e.sh_sizeof() {
        return if ms.print_str(b", corrupted section header size") == -1 { -1 } else { 0 };
    }
    let mut stripped = true;
    let mut has_debug_info = false;
    let mut nbadcap = 0usize;
    let mut cap_hw1: u64 = 0;
    let mut cap_sf1: u64 = 0;
    let mut sh = vec![0u8; e.sh_sizeof()];

    // The name section, to read section names through.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    let offs = (off as u64).wrapping_add((size as u64).wrapping_mul(i64::from(strtab) as u64)) as i64;
    if pread_at(&mut ms.errno, fd, &mut sh, offs).is_none_or(|n| n < sh.len()) {
        return if ms.printf(b", missing section headers at %jd", &[Arg::I64(offs as u64)]) == -1 { -1 } else { 0 };
    }
    // `sh32`/`sh64` holds the name section's header now -- and the first
    // name read below is through it, not through the section's own.
    let mut cur = Shdr::read(e, &sh);
    let name_off = cur.offset;
    if fsize != SIZE_UNKNOWN && fsize < name_off {
        return if ms.printf(b", too large section header offset %jd", &[Arg::I64(name_off as u64)]) == -1 {
            -1
        } else {
            0
        };
    }

    let mut off = off;
    for _ in 0..num {
        // The name of this section.
        let offs = name_off.wrapping_add(i64::from(cur.name));
        let mut name = [0u8; 50];
        let Some(namesize) = pread_at(&mut ms.errno, fd, &mut name[..49], offs) else {
            return if ms.printf(b", can't read name of elf section at %jd", &[Arg::I64(offs as u64)]) == -1 {
                -1
            } else {
                0
            };
        };
        name[namesize] = 0;
        if crate::cstd::cstr(&name) == b".debug_info" {
            has_debug_info = true;
            stripped = false;
        }
        if pread_at(&mut ms.errno, fd, &mut sh, off).is_none_or(|n| n < sh.len()) {
            return if ms.printf(b", can't read elf section at %jd", &[Arg::I64(off as u64)]) == -1 { -1 } else { 0 };
        }
        off = off.wrapping_add(size as i64);
        cur = Shdr::read(e, &sh);

        // Things we can tell before we seek.
        if cur.typ == SHT_SYMTAB {
            stripped = false;
        } else if fsize != SIZE_UNKNOWN && cur.offset > fsize {
            continue;
        }

        // Things we can tell when we seek.
        match cur.typ {
            SHT_NOTE => {
                #[allow(clippy::cast_sign_loss)]
                if cur.size.wrapping_add(cur.offset as u64) > fsize as u64 {
                    #[allow(clippy::cast_sign_loss)]
                    let a = [Arg::I64(cur.offset as u64), Arg::I64(cur.size), Arg::I64(fsize as u64)];
                    return if ms.printf(b", note offset/size %#jx+%#jx exceeds file size %#jx", &a) == -1 { -1 } else { 0 };
                }
                if cur.size > ms.elf_shsize_max as u64 {
                    // `file_error(ms, errno, ...)` with whatever `errno` holds:
                    // usually the CDF probe's EFTYPE, which is EINVAL here.
                    let stale = errno_error(ms.errno);
                    ms.error(
                        stale.as_ref(),
                        format!("Note section size too big ({} > {})", cur.size, ms.elf_shsize_max).as_bytes(),
                    );
                    return -1;
                }
                let Ok(sz) = usize::try_from(cur.size) else {
                    return -1;
                };
                let mut nbuf = vec![0u8; sz];
                if pread_at(&mut ms.errno, fd, &mut nbuf, cur.offset).is_none_or(|n| n < sz) {
                    return if ms.printf(b", can't read elf note at %jd", &[Arg::I64(cur.offset as u64)]) == -1 {
                        -1
                    } else {
                        0
                    };
                }
                let mut noff = 0usize;
                loop {
                    if noff >= sz {
                        break;
                    }
                    noff = donote(ms, cx, &nbuf, noff, sz, 4, false);
                    if noff == 0 {
                        break;
                    }
                }
            }
            SHT_SUNW_CAP => {
                if !matches!(mach, EM_SPARC | EM_SPARCV9 | EM_IA_64 | EM_386 | EM_AMD64) {
                    continue;
                }
                if nbadcap > 5 {
                    continue;
                }
                // `lseek` there, then `read` one capability at a time. A
                // negative offset is `lseek`'s EINVAL: `file_badseek`.
                let Ok(mut pos) = u64::try_from(cur.offset) else {
                    let e2 = std::io::Error::from(std::io::ErrorKind::InvalidInput);
                    ms.error(Some(&e2), b"error seeking");
                    return -1;
                };
                let csz = e.cap_sizeof();
                // `coff` is an `off_t`, compared with the size as one.
                #[allow(clippy::cast_possible_wrap)]
                let limit = cur.size as i64;
                let mut coff: i64 = 0;
                loop {
                    coff = coff.wrapping_add(csz as i64);
                    if coff > limit {
                        break;
                    }
                    let mut cbuf = vec![0u8; csz];
                    match pread(fd, &mut cbuf, pos) {
                        Ok(n) if n == csz => {}
                        // A short read sets no errno: `file_badread` names
                        // whatever it held before.
                        Ok(_) => {
                            let stale = errno_error(ms.errno);
                            ms.error(stale.as_ref(), b"error reading");
                            return -1;
                        }
                        Err(err) => {
                            ms.errno = Some(Errno::of(&err));
                            ms.badread(&err);
                            return -1;
                        }
                    }
                    pos += csz as u64;
                    if cbuf[0] == b'A' {
                        break;
                    }
                    let (tag, val) = if e.is32() {
                        (u64::from(e.u32(&cbuf, 0)), u64::from(e.u32(&cbuf, 4)))
                    } else {
                        (e.u64(&cbuf, 0), e.u64(&cbuf, 8))
                    };
                    match tag {
                        CA_SUNW_NULL => {}
                        CA_SUNW_HW_1 => cap_hw1 |= val,
                        CA_SUNW_SF_1 => cap_sf1 |= val,
                        _ => {
                            if ms.printf(
                                b", with unknown capability %#llx = %#llx",
                                &[Arg::I64(tag), Arg::I64(val)],
                            ) == -1
                            {
                                return -1;
                            }
                            let n0 = nbadcap;
                            nbadcap += 1;
                            if n0 > 2 {
                                coff = limit;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if has_debug_info && ms.print_str(b", with debug_info") == -1 {
        return -1;
    }
    let s: &[u8] = if stripped { b"" } else { b"not " };
    if ms.printf(b", %sstripped", &[Arg::Str(s)]) == -1 {
        return -1;
    }
    if cap_hw1 != 0 {
        let cdp: Option<&[(u64, &str)]> = match mach {
            EM_SPARC | EM_SPARC32PLUS | EM_SPARCV9 => Some(CAP_DESC_SPARC),
            EM_386 | EM_IA_64 | EM_AMD64 => Some(CAP_DESC_386),
            _ => None,
        };
        if ms.print_str(b", uses") == -1 {
            return -1;
        }
        match cdp {
            Some(table) => {
                for &(mask, name) in table {
                    if cap_hw1 & mask != 0 {
                        if ms.printf(b" %s", &[Arg::Str(name.as_bytes())]) == -1 {
                            return -1;
                        }
                        cap_hw1 &= !mask;
                    }
                }
                if cap_hw1 != 0 && ms.printf(b" unknown hardware capability %#llx", &[Arg::I64(cap_hw1)]) == -1 {
                    return -1;
                }
            }
            None => {
                if ms.printf(b" hardware capability %#llx", &[Arg::I64(cap_hw1)]) == -1 {
                    return -1;
                }
            }
        }
    }
    if cap_sf1 != 0 {
        if cap_sf1 & SF1_SUNW_FPUSED != 0 {
            let w: &[u8] = if cap_sf1 & SF1_SUNW_FPKNWN != 0 {
                b", uses frame pointer"
            } else {
                b", not known to use frame pointer"
            };
            if ms.print_str(w) == -1 {
                return -1;
            }
        }
        cap_sf1 &= !SF1_SUNW_MASK;
        if cap_sf1 != 0 && ms.printf(b", with unknown software capability %#llx", &[Arg::I64(cap_sf1)]) == -1 {
            return -1;
        }
    }
    0
}

/// `dophn_exec`: from the program headers, how the file is linked, its
/// interpreter, and -- when there are no section headers -- its notes.
#[allow(clippy::too_many_lines)]
fn dophn_exec(ms: &mut Ms, cx: &mut NoteCx<'_>, off: i64, num: u16, size: usize, sh_num: u16) -> i32 {
    let e = cx.e;
    let fd = cx.fd;
    if num == 0 {
        return if ms.print_str(b", no program header") == -1 { -1 } else { 0 };
    }
    if size != e.ph_sizeof() {
        return if ms.print_str(b", corrupted program header size") == -1 { -1 } else { 0 };
    }
    let mut interp: Vec<u8> = Vec::new();
    let mut nbuf = vec![0u8; BUFSIZ];
    let mut need = 0usize;
    let mut pie = false;
    let mut dynamic = false;
    let mut ph = vec![0u8; e.ph_sizeof()];
    let mut off = off;
    let mime = ms.flags & MAGIC_MIME != 0;
    for _ in 0..num {
        if pread_at(&mut ms.errno, fd, &mut ph, off).is_none_or(|n| n < ph.len()) {
            return if ms.printf(b", can't read elf program headers at %jd", &[Arg::I64(off as u64)]) == -1 {
                -1
            } else {
                0
            };
        }
        off = off.wrapping_add(size as i64);
        let p = Phdr::read(e, &ph);
        let mut bufsize = 0usize;
        let mut align = 4usize;
        // Things we can tell before we seek.
        let doread = match p.typ {
            PT_DYNAMIC | PT_INTERP => true,
            PT_NOTE => {
                if sh_num != 0 {
                    // Done through the section headers.
                    continue;
                }
                #[allow(clippy::cast_possible_truncation)]
                {
                    align = p.align as usize;
                }
                if align & 0x8000_0000 != 0 || align < 4 {
                    if ms.printf(b", invalid note alignment %#lx", &[Arg::I64(align as u64)]) == -1 {
                        return -1;
                    }
                    align = 4;
                }
                true
            }
            _ => {
                if cx.fsize != SIZE_UNKNOWN && p.offset > cx.fsize {
                    continue;
                }
                false
            }
        };
        if doread {
            let len = usize::try_from(p.filesz).map_or(BUFSIZ, |f| f.min(BUFSIZ));
            match pread_at(&mut ms.errno, fd, &mut nbuf[..len], p.offset) {
                Some(n) => bufsize = n,
                None => {
                    return if ms.printf(b", can't read section at %jd", &[Arg::I64(p.offset as u64)]) == -1 {
                        -1
                    } else {
                        0
                    };
                }
            }
        }
        // Things we can tell when we seek.
        match p.typ {
            PT_DYNAMIC => {
                dynamic = true;
                let mut offset = 0usize;
                // Let DF_1 decide whether it is PIE.
                ms.mode &= !0o111;
                loop {
                    if offset >= bufsize {
                        break;
                    }
                    offset = dodynamic(ms, e, &nbuf, offset, bufsize, &mut pie, &mut need);
                    if offset == 0 {
                        break;
                    }
                }
            }
            PT_INTERP => {
                need += 1;
                if mime {
                    continue;
                }
                if bufsize != 0 && nbuf[0] != 0 {
                    nbuf[bufsize - 1] = 0;
                    interp = crate::cstd::cstr(&nbuf[..bufsize]).to_vec();
                } else {
                    interp = b"*empty*".to_vec();
                }
            }
            PT_NOTE => {
                if mime {
                    return 0;
                }
                let mut offset = 0usize;
                loop {
                    if offset >= bufsize {
                        break;
                    }
                    offset = donote(ms, cx, &nbuf[..bufsize], offset, bufsize, align, false);
                    if offset == 0 {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if mime {
        return 0;
    }
    let linking_style: &[u8] = if dynamic {
        if pie && need == 0 { b"static-pie" } else { b"dynamically" }
    } else {
        b"statically"
    };
    if ms.printf(b", %s linked", &[Arg::Str(linking_style)]) == -1 {
        return -1;
    }
    if !interp.is_empty() {
        let shown = file_printable(ms.flags & MAGIC_RAW != 0, BUFSIZ, &interp, BUFSIZ);
        if ms.printf(b", interpreter %s", &[Arg::Str(&shown)]) == -1 {
            return -1;
        }
    }
    0
}

/// `file_tryelf`: 1 when the file was an ELF file and was described.
pub fn file_tryelf(ms: &mut Ms, b: &Buffer<'_>) -> i32 {
    let buf = b.fbuf;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    // Not ELF: return before anything is read.
    if byte(buf, 0) != 0x7f
        || (byte(buf, 1) != b'E' && byte(buf, 1) != b'O')
        || byte(buf, 2) != b'L'
        || byte(buf, 3) != b'F'
    {
        return 0;
    }
    // `file_buffer` comes here only with a descriptor.
    let Some(orig) = b.fd else {
        return 0;
    };
    // A pipe cannot be read at offsets: copy it to a file first, which takes
    // over the pipe's descriptor.
    #[cfg(not(unix))]
    let copy;
    let fd: &File = if crate::compress::is_pipe(ms, orig) {
        match crate::compress::file_pipe2file(ms, orig, buf) {
            Some(crate::compress::PipeCopy::InPlace) => orig,
            #[cfg(not(unix))]
            Some(crate::compress::PipeCopy::Apart(f)) => {
                copy = f;
                &copy
            }
            None => {
                // The copy's own error is the one recorded; this one, which
                // upstream adds, is dropped as the second.
                let e = std::io::Error::from(std::io::ErrorKind::Other);
                ms.badread(&e);
                return -1;
            }
        }
    } else {
        orig
    };
    // `b->st.st_size != 0` if the earlier `fstat` worked.
    let st = if b.st.size == 0 {
        match fd.metadata() {
            Ok(md) => crate::buffer::Stat::from_metadata(&md),
            Err(err) => {
                ms.badread(&err);
                return -1;
            }
        }
    } else {
        b.st
    };
    #[allow(clippy::cast_possible_wrap)]
    let fsize = if st.is_reg() || st.size != 0 { st.size as i64 } else { SIZE_UNKNOWN };

    let clazz = byte(buf, 4);
    if clazz != ELFCLASS32 && clazz != ELFCLASS64 {
        return if ms.printf(b", unknown class %d", &[Arg::I32(u32::from(clazz))]) == -1 { -1 } else { 0 };
    }
    // `elfclass.h`: the header must fit, with a byte to spare.
    let ehdr_size = if clazz == ELFCLASS32 { 52 } else { 64 };
    if buf.len() <= ehdr_size {
        return 0;
    }
    // A little-endian host: the file needs swapping unless it says LSB.
    let swap = byte(buf, 5) != 1;
    let e = Elf { clazz, swap };
    let typ = e.u16(buf, 16);
    let phoff = e.addr(buf, 28, 32);
    let shoff = e.addr(buf, 32, 40);
    let (phentsize, phnum, shentsize, shnum, shstrndx) = if e.is32() {
        (e.u16(buf, 42), e.u16(buf, 44), e.u16(buf, 46), e.u16(buf, 48), e.u16(buf, 50))
    } else {
        (e.u16(buf, 54), e.u16(buf, 56), e.u16(buf, 58), e.u16(buf, 60), e.u16(buf, 62))
    };
    let machine = e.u16(buf, 18);
    let mut cx = NoteCx {
        e,
        flags: 0,
        notecount: ms.elf_notes_max,
        fd,
        ph_off: 0,
        ph_num: 0,
        fsize,
    };
    #[allow(clippy::cast_possible_wrap)]
    match typ {
        ET_CORE => {
            if phnum > ms.elf_phnum_max {
                return toomany(ms, "program headers", phnum);
            }
            cx.flags |= FLAGS_IS_CORE;
            cx.ph_off = phoff as i64;
            cx.ph_num = u32::from(phnum);
            if dophn_core(ms, &mut cx, phoff as i64, phnum, usize::from(phentsize)) == -1 {
                return -1;
            }
        }
        ET_EXEC | ET_DYN | ET_REL => {
            if typ != ET_REL {
                if phnum > ms.elf_phnum_max {
                    return toomany(ms, "program", phnum);
                }
                if shnum > ms.elf_shnum_max {
                    return toomany(ms, "section", shnum);
                }
                if dophn_exec(ms, &mut cx, phoff as i64, phnum, usize::from(phentsize), shnum) == -1 {
                    return -1;
                }
            }
            if shnum > ms.elf_shnum_max {
                return toomany(ms, "section headers", shnum);
            }
            if doshn(
                ms,
                &mut cx,
                shoff as i64,
                shnum,
                usize::from(shentsize),
                machine,
                i32::from(shstrndx),
            ) == -1
            {
                return -1;
            }
        }
        _ => {}
    }
    if cx.notecount == 0 {
        return toomany(ms, "notes", ms.elf_notes_max);
    }
    1
}
