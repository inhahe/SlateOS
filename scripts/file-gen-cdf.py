#!/usr/bin/env python3
"""Generate Composite Document Files (OLE2) for testing libmagic's readcdf.c.

    python3 scripts/file-gen-cdf.py OUTDIR SEED COUNT

Writes COUNT random CDFs, and a mutated copy of each, into OUTDIR: summary and
document-summary information with properties of every type readcdf.c prints
(and the vectors and types it gives up on), long and short (mini-stream)
streams, the stream names that identify Word, Excel, PowerPoint, Outlook,
encrypted and QuickBooks files, HWP file headers and the MSI class ID. The
mutations reach the header, SAT, directory and property-set error paths. Used
by scripts/file-diff.sh.
"""
import os
import random
import struct
import sys

OUT, SEED, COUNT = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
os.makedirs(OUT, exist_ok=True)
R = random.Random(SEED)

SS = 512
MSS = 64
END, FREE, FATSECT = 0xFFFFFFFE, 0xFFFFFFFF, 0xFFFFFFFD


def utf16name(s):
    b = s.encode("utf-16-le") + b"\0\0"
    return b.ljust(64, b"\0")[:64], len(b)


def prop_value(t, rnd):
    """(type, bytes) for a property of type t."""
    if t == 0x02:
        return struct.pack("<Ih", t, rnd.randint(-32768, 32767)) + b"\0\0"
    if t in (0x03, 0x0b):
        return struct.pack("<Ii", t, rnd.randint(-2**31, 2**31 - 1))
    if t == 0x13:
        return struct.pack("<II", t, rnd.randint(0, 2**32 - 1))
    if t == 0x04:
        return struct.pack("<If", t, rnd.choice([0.0, 1.5, -3.25, 1e30, 1e-30, 123456.789]))
    if t == 0x05:
        return struct.pack("<Id", t, rnd.choice([0.0, 2.5, -1e300, 1e-300, 3.141592653589793, 1e15]))
    if t in (0x14, 0x15):
        return struct.pack("<Iq", t, rnd.randint(-2**63, 2**63 - 1))
    if t == 0x40:
        v = rnd.choice([0, 1, 10**7 * 61, 10**7 * 3600 * 30, 116444736000000000 + rnd.randint(0, 10**17),
                        132000000000000000, 999999999999999, 10**18, -5, 2**62, 1000000000000000])
        return struct.pack("<Iq", t, v)
    if t in (0x1e, 0x1f):
        s = rnd.choice([b"Microsoft Office Word", b"Microsoft Excel", b"Title text", b"", b"x",
                        b"A" * 300, b"a\x01b\xffc", b"Windows Installer XML", b"PowerPoint", b"Crystal Reports"])
        if t == 0x1f:
            s = s.decode("latin-1").encode("utf-16-le")
            n = len(s) // 2 + 1
            body = s + b"\0\0"
        else:
            n = len(s) + 1
            body = s + b"\0"
        if rnd.random() < 0.1:
            n = rnd.choice([0, 1, 2, n + 3, 0x7fffffff])
        body += b"\0" * ((-len(body)) % 4)
        return struct.pack("<II", t, n) + body
    if t == 0x101e:
        items = [b"one", b"two", b"three"][: rnd.randint(1, 3)]
        out = struct.pack("<II", t, len(items))
        for s in items:
            b = s + b"\0"
            out += struct.pack("<I", len(b)) + b + b"\0" * ((-len(b)) % 4)
        return out
    if t == 0x1003:
        return struct.pack("<III", t, 2, 7) + struct.pack("<I", 9)
    if t in (0x00, 0x01, 0x47):
        return struct.pack("<I", t) + b"\0" * 4
    return struct.pack("<I", t) + b"\0" * 8


def property_set(rnd, os_kind):
    ntypes = [0x02, 0x03, 0x13, 0x04, 0x05, 0x1e, 0x1f, 0x40, 0x40, 0x1e, 0x01, 0x47, 0x0b, 0x14, 0x101e, 0x1003, 0x00, 0x2003]
    n = rnd.randint(0, 8)
    props = []
    for k in range(n):
        pid = rnd.choice([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 0x80000000, 0x77])
        t = rnd.choice(ntypes) if rnd.random() < 0.9 else rnd.randint(0, 0x50)
        props.append((pid, prop_value(t, rnd)))
    # Section: size, count, (id, offset)*, values.
    hdr_len = 8 + 8 * len(props)
    vals = b""
    offs = []
    for pid, v in props:
        offs.append(hdr_len + len(vals))
        vals += v
    sec = struct.pack("<II", hdr_len + len(vals), len(props))
    for (pid, _), o in zip(props, offs):
        sec += struct.pack("<II", pid, o)
    sec += vals
    osver = {"win": (2, rnd.choice([0x0206, 0x0a00, 0x0105])), "mac": (1, 0x0a05), "other": (7, 0x0304)}[os_kind]
    bo = 0xFFFE if rnd.random() < 0.9 else 0xFEFF
    head = struct.pack("<HHHH", bo, 0, osver[1], osver[0]) + b"\0" * 16 + struct.pack("<I", 1)
    head += b"\xe0\x85\x9f\xf2\xf9\x4f\x68\x10\xab\x91\x08\x00\x2b\x27\xb3\xd9" + struct.pack("<I", 48)
    return head + sec


class Cdf:
    def __init__(self, rnd, cutoff=4096):
        self.rnd = rnd
        self.cutoff = cutoff
        self.streams = []  # (name, type, data, clsid)

    def add(self, name, data, typ=2, clsid=b"\0" * 16):
        self.streams.append((name, typ, data, clsid))

    def build(self, root_clsid=b"\0" * 16):
        sectors = []  # list of 512-byte sectors (data)
        fat = []

        def alloc_chain(data):
            n = max(1, (len(data) + SS - 1) // SS)
            start = len(sectors)
            for k in range(n):
                sectors.append(data[k * SS:(k + 1) * SS].ljust(SS, b"\0"))
                fat.append(start + k + 1 if k < n - 1 else END)
            return start

        # Mini stream for small streams.
        mini = b""
        minifat = []
        ents = []
        for name, typ, data, clsid in self.streams:
            if typ == 2 and len(data) < self.cutoff and self.cutoff > 0:
                n = max(1, (len(data) + MSS - 1) // MSS)
                start = len(mini) // MSS
                mini += data.ljust(n * MSS, b"\0")
                for k in range(n):
                    minifat.append(start + k + 1 if k < n - 1 else END)
                ents.append((name, typ, start if data else END, len(data), clsid))
            elif typ == 2:
                ents.append((name, typ, alloc_chain(data), len(data), clsid))
            else:
                ents.append((name, typ, END, 0, clsid))
        root_start = alloc_chain(mini) if mini else END
        minifat_start = alloc_chain(b"".join(struct.pack("<I", v) for v in minifat)) if minifat else END
        # Directory: root first, then the entries as a right-leaning chain.
        dirs = []
        allents = [("Root Entry", 5, root_start, len(mini), root_clsid)] + ents
        for k, (name, typ, start, size, clsid) in enumerate(allents):
            nm, nl = utf16name(name)
            child = 1 if (k == 0 and len(allents) > 1) else FREE
            right = k + 1 if (k > 0 and k + 1 < len(allents)) else FREE
            d = nm + struct.pack("<HBB", nl, typ, 1) + struct.pack("<III", FREE, right, child)
            d += clsid + struct.pack("<I", 0) + struct.pack("<QQ", 0, 0) + struct.pack("<III", start, size, 0)
            dirs.append(d)
        dirdata = b"".join(dirs)
        dir_start = alloc_chain(dirdata)
        # FAT sectors: enough for all sectors including themselves.
        nfat = 1
        while (len(sectors) + nfat) > nfat * (SS // 4):
            nfat += 1
        fat_start = len(sectors)
        for k in range(nfat):
            sectors.append(b"")
            fat.append(FATSECT)
        fatbytes = b"".join(struct.pack("<I", v) for v in fat).ljust(nfat * SS, b"\xff")
        for k in range(nfat):
            sectors[fat_start + k] = fatbytes[k * SS:(k + 1) * SS]
        difat = [fat_start + k for k in range(nfat)] + [FREE] * (109 - nfat)
        hdr = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" + b"\0" * 16
        hdr += struct.pack("<HHHHH", 0x3e, 3, 0xfffe, 9, 6) + b"\0" * 6
        hdr += struct.pack("<IIIIIIII", 0, nfat, dir_start, 0, self.cutoff, minifat_start,
                           len(minifat) * 4 // SS + (1 if minifat else 0), END)
        hdr += struct.pack("<I", 0)
        hdr += b"".join(struct.pack("<I", v) for v in difat)
        assert len(hdr) == 512, len(hdr)
        return hdr + b"".join(sectors)


NAMES = ["Book", "Workbook", "WordDocument", "PowerPoint Document", "PowerPoint", "EncryptedPackage",
         "EncryptedSummary", "mfbu_header", "__properties_version1.0", "DigitalSignature", "Catalog",
         "Data", "1Table", "CompObj"]


def make(rnd):
    c = Cdf(rnd, cutoff=rnd.choice([4096, 4096, 0, 64]))
    if rnd.random() < 0.8:
        c.add("\x05SummaryInformation", property_set(rnd, rnd.choice(["win", "win", "mac", "other"])))
    if rnd.random() < 0.4:
        c.add("\x05DocumentSummaryInformation", property_set(rnd, "win"))
    if rnd.random() < 0.05:
        c.add("FileHeader", b"HWP Document File V5.00" + b"\0" * 40)
    for _ in range(rnd.randint(0, 4)):
        nm = rnd.choice(NAMES)
        if nm == "__properties_version1.0" and rnd.random() < 0.5:
            c.add("__recip_version1.0_#00000000", b"", typ=1)
        c.add(nm, os.urandom(rnd.choice([10, 100, 5000])))
    msi = b"\x84\x10\x0c\x00\x00\x00\x00\x00\xc0\x00\x00\x00\x00\x00\x00\x46"
    return c.build(root_clsid=msi if rnd.random() < 0.2 else b"\0" * 16)


def mutate(rnd, d):
    d = bytearray(d)
    for _ in range(rnd.choice([1, 1, 2, 3, 6])):
        r = rnd.random()
        if r < 0.3:
            p = rnd.randrange(0, 512)
        elif r < 0.6 and len(d) > 512:
            p = rnd.randrange(512, len(d))
        else:
            p = rnd.randrange(0, len(d))
        k = rnd.random()
        if k < 0.4:
            d[p] = rnd.randrange(256)
        elif k < 0.7:
            d[p] ^= 1 << rnd.randrange(8)
        else:
            w = rnd.choice([2, 4])
            if p + w <= len(d):
                d[p:p + w] = rnd.choice([b"\xff" * w, b"\0" * w, b"\xfe" + b"\xff" * (w - 1), os.urandom(w)])
    if rnd.random() < 0.08:
        d = d[:rnd.randrange(512, len(d) + 1)]
    return bytes(d)


for i in range(COUNT):
    d = make(R)
    with open(os.path.join(OUT, "c_%05d.doc" % i), "wb") as f:
        f.write(d)
    with open(os.path.join(OUT, "m_%05d.doc" % i), "wb") as f:
        f.write(mutate(R, d))
print(2 * COUNT, "files")
