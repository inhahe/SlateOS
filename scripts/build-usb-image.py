#!/usr/bin/env python3
"""Build a real, GPT-partitioned, UEFI-bootable raw disk image for SlateOS.

Why this exists
---------------
`scripts/boot-test.sh` hands QEMU ``-drive format=raw,file=fat:rw:build/esp``.
That is QEMU's *virtual* FAT: it synthesises a filesystem on the fly from a host
directory.  It is excellent for a test harness -- no image to rebuild, no stale
copy to boot by accident -- but it means the project has never produced a disk
image at all.  There is nothing to write to a USB stick, and nothing that a real
UEFI firmware could start.  `scripts/build-iso.sh` does not fill the gap: it
needs `xorriso`, which is not installed on this machine (checked 2026-08-21),
and an ISO9660 image is read-only, so it could never carry a writable ESP or a
second partition for the rootfs.

This script produces the missing artefact: a raw image containing a protective
MBR, a primary and backup GPT, and one EFI System Partition holding a FAT32
filesystem built from a staged directory tree (normally ``build/esp``, exactly
what the boot test stages).  ``dd`` it to a flash drive and a PC's firmware will
boot it.  Point QEMU at it with ``-drive format=raw,file=...`` instead of
``fat:rw:`` and you are testing the same bytes the stick will contain -- which
is the point: the virtual-FAT path exercises *none* of the partition table, the
BPB, or the on-disk directory structure that real firmware must parse.

Deliberately dependency-free
----------------------------
Everything here is stdlib Python.  No mtools, no mkfs.vfat, no sgdisk, no
xorriso.  That is not asceticism: the boot test already has a prerequisite gate
that exists because a missing `limine/` surfaced as a `cp: cannot stat` after a
full workspace build, and adding a fresh set of external tools would recreate
that failure mode in a fresh clone.  `scripts/create-disk.py` already writes
FAT16/FAT32 from scratch in Python for the FAT driver self-test, so the approach
is the project's established one.

Reproducibility
---------------
The image is a pure function of its inputs.  File timestamps are pinned to a
fixed epoch, and the disk/partition GUIDs and the FAT volume serial are derived
by SHA-256 from the staged content, so identical content produces a
byte-identical image and different content produces different GUIDs.  That
matters for the same reason `libc.a`'s byte-reproducibility mattered: it makes
"did this actually change?" answerable by comparing hashes instead of mtimes.

Usage
-----
    python scripts/build-usb-image.py                        # build/esp -> build/slateos-usb.img
    python scripts/build-usb-image.py --source DIR --output IMG
    python scripts/build-usb-image.py --esp-size-mib 768     # force a size
    python scripts/build-usb-image.py --exclude NvVars       # repeatable

Exit codes: 0 built; 1 bad input (missing source, too-small ESP, name clash).
"""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
import uuid
from binascii import crc32
from dataclasses import dataclass, field
from pathlib import Path

SECTOR = 512

# GPT type GUID for an EFI System Partition.
ESP_TYPE_GUID = uuid.UUID("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")

# GPT reserves 33 sectors at each end: 1 header + 32 sectors of entries
# (128 entries x 128 bytes).  Both copies are mandatory -- a firmware that finds
# a primary header whose CRC does not match will fall back to the backup, and
# some firmwares refuse a disk that has no backup at all.
GPT_ENTRY_COUNT = 128
GPT_ENTRY_SIZE = 128
GPT_ENTRIES_SECTORS = (GPT_ENTRY_COUNT * GPT_ENTRY_SIZE) // SECTOR  # 32
GPT_RESERVED_SECTORS = 1 + GPT_ENTRIES_SECTORS                      # 33

# 1 MiB alignment for the first partition.  Not superstition: flash translation
# layers erase in blocks far larger than a sector, and a partition that starts
# mid-erase-block makes every write a read-modify-write.  1 MiB is what every
# modern partitioner uses and what firmware expects to see.
PART_ALIGN_SECTORS = (1024 * 1024) // SECTOR  # 2048

# FAT32 geometry.  4 KiB clusters (8 sectors) across the whole supported size
# range -- large enough that the FAT stays small for a 41 MiB kernel, small
# enough that a 384 MiB partition still clears the FAT32 cluster-count floor.
SECTORS_PER_CLUSTER = 8
RESERVED_SECTORS = 32
NUM_FATS = 2

# A filesystem with fewer than 65525 clusters is FAT16 *by definition* (the
# type is determined by cluster count, not by what the BPB claims), so a
# too-small "FAT32" is one that firmware will read with the wrong driver.
FAT32_MIN_CLUSTERS = 65525

# Fixed timestamp for every directory entry: 2026-01-01 00:00:00.
# FAT packs the date as (year-1980)<<9 | month<<5 | day and the time as
# hour<<11 | minute<<5 | (second/2).
FIXED_FAT_DATE = ((2026 - 1980) << 9) | (1 << 5) | 1
FIXED_FAT_TIME = 0

# Long-file-name entries are capped at 20 per name by the spec (20 x 13 = 260).
LFN_MAX_ENTRIES = 20

_SHORTNAME_OK = set(b"$%'-_@~`!(){}^#& " + bytes(range(ord("0"), ord("9") + 1))
                    + bytes(range(ord("A"), ord("Z") + 1)))


# ---------------------------------------------------------------------------
# Short (8.3) names and long-file-name entries
# ---------------------------------------------------------------------------

def _is_clean_83(name: str) -> bool:
    """True if `name` can be stored as a bare 8.3 entry with no LFN.

    Requires the name to already be uppercase: FAT short names have no case,
    so storing `boot` as a short entry would silently rename it to `BOOT`.
    Lookups are case-insensitive so that would still *work*, but the directory
    would no longer describe what was staged, and a later diff of the image
    against the tree would show a phantom change.  Emitting an LFN instead
    preserves the exact bytes.
    """
    if name in (".", ".."):
        return False
    base, dot, ext = name.partition(".")
    if dot and "." in ext:
        return False                       # more than one dot -> needs an LFN
    if not base or len(base) > 8 or len(ext) > 3:
        return False
    for part in (base, ext):
        for ch in part.encode("utf-8", "surrogateescape"):
            if ch not in _SHORTNAME_OK:
                return False
    return True


def _short_name(name: str, taken: set[bytes]) -> bytes:
    """Return the 11-byte padded 8.3 name for `name`, unique within `taken`."""
    base, dot, ext = name.partition(".")
    if not dot:
        base, ext = name, ""

    def scrub(s: str, limit: int) -> bytes:
        out = bytearray()
        for ch in s.upper().encode("utf-8", "surrogateescape"):
            if ch == ord(" "):
                continue                   # spaces are dropped, not mapped
            out.append(ch if ch in _SHORTNAME_OK else ord("_"))
            if len(out) >= limit:
                break
        return bytes(out)

    stem = scrub(base, 8) or b"_"
    tail = scrub(ext, 3)

    # ~N numeric tail, as Windows generates it.  N is bounded because a
    # directory cannot hold unbounded colliding names in this project, but the
    # loop is written to terminate rather than trusted to.
    for n in range(1, 1000000):
        suffix = b"~" + str(n).encode()
        cand = stem[: max(1, 8 - len(suffix))] + suffix
        padded = cand.ljust(8, b" ") + tail.ljust(3, b" ")
        if padded not in taken:
            taken.add(padded)
            return padded
    raise ValueError(f"cannot generate a unique short name for {name!r}")


def _lfn_checksum(short: bytes) -> int:
    """The one-byte checksum that ties LFN entries to their short entry."""
    csum = 0
    for byte in short:
        csum = (((csum & 1) << 7) + (csum >> 1) + byte) & 0xFF
    return csum


def _lfn_entries(name: str, short: bytes) -> list[bytes]:
    """Build the LFN entries preceding a short entry, last-part-first."""
    utf16 = name.encode("utf-16-le")
    chars = [utf16[i:i + 2] for i in range(0, len(utf16), 2)]
    chars.append(b"\x00\x00")                       # NUL terminator
    while len(chars) % 13:
        chars.append(b"\xFF\xFF")                   # pad to a whole entry
    parts = [chars[i:i + 13] for i in range(0, len(chars), 13)]
    if len(parts) > LFN_MAX_ENTRIES:
        raise ValueError(f"name too long for FAT: {name!r}")

    csum = _lfn_checksum(short)
    out = []
    for idx, part in enumerate(parts, start=1):
        entry = bytearray(32)
        entry[0] = idx | (0x40 if idx == len(parts) else 0)
        entry[11] = 0x0F                            # attr: LFN
        entry[12] = 0
        entry[13] = csum
        entry[26:28] = b"\x00\x00"                  # first cluster: always 0
        blob = b"".join(part)
        entry[1:11] = blob[0:10]
        entry[14:26] = blob[10:22]
        entry[28:32] = blob[22:26]
        out.append(bytes(entry))
    out.reverse()                                   # on disk: highest index first
    return out


def _dir_entry(short: bytes, attr: int, cluster: int, size: int) -> bytes:
    entry = bytearray(32)
    entry[0:11] = short
    entry[11] = attr
    struct.pack_into("<H", entry, 14, FIXED_FAT_TIME)   # creation time
    struct.pack_into("<H", entry, 16, FIXED_FAT_DATE)   # creation date
    struct.pack_into("<H", entry, 18, FIXED_FAT_DATE)   # last access date
    struct.pack_into("<H", entry, 20, (cluster >> 16) & 0xFFFF)
    struct.pack_into("<H", entry, 22, FIXED_FAT_TIME)   # write time
    struct.pack_into("<H", entry, 24, FIXED_FAT_DATE)   # write date
    struct.pack_into("<H", entry, 26, cluster & 0xFFFF)
    struct.pack_into("<I", entry, 28, size)
    return bytes(entry)


# ---------------------------------------------------------------------------
# The staged tree
# ---------------------------------------------------------------------------

@dataclass
class Node:
    name: str
    is_dir: bool
    source: Path | None = None          # files only
    size: int = 0
    children: list["Node"] = field(default_factory=list)
    first_cluster: int = 0
    clusters: int = 0
    short: bytes = b""

    def entry_bytes(self) -> int:
        """Bytes this node occupies in its parent's directory."""
        if _is_clean_83(self.name):
            return 32
        return 32 * (1 + len(_lfn_entries(self.name, b"X" * 11)))


def scan_tree(root: Path, exclude: set[str]) -> Node:
    def walk(path: Path, name: str) -> Node:
        if path.is_dir():
            node = Node(name=name, is_dir=True)
            # Sorted, so the image is a function of the tree and not of the
            # order the filesystem happens to hand back.
            for child in sorted(path.iterdir(), key=lambda p: p.name):
                if child.name in exclude:
                    continue
                if child.is_symlink():
                    raise ValueError(f"symlink in staged tree: {child}")
                node.children.append(walk(child, child.name))
            return node
        return Node(name=name, is_dir=False, source=path, size=path.stat().st_size)

    return walk(root, "")


# ---------------------------------------------------------------------------
# FAT32
# ---------------------------------------------------------------------------

@dataclass
class Fat32Geometry:
    total_sectors: int
    sectors_per_fat: int
    data_start_sector: int
    cluster_count: int


def fat32_geometry(total_sectors: int) -> Fat32Geometry:
    """Solve for the FAT size, which depends on the cluster count it sizes."""
    data_sectors = total_sectors - RESERVED_SECTORS
    sectors_per_fat = 0
    for _ in range(32):
        clusters = data_sectors // SECTORS_PER_CLUSTER
        new_spf = ((clusters + 2) * 4 + SECTOR - 1) // SECTOR
        if new_spf == sectors_per_fat:
            break
        sectors_per_fat = new_spf
        data_sectors = total_sectors - RESERVED_SECTORS - NUM_FATS * sectors_per_fat
    data_start = RESERVED_SECTORS + NUM_FATS * sectors_per_fat
    return Fat32Geometry(
        total_sectors=total_sectors,
        sectors_per_fat=sectors_per_fat,
        data_start_sector=data_start,
        cluster_count=(total_sectors - data_start) // SECTORS_PER_CLUSTER,
    )


def assign_clusters(root: Node, geom: Fat32Geometry) -> int:
    """Assign every node a cluster chain.  Returns the next free cluster.

    A directory's size depends only on how many entries it holds, and an entry's
    size depends only on its name -- never on where its target landed.  That is
    what makes a single pass possible: sizes are known before any cluster number
    is chosen, and the numbers can then be handed out in one walk.
    """
    cluster_bytes = SECTORS_PER_CLUSTER * SECTOR
    next_free = 2

    def size_of_dir(node: Node) -> int:
        total = 0 if node is root else 64          # "." and ".."
        for child in node.children:
            total += child.entry_bytes()
        total += 32                                 # end-of-directory marker
        return total

    def visit(node: Node) -> None:
        nonlocal next_free
        if node.is_dir:
            need = size_of_dir(node)
        else:
            need = node.size
        node.clusters = max(1, -(-need // cluster_bytes)) if (need or node.is_dir) else 0
        if node.clusters:
            node.first_cluster = next_free
            next_free += node.clusters
        else:
            node.first_cluster = 0                  # empty file: no chain
        for child in node.children:
            visit(child)

    visit(root)
    if next_free - 2 > geom.cluster_count:
        raise ValueError(
            f"staged content needs {next_free - 2} clusters "
            f"but the partition holds only {geom.cluster_count}"
        )
    return next_free


def build_directory(node: Node, root: Node) -> bytes:
    """Serialise one directory's entries."""
    out = bytearray()

    # Reserve every already-8.3-clean name *before* generating any ~N name, so
    # a generated name can never collide with a literal one later in the same
    # directory.  Doing this lazily would make the outcome depend on order.
    taken: set[bytes] = set()
    for child in node.children:
        if _is_clean_83(child.name):
            base, _, ext = child.name.partition(".")
            taken.add(base.ljust(8).encode()[:8] + ext.ljust(3).encode()[:3])

    if node is not root:
        out += _dir_entry(b".          ", 0x10, node.first_cluster, 0)
        # ".." points at cluster 0 when the parent is the root, per the spec --
        # not at 2.  A firmware that follows a literal 2 would still land on the
        # root here, but writing 0 is what every other implementation checks.
        parent_cluster = 0 if node.parent is root else node.parent.first_cluster  # type: ignore[attr-defined]
        out += _dir_entry(b"..         ", 0x10, parent_cluster, 0)

    for child in node.children:
        if _is_clean_83(child.name):
            base, _, ext = child.name.partition(".")
            short = base.ljust(8).encode()[:8] + ext.ljust(3).encode()[:3]
        else:
            short = _short_name(child.name, taken)
            out += b"".join(_lfn_entries(child.name, short))
        child.short = short
        attr = 0x10 if child.is_dir else 0x20
        out += _dir_entry(short, attr, child.first_cluster,
                          0 if child.is_dir else child.size)

    return bytes(out)


def build_fat(root: Node, geom: Fat32Geometry, next_free: int) -> bytes:
    fat = bytearray(geom.sectors_per_fat * SECTOR)
    struct.pack_into("<I", fat, 0, 0x0FFFFFF8)      # media descriptor
    struct.pack_into("<I", fat, 4, 0x0FFFFFFF)      # end-of-chain marker

    def chain(node: Node) -> None:
        if node.clusters:
            for i in range(node.clusters):
                cluster = node.first_cluster + i
                nxt = 0x0FFFFFFF if i == node.clusters - 1 else cluster + 1
                struct.pack_into("<I", fat, cluster * 4, nxt)
        for child in node.children:
            chain(child)

    chain(root)
    assert next_free * 4 <= len(fat), "FAT is too small for the assigned chains"
    return bytes(fat)


def build_boot_sector(geom: Fat32Geometry, hidden_sectors: int,
                      volume_serial: int, label: str) -> bytes:
    bpb = bytearray(SECTOR)
    bpb[0:3] = b"\xEB\x58\x90"                      # jmp short +0x58; nop
    bpb[3:11] = b"SLATEOS "
    struct.pack_into("<H", bpb, 11, SECTOR)
    bpb[13] = SECTORS_PER_CLUSTER
    struct.pack_into("<H", bpb, 14, RESERVED_SECTORS)
    bpb[16] = NUM_FATS
    struct.pack_into("<H", bpb, 17, 0)              # root entries: 0 on FAT32
    struct.pack_into("<H", bpb, 19, 0)              # 16-bit total: 0 on FAT32
    bpb[21] = 0xF8                                  # media: fixed disk
    struct.pack_into("<H", bpb, 22, 0)              # 16-bit FAT size: 0 on FAT32
    struct.pack_into("<H", bpb, 24, 32)             # sectors per track (legacy)
    struct.pack_into("<H", bpb, 26, 64)             # heads (legacy)
    struct.pack_into("<I", bpb, 28, hidden_sectors) # sectors before the partition
    struct.pack_into("<I", bpb, 32, geom.total_sectors)
    struct.pack_into("<I", bpb, 36, geom.sectors_per_fat)
    struct.pack_into("<H", bpb, 40, 0)              # both FATs live (no mirror off)
    struct.pack_into("<H", bpb, 42, 0)              # filesystem version
    struct.pack_into("<I", bpb, 44, 2)              # root directory cluster
    struct.pack_into("<H", bpb, 48, 1)              # FSInfo sector
    struct.pack_into("<H", bpb, 50, 6)              # backup boot sector
    bpb[64] = 0x80                                  # BIOS drive number
    bpb[66] = 0x29                                  # extended boot signature
    struct.pack_into("<I", bpb, 67, volume_serial)
    bpb[71:82] = label.upper().ljust(11).encode("ascii", "replace")[:11]
    bpb[82:90] = b"FAT32   "
    bpb[510] = 0x55
    bpb[511] = 0xAA
    return bytes(bpb)


def build_fsinfo(free_clusters: int, next_free: int) -> bytes:
    fsinfo = bytearray(SECTOR)
    struct.pack_into("<I", fsinfo, 0, 0x41615252)
    struct.pack_into("<I", fsinfo, 484, 0x61417272)
    struct.pack_into("<I", fsinfo, 488, free_clusters)
    struct.pack_into("<I", fsinfo, 492, next_free)
    struct.pack_into("<I", fsinfo, 508, 0xAA550000)
    return bytes(fsinfo)


# ---------------------------------------------------------------------------
# GPT
# ---------------------------------------------------------------------------

def build_protective_mbr(total_sectors: int) -> bytes:
    """An MBR whose single entry claims the whole disk as type 0xEE.

    Its job is to make a tool that only understands MBR see one unknown,
    full-disk partition rather than free space it might offer to partition.
    """
    mbr = bytearray(SECTOR)
    entry = bytearray(16)
    entry[0] = 0x00                                  # not bootable
    entry[1:4] = b"\x00\x02\x00"                     # CHS start (legacy 0/2/0)
    entry[4] = 0xEE                                  # GPT protective
    entry[5:8] = b"\xFF\xFF\xFF"                     # CHS end: saturated
    struct.pack_into("<I", entry, 8, 1)              # first LBA
    struct.pack_into("<I", entry, 12, min(total_sectors - 1, 0xFFFFFFFF))
    mbr[446:462] = entry
    mbr[510] = 0x55
    mbr[511] = 0xAA
    return bytes(mbr)


def build_gpt_entries(partitions: list[tuple[uuid.UUID, uuid.UUID, int, int, str]]) -> bytes:
    table = bytearray(GPT_ENTRY_COUNT * GPT_ENTRY_SIZE)
    for idx, (type_guid, part_guid, first, last, name) in enumerate(partitions):
        off = idx * GPT_ENTRY_SIZE
        table[off:off + 16] = type_guid.bytes_le
        table[off + 16:off + 32] = part_guid.bytes_le
        struct.pack_into("<Q", table, off + 32, first)
        struct.pack_into("<Q", table, off + 40, last)
        struct.pack_into("<Q", table, off + 48, 0)   # attributes
        encoded = name.encode("utf-16-le")[:70]
        table[off + 56:off + 56 + len(encoded)] = encoded
    return bytes(table)


def build_gpt_header(*, my_lba: int, alt_lba: int, entries_lba: int,
                     first_usable: int, last_usable: int,
                     disk_guid: uuid.UUID, entries_crc: int) -> bytes:
    header = bytearray(SECTOR)
    header[0:8] = b"EFI PART"
    struct.pack_into("<I", header, 8, 0x00010000)    # revision 1.0
    struct.pack_into("<I", header, 12, 92)           # header size
    struct.pack_into("<I", header, 16, 0)            # CRC placeholder
    struct.pack_into("<I", header, 20, 0)            # reserved
    struct.pack_into("<Q", header, 24, my_lba)
    struct.pack_into("<Q", header, 32, alt_lba)
    struct.pack_into("<Q", header, 40, first_usable)
    struct.pack_into("<Q", header, 48, last_usable)
    header[56:72] = disk_guid.bytes_le
    struct.pack_into("<Q", header, 72, entries_lba)
    struct.pack_into("<I", header, 80, GPT_ENTRY_COUNT)
    struct.pack_into("<I", header, 84, GPT_ENTRY_SIZE)
    struct.pack_into("<I", header, 88, entries_crc)
    # The header CRC covers exactly the first `header size` bytes with the CRC
    # field itself zeroed -- not the whole sector.
    struct.pack_into("<I", header, 16, crc32(bytes(header[:92])) & 0xFFFFFFFF)
    return bytes(header)


# ---------------------------------------------------------------------------
# Assembly
# ---------------------------------------------------------------------------

def content_digest(root: Node) -> bytes:
    """A hash over the staged tree: names, sizes and file contents."""
    digest = hashlib.sha256()

    def visit(node: Node, prefix: str) -> None:
        path = f"{prefix}/{node.name}"
        digest.update(path.encode("utf-8", "surrogateescape"))
        digest.update(b"\x00d" if node.is_dir else b"\x00f")
        if not node.is_dir and node.source is not None:
            digest.update(struct.pack("<Q", node.size))
            with node.source.open("rb") as handle:
                for chunk in iter(lambda: handle.read(1 << 20), b""):
                    digest.update(chunk)
        for child in node.children:
            visit(child, path)

    visit(root, "")
    return digest.digest()


def _link_parents(node: Node) -> None:
    for child in node.children:
        child.parent = node          # type: ignore[attr-defined]
        _link_parents(child)


def build_image(source: Path, output: Path, esp_size_mib: int | None,
                exclude: set[str], label: str) -> None:
    if not source.is_dir():
        raise ValueError(f"staged source directory not found: {source}")

    root = scan_tree(source, exclude)
    root.parent = None               # type: ignore[attr-defined]
    _link_parents(root)

    staged_bytes = 0

    def total(node: Node) -> None:
        nonlocal staged_bytes
        staged_bytes += node.size
        for child in node.children:
            total(child)

    total(root)

    # Size the ESP.  The floor is not a taste call: below ~268 MiB of data area
    # a 4 KiB-cluster filesystem has fewer than 65525 clusters and is therefore
    # FAT16 no matter what the BPB says.
    if esp_size_mib is None:
        wanted = staged_bytes + (96 << 20)           # slack for a debug kernel
        esp_size_mib = max(512, -(-wanted // (1 << 20)))
        esp_size_mib = -(-esp_size_mib // 16) * 16   # round up to 16 MiB

    esp_sectors = (esp_size_mib * (1 << 20)) // SECTOR
    part_first = PART_ALIGN_SECTORS
    part_last = part_first + esp_sectors - 1
    # Leave a full alignment unit after the partition so the backup GPT is not
    # crammed against it, then the mandatory 33 reserved sectors.
    total_sectors = part_last + 1 + PART_ALIGN_SECTORS

    geom = fat32_geometry(esp_sectors)
    if geom.cluster_count < FAT32_MIN_CLUSTERS:
        raise ValueError(
            f"--esp-size-mib {esp_size_mib} yields {geom.cluster_count} clusters; "
            f"FAT32 requires at least {FAT32_MIN_CLUSTERS}. Use 384 MiB or more."
        )

    next_free = assign_clusters(root, geom)

    seed = content_digest(root)
    disk_guid = uuid.UUID(bytes=hashlib.sha256(seed + b"disk").digest()[:16])
    part_guid = uuid.UUID(bytes=hashlib.sha256(seed + b"part").digest()[:16])
    volume_serial = struct.unpack("<I", hashlib.sha256(seed + b"vol").digest()[:4])[0]

    entries = build_gpt_entries(
        [(ESP_TYPE_GUID, part_guid, part_first, part_last, "EFI System Partition")]
    )
    entries_crc = crc32(entries) & 0xFFFFFFFF
    last_lba = total_sectors - 1
    backup_entries_lba = last_lba - GPT_ENTRIES_SECTORS

    primary = build_gpt_header(
        my_lba=1, alt_lba=last_lba, entries_lba=2,
        first_usable=GPT_RESERVED_SECTORS + 1,
        last_usable=backup_entries_lba - 1,
        disk_guid=disk_guid, entries_crc=entries_crc,
    )
    backup = build_gpt_header(
        my_lba=last_lba, alt_lba=1, entries_lba=backup_entries_lba,
        first_usable=GPT_RESERVED_SECTORS + 1,
        last_usable=backup_entries_lba - 1,
        disk_guid=disk_guid, entries_crc=entries_crc,
    )

    fat = build_fat(root, geom, next_free)
    used = next_free - 2
    boot_sector = build_boot_sector(geom, part_first, volume_serial, label)
    fsinfo = build_fsinfo(geom.cluster_count - used, next_free)

    part_off = part_first * SECTOR
    data_off = part_off + geom.data_start_sector * SECTOR
    cluster_bytes = SECTORS_PER_CLUSTER * SECTOR

    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as img:
        # Establish the full length first.  Seeking past the end and writing
        # leaves a sparse file on filesystems that support it and a
        # zero-filled one where they do not; either way every byte we never
        # touch reads as zero, which is what an empty cluster must be.
        img.truncate(total_sectors * SECTOR)

        img.seek(0)
        img.write(build_protective_mbr(total_sectors))
        img.write(primary)
        img.write(entries)

        img.seek(backup_entries_lba * SECTOR)
        img.write(entries)
        img.write(backup)

        img.seek(part_off)
        img.write(boot_sector)
        img.write(fsinfo)
        # The backup boot sector at partition-relative 6 is what a firmware
        # falls back to when sector 0 is damaged; leaving it zeroed is a
        # filesystem that cannot be repaired.
        img.seek(part_off + 6 * SECTOR)
        img.write(boot_sector)
        img.write(fsinfo)

        for i in range(NUM_FATS):
            img.seek(part_off + (RESERVED_SECTORS + i * geom.sectors_per_fat) * SECTOR)
            img.write(fat)

        def emit(node: Node) -> None:
            offset = data_off + (node.first_cluster - 2) * cluster_bytes
            if node.is_dir:
                img.seek(offset)
                img.write(build_directory(node, root))
            elif node.source is not None and node.size:
                img.seek(offset)
                with node.source.open("rb") as handle:
                    for chunk in iter(lambda: handle.read(1 << 20), b""):
                        img.write(chunk)
            for child in node.children:
                emit(child)

        # Directory entries are filled in by build_directory using each child's
        # already-assigned first cluster, so a single walk suffices.
        emit(root)

    print(f"=== USB image written: {output}")
    print(f"    disk        {total_sectors * SECTOR / (1 << 20):.0f} MiB "
          f"({total_sectors} sectors), GPT + protective MBR")
    print(f"    ESP         LBA {part_first}..{part_last} "
          f"({esp_sectors * SECTOR / (1 << 20):.0f} MiB FAT32, "
          f"{geom.cluster_count} clusters of {cluster_bytes // 1024} KiB)")
    print(f"    staged      {staged_bytes / (1 << 20):.1f} MiB in "
          f"{used} clusters ({100.0 * used / geom.cluster_count:.1f}% full)")
    print(f"    disk GUID   {disk_guid}")
    print(f"    content id  {seed.hex()[:16]}")
    print()
    print("    Boot it in QEMU exactly as firmware would see it:")
    print(f"      ./scripts/boot-test.sh --usb-image")
    print("    Write it to a stick: see bare-metal-boot.md (read the")
    print("    recovery section BEFORE choosing a target device).")


def main(argv: list[str] | None = None) -> int:
    project_root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--source", type=Path,
                        default=project_root / "build" / "esp",
                        help="staged ESP tree (default: build/esp)")
    parser.add_argument("--output", type=Path,
                        default=project_root / "build" / "slateos-usb.img",
                        help="output image (default: build/slateos-usb.img)")
    parser.add_argument("--esp-size-mib", type=int, default=None,
                        help="force the ESP size instead of deriving it")
    parser.add_argument("--exclude", action="append", default=[],
                        help="staged entry name to skip (repeatable)")
    parser.add_argument("--label", default="SLATEOS",
                        help="FAT32 volume label (default: SLATEOS)")
    args = parser.parse_args(argv)

    # NvVars is OVMF's own non-volatile variable store, written back into the
    # staged directory by every QEMU run.  It is host state, not boot state:
    # copying it in would make the image differ between runs for no reason.
    exclude = set(args.exclude) | {"NvVars"}

    try:
        build_image(args.source, args.output, args.esp_size_mib, exclude, args.label)
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
