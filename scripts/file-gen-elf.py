#!/usr/bin/env python3
"""Generate ELF files exercising every path of libmagic's readelf.c.

    python3 scripts/file-gen-elf.py OUTDIR

Writes one file per case into OUTDIR, in all four class/byte-order variants
(32/64-bit, little/big-endian): executables, shared objects and static PIEs;
every OS note readelf.c knows (GNU ABI tags, NetBSD/FreeBSD/OpenBSD/DragonFly
versions, SuSE, build IDs of every size, Go build IDs, PaX, NetBSD march,
cmodel and emulation) both as sections and as program-header notes; bad note
sizes and alignments; SunOS capability sections on each machine; Linux,
FreeBSD and NetBSD core files with auxiliary vectors; and the limits (too
many program headers, sections and notes). Used by scripts/file-diff.sh, which
compares our `file` with file 5.45 on them.
"""
import os
import struct
import sys

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)

PT_LOAD, PT_DYNAMIC, PT_INTERP, PT_NOTE = 1, 2, 3, 4
SHT_PROGBITS, SHT_SYMTAB, SHT_STRTAB, SHT_NOTE, SHT_SUNW_CAP = 1, 2, 3, 7, 0x6FFFFFF5


def note(e, name, typ, desc, align=4, namesz=None, descsz=None):
    nb = name if isinstance(name, bytes) else (name.encode() + b"\0")
    hdr = struct.pack(e + "III", len(nb) if namesz is None else namesz,
                      len(desc) if descsz is None else descsz, typ)
    pad = lambda b: b + b"\0" * ((-len(b)) % align)
    return hdr + pad(nb) + pad(desc)


class Elf:
    def __init__(self, cls, e, etype=2, machine=62):
        self.cls, self.e, self.etype, self.machine = cls, e, etype, machine
        self.segs = []   # (type, data, vaddr, align, offset_override)
        self.secs = []   # (name, type, data)
        self.with_sections = True
        self.phentsize = None
        self.shentsize = None
        self.phnum = None
        self.shnum = None
        self.shstrndx = None
        self.shstrtab_name = ".shstrtab"

    def seg(self, typ, data, vaddr=0x1000, align=4, offset=None, filesz=None):
        self.segs.append((typ, data, vaddr, align, offset, filesz))

    def sec(self, name, typ, data):
        self.secs.append((name, typ, data))

    def build(self):
        e, is64 = self.e, self.cls == 64
        ehsize = 64 if is64 else 52
        phent = 56 if is64 else 32
        shent = 64 if is64 else 40
        body = bytearray(b"\0" * ehsize)
        phoff = len(body)
        body += b"\0" * (phent * len(self.segs))
        seg_off = []
        for (_, data, _, _, off, _) in self.segs:
            seg_off.append(len(body) if off is None else off)
            body += data
            body += b"\0" * ((-len(body)) % 8)
        secs = list(self.secs)
        shstr = bytearray(b"\0")
        names = []
        if self.with_sections:
            for (name, _, _) in secs + [(self.shstrtab_name, SHT_STRTAB, b"")]:
                names.append(len(shstr))
                shstr += name.encode() + b"\0"
        sec_off = []
        for (_, _, data) in secs:
            sec_off.append(len(body))
            body += data
            body += b"\0" * ((-len(body)) % 8)
        shstr_off = len(body)
        body += shstr
        body += b"\0" * ((-len(body)) % 8)
        shoff = len(body) if self.with_sections else 0
        nsec = 0
        if self.with_sections:
            # Section 0 is SHT_NULL.
            allsecs = [(0, 0, 0, 0)] + [
                (names[i], t, sec_off[i], len(d)) for i, (_, t, d) in enumerate(secs)
            ] + [(names[-1], SHT_STRTAB, shstr_off, len(shstr))]
            nsec = len(allsecs)
            for (nm, t, off, sz) in allsecs:
                if is64:
                    body += struct.pack(e + "IIQQQQIIQQ", nm, t, 0, 0, off, sz, 0, 0, 1, 0)
                else:
                    body += struct.pack(e + "IIIIIIIIII", nm, t, 0, 0, off, sz, 0, 0, 1, 0)
        for i, (t, data, vaddr, align, _, filesz) in enumerate(self.segs):
            fs = len(data) if filesz is None else filesz
            o = phoff + i * phent
            if is64:
                struct.pack_into(e + "IIQQQQQQ", body, o, t, 5, seg_off[i], vaddr, vaddr, fs, fs, align)
            else:
                struct.pack_into(e + "IIIIIIII", body, o, t, seg_off[i], vaddr, vaddr, fs, fs, 5, align)
        ident = b"\x7fELF" + bytes([2 if is64 else 1, 1 if e == "<" else 2, 1, 0]) + b"\0" * 8
        body[0:16] = ident
        phnum = len(self.segs) if self.phnum is None else self.phnum
        shnum = nsec if self.shnum is None else self.shnum
        shstrndx = (nsec - 1 if nsec else 0) if self.shstrndx is None else self.shstrndx
        pe = self.phentsize or phent
        se = self.shentsize or shent
        if is64:
            struct.pack_into(e + "HHIQQQIHHHHHH", body, 16, self.etype, self.machine, 1, 0x1000,
                             phoff if self.segs or self.phnum else 0, shoff, 0, ehsize, pe, phnum, se, shnum, shstrndx)
        else:
            struct.pack_into(e + "HHIIIIIHHHHHH", body, 16, self.etype, self.machine, 1, 0x1000,
                             phoff if self.segs or self.phnum else 0, shoff, 0, ehsize, pe, phnum, se, shnum, shstrndx)
        return bytes(body)


def dyn(e, is64, entries):
    f = e + ("qQ" if is64 else "iI")
    return b"".join(struct.pack(f, t, v) for t, v in entries)


def u32(e, v):
    return struct.pack(e + "I", v)


CASES = {}


def case(fn):
    CASES[fn.__name__] = fn
    return fn


# --- executables: linking, interpreter, PIE ---------------------------------

@case
def dyn_pie(c, e):
    x = Elf(c, e, 3)
    x.seg(PT_INTERP, b"/lib/ld-test.so.1\0")
    x.seg(PT_DYNAMIC, dyn(e, c == 64, [(1, 1), (0x6FFFFFFB, 0x08000001), (0, 0)]))
    x.sec(".text", SHT_PROGBITS, b"\x90" * 16)
    return x


@case
def dyn_shared(c, e):
    x = Elf(c, e, 3)
    x.seg(PT_DYNAMIC, dyn(e, c == 64, [(1, 1), (0x6FFFFFFB, 1), (0, 0)]))
    x.sec(".symtab", SHT_SYMTAB, b"\0" * 24)
    return x


@case
def static_pie(c, e):
    x = Elf(c, e, 3)
    x.seg(PT_DYNAMIC, dyn(e, c == 64, [(0x6FFFFFFB, 0x08000000), (0, 0)]))
    return x


@case
def static_exec(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_LOAD, b"\x90" * 32)
    x.with_sections = False
    return x


@case
def interp_empty(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_INTERP, b"")
    return x


@case
def interp_weird(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_INTERP, b"/lib/\x01od\xffd.so\n\0junk")
    return x


@case
def interp_nul_first(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_INTERP, b"\0abc\0")
    return x


# --- notes through segments (no section headers) ----------------------------

def os_notes(e):
    return [
        ("gnu_linux", note(e, "GNU", 1, u32(e, 0) + u32(e, 3) + u32(e, 2) + u32(e, 0))),
        ("gnu_hurd", note(e, "GNU", 1, u32(e, 1) + u32(e, 0) + u32(e, 5) + u32(e, 1))),
        ("gnu_solaris", note(e, "GNU", 1, u32(e, 2) + u32(e, 2) + u32(e, 6) + u32(e, 0))),
        ("gnu_kfreebsd", note(e, "GNU", 1, u32(e, 3) + u32(e, 10) + u32(e, 1) + u32(e, 0))),
        ("gnu_knetbsd", note(e, "GNU", 1, u32(e, 4) + u32(e, 1) + u32(e, 2) + u32(e, 3))),
        ("gnu_unknown", note(e, "GNU", 1, u32(e, 9) + u32(e, 0xffffffff) + u32(e, 0x80000000) + u32(e, 7))),
        ("suse", note(e, "SuSE", 1, b"\x0a\x03")),
        ("netbsd_old", note(e, "NetBSD", 1, u32(e, 199905))),
        ("netbsd_8", note(e, "NetBSD", 1, u32(e, 800000000))),
        ("netbsd_7_1", note(e, "NetBSD", 1, u32(e, 701000000))),
        ("netbsd_6_1_5", note(e, "NetBSD", 1, u32(e, 601000500))),
        ("netbsd_rel", note(e, "NetBSD", 1, u32(e, 599002300))),
        ("netbsd_relz", note(e, "NetBSD", 1, u32(e, 599700000))),
        ("netbsd_9_2", note(e, "NetBSD", 1, u32(e, 902000000))),
        ("netbsd_10", note(e, "NetBSD", 1, u32(e, 1000000100))),
        ("freebsd_462", note(e, "FreeBSD", 1, u32(e, 460002))),
        ("freebsd_45", note(e, "FreeBSD", 1, u32(e, 450000))),
        ("freebsd_44", note(e, "FreeBSD", 1, u32(e, 440001))),
        ("freebsd_47", note(e, "FreeBSD", 1, u32(e, 470000))),
        ("freebsd_48x", note(e, "FreeBSD", 1, u32(e, 480120))),
        ("freebsd_49", note(e, "FreeBSD", 1, u32(e, 491010))),
        ("freebsd_13", note(e, "FreeBSD", 1, u32(e, 1300139))),
        ("freebsd_14", note(e, "FreeBSD", 1, u32(e, 1400000))),
        ("freebsd_3", note(e, "FreeBSD", 1, u32(e, 300000))),
        ("freebsd_big", note(e, "FreeBSD", 1, u32(e, 0xfffffff0))),
        ("openbsd", note(e, "OpenBSD", 1, u32(e, 0))),
        ("dragonfly", note(e, "DragonFly", 1, u32(e, 400703))),
        ("buildid_4", note(e, "GNU", 3, b"\x01\x02\x03\x04")),
        ("buildid_8", note(e, "GNU", 3, bytes(range(8)))),
        ("buildid_16", note(e, "GNU", 3, bytes(range(16)))),
        ("buildid_20", note(e, "GNU", 3, bytes(range(20)))),
        ("buildid_21", note(e, "GNU", 3, bytes(range(21)))),
        ("buildid_3", note(e, "GNU", 3, bytes(range(3)))),
        ("go_buildid", note(e, b"Go\0\0", 4, b"abc/def\0ghi")),
        ("go_buildid_long", note(e, b"Go\0\0", 4, b"x" * 130)),
        ("pax_0", note(e, "PaX", 3, u32(e, 0))),
        ("pax_15", note(e, "PaX", 3, u32(e, 0x15))),
        ("pax_3f", note(e, "PaX", 3, u32(e, 0x3F))),
        ("netbsd_march", note(e, "NetBSD", 5, b"earmv7hf\0")),
        ("netbsd_cmodel", note(e, "NetBSD", 6, b"medany\0")),
        ("netbsd_emul", note(e, "NetBSD", 2, b"netbsd" + b"y" * 120)),
        ("netbsd_unknown", note(e, "NetBSD", 77, b"zz\0\0")),
        ("bad_namesz", note(e, "GNU", 3, b"", namesz=0x80000001)),
        ("bad_descsz", note(e, "GNU", 3, b"", descsz=0x80000002)),
        ("empty_note", note(e, b"", 0, b"")),
        ("past_end", note(e, "GNU", 3, b"abcd", descsz=400)),
        ("name_past_end", note(e, "GNU", 3, b"", namesz=300)),
        ("multi", note(e, "GNU", 3, bytes(range(20))) + note(e, "GNU", 1, u32(e, 0) * 4)
            + note(e, "NetBSD", 5, b"x\0") + note(e, "NetBSD", 5, b"y\0") + note(e, "NetBSD", 99, b"")
            + note(e, "NetBSD", 98, b"") + note(e, "PaX", 3, u32(e, 1)) + note(e, "PaX", 3, u32(e, 2))),
    ]


def add_note_cases():
    for nm, _ in os_notes("<"):
        def seg_case(c, e, nm=nm):
            x = Elf(c, e, 2)
            x.seg(PT_LOAD, b"\x90" * 8)
            x.seg(PT_NOTE, dict(os_notes(e))[nm])
            x.with_sections = False
            return x

        def sec_case(c, e, nm=nm):
            x = Elf(c, e, 2)
            x.seg(PT_LOAD, b"\x90" * 8)
            x.sec(".note.x", SHT_NOTE, dict(os_notes(e))[nm])
            return x

        seg_case.__name__ = "segnote_" + nm
        sec_case.__name__ = "secnote_" + nm
        case(seg_case)
        case(sec_case)


add_note_cases()


@case
def note_align_values(c, e):
    x = Elf(c, e, 2)
    for a in (0, 1, 2, 3, 8, 0x80000000, 16):
        x.seg(PT_NOTE, note(e, "GNU", 3, bytes(range(20)), align=max(4, min(a, 8)) if a else 4), align=a)
    x.with_sections = False
    return x


@case
def note_align8(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_NOTE, note(e, "GNU", 1, u32(e, 0) * 4, align=8) + note(e, "GNU", 3, bytes(range(20)), align=8), align=8)
    x.with_sections = False
    return x


@case
def many_notes(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_NOTE, note(e, "XX", 9, b"") * 300)
    x.with_sections = False
    return x


@case
def many_notes_sec(c, e):
    x = Elf(c, e, 2)
    x.sec(".note.many", SHT_NOTE, note(e, "XX", 9, b"abcd") * 260)
    return x


# --- sections ----------------------------------------------------------------

@case
def debug_info_middle(c, e):
    x = Elf(c, e, 1)
    x.sec(".text", SHT_PROGBITS, b"\x90")
    x.sec(".debug_info", SHT_PROGBITS, b"\0" * 8)
    x.sec(".data", SHT_PROGBITS, b"\0")
    return x


@case
def debug_info_last(c, e):
    # Its name is read one section late: as the last, it is never seen.
    x = Elf(c, e, 1)
    x.sec(".text", SHT_PROGBITS, b"\x90")
    x.sec(".debug_info", SHT_PROGBITS, b"\0" * 8)
    x.shstrtab_name = ".shstrtab"
    return x


@case
def shstrtab_named_debug_info(c, e):
    x = Elf(c, e, 1)
    x.sec(".text", SHT_PROGBITS, b"\x90")
    x.shstrtab_name = ".debug_info"
    return x


@case
def symtab(c, e):
    x = Elf(c, e, 1)
    x.sec(".symtab", SHT_SYMTAB, b"\0" * 16)
    return x


@case
def no_sections(c, e):
    x = Elf(c, e, 1)
    x.with_sections = False
    return x


@case
def bad_shentsize(c, e):
    x = Elf(c, e, 1)
    x.sec(".text", SHT_PROGBITS, b"\x90")
    x.shentsize = 12
    return x


@case
def bad_phentsize(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_LOAD, b"\x90")
    x.phentsize = 7
    return x


@case
def no_program_headers(c, e):
    x = Elf(c, e, 2)
    x.sec(".text", SHT_PROGBITS, b"\x90")
    return x


@case
def too_many_ph(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_LOAD, b"\x90")
    x.phnum = 2049
    return x


@case
def too_many_sh(c, e):
    x = Elf(c, e, 2)
    x.seg(PT_LOAD, b"\x90")
    x.shnum = 40000
    return x


@case
def too_many_sh_rel(c, e):
    x = Elf(c, e, 1)
    x.shnum = 40000
    return x


@case
def shstrndx_out(c, e):
    x = Elf(c, e, 1)
    x.sec(".text", SHT_PROGBITS, b"\x90")
    x.shstrndx = 500
    return x


@case
def note_past_file(c, e):
    x = Elf(c, e, 1)
    x.sec(".note.x", SHT_NOTE, note(e, "GNU", 3, bytes(range(20))))
    b = bytearray(x.build())
    return b


def cap(e, is64, entries):
    f = e + ("QQ" if is64 else "II")
    return b"".join(struct.pack(f, t, v) for t, v in entries)


def add_cap_cases():
    for mach in (2, 18, 43, 50, 3, 62, 40):
        for nm, ents in (
            ("hw", [(1, 0x1 | 0x20 | 0x4000 | 0x10000000), (0, 0)]),
            ("hw_all", [(1, 0xffffffff)]),
            ("sf", [(2, 0x3)]),
            ("sf_unknown", [(2, 0x2 | 0x40)]),
            ("sf_noknwn", [(2, 0x2)]),
            ("unknown_tags", [(7, 1), (8, 2), (9, 3), (10, 4), (11, 5), (1, 0x8)]),
        ):
            def cap_case(c, e, mach=mach, ents=ents):
                x = Elf(c, e, 2, machine=mach)
                x.seg(PT_LOAD, b"\x90")
                x.sec(".SUNW_cap", SHT_SUNW_CAP, cap(e, c == 64, ents))
                return x

            cap_case.__name__ = "cap_%d_%s" % (mach, nm)
            case(cap_case)

    def cap_a(c, e):
        x = Elf(c, e, 2, machine=62)
        x.sec(".SUNW_cap", SHT_SUNW_CAP, b"A" + b"\0" * 31)
        return x

    case(cap_a)


add_cap_cases()


# --- core files --------------------------------------------------------------

def prpsinfo_linux(e, is64, fname, psargs):
    # struct elf_prpsinfo: fname at 40/28, psargs at 56/44.
    if is64:
        d = bytearray(136)
        d[40:40 + 16] = fname[:16].ljust(16, b"\0")
        d[56:56 + 80] = psargs[:80].ljust(80, b"\0")
    else:
        d = bytearray(124)
        d[28:28 + 16] = fname[:16].ljust(16, b"\0")
        d[44:44 + 80] = psargs[:80].ljust(80, b"\0")
    return bytes(d)


def auxv(e, is64, entries):
    f = e + ("QQ" if is64 else "II")
    return b"".join(struct.pack(f, t, v) for t, v in entries)


def core_case(name, build):
    def fn(c, e):
        return build(c, e, c == 64)

    fn.__name__ = name
    case(fn)


def core_linux(c, e, is64, fname=b"sleep", psargs=b"sleep 100 ", aux=True, cname="CORE"):
    x = Elf(c, e, 4)
    strings = b"/usr/bin/sleep\0x86_64\0"
    notes = note(e, cname, 1, b"\0" * 100) + note(e, cname, 3, prpsinfo_linux(e, is64, fname, psargs))
    if aux:
        notes += note(e, cname, 6, auxv(e, is64, [(11, 1000), (12, 1001), (13, 0xfffffffe), (14, 7),
                                                     (31, 0x400000), (15, 0x400000 + 15), (16, 0), (0, 0)]))
    x.seg(PT_NOTE, notes, vaddr=0)
    x.seg(PT_LOAD, strings, vaddr=0x400000)
    x.with_sections = False
    return x


core_case("core_linux", core_linux)
core_case("core_linux_noaux", lambda c, e, i: core_linux(c, e, i, aux=False))
core_case("core_linux_quote", lambda c, e, i: core_linux(c, e, i, fname=b"a'b", psargs=b"x\"y"))
core_case("core_linux_unprintable", lambda c, e, i: core_linux(c, e, i, fname=b"a\x01b", psargs=b"\x02"))
core_case("core_linux_empty", lambda c, e, i: core_linux(c, e, i, fname=b"", psargs=b""))
core_case("core_linux_long", lambda c, e, i: core_linux(c, e, i, fname=b"abcdefghijklmnop", psargs=b"q" * 79))
core_case("core_linux_core4", lambda c, e, i: core_linux(c, e, i, cname=b"CORE"))


def core_badaux(c, e, is64):
    x = Elf(c, e, 4)
    notes = note(e, "CORE", 3, prpsinfo_linux(e, is64, b"vi", b"vi x"))
    notes += note(e, "CORE", 6, auxv(e, is64, [(31, 0x999999), (15, 0x400000), (11, 5)] + [(11, i) for i in range(60)]))
    x.seg(PT_NOTE, notes, vaddr=0)
    x.seg(PT_LOAD, b"\x01\x02bad\0", vaddr=0x400000)
    x.with_sections = False
    return x


core_case("core_badaux", core_badaux)


def core_freebsd(c, e, is64):
    x = Elf(c, e, 4)
    d = bytearray(200)
    argoff = (4 + 4 + 8 + 17) if is64 else (4 + 4 + 17)
    d[argoff:argoff + 10] = b"freebsdcmd"
    struct.pack_into(e + "I", d, argoff + 81 + 2, 4242)
    x.seg(PT_NOTE, note(e, "FreeBSD", 3, bytes(d)), vaddr=0)
    x.with_sections = False
    return x


core_case("core_freebsd", core_freebsd)


def core_netbsd(c, e, is64):
    x = Elf(c, e, 4)
    d = bytearray(160)
    for off, v in ((8, 11), (12, 2), (80, 77), (100, 1000), (112, 100), (120, 3), (156, 1)):
        struct.pack_into(e + "I", d, off, v)
    d[124:124 + 6] = b"nbcmd\x01"
    x.seg(PT_NOTE, note(e, "NetBSD-CORE", 1, bytes(d)), vaddr=0)
    x.with_sections = False
    return x


core_case("core_netbsd", core_netbsd)


def core_noph(c, e, is64):
    x = Elf(c, e, 4)
    x.with_sections = False
    return x


core_case("core_noph", core_noph)


def core_badphsize(c, e, is64):
    x = Elf(c, e, 4)
    x.seg(PT_NOTE, b"")
    x.phentsize = 3
    return x


core_case("core_badphsize", core_badphsize)


def core_too_many(c, e, is64):
    x = Elf(c, e, 4)
    x.seg(PT_NOTE, b"")
    x.phnum = 3000
    return x


core_case("core_too_many", core_too_many)


# --- write ---------------------------------------------------------------------

n = 0
for name, fn in CASES.items():
    for c in (32, 64):
        for e in ("<", ">"):
            x = fn(c, e)
            data = x if isinstance(x, (bytes, bytearray)) else x.build()
            with open(os.path.join(OUT, "%s_%d%s" % (name, c, "le" if e == "<" else "be")), "wb") as f:
                f.write(data)
            n += 1
print(n, "files")
