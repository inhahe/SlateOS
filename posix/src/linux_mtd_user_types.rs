//! `<mtd/mtd-user.h>` — Memory Technology Devices (raw NAND/NOR flash) ABI.
//!
//! MTD is the kernel's flash-chip layer used by SBCs, IoT devices,
//! and embedded routers. `mtd-utils` (`flash_erase`, `nandwrite`,
//! `ubiformat`, etc.) speak directly to `/dev/mtdN` via the ioctls
//! below. The ABI predates the block layer for flash; UBIFS and
//! JFFS2 sit on top of it.

// ---------------------------------------------------------------------------
// MTD type values (`mtd_info.type`)
// ---------------------------------------------------------------------------

pub const MTD_ABSENT: u32 = 0;
pub const MTD_RAM: u32 = 1;
pub const MTD_ROM: u32 = 2;
pub const MTD_NORFLASH: u32 = 3;
pub const MTD_NANDFLASH: u32 = 4;
pub const MTD_DATAFLASH: u32 = 6;
pub const MTD_UBIVOLUME: u32 = 7;
pub const MTD_MLCNANDFLASH: u32 = 8;

// ---------------------------------------------------------------------------
// Flags (`mtd_info.flags`)
// ---------------------------------------------------------------------------

pub const MTD_WRITEABLE: u32 = 0x400;
pub const MTD_BIT_WRITEABLE: u32 = 0x800;
pub const MTD_NO_ERASE: u32 = 0x1000;
pub const MTD_POWERUP_LOCK: u32 = 0x2000;
pub const MTD_SLC_ON_MLC_EMULATION: u32 = 0x4000;

pub const MTD_CAP_ROM: u32 = 0;
pub const MTD_CAP_RAM: u32 = MTD_WRITEABLE | MTD_BIT_WRITEABLE | MTD_NO_ERASE;
pub const MTD_CAP_NORFLASH: u32 = MTD_WRITEABLE | MTD_BIT_WRITEABLE;
pub const MTD_CAP_NANDFLASH: u32 = MTD_WRITEABLE;

// ---------------------------------------------------------------------------
// ioctls (magic 'M' = 0x4D, 'm' = 0x6D used historically)
// ---------------------------------------------------------------------------

pub const MEMGETINFO_NR: u32 = 1;
pub const MEMERASE_NR: u32 = 2;
pub const MEMWRITEOOB_NR: u32 = 3;
pub const MEMREADOOB_NR: u32 = 4;
pub const MEMLOCK_NR: u32 = 5;
pub const MEMUNLOCK_NR: u32 = 6;
pub const MEMGETREGIONCOUNT_NR: u32 = 7;
pub const MEMGETREGIONINFO_NR: u32 = 8;
pub const MEMGETOOBSEL_NR: u32 = 10;
pub const MEMGETBADBLOCK_NR: u32 = 11;
pub const MEMSETBADBLOCK_NR: u32 = 12;

// ---------------------------------------------------------------------------
// Erase states (`erase_info.state`)
// ---------------------------------------------------------------------------

pub const MTD_ERASE_PENDING: u32 = 0x01;
pub const MTD_ERASING: u32 = 0x02;
pub const MTD_ERASE_SUSPEND: u32 = 0x04;
pub const MTD_ERASE_DONE: u32 = 0x08;
pub const MTD_ERASE_FAILED: u32 = 0x10;

// ---------------------------------------------------------------------------
// OOB modes
// ---------------------------------------------------------------------------

pub const MTD_OPS_PLACE_OOB: u32 = 0;
pub const MTD_OPS_AUTO_OOB: u32 = 1;
pub const MTD_OPS_RAW: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct_and_in_range() {
        let t = [
            MTD_ABSENT,
            MTD_RAM,
            MTD_ROM,
            MTD_NORFLASH,
            MTD_NANDFLASH,
            MTD_DATAFLASH,
            MTD_UBIVOLUME,
            MTD_MLCNANDFLASH,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
        // All MTD types fit in a u8 in practice.
        for v in t {
            assert!(v <= 8);
        }
    }

    #[test]
    fn test_flag_bits_single_bit_and_distinct() {
        let f = [
            MTD_WRITEABLE,
            MTD_BIT_WRITEABLE,
            MTD_NO_ERASE,
            MTD_POWERUP_LOCK,
            MTD_SLC_ON_MLC_EMULATION,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // OR of all bits == 0x7C00 (five bits 10..14).
        assert_eq!(f.iter().fold(0, |a, b| a | b), 0x7C00);
    }

    #[test]
    fn test_capability_combinations() {
        assert_eq!(MTD_CAP_ROM, 0);
        assert_eq!(
            MTD_CAP_RAM,
            MTD_WRITEABLE | MTD_BIT_WRITEABLE | MTD_NO_ERASE
        );
        assert_eq!(MTD_CAP_NORFLASH, MTD_WRITEABLE | MTD_BIT_WRITEABLE);
        // NAND can't bit-write — only word/page write.
        assert_eq!(MTD_CAP_NANDFLASH, MTD_WRITEABLE);
    }

    #[test]
    fn test_ioctl_numbers_distinct() {
        let n = [
            MEMGETINFO_NR,
            MEMERASE_NR,
            MEMWRITEOOB_NR,
            MEMREADOOB_NR,
            MEMLOCK_NR,
            MEMUNLOCK_NR,
            MEMGETREGIONCOUNT_NR,
            MEMGETREGIONINFO_NR,
            MEMGETOOBSEL_NR,
            MEMGETBADBLOCK_NR,
            MEMSETBADBLOCK_NR,
        ];
        for i in 0..n.len() {
            for j in (i + 1)..n.len() {
                assert_ne!(n[i], n[j]);
            }
        }
    }

    #[test]
    fn test_erase_states_single_bit_and_dense() {
        let s = [
            MTD_ERASE_PENDING,
            MTD_ERASING,
            MTD_ERASE_SUSPEND,
            MTD_ERASE_DONE,
            MTD_ERASE_FAILED,
        ];
        for v in s {
            assert!(v.is_power_of_two());
        }
        // Five dense bits 0..4.
        assert_eq!(s.iter().fold(0, |a, b| a | b), 0x1F);
    }

    #[test]
    fn test_oob_modes_dense_0_to_2() {
        assert_eq!(MTD_OPS_PLACE_OOB, 0);
        assert_eq!(MTD_OPS_AUTO_OOB, 1);
        assert_eq!(MTD_OPS_RAW, 2);
    }
}
