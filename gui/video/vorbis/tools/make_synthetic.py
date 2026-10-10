"""Writes tests/data/synthetic_*.ogg: Vorbis streams no encoder writes --
random, valid setup headers (floor 0 and floor 1, residues 0, 1 and 2,
several submaps and modes, lattice, listed and sequence books, sparse and
single-entry books, every block size) followed by random audio packets --
so that the paths libvorbis never exercises are held to Tremor too.

    python3 tools/make_synthetic.py [count]

A stream's audio is noise: what is tested is that Tremor and the port read
the same bits the same way. Values stay in the ranges an encoder would use
(the books' values small, the floors' amplitudes moderate), so that no
stream relies on C's undefined arithmetic.
"""

import pathlib
import random
import sys

OUT = pathlib.Path(__file__).resolve().parent.parent / "tests" / "data"


class Bits:
    """libogg's oggpack_write: LSb first."""

    def __init__(self):
        self.bytes = bytearray()
        self.bit = 0

    def put(self, value, bits):
        for k in range(bits):
            if self.bit % 8 == 0:
                self.bytes.append(0)
            if (value >> k) & 1:
                self.bytes[-1] |= 1 << (self.bit % 8)
            self.bit += 1


def ilog(v):
    return v.bit_length()


def quantvals(entries, dim):
    """_book_maptype1_quantvals."""
    bits = ilog(entries)
    vals = entries >> ((bits - 1) * (dim - 1) // dim)
    while True:
        acc = acc1 = 1
        i = 0
        while i < dim:
            if entries // vals < acc:
                break
            acc *= vals
            acc1 *= vals + 1
            i += 1
        if i >= dim and acc <= entries and acc1 > entries:
            return vals
        if i < dim or acc > entries:
            vals -= 1
        else:
            vals += 1


def float32(mantissa, exp):
    """Vorbis's packed float: mantissa * 2^(exp - 788)."""
    sign = 0x80000000 if mantissa < 0 else 0
    return sign | ((exp & 0x3FF) << 21) | (abs(mantissa) & 0x1FFFFF)


def tree_lengths(rng, n, maxlen):
    """Codeword lengths of a random full binary tree with n leaves."""
    if n == 1:
        return [rng.randint(1, 3)]
    depths = [0]
    while len(depths) < n:
        candidates = [i for i, d in enumerate(depths) if d < maxlen]
        i = rng.choice(candidates)
        d = depths.pop(i)
        depths += [d + 1, d + 1]
    return depths


class Book:
    def __init__(self, dim, entries, lengths, maptype=0, values=None, q=None):
        self.dim, self.entries, self.lengths = dim, entries, lengths
        self.maptype, self.values, self.q = maptype, values, q

    def write(self, w, rng):
        w.put(0x564342, 24)
        w.put(self.dim, 16)
        w.put(self.entries, 24)
        sparse = any(l == 0 for l in self.lengths)
        ordered = not sparse and self.lengths == sorted(self.lengths) and rng.random() < 0.8
        if ordered:
            w.put(1, 1)
            length = self.lengths[0]
            w.put(length - 1, 5)
            i = 0
            while i < self.entries:
                num = sum(1 for l in self.lengths[i:] if l == length)
                w.put(num, ilog(self.entries - i))
                i += num
                length += 1
        else:
            w.put(0, 1)
            w.put(1 if sparse else 0, 1)
            for l in self.lengths:
                if sparse:
                    w.put(1 if l else 0, 1)
                    if l:
                        w.put(l - 1, 5)
                else:
                    w.put(l - 1, 5)
        w.put(self.maptype, 4)
        if self.maptype:
            q_min, q_delta, q_quant, seq = self.q
            w.put(q_min, 32)
            w.put(q_delta, 32)
            w.put(q_quant - 1, 4)
            w.put(seq, 1)
            for v in self.values:
                w.put(v, q_quant)


def random_book(rng, dim, entries, maptype, sparse=False, maxlen=16, small=True):
    used = entries
    lengths_used = tree_lengths(rng, entries if not sparse else max(1, rng.randint(1, entries)), maxlen)
    if sparse and len(lengths_used) < entries:
        lengths = [0] * entries
        slots = rng.sample(range(entries), len(lengths_used))
        for s, l in zip(slots, lengths_used):
            lengths[s] = l
    else:
        lengths = lengths_used
        # Sorted, a book can be sent length-ordered; else in any order.
        if rng.random() < 0.4:
            lengths.sort()
        else:
            rng.shuffle(lengths)
    del used
    if maptype == 0:
        return Book(dim, entries, lengths)
    q_quant = rng.randint(1, 6 if small else 10)
    # Minimum and step as an encoder's: small, a few binary places.
    q_min = float32(-rng.randint(0, 40), 788 - rng.randint(0, 3))
    q_delta = float32(rng.randint(1, 8), 788 - rng.randint(0, 4))
    seq = 1 if rng.random() < 0.3 else 0
    count = quantvals(entries, dim) if maptype == 1 else entries * dim
    values = [rng.randrange(1 << q_quant) for _ in range(count)]
    return Book(dim, entries, lengths, maptype, values, (q_min, q_delta, q_quant, seq))


def lsp_book(rng):
    """A floor 0 book: values from 0 up, small steps (cumulated by the floor)."""
    dim = rng.randint(1, 4)
    entries = rng.randint(2, 32)
    lengths = tree_lengths(rng, entries, 10)
    q_quant = rng.randint(2, 6)
    values = [rng.randrange(1 << q_quant) for _ in range(entries * dim)]
    q_min = float32(0, 788)
    q_delta = float32(1, 788 - rng.randint(4, 7))
    return Book(dim, entries, lengths, 2, values, (q_min, q_delta, q_quant, rng.randint(0, 1)))


def make(seed):
    rng = random.Random(seed)
    channels = rng.choice([1, 1, 2, 2, 2, 3, 4, 6])
    exps = sorted([rng.randint(6, 13), rng.randint(6, 13)])
    if rng.random() < 0.2:
        exps[1] = exps[0]
    bs = [1 << exps[0], 1 << exps[1]]
    rate = rng.choice([8000, 22050, 44100, 48000])
    books = []

    def add(book):
        books.append(book)
        return len(books) - 1

    # Value books for residues: lattice or listed, several dimensions.
    value_books = []
    for _ in range(rng.randint(2, 6)):
        dim = rng.choice([1, 2, 2, 4, 4, 8])
        maptype = rng.choice([1, 2])
        entries = rng.choice([3, 9, 16, 25, 27, 64, 81]) if maptype == 1 else rng.randint(2, 40)
        if maptype == 1 and dim > 4:
            entries = rng.choice([1, 2, 3, 256])
        value_books.append(add(random_book(rng, dim, entries, maptype, sparse=rng.random() < 0.2)))
    # Plain books for floor 1 and classification.
    plain = [add(random_book(rng, 1, rng.randint(1, 128), 0, sparse=rng.random() < 0.3)) for _ in range(rng.randint(1, 4))]

    floors = []
    for _ in range(rng.randint(1, 3)):
        if rng.random() < 0.4:
            lsp = [add(lsp_book(rng)) for _ in range(rng.randint(1, 3))]
            floors.append(("floor0", {
                "order": rng.randint(1, 24), "rate": rate, "barkmap": rng.randint(32, 400),
                "ampbits": rng.randint(4, 12), "ampdB": rng.randint(30, 140), "books": lsp,
            }))
        else:
            nclass = rng.randint(1, 4)
            classes = []
            for _ in range(nclass):
                subs = rng.randint(0, 2)
                cbook = rng.choice(plain) if subs else 0
                subbooks = [rng.choice([-1] + plain) for _ in range(1 << subs)]
                classes.append((rng.randint(1, 4), subs, cbook, subbooks))
            partitions = rng.randint(1, 8)
            pclass = [rng.randrange(nclass) for _ in range(partitions)]
            pclass[rng.randrange(partitions)] = nclass - 1  # every class used up to the last
            rangebits = rng.randint(7, 12)
            count = sum(classes[c][0] for c in pclass)
            while count > 63:
                pclass.pop()
                count = sum(classes[c][0] for c in pclass)
            posts = rng.sample(range(1, 1 << rangebits), count)
            floors.append(("floor1", {
                "pclass": pclass, "classes": classes, "mult": rng.randint(1, 4),
                "rangebits": rangebits, "posts": posts,
            }))

    residues = []
    for _ in range(rng.randint(1, 3)):
        kind = rng.randint(0, 2)
        partitions = rng.randint(1, 6)
        gdim = rng.choice([1, 1, 2])
        group = add(Book(gdim, partitions ** gdim, tree_lengths(rng, partitions ** gdim, 12)))
        cascades = [rng.choice([0, 1, 2, 3, 5, 1 | 8]) for _ in range(partitions)]
        stagebooks = [[rng.choice(value_books) for _ in range(bin(c).count("1"))] for c in cascades]
        limit = bs[1] // 2 * (channels if kind == 2 else 1)
        begin = rng.choice([0, 0, 16, 32])
        end = rng.randint(begin, limit + 64)
        grouping = rng.choice([2, 4, 8, 16, 32])
        residues.append((kind, begin, end, grouping, partitions, group, cascades, stagebooks))

    maps = []
    for _ in range(rng.randint(1, 2)):
        submaps = rng.randint(1, min(3, channels)) if rng.random() < 0.5 else 1
        coupling = []
        if channels > 1 and rng.random() < 0.6:
            for _ in range(rng.randint(1, channels)):
                m, a = rng.sample(range(channels), 2)
                coupling.append((m, a))
        chmux = [rng.randrange(submaps) for _ in range(channels)]
        subs = [(rng.randrange(len(floors)), rng.randrange(len(residues))) for _ in range(submaps)]
        maps.append((submaps, coupling, chmux, subs))

    modes = [(rng.randint(0, 1), rng.randrange(len(maps))) for _ in range(rng.randint(1, 4))]

    # The headers.
    ident = Bits()
    ident.put(1, 8)
    for b in b"vorbis":
        ident.put(b, 8)
    ident.put(0, 32)
    ident.put(channels, 8)
    ident.put(rate, 32)
    ident.put(0, 32)
    ident.put(rng.randint(32000, 256000), 32)
    ident.put(0, 32)
    ident.put(exps[0], 4)
    ident.put(exps[1], 4)
    ident.put(1, 1)

    comment = Bits()
    comment.put(3, 8)
    for b in b"vorbis":
        comment.put(b, 8)
    vendor = f"synthetic {seed}".encode()
    comment.put(len(vendor), 32)
    for b in vendor:
        comment.put(b, 8)
    tags = [f"SEED={seed}".encode(), b"TITLE=noise"]
    comment.put(len(tags), 32)
    for t in tags:
        comment.put(len(t), 32)
        for b in t:
            comment.put(b, 8)
    comment.put(1, 1)

    setup = Bits()
    setup.put(5, 8)
    for b in b"vorbis":
        setup.put(b, 8)
    setup.put(len(books) - 1, 8)
    for book in books:
        book.write(setup, rng)
    setup.put(0, 6)  # one time backend
    setup.put(0, 16)
    setup.put(len(floors) - 1, 6)
    for kind, f in floors:
        if kind == "floor0":
            setup.put(0, 16)
            setup.put(f["order"], 8)
            setup.put(f["rate"], 16)
            setup.put(f["barkmap"], 16)
            setup.put(f["ampbits"], 6)
            setup.put(f["ampdB"], 8)
            setup.put(len(f["books"]) - 1, 4)
            for b in f["books"]:
                setup.put(b, 8)
        else:
            setup.put(1, 16)
            setup.put(len(f["pclass"]), 5)
            for c in f["pclass"]:
                setup.put(c, 4)
            # Tremor reads as many classes as the highest one used.
            for dim, subs, cbook, subbooks in f["classes"][: max(f["pclass"]) + 1]:
                setup.put(dim - 1, 3)
                setup.put(subs, 2)
                if subs:
                    setup.put(cbook, 8)
                for sb in subbooks:
                    setup.put(sb + 1, 8)
            setup.put(f["mult"] - 1, 2)
            setup.put(f["rangebits"], 4)
            for p in f["posts"]:
                setup.put(p, f["rangebits"])
    setup.put(len(residues) - 1, 6)
    for kind, begin, end, grouping, partitions, group, cascades, stagebooks in residues:
        setup.put(kind, 16)
        setup.put(begin, 24)
        setup.put(end, 24)
        setup.put(grouping - 1, 24)
        setup.put(partitions - 1, 6)
        setup.put(group, 8)
        for c in cascades:
            setup.put(c & 7, 3)
            setup.put(1 if c >> 3 else 0, 1)
            if c >> 3:
                setup.put(c >> 3, 5)
        for sb in stagebooks:
            for b in sb:
                setup.put(b, 8)
    setup.put(len(maps) - 1, 6)
    for submaps, coupling, chmux, subs in maps:
        setup.put(0, 16)
        setup.put(1 if submaps > 1 else 0, 1)
        if submaps > 1:
            setup.put(submaps - 1, 4)
        setup.put(1 if coupling else 0, 1)
        if coupling:
            setup.put(len(coupling) - 1, 8)
            bits = ilog(channels - 1)
            for m, a in coupling:
                setup.put(m, bits)
                setup.put(a, bits)
        setup.put(0, 2)
        if submaps > 1:
            for c in chmux:
                setup.put(c, 4)
        for f, r in subs:
            setup.put(0, 8)
            setup.put(f, 8)
            setup.put(r, 8)
    setup.put(len(modes) - 1, 6)
    for blockflag, mapping in modes:
        setup.put(blockflag, 1)
        setup.put(0, 16)
        setup.put(0, 16)
        setup.put(mapping, 8)
    setup.put(1, 1)

    # Audio: random bits, each packet's first bit clear (an audio packet)
    # and its mode one the stream has, mostly.
    modebits = ilog(len(modes) - 1)
    packets = []
    for _ in range(rng.randint(30, 60)):
        p = Bits()
        p.put(0 if rng.random() < 0.95 else 1, 1)
        p.put(rng.randrange(len(modes)) if rng.random() < 0.95 else rng.randrange(1 << modebits) if modebits else 0, modebits)
        for _ in range(rng.randint(0, 300) * 8 + rng.randint(0, 7)):
            p.put(rng.getrandbits(1), 1)
        packets.append(bytes(p.bytes))
    return [bytes(ident.bytes), bytes(comment.bytes), bytes(setup.bytes)] + packets


CRC_TABLE = []
for i in range(256):
    r = i << 24
    for _ in range(8):
        r = ((r << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if r & 0x80000000 else (r << 1) & 0xFFFFFFFF
    CRC_TABLE.append(r)


def crc(data):
    c = 0
    for b in data:
        c = ((c << 8) & 0xFFFFFFFF) ^ CRC_TABLE[((c >> 24) & 0xFF) ^ b]
    return c


def ogg(packets, serial):
    """Each header on its own page, the audio packets a page each."""
    out = bytearray()
    for seq, p in enumerate(packets):
        lacing = [255] * (len(p) // 255) + [len(p) % 255]
        assert len(lacing) <= 255
        flags = (2 if seq == 0 else 0) | (4 if seq == len(packets) - 1 else 0)
        granule = 0 if seq < 3 else (seq - 2) * 64
        header = bytearray(b"OggS" + bytes([0, flags]) + granule.to_bytes(8, "little") + serial.to_bytes(4, "little")
                           + seq.to_bytes(4, "little") + bytes(4) + bytes([len(lacing)]) + bytes(lacing))
        page = header + p
        page[22:26] = crc(page).to_bytes(4, "little")
        out += page
    return bytes(out)


if __name__ == "__main__":
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 24
    for old in OUT.glob("synthetic_*.ogg"):
        old.unlink()
    total = 0
    for seed in range(1, count + 1):
        data = ogg(make(seed), 0x5EED0000 + seed)
        (OUT / f"synthetic_{seed:02}.ogg").write_bytes(data)
        total += len(data)
    print(f"{count} streams, {total} bytes")
