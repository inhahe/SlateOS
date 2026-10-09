#!/usr/bin/env python3
"""Write a Linux swap-area header (mkswap's, version 1) onto an image, so SlateOS's kernel takes it as swap.

The kernel uses a whole disk for swap only when the disk's first slot holds
this header -- `SWAPSPACE2` at the end of the header page, version 1, the
area's last page (`kernel/src/mm/swap.rs`, `parse_header`). Until 2026-10-09 it
took the first 16 MiB of any virtio disk without an ext4 superblock at its
start, which is how it would have written swap over a partitioned or a FAT
disk (known-issues
`A-SWAP-HAS-NO-SWAP-FILE-AND-CLAIMS-ANY-DISK-WITHOUT-AN-EXT4-SUPERBLOCK`). The
boot test runs this over `build/swap.img` before every boot, so an image made
before the change -- all zeros -- is still taken.

    python scripts/mkswap.py [--page-size N] [--label TEXT] IMAGE
    python scripts/mkswap.py --self-test

Only the header page is written; the rest of the image is left alone, so a
second run changes nothing (the UUID already there is kept). The page size is
16384 by default, SlateOS's page; Linux's `mkswap` on x86 writes 4096, which
the kernel reads as well. Exit 0 when the header is in place, 2 on a usage
error or an image too small to hold a page after its header.
"""

import argparse
import os
import struct
import sys
import tempfile
import uuid

MAGIC = b"SWAPSPACE2"
VERSION_AT = 1024  # version, last_page, nr_badpages: three little-endian u32s
UUID_AT = 1036
LABEL_AT = 1052
PAGE_SIZES = (4096, 8192, 16384)


def existing_uuid(head: bytes, page_size: int):
    """The UUID of a header already at the start of `head` for `page_size`, or None."""
    if len(head) >= page_size and head[page_size - len(MAGIC):page_size] == MAGIC:
        return bytes(head[UUID_AT:UUID_AT + 16])
    return None


def make_header(size: int, page_size: int, label: bytes, keep_uuid=None) -> bytes:
    """The header page for an image of `size` bytes."""
    pages = size // page_size
    if pages < 2:
        raise ValueError("an image of %d bytes holds no page after a %d-byte header" % (size, page_size))
    page = bytearray(page_size)
    struct.pack_into("<III", page, VERSION_AT, 1, pages - 1, 0)
    page[UUID_AT:UUID_AT + 16] = keep_uuid or uuid.uuid4().bytes
    label = label[:16]
    page[LABEL_AT:LABEL_AT + len(label)] = label
    page[page_size - len(MAGIC):] = MAGIC
    return bytes(page)


def write_header(path: str, page_size: int, label: bytes) -> int:
    """Write the header onto `path`; returns the area's last page."""
    size = os.path.getsize(path)
    with open(path, "r+b") as f:
        head = f.read(page_size)
        header = make_header(size, page_size, label, existing_uuid(head, page_size))
        f.seek(0)
        f.write(header)
    return size // page_size - 1


def self_test() -> int:
    """Headers for both page sizes, read back field by field; idempotence; refusal."""
    failures = []
    with tempfile.TemporaryDirectory() as d:
        img = os.path.join(d, "swap.img")
        with open(img, "wb") as f:
            f.write(b"\xAA" * (1 << 20))
        last = write_header(img, 16384, b"slate")
        with open(img, "rb") as f:
            data = f.read()
        version, last_page, bad = struct.unpack_from("<III", data, VERSION_AT)
        if (version, last_page, bad) != (1, 63, 0) or last != 63:
            failures.append("16 KiB fields: %r" % ((version, last_page, bad),))
        if data[16384 - 10:16384] != MAGIC:
            failures.append("16 KiB magic missing")
        if data[LABEL_AT:LABEL_AT + 5] != b"slate":
            failures.append("label missing")
        if data[16384:16384 + 4] != b"\xAA" * 4:
            failures.append("bytes past the header page were changed")
        first_uuid = data[UUID_AT:UUID_AT + 16]
        write_header(img, 16384, b"slate")
        with open(img, "rb") as f:
            again = f.read()
        if again != data:
            failures.append("a second run changed the image")
        if first_uuid == bytes(16):
            failures.append("no UUID written")
        small = os.path.join(d, "small.img")
        with open(small, "wb") as f:
            f.write(bytes(16384))
        try:
            write_header(small, 16384, b"")
            failures.append("an image with no room after its header was taken")
        except ValueError:
            pass
        linux = os.path.join(d, "linux.img")
        with open(linux, "wb") as f:
            f.write(bytes(1 << 20))
        write_header(linux, 4096, b"")
        with open(linux, "rb") as f:
            data = f.read()
        if data[4086:4096] != MAGIC or struct.unpack_from("<I", data, VERSION_AT + 4)[0] != 255:
            failures.append("4 KiB header wrong")
    for f in failures:
        print("mkswap self-test FAIL:", f)
    print("mkswap self-test: %s" % ("FAILED" if failures else "ok (5 checks)"))
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("image", nargs="?")
    ap.add_argument("--page-size", type=int, default=16384, choices=PAGE_SIZES)
    ap.add_argument("--label", default="", help="up to 16 bytes")
    ap.add_argument("--self-test", "--selftest", dest="selftest", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return self_test()
    if not args.image:
        ap.print_usage(sys.stderr)
        return 2
    try:
        last = write_header(args.image, args.page_size, args.label.encode("utf-8"))
    except (OSError, ValueError) as e:
        print("mkswap: %s: %s" % (args.image, e), file=sys.stderr)
        return 2
    print("mkswap: %s: swap header, %d-byte pages, last page %d" % (args.image, args.page_size, last))
    return 0


if __name__ == "__main__":
    sys.exit(main())
