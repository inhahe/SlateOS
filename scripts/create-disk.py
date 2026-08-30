#!/usr/bin/env python3
"""Create a fresh FAT test disk image for the OS kernel.

Generates a raw disk image with a FAT16 or FAT32 filesystem containing
test files used by the kernel's FAT driver self-test.

Usage:
    python create-disk.py                 # 4 MiB FAT16 (default)
    python create-disk.py --fat32         # 64 MiB FAT32
    python create-disk.py --fat32 output  # FAT32 to specific path
"""

import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512

# Test file contents (shared by both FAT16 and FAT32)
HELLO_DATA = b"Hello from the FAT16 filesystem!\n"
README_DATA = (
    b"This is a test FAT disk image for the OS kernel.\n"
    b"It contains sample files used by the FAT driver self-test.\n"
)


def make_dir_entry(name_8_3: bytes, attr: int, cluster: int, size: int) -> bytes:
    """Create a 32-byte FAT directory entry."""
    entry = bytearray(32)
    entry[0:11] = name_8_3[:11]
    entry[11] = attr
    struct.pack_into("<H", entry, 26, cluster & 0xFFFF)            # first cluster low
    struct.pack_into("<H", entry, 20, (cluster >> 16) & 0xFFFF)    # first cluster high
    struct.pack_into("<I", entry, 28, size)
    return bytes(entry)


# ---------------------------------------------------------------------------
# FAT16 image creation
# ---------------------------------------------------------------------------

def create_fat16(out_path: Path):
    """Create a 4 MiB FAT16 disk image."""
    total_size = 4 * 1024 * 1024  # 4 MiB
    total_sectors = total_size // SECTOR_SIZE  # 8192
    sectors_per_cluster = 4
    reserved_sectors = 1
    num_fats = 2
    root_entry_count = 512
    root_dir_sectors = (root_entry_count * 32 + SECTOR_SIZE - 1) // SECTOR_SIZE

    # Sectors per FAT (16-bit entries)
    data_max = total_sectors - reserved_sectors - root_dir_sectors
    total_clusters = data_max // sectors_per_cluster
    sectors_per_fat = (total_clusters * 2 + SECTOR_SIZE - 1) // SECTOR_SIZE

    fat_start = reserved_sectors
    root_dir_start = fat_start + num_fats * sectors_per_fat
    data_start = root_dir_start + root_dir_sectors
    actual_clusters = (total_sectors - data_start) // sectors_per_cluster

    img = bytearray(total_size)

    # Boot sector / BPB
    bpb = bytearray(SECTOR_SIZE)
    bpb[0:3] = b"\xEB\x3C\x90"
    bpb[3:11] = b"MYKERNEL"
    struct.pack_into("<H", bpb, 11, SECTOR_SIZE)
    bpb[13] = sectors_per_cluster
    struct.pack_into("<H", bpb, 14, reserved_sectors)
    bpb[16] = num_fats
    struct.pack_into("<H", bpb, 17, root_entry_count)
    struct.pack_into("<H", bpb, 19, total_sectors if total_sectors < 65536 else 0)
    bpb[21] = 0xF8
    struct.pack_into("<H", bpb, 22, sectors_per_fat)
    struct.pack_into("<H", bpb, 24, 63)
    struct.pack_into("<H", bpb, 26, 255)
    # Extended BPB (FAT16, offset 36-61)
    bpb[36] = 0x80                                    # drive number
    bpb[38] = 0x29                                    # ext boot sig
    struct.pack_into("<I", bpb, 39, 0x12345678)       # volume serial
    bpb[43:54] = b"TEST DISK  "                       # volume label
    bpb[54:62] = b"FAT16   "                          # FS type
    bpb[510] = 0x55; bpb[511] = 0xAA
    img[0:SECTOR_SIZE] = bpb

    # FAT tables
    fat = bytearray(sectors_per_fat * SECTOR_SIZE)
    struct.pack_into("<H", fat, 0, 0xFFF8)    # media type
    struct.pack_into("<H", fat, 2, 0xFFFF)    # reserved
    struct.pack_into("<H", fat, 4, 0xFFFF)    # cluster 2: HELLO.TXT
    struct.pack_into("<H", fat, 6, 0xFFFF)    # cluster 3: README.TXT
    for i in range(num_fats):
        start = (fat_start + i * sectors_per_fat) * SECTOR_SIZE
        img[start:start + len(fat)] = fat

    # Root directory
    root = bytearray(root_dir_sectors * SECTOR_SIZE)
    root[0:32]  = make_dir_entry(b"TEST DISK  ", 0x08, 0, 0)       # volume label
    root[32:64] = make_dir_entry(b"HELLO   TXT", 0x20, 2, len(HELLO_DATA))
    root[64:96] = make_dir_entry(b"README  TXT", 0x20, 3, len(README_DATA))
    off = root_dir_start * SECTOR_SIZE
    img[off:off + len(root)] = root

    # File data
    def write_file(cluster, data):
        lba = data_start + (cluster - 2) * sectors_per_cluster
        o = lba * SECTOR_SIZE
        img[o:o + len(data)] = data

    write_file(2, HELLO_DATA)
    write_file(3, README_DATA)

    out_path.write_bytes(bytes(img))
    print(f"Created {out_path} ({total_size} bytes)")
    print(f"  FAT16, {actual_clusters} data clusters, {sectors_per_cluster} sectors/cluster")
    print(f"  Layout: reserved={reserved_sectors}, FAT={sectors_per_fat}x{num_fats}, "
          f"rootdir={root_dir_sectors}, data starts at LBA {data_start}")
    print(f"  Files: HELLO.TXT ({len(HELLO_DATA)} bytes), README.TXT ({len(README_DATA)} bytes)")


# ---------------------------------------------------------------------------
# FAT32 image creation
# ---------------------------------------------------------------------------

def create_fat32(out_path: Path):
    """Create a 64 MiB FAT32 disk image."""
    total_size = 64 * 1024 * 1024  # 64 MiB
    total_sectors = total_size // SECTOR_SIZE  # 131072
    sectors_per_cluster = 1   # 1 sector = 1 cluster for maximum cluster count
    reserved_sectors = 32     # standard for FAT32
    num_fats = 2
    root_entry_count = 0      # FAT32: root is a cluster chain
    root_cluster = 2          # standard: root dir starts at cluster 2

    # Estimate sectors per FAT (4 bytes per entry for FAT32)
    # data_sectors ≈ total - reserved - 2*fat_sectors
    # fat_sectors = ceil(data_clusters * 4 / 512)
    # Iterate to converge:
    est_data = total_sectors - reserved_sectors
    for _ in range(10):
        est_clusters = est_data // sectors_per_cluster
        sectors_per_fat = (est_clusters * 4 + SECTOR_SIZE - 1) // SECTOR_SIZE
        est_data = total_sectors - reserved_sectors - num_fats * sectors_per_fat

    fat_start = reserved_sectors
    data_start = fat_start + num_fats * sectors_per_fat
    actual_clusters = (total_sectors - data_start) // sectors_per_cluster

    img = bytearray(total_size)

    # Boot sector / BPB
    bpb = bytearray(SECTOR_SIZE)
    bpb[0:3] = b"\xEB\x58\x90"      # JMP short to code (0x5A), NOP
    bpb[3:11] = b"MYKERNEL"
    struct.pack_into("<H", bpb, 11, SECTOR_SIZE)
    bpb[13] = sectors_per_cluster
    struct.pack_into("<H", bpb, 14, reserved_sectors)
    bpb[16] = num_fats
    struct.pack_into("<H", bpb, 17, 0)                    # root entry count = 0 (FAT32)
    struct.pack_into("<H", bpb, 19, 0)                    # total_sectors_16 = 0
    bpb[21] = 0xF8                                         # media type
    struct.pack_into("<H", bpb, 22, 0)                    # sectors_per_fat_16 = 0 (FAT32!)
    struct.pack_into("<H", bpb, 24, 63)
    struct.pack_into("<H", bpb, 26, 255)
    struct.pack_into("<I", bpb, 28, 0)                    # hidden sectors
    struct.pack_into("<I", bpb, 32, total_sectors)        # total_sectors_32

    # FAT32 extended BPB (offset 36-89)
    struct.pack_into("<I", bpb, 36, sectors_per_fat)      # sectors per FAT (32-bit)
    struct.pack_into("<H", bpb, 40, 0)                    # ext flags
    struct.pack_into("<H", bpb, 42, 0)                    # FS version
    struct.pack_into("<I", bpb, 44, root_cluster)         # root cluster
    struct.pack_into("<H", bpb, 48, 1)                    # FSInfo sector
    struct.pack_into("<H", bpb, 50, 6)                    # backup boot sector
    # Bytes 52-63: reserved (zeros)
    bpb[64] = 0x80                                         # drive number
    bpb[65] = 0                                            # reserved
    bpb[66] = 0x29                                         # ext boot sig
    struct.pack_into("<I", bpb, 67, 0x87654321)           # volume serial
    bpb[71:82] = b"FAT32 DISK "                           # volume label (11 bytes)
    bpb[82:90] = b"FAT32   "                              # FS type (8 bytes)
    bpb[510] = 0x55; bpb[511] = 0xAA
    img[0:SECTOR_SIZE] = bpb

    # FSInfo sector (sector 1)
    fsinfo = bytearray(SECTOR_SIZE)
    struct.pack_into("<I", fsinfo, 0, 0x41615252)         # lead signature
    struct.pack_into("<I", fsinfo, 484, 0x61417272)       # struct signature
    struct.pack_into("<I", fsinfo, 488, actual_clusters - 3)  # free cluster count (minus root + 2 files)
    struct.pack_into("<I", fsinfo, 492, 5)                # next free cluster hint
    struct.pack_into("<I", fsinfo, 508, 0xAA550000)       # trail signature
    img[SECTOR_SIZE:2*SECTOR_SIZE] = fsinfo

    # FAT table
    fat = bytearray(sectors_per_fat * SECTOR_SIZE)
    struct.pack_into("<I", fat, 0,  0x0FFFFFF8)   # FAT[0]: media type
    struct.pack_into("<I", fat, 4,  0x0FFFFFFF)   # FAT[1]: reserved
    # Cluster 2: root directory (1 cluster, EOC)
    struct.pack_into("<I", fat, 8,  0x0FFFFFFF)   # FAT[2]: root dir EOC
    # Cluster 3: HELLO.TXT
    struct.pack_into("<I", fat, 12, 0x0FFFFFFF)   # FAT[3]: HELLO.TXT EOC
    # Cluster 4: README.TXT
    struct.pack_into("<I", fat, 16, 0x0FFFFFFF)   # FAT[4]: README.TXT EOC

    for i in range(num_fats):
        start = (fat_start + i * sectors_per_fat) * SECTOR_SIZE
        img[start:start + len(fat)] = fat

    # Root directory (cluster 2 — it's a regular cluster chain on FAT32)
    root_lba = data_start + (root_cluster - 2) * sectors_per_cluster
    root_off = root_lba * SECTOR_SIZE
    root_data = bytearray(sectors_per_cluster * SECTOR_SIZE)
    root_data[0:32]  = make_dir_entry(b"FAT32 DISK ", 0x08, 0, 0)  # volume label
    root_data[32:64] = make_dir_entry(b"HELLO   TXT", 0x20, 3, len(HELLO_DATA))
    root_data[64:96] = make_dir_entry(b"README  TXT", 0x20, 4, len(README_DATA))
    img[root_off:root_off + len(root_data)] = root_data

    # File data
    def write_file(cluster, data):
        lba = data_start + (cluster - 2) * sectors_per_cluster
        o = lba * SECTOR_SIZE
        img[o:o + len(data)] = data

    write_file(3, HELLO_DATA)
    write_file(4, README_DATA)

    out_path.write_bytes(bytes(img))
    print(f"Created {out_path} ({total_size} bytes)")
    print(f"  FAT32, {actual_clusters} data clusters, {sectors_per_cluster} sectors/cluster")
    print(f"  Layout: reserved={reserved_sectors}, FAT={sectors_per_fat}x{num_fats}, "
          f"data starts at LBA {data_start}, root cluster={root_cluster}")
    print(f"  Files: HELLO.TXT ({len(HELLO_DATA)} bytes), README.TXT ({len(README_DATA)} bytes)")


def main():
    project_root = Path(__file__).resolve().parent.parent
    out_path = project_root / "disk.img"

    fat32 = "--fat32" in sys.argv

    if "--help" in sys.argv or "-h" in sys.argv:
        print(f"Usage: {sys.argv[0]} [--fat32] [output_path]")
        print(f"  Default: 4 MiB FAT16 at {out_path}")
        print(f"  --fat32: 64 MiB FAT32 image")
        sys.exit(0)

    for arg in sys.argv[1:]:
        if not arg.startswith("-"):
            out_path = Path(arg)
            break

    if fat32:
        create_fat32(out_path)
    else:
        create_fat16(out_path)


if __name__ == "__main__":
    main()
