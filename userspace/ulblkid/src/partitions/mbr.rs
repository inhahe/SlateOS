//! `pt-mbr.h`: the master boot record's layout -- the disk ID at 440, four
//! 16-byte entries from 0x1be, the 55AA signature at 510. The DOS prober
//! reads it, and so do GPT's protective-MBR check and Minix's subpartition
//! table, which is an MBR inside a partition.

use crate::Bytes;

/// `MBR_PT_OFFSET`: where the four entries start.
pub(crate) const MBR_PT_OFFSET: u64 = 0x1be;
/// `MBR_GPT_PARTITION`: a protective MBR's entry, covering a GPT disk.
pub(crate) const MBR_GPT_PARTITION: u8 = 0xee;
/// `MBR_MINIX_PARTITION`: Minix 1.4b and later.
pub(crate) const MBR_MINIX_PARTITION: u8 = 0x81;

/// `sizeof(struct dos_partition)`.
const ENTRY_SIZE: usize = 16;

/// `struct dos_partition`: one entry, the fields libblkid reads (the CHS
/// addresses it never does).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DosPartition {
    /// `boot_ind`: 0x80 for the active partition, else 0.
    pub(crate) boot_ind: u8,
    /// `sys_ind`: the type byte.
    pub(crate) sys_ind: u8,
    /// `dos_partition_get_start`: the first sector.
    pub(crate) start: u32,
    /// `dos_partition_get_size`: the number of sectors.
    pub(crate) size: u32,
}

/// `mbr_get_partition(mbr, i)`, read out: entry `i` of the sector `mbr`.
#[must_use]
pub(crate) fn get_partition(mbr: &[u8], i: usize) -> DosPartition {
    // MBR_PT_OFFSET is 0x1be, which fits any usize.
    let off = i
        .saturating_mul(ENTRY_SIZE)
        .saturating_add(usize::try_from(MBR_PT_OFFSET).unwrap_or(0x1be));
    DosPartition {
        boot_ind: mbr.u8_at(off),
        sys_ind: mbr.u8_at(off.saturating_add(4)),
        start: mbr.le32(off.saturating_add(8)),
        size: mbr.le32(off.saturating_add(12)),
    }
}

/// All four entries.
#[must_use]
pub(crate) fn partitions(mbr: &[u8]) -> [DosPartition; 4] {
    [
        get_partition(mbr, 0),
        get_partition(mbr, 1),
        get_partition(mbr, 2),
        get_partition(mbr, 3),
    ]
}

/// `mbr_is_valid_magic`: 55AA at 510.
#[must_use]
pub(crate) fn is_valid_magic(mbr: &[u8]) -> bool {
    mbr.u8_at(510) == 0x55 && mbr.u8_at(511) == 0xAA
}

/// `mbr_get_id`: the disk signature at 440.
#[must_use]
pub(crate) fn get_id(mbr: &[u8]) -> u32 {
    mbr.le32(440)
}
