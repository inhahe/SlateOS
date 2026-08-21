#!/usr/bin/env python3
"""Regression tests for `scripts/build-usb-image.py`.

Run: `python scripts/test-build-usb-image.py` (exit 0 = pass, 1 = fail).
No pytest dependency, matching `scripts/test-src-digest.py` and
`scripts/test-bench-history.py`: this has to run from a bare checkout.

Why this file exists
--------------------
The image builder writes structures that *we* never read back. Nothing in this
repository parses a GPT or a FAT32 directory; the only consumers are a PC's
firmware and Limine, and the only place their verdict is delivered is a machine
that either boots or does not. That is the worst possible feedback loop for a
byte-layout bug: the error message is a black screen, and it arrives after
someone has walked to another room with a flash drive.

So these tests are a *reader* — an implementation deliberately written
independently of the writer, which walks the produced image the way firmware
would: protective MBR, GPT header CRCs, partition entry, BPB, FAT chains,
directory entries, file contents. A test that merely re-derived the writer's
own arithmetic would agree with it about everything including its mistakes.

The cases that matter most are the ones a small staged tree would never hit on
its own and that hardware hits immediately:

* a file **larger than one cluster**, so the FAT chain is actually walked
  rather than assumed (the staged kernel is ~41 MiB — 10,000 clusters);
* a name needing a **long filename entry**, because `limine.conf` has a
  four-character extension and 8.3 allows three, so a broken LFN means Limine
  finds no config and boots nothing;
* a **nested directory**, because `EFI/BOOT/BOOTX64.EFI` is where firmware
  looks and nowhere else;
* an **empty file**, whose first-cluster field must be 0 and not a stray
  allocation.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import shutil
import struct
import sys
import tempfile
from binascii import crc32
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SPEC = importlib.util.spec_from_file_location(
    "build_usb_image", _HERE / "build-usb-image.py"
)
assert _SPEC and _SPEC.loader
usbimg = importlib.util.module_from_spec(_SPEC)
# Registered before exec: @dataclass resolves annotations via
# sys.modules[cls.__module__], which does not exist yet for a module loaded
# straight from a spec, and fails with a bare AttributeError if it is missing.
sys.modules["build_usb_image"] = usbimg
_SPEC.loader.exec_module(usbimg)

SECTOR = 512
_FAILURES: list[str] = []


def check(cond: bool, label: str) -> None:
    if not cond:
        _FAILURES.append(label)
        print(f"  FAIL: {label}")


def build(src: Path, out: Path, esp_mib: int = 384) -> None:
    """build_image with its progress report swallowed.

    The builder's summary is useful at a prompt and pure noise here, where a
    dozen images are made; suppressing it keeps a failure line visible instead
    of scrolled away.
    """
    with contextlib.redirect_stdout(io.StringIO()):
        usbimg.build_image(src, out, esp_mib, {"NvVars"}, "SLATEOS")


# ---------------------------------------------------------------------------
# An independent reader
# ---------------------------------------------------------------------------

class GptDisk:
    def __init__(self, data: bytes):
        self.data = data
        self.sectors = len(data) // SECTOR

    def sector(self, lba: int, count: int = 1) -> bytes:
        return self.data[lba * SECTOR:(lba + count) * SECTOR]

    def protective_mbr(self) -> dict:
        mbr = self.sector(0)
        entry = mbr[446:462]
        return {
            "signature_ok": mbr[510] == 0x55 and mbr[511] == 0xAA,
            "type": entry[4],
            "first_lba": struct.unpack_from("<I", entry, 8)[0],
            "sectors": struct.unpack_from("<I", entry, 12)[0],
            "other_entries_blank": mbr[462:510] == bytes(48),
        }

    def gpt_header(self, lba: int) -> dict:
        raw = self.sector(lba)
        stored_crc = struct.unpack_from("<I", raw, 16)[0]
        zeroed = bytearray(raw[:92])
        struct.pack_into("<I", zeroed, 16, 0)
        return {
            "signature": raw[0:8],
            "revision": struct.unpack_from("<I", raw, 8)[0],
            "header_size": struct.unpack_from("<I", raw, 12)[0],
            "crc_ok": stored_crc == (crc32(bytes(zeroed)) & 0xFFFFFFFF),
            "my_lba": struct.unpack_from("<Q", raw, 24)[0],
            "alt_lba": struct.unpack_from("<Q", raw, 32)[0],
            "first_usable": struct.unpack_from("<Q", raw, 40)[0],
            "last_usable": struct.unpack_from("<Q", raw, 48)[0],
            "disk_guid": raw[56:72],
            "entries_lba": struct.unpack_from("<Q", raw, 72)[0],
            "entry_count": struct.unpack_from("<I", raw, 80)[0],
            "entry_size": struct.unpack_from("<I", raw, 84)[0],
            "entries_crc": struct.unpack_from("<I", raw, 88)[0],
        }

    def partitions(self, header: dict) -> list[dict]:
        table = self.sector(header["entries_lba"],
                            header["entry_count"] * header["entry_size"] // SECTOR)
        out = []
        for i in range(header["entry_count"]):
            off = i * header["entry_size"]
            type_guid = table[off:off + 16]
            if type_guid == bytes(16):
                continue
            name = table[off + 56:off + 128].decode("utf-16-le").rstrip("\x00")
            out.append({
                "index": i,
                "type_guid": type_guid,
                "part_guid": table[off + 16:off + 32],
                "first_lba": struct.unpack_from("<Q", table, off + 32)[0],
                "last_lba": struct.unpack_from("<Q", table, off + 40)[0],
                "name": name,
            })
        return out

    def entries_crc_ok(self, header: dict) -> bool:
        table = self.sector(header["entries_lba"],
                            header["entry_count"] * header["entry_size"] // SECTOR)
        return (crc32(table) & 0xFFFFFFFF) == header["entries_crc"]


class Fat32:
    """A minimal FAT32 reader, written against the spec rather than the writer."""

    def __init__(self, data: bytes, part_lba: int):
        self.data = data
        self.base = part_lba * SECTOR
        bpb = data[self.base:self.base + SECTOR]
        self.bytes_per_sector = struct.unpack_from("<H", bpb, 11)[0]
        self.sectors_per_cluster = bpb[13]
        self.reserved = struct.unpack_from("<H", bpb, 14)[0]
        self.num_fats = bpb[16]
        self.root_entries_16 = struct.unpack_from("<H", bpb, 17)[0]
        self.total_16 = struct.unpack_from("<H", bpb, 19)[0]
        self.media = bpb[21]
        self.fat_size_16 = struct.unpack_from("<H", bpb, 22)[0]
        self.hidden = struct.unpack_from("<I", bpb, 28)[0]
        self.total_sectors = struct.unpack_from("<I", bpb, 32)[0]
        self.fat_size = struct.unpack_from("<I", bpb, 36)[0]
        self.root_cluster = struct.unpack_from("<I", bpb, 44)[0]
        self.fsinfo_sector = struct.unpack_from("<H", bpb, 48)[0]
        self.backup_boot = struct.unpack_from("<H", bpb, 50)[0]
        self.boot_sig = (bpb[510], bpb[511])
        self.fs_type = bpb[82:90]
        self.label = bpb[71:82]

        self.data_start = self.reserved + self.num_fats * self.fat_size
        self.cluster_count = (self.total_sectors - self.data_start) // self.sectors_per_cluster
        self.cluster_bytes = self.sectors_per_cluster * self.bytes_per_sector

    def _fat_entry(self, cluster: int) -> int:
        off = self.base + self.reserved * SECTOR + cluster * 4
        return struct.unpack_from("<I", self.data, off)[0] & 0x0FFFFFFF

    def chain(self, first: int) -> list[int]:
        out = []
        cluster = first
        while 2 <= cluster < 0x0FFFFFF8:
            if cluster in out:
                raise ValueError(f"cluster chain loops at {cluster}")
            out.append(cluster)
            cluster = self._fat_entry(cluster)
        return out

    def cluster_data(self, cluster: int) -> bytes:
        off = (self.base + self.data_start * SECTOR
               + (cluster - 2) * self.cluster_bytes)
        return self.data[off:off + self.cluster_bytes]

    def read_chain(self, first: int) -> bytes:
        return b"".join(self.cluster_data(c) for c in self.chain(first))

    def list_dir(self, cluster: int) -> list[dict]:
        raw = self.read_chain(cluster)
        out = []
        lfn_parts: dict[int, bytes] = {}
        for off in range(0, len(raw), 32):
            entry = raw[off:off + 32]
            if len(entry) < 32 or entry[0] == 0x00:
                break
            if entry[0] == 0xE5:
                lfn_parts.clear()
                continue
            if entry[11] == 0x0F:
                seq = entry[0] & 0x3F
                lfn_parts[seq] = entry[1:11] + entry[14:26] + entry[28:32]
                continue
            short = entry[0:11]
            name = None
            if lfn_parts:
                blob = b"".join(lfn_parts[k] for k in sorted(lfn_parts))
                text = blob.decode("utf-16-le")
                for stop in ("\x00", "\uffff"):
                    if stop in text:
                        text = text[:text.index(stop)]
                name = text
            lfn_parts = {}
            first_cluster = (struct.unpack_from("<H", entry, 20)[0] << 16) \
                | struct.unpack_from("<H", entry, 26)[0]
            out.append({
                "short": short,
                "name": name or short[:8].decode().rstrip() + (
                    "." + short[8:11].decode().rstrip() if short[8:11].strip() else ""),
                "attr": entry[11],
                "is_dir": bool(entry[11] & 0x10),
                "first_cluster": first_cluster,
                "size": struct.unpack_from("<I", entry, 28)[0],
            })
        return out

    def find(self, path: str) -> dict | None:
        cluster = self.root_cluster
        node = None
        for part in path.strip("/").split("/"):
            found = None
            for entry in self.list_dir(cluster):
                if entry["short"] in (b".          ", b"..         "):
                    continue
                if entry["name"].lower() == part.lower():
                    found = entry
                    break
            if found is None:
                return None
            node = found
            if node["is_dir"]:
                cluster = node["first_cluster"]
        return node

    def read_file(self, path: str) -> bytes | None:
        node = self.find(path)
        if node is None or node["is_dir"]:
            return None
        if node["size"] == 0:
            return b""
        return self.read_chain(node["first_cluster"])[:node["size"]]


# ---------------------------------------------------------------------------
# Fixture
# ---------------------------------------------------------------------------

def make_tree(root: Path) -> dict[str, bytes]:
    """A staged tree shaped like the real ESP, plus the awkward cases."""
    files: dict[str, bytes] = {}

    def put(rel: str, data: bytes) -> None:
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        files[rel] = data

    # Nested, 8.3-clean, exactly where firmware looks.
    put("EFI/BOOT/BOOTX64.EFI", bytes(range(256)) * 400)          # 100 KiB
    # Four-character extension -> needs a long filename entry.
    put("limine.conf", b"timeout: 3\n\n/OS Kernel\n    protocol: limine\n")
    # Lowercase name -> also needs an LFN if the case is to survive.
    # 300 KiB across 4 KiB clusters = 75 clusters, so the chain is really walked.
    put("boot/kernel", bytes((i * 7 + 3) & 0xFF for i in range(300 * 1024)))
    # A zero-length file must get first_cluster 0, not a stray allocation.
    put("boot/empty.bin", b"")
    # Two names that both want the same 8.3 short name.
    put("long name one.txt", b"one\n")
    put("long name two.txt", b"two\n")
    return files


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_structure(tmp: Path) -> None:
    print("structure: protective MBR, both GPTs, partition entry")
    src = tmp / "esp"
    src.mkdir()
    make_tree(src)
    img = tmp / "out.img"
    build(src, img)
    disk = GptDisk(img.read_bytes())

    mbr = disk.protective_mbr()
    check(mbr["signature_ok"], "MBR lacks the 0x55AA signature")
    check(mbr["type"] == 0xEE, f"MBR partition type is {mbr['type']:#x}, want 0xEE")
    check(mbr["first_lba"] == 1, "protective MBR must start at LBA 1")
    check(mbr["sectors"] == min(disk.sectors - 1, 0xFFFFFFFF),
          "protective MBR does not cover the whole disk")
    check(mbr["other_entries_blank"], "protective MBR has extra partition entries")

    primary = disk.gpt_header(1)
    check(primary["signature"] == b"EFI PART", "primary GPT signature wrong")
    check(primary["revision"] == 0x00010000, "primary GPT revision wrong")
    check(primary["header_size"] == 92, "primary GPT header size wrong")
    check(primary["crc_ok"], "primary GPT header CRC does not verify")
    check(primary["my_lba"] == 1, "primary GPT my_lba wrong")
    check(primary["alt_lba"] == disk.sectors - 1, "primary GPT alt_lba is not the last sector")
    check(primary["entries_lba"] == 2, "primary GPT entries must start at LBA 2")
    check(disk.entries_crc_ok(primary), "primary GPT entry-array CRC does not verify")

    backup = disk.gpt_header(disk.sectors - 1)
    check(backup["signature"] == b"EFI PART", "backup GPT signature wrong")
    check(backup["crc_ok"], "backup GPT header CRC does not verify")
    check(backup["my_lba"] == disk.sectors - 1, "backup GPT my_lba wrong")
    check(backup["alt_lba"] == 1, "backup GPT alt_lba wrong")
    check(backup["disk_guid"] == primary["disk_guid"], "the two GPTs disagree on the disk GUID")
    check(backup["entries_crc"] == primary["entries_crc"],
          "the two GPT entry arrays differ")
    check(disk.entries_crc_ok(backup), "backup GPT entry-array CRC does not verify")
    check(backup["entries_lba"] == disk.sectors - 1 - 32,
          "backup GPT entries are not in the 32 sectors before the header")

    parts = disk.partitions(primary)
    check(len(parts) == 1, f"expected exactly 1 partition, got {len(parts)}")
    if parts:
        esp = parts[0]
        check(esp["index"] == 0, "the ESP must be the first entry")
        check(esp["type_guid"] == usbimg.ESP_TYPE_GUID.bytes_le,
              "partition type GUID is not the EFI System Partition GUID")
        check(esp["name"] == "EFI System Partition",
              f"partition name is {esp['name']!r}")
        check(esp["first_lba"] == 2048, "the ESP is not 1 MiB-aligned")
        check(esp["last_lba"] <= primary["last_usable"],
              "the ESP runs past last_usable and would collide with the backup GPT")
        check(esp["first_lba"] >= primary["first_usable"],
              "the ESP starts before first_usable")


def test_filesystem(tmp: Path) -> None:
    print("filesystem: BPB, FAT chains, directories, long names, file contents")
    src = tmp / "esp2"
    src.mkdir()
    expected = make_tree(src)
    img = tmp / "out2.img"
    build(src, img)
    raw = img.read_bytes()
    disk = GptDisk(raw)
    esp = disk.partitions(disk.gpt_header(1))[0]
    fs = Fat32(raw, esp["first_lba"])

    check(fs.boot_sig == (0x55, 0xAA), "FAT boot sector lacks 0x55AA")
    check(fs.bytes_per_sector == 512, "bytes per sector is not 512")
    check(fs.fs_type == b"FAT32   ", f"FS type string is {fs.fs_type!r}")
    check(fs.root_entries_16 == 0, "FAT32 must report 0 root entries")
    check(fs.total_16 == 0, "FAT32 must report a 0 sixteen-bit sector count")
    check(fs.fat_size_16 == 0, "FAT32 must report a 0 sixteen-bit FAT size")
    check(fs.media == 0xF8, "media descriptor is not 0xF8")
    check(fs.root_cluster == 2, "root directory cluster is not 2")
    check(fs.backup_boot == 6, "backup boot sector is not at 6")
    check(fs.hidden == esp["first_lba"],
          f"hidden sectors is {fs.hidden}, must equal the partition's first LBA")
    check(fs.total_sectors == esp["last_lba"] - esp["first_lba"] + 1,
          "the BPB's sector count does not match the partition size")
    # Below 65525 clusters the volume is FAT16 by definition, whatever the BPB
    # claims -- so firmware would read it with the wrong driver.
    check(fs.cluster_count >= usbimg.FAT32_MIN_CLUSTERS,
          f"only {fs.cluster_count} clusters; FAT32 needs {usbimg.FAT32_MIN_CLUSTERS}")

    # The backup boot sector must actually be a copy, or a damaged sector 0 is
    # unrecoverable.
    boot0 = raw[fs.base:fs.base + SECTOR]
    boot6 = raw[fs.base + 6 * SECTOR:fs.base + 7 * SECTOR]
    check(boot0 == boot6, "the backup boot sector is not a copy of sector 0")

    # Both FATs must be identical: firmware may read either.
    fat0_off = fs.base + fs.reserved * SECTOR
    fat1_off = fat0_off + fs.fat_size * SECTOR
    fat_len = fs.fat_size * SECTOR
    check(raw[fat0_off:fat0_off + fat_len] == raw[fat1_off:fat1_off + fat_len],
          "the two FAT copies differ")
    check(struct.unpack_from("<I", raw, fat0_off)[0] & 0x0FFFFFFF == 0x0FFFFFF8,
          "FAT[0] is not the media descriptor entry")

    for rel, data in expected.items():
        got = fs.read_file(rel)
        if got is None:
            check(False, f"{rel} not found in the image")
            continue
        check(got == data, f"{rel} content differs ({len(got)} vs {len(data)} bytes)")

    # The multi-cluster file must really be a chain, not one giant cluster.
    kernel = fs.find("boot/kernel")
    if kernel:
        chain = fs.chain(kernel["first_cluster"])
        check(len(chain) > 1, "boot/kernel occupies a single cluster; the chain is untested")
        check(chain == list(range(chain[0], chain[0] + len(chain))),
              "boot/kernel's clusters are not contiguous, so the chain order matters "
              "and the reader agreeing is a coincidence worth investigating")

    empty = fs.find("boot/empty.bin")
    check(empty is not None and empty["first_cluster"] == 0,
          "a zero-length file must have first_cluster 0")

    # Case must survive: the tree staged `boot`, not `BOOT`.
    root_names = {e["name"] for e in fs.list_dir(fs.root_cluster)}
    check("boot" in root_names, f"lowercase 'boot' did not survive; root has {sorted(root_names)}")
    check("EFI" in root_names, "EFI directory missing from the root")
    check("limine.conf" in root_names, "limine.conf missing from the root")

    # Colliding long names must get distinct short names, or one shadows the other.
    shorts = [e["short"] for e in fs.list_dir(fs.root_cluster)
              if e["name"].startswith("long name")]
    check(len(shorts) == 2, f"expected 2 colliding long names, saw {len(shorts)}")
    check(len(set(shorts)) == len(shorts),
          f"colliding long names share a short name: {shorts}")

    # "." and ".." in a subdirectory, with ".." pointing at 0 for a root parent.
    boot = fs.find("boot")
    if boot:
        entries = {e["short"]: e for e in fs.list_dir(boot["first_cluster"])}
        check(b".          " in entries, "subdirectory has no '.' entry")
        check(b"..         " in entries, "subdirectory has no '..' entry")
        if b".          " in entries:
            check(entries[b".          "]["first_cluster"] == boot["first_cluster"],
                  "'.' does not point at its own directory")
        if b"..         " in entries:
            check(entries[b"..         "]["first_cluster"] == 0,
                  "'..' must be 0 when the parent is the root directory")


def test_reproducible_and_sensitive(tmp: Path) -> None:
    print("reproducibility: same tree -> same bytes; changed tree -> changed bytes")
    src = tmp / "esp3"
    src.mkdir()
    make_tree(src)
    a, b = tmp / "a.img", tmp / "b.img"
    build(src, a)
    build(src, b)
    check(a.read_bytes() == b.read_bytes(),
          "two builds of the same tree produced different images")

    # Sensitivity is the half that matters: a builder that returned a constant
    # image would pass the equality check above perfectly.
    (src / "limine.conf").write_bytes(b"timeout: 0\n")
    c = tmp / "c.img"
    build(src, c)
    check(a.read_bytes() != c.read_bytes(),
          "changing a staged file did not change the image")

    disk_a = GptDisk(a.read_bytes()).gpt_header(1)["disk_guid"]
    disk_c = GptDisk(c.read_bytes()).gpt_header(1)["disk_guid"]
    check(disk_a != disk_c, "the disk GUID did not follow the content change")


def test_excludes(tmp: Path) -> None:
    print("excludes: OVMF's NvVars must not reach the image")
    src = tmp / "esp4"
    src.mkdir()
    make_tree(src)
    (src / "NvVars").write_bytes(b"host state, not boot state")
    img = tmp / "d.img"
    build(src, img)
    raw = img.read_bytes()
    fs = Fat32(raw, GptDisk(raw).partitions(GptDisk(raw).gpt_header(1))[0]["first_lba"])
    check(fs.find("NvVars") is None, "NvVars was copied into the image")


def test_rejections(tmp: Path) -> None:
    print("rejections: too-small ESP, content that does not fit")
    src = tmp / "esp5"
    src.mkdir()
    make_tree(src)

    # 64 MiB yields far fewer than 65525 clusters, so it is FAT16 by definition.
    try:
        build(src, tmp / "e.img", 64)
        check(False, "a 64 MiB ESP was accepted; it cannot be FAT32")
    except ValueError as exc:
        check("65525" in str(exc) or "clusters" in str(exc),
              f"wrong error for a too-small ESP: {exc}")

    # Content larger than the partition must be refused, not silently truncated
    # -- a truncated kernel is a boot that gets further than it should.
    big = src / "boot" / "huge.bin"
    big.write_bytes(b"\x5a" * (400 * 1024 * 1024))
    try:
        build(src, tmp / "f.img")
        check(False, "content larger than the ESP was accepted")
    except ValueError as exc:
        check("clusters" in str(exc), f"wrong error for oversized content: {exc}")
    finally:
        big.unlink()


def test_shortname_rules() -> None:
    print("short names: what needs a long filename entry and what does not")
    clean = ["EFI", "BOOT", "BOOTX64.EFI", "KERNEL", "A.B", "README.TXT"]
    dirty = ["boot", "kernel", "limine.conf", "a.b.c", "TOOLONGNAME.X",
             "has space.txt", "EXT.LONG", ".", ".."]
    for name in clean:
        check(usbimg._is_clean_83(name), f"{name!r} should need no long filename entry")
    for name in dirty:
        check(not usbimg._is_clean_83(name), f"{name!r} should need a long filename entry")

    # The checksum ties LFN entries to their short entry; a wrong one makes
    # every reader discard the long name and fall back to `LIMINE~1.CON`.
    check(usbimg._lfn_checksum(b"LIMINE~1CON") ==
          usbimg._lfn_checksum(b"LIMINE~1CON"), "checksum is not deterministic")
    check(usbimg._lfn_checksum(b"LIMINE~1CON") !=
          usbimg._lfn_checksum(b"LIMINE~2CON"), "checksum ignores the short name")

    taken: set[bytes] = set()
    first = usbimg._short_name("long name one.txt", taken)
    second = usbimg._short_name("long name two.txt", taken)
    check(first != second, "two colliding names produced the same short name")
    check(first.endswith(b"TXT") and second.endswith(b"TXT"),
          "the extension was not carried into the short name")
    check(b" " not in first[:8].rstrip(b" "), "the short name has an embedded space")


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="usbimg-test-"))
    try:
        tests = [
            lambda: test_structure(tmp),
            lambda: test_filesystem(tmp),
            lambda: test_reproducible_and_sensitive(tmp),
            lambda: test_excludes(tmp),
            lambda: test_rejections(tmp),
            test_shortname_rules,
        ]
        for test in tests:
            test()
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    if _FAILURES:
        print(f"\n{len(_FAILURES)} FAILED")
        return 1
    print(f"\nall {len(_FAILURES) == 0 and 6} build-usb-image test groups passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
