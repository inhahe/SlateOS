"""Seeds for elfparse's `elf` target: an ELF file of each shape the parser
tells apart, made here, and the small native programs a provisioned checkout
has built (the services the kernel embeds, made by
`scripts/bootstrap-worktree.sh --services`), when they are there.
Called by `scripts/fuzz.py` as `seed(src, corpus)`."""
import os
import struct

ET_EXEC, ET_DYN = 2, 3
PT_LOAD, PT_INTERP, PT_NOTE, PT_GNU_PROPERTY = 1, 3, 4, 0x6474E553
PF_X, PF_W, PF_R = 1, 2, 4
EHDR, PHDR = 64, 56

# The services `kernel/src` embeds, smallest first; fuzz.py skips any seed
# over 256 KiB.
SERVICES = ["hello", "ticker", "httpget", "udpget", "init", "netstack"]


def elf(e_type, entry, osabi, phdrs, payload):
    """An ELF64 x86-64 file: header, program headers `phdrs` (tuples of
    type, flags, offset, vaddr, filesz, memsz, align), then `payload`."""
    ident = bytes([0x7F]) + b"ELF" + bytes([2, 1, 1, osabi]) + bytes(8)
    phoff = EHDR if phdrs else 0
    header = ident + struct.pack("<HHIQQQIHHHHHH", e_type, 62, 1, entry, phoff, 0, 0, EHDR,
                                 PHDR, len(phdrs), 64, 0, 0)
    table = b"".join(struct.pack("<IIQQQQQQ", t, f, o, v, v, fs, ms, a)
                     for (t, f, o, v, fs, ms, a) in phdrs)
    return header + table + payload


def at(n):
    """Where the payload of a file with `n` program headers begins."""
    return EHDR + n * PHDR


def note(name, n_type, desc, align):
    """One note, each part padded to `align`."""
    def pad(b):
        return b + bytes(-len(b) % align)
    return pad(struct.pack("<III", len(name), len(desc), n_type) + name) + pad(desc)


def made():
    code = bytes([0x90]) * 32
    interp_glibc = b"/lib64/ld-linux-x86-64.so.2" + bytes(1)
    interp_musl = b"/lib/ld-musl-x86_64.so.1" + bytes(1)
    slate4 = note(b"SlateOS" + bytes(1), 1, struct.pack("<I", 1), 4)
    slate8 = note(b"SlateOS" + bytes(1), 1, struct.pack("<I", 1), 8)
    gnu = note(b"GNU" + bytes(1), 5, bytes(16), 8)
    files = {
        "static": elf(ET_EXEC, 0x400000, 0, [(PT_LOAD, PF_R | PF_X, at(1), 0x400000, 32, 32, 0x1000)],
                      code),
        "static-bss": elf(ET_EXEC, 0x400000, 0,
                          [(PT_LOAD, PF_R | PF_X, at(2), 0x400000, 16, 16, 0x1000),
                           (PT_LOAD, PF_R | PF_W, at(2) + 16, 0x600000, 16, 0x5000, 0x1000)],
                          code),
        "pie-glibc": elf(ET_DYN, 0x1000, 0,
                         [(PT_INTERP, PF_R, at(2), 0, len(interp_glibc), len(interp_glibc), 1),
                          (PT_LOAD, PF_R | PF_X, 0, 0, at(2) + len(interp_glibc),
                           at(2) + len(interp_glibc), 0x1000)],
                         interp_glibc),
        "pie-musl-osabi3": elf(ET_DYN, 0x1000, 3,
                               [(PT_INTERP, PF_R, at(2), 0, len(interp_musl), len(interp_musl), 1),
                                (PT_LOAD, PF_R | PF_X, 0, 0, at(2) + len(interp_musl),
                                 at(2) + len(interp_musl), 0x1000)],
                               interp_musl),
        "slateos-note4": elf(ET_DYN, 0x1000, 3,
                             [(PT_NOTE, PF_R, at(2), 0, len(slate4), len(slate4), 4),
                              (PT_LOAD, PF_R | PF_X, 0, 0, at(2) + len(slate4), at(2) + len(slate4),
                               0x1000)],
                             slate4),
        "slateos-note8": elf(ET_DYN, 0x1000, 0,
                             [(PT_NOTE, PF_R, at(2), 0, len(gnu + slate8), len(gnu + slate8), 8),
                              (PT_LOAD, PF_R | PF_X, 0, 0, at(2) + len(gnu + slate8),
                               at(2) + len(gnu + slate8), 0x1000)],
                             gnu + slate8),
        "slateos-osabi": elf(ET_EXEC, 0x400000, 255,
                             [(PT_LOAD, PF_R | PF_X, at(1), 0x400000, 32, 32, 0x1000)], code),
        "gnu-property": elf(ET_EXEC, 0x400000, 0,
                            [(PT_GNU_PROPERTY, PF_R, at(2), 0, len(gnu), len(gnu), 8),
                             (PT_LOAD, PF_R | PF_X, at(2), 0x400000, len(gnu), len(gnu), 0x1000)],
                            gnu),
        "no-phdrs": elf(ET_DYN, 0, 0, [], b""),
    }
    return files


def seed(src, corpus):
    out = corpus("elf")
    for label, data in made().items():
        with open(os.path.join(out, label), "wb") as f:
            f.write(data)
    for name in SERVICES:
        path = os.path.join(src, "services", name, "target", "x86_64-unknown-none", "release", name)
        if os.path.isfile(path):
            with open(path, "rb") as f:
                data = f.read()
            with open(os.path.join(out, "service-" + name), "wb") as f:
                f.write(data)
