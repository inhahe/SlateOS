#!/usr/bin/env python3
"""Create a fresh FAT16 test disk image for the OS kernel.

Generates a 4 MiB raw disk image with a FAT16 filesystem containing
test files used by the kernel's FAT self-test.
"""

import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512
SECTORS_PER_CLUSTER = 4
CLUSTER_SIZE = SECTOR_SIZE * SECTORS_PER_CLUSTER  # 2048 bytes
TOTAL_SIZE = 4 * 1024 * 1024  # 4 MiB
TOTAL_SECTORS = TOTAL_SIZE // SECTOR_SIZE  # 8192

# FAT16 layout parameters
RESERVED_SECTORS = 1
NUM_FATS = 2
ROOT_ENTRY_COUNT = 512  # 16 KiB of root directory entries
ROOT_DIR_SECTORS = (ROOT_ENTRY_COUNT * 32 + SECTOR_SIZE - 1) // SECTOR_SIZE  # 32

# Calculate sectors per FAT
DATA_SECTORS_MAX = TOTAL_SECTORS - RESERVED_SECTORS - ROOT_DIR_SECTORS
TOTAL_CLUSTERS = DATA_SECTORS_MAX // SECTORS_PER_CLUSTER
# FAT16 entries are 2 bytes each, so sectors per FAT:
SECTORS_PER_FAT = (TOTAL_CLUSTERS * 2 + SECTOR_SIZE - 1) // SECTOR_SIZE  # round up

# Verify layout
FAT_START = RESERVED_SECTORS
ROOT_DIR_START = FAT_START + NUM_FATS * SECTORS_PER_FAT
DATA_START = ROOT_DIR_START + ROOT_DIR_SECTORS

# Actual data clusters (accounting for both FATs and root dir)
ACTUAL_DATA_SECTORS = TOTAL_SECTORS - DATA_START
ACTUAL_CLUSTERS = ACTUAL_DATA_SECTORS // SECTORS_PER_CLUSTER


def main():
    project_root = Path(__file__).resolve().parent.parent
    out_path = project_root / "disk.img"

    if "--help" in sys.argv or "-h" in sys.argv:
        print(f"Usage: {sys.argv[0]} [output_path]")
        print(f"  Default output: {out_path}")
        sys.exit(0)

    if len(sys.argv) > 1 and not sys.argv[1].startswith("-"):
        out_path = Path(sys.argv[1])

    # Allocate the full image
    img = bytearray(TOTAL_SIZE)

    # --- Boot sector / BPB ---
    bpb = bytearray(SECTOR_SIZE)
    bpb[0:3] = b"\xEB\x3C\x90"  # JMP short + NOP
    bpb[3:11] = b"MYKERNEL"     # OEM name (8 bytes)
    struct.pack_into("<H", bpb, 11, SECTOR_SIZE)          # bytes per sector
    bpb[13] = SECTORS_PER_CLUSTER                          # sectors per cluster
    struct.pack_into("<H", bpb, 14, RESERVED_SECTORS)      # reserved sectors
    bpb[16] = NUM_FATS                                     # number of FATs
    struct.pack_into("<H", bpb, 17, ROOT_ENTRY_COUNT)      # root entry count
    struct.pack_into("<H", bpb, 19, TOTAL_SECTORS if TOTAL_SECTORS < 65536 else 0)
    bpb[21] = 0xF8                                         # media type (hard disk)
    struct.pack_into("<H", bpb, 22, SECTORS_PER_FAT)       # sectors per FAT (16-bit)
    struct.pack_into("<H", bpb, 24, 63)                    # sectors per track
    struct.pack_into("<H", bpb, 26, 255)                   # number of heads
    struct.pack_into("<I", bpb, 28, 0)                     # hidden sectors
    struct.pack_into("<I", bpb, 32, 0)                     # total sectors 32-bit

    # Extended BPB (FAT16)
    bpb[36] = 0x80                                         # drive number
    bpb[37] = 0                                            # reserved
    bpb[38] = 0x29                                         # extended boot signature
    struct.pack_into("<I", bpb, 39, 0x12345678)            # volume serial number
    bpb[43:54] = b"TEST DISK  "                            # volume label (11 bytes)
    bpb[54:62] = b"FAT16   "                               # filesystem type (8 bytes)

    # Boot signature
    bpb[510] = 0x55
    bpb[511] = 0xAA

    img[0:SECTOR_SIZE] = bpb

    # --- FAT tables ---
    # First two entries are reserved: media type + 0xFFFF
    fat = bytearray(SECTORS_PER_FAT * SECTOR_SIZE)
    struct.pack_into("<H", fat, 0, 0xFFF8)   # FAT[0]: media type
    struct.pack_into("<H", fat, 2, 0xFFFF)   # FAT[1]: end-of-chain marker

    # Files will use clusters starting at 2.
    # Cluster 2: HELLO.TXT (1 cluster)
    struct.pack_into("<H", fat, 4, 0xFFFF)   # FAT[2]: end-of-chain

    # Cluster 3: README.TXT (1 cluster)
    struct.pack_into("<H", fat, 6, 0xFFFF)   # FAT[3]: end-of-chain

    # Write both FATs
    fat_bytes = bytes(fat)
    for i in range(NUM_FATS):
        start = (FAT_START + i * SECTORS_PER_FAT) * SECTOR_SIZE
        img[start:start + len(fat_bytes)] = fat_bytes

    # --- Root directory ---
    root = bytearray(ROOT_DIR_SECTORS * SECTOR_SIZE)

    def make_dir_entry(name_8_3: bytes, attr: int, cluster: int, size: int) -> bytes:
        """Create a 32-byte FAT directory entry."""
        entry = bytearray(32)
        entry[0:11] = name_8_3[:11]
        entry[11] = attr
        # Creation time/date (zeros — we don't track timestamps yet)
        struct.pack_into("<H", entry, 26, cluster & 0xFFFF)   # first cluster low
        struct.pack_into("<H", entry, 20, (cluster >> 16) & 0xFFFF)  # first cluster high
        struct.pack_into("<I", entry, 28, size)
        return bytes(entry)

    # Volume label entry
    root[0:32] = make_dir_entry(b"TEST DISK  ", 0x08, 0, 0)

    # HELLO.TXT
    hello_data = b"Hello from the FAT16 filesystem!\n"
    root[32:64] = make_dir_entry(b"HELLO   TXT", 0x20, 2, len(hello_data))

    # README.TXT
    readme_data = (
        b"This is a test FAT16 disk image for the OS kernel.\n"
        b"It contains sample files used by the FAT driver self-test.\n"
    )
    root[64:96] = make_dir_entry(b"README  TXT", 0x20, 3, len(readme_data))

    # Write root directory
    root_start = ROOT_DIR_START * SECTOR_SIZE
    img[root_start:root_start + len(root)] = root

    # --- File data ---
    def write_file(cluster: int, data: bytes):
        lba = DATA_START + (cluster - 2) * SECTORS_PER_CLUSTER
        offset = lba * SECTOR_SIZE
        img[offset:offset + len(data)] = data

    write_file(2, hello_data)
    write_file(3, readme_data)

    # --- Write image ---
    out_path.write_bytes(bytes(img))

    print(f"Created {out_path} ({TOTAL_SIZE} bytes)")
    print(f"  FAT16, {ACTUAL_CLUSTERS} data clusters, {SECTORS_PER_CLUSTER} sectors/cluster")
    print(f"  Layout: reserved={RESERVED_SECTORS}, FAT={SECTORS_PER_FAT}x{NUM_FATS}, "
          f"rootdir={ROOT_DIR_SECTORS}, data starts at LBA {DATA_START}")
    print(f"  Files: HELLO.TXT ({len(hello_data)} bytes), README.TXT ({len(readme_data)} bytes)")


if __name__ == "__main__":
    main()
