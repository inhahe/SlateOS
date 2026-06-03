//! `<unistd.h>` `lseek(2)` / `llseek(2)` whence values.
//!
//! `SEEK_SET`/`SEEK_CUR`/`SEEK_END` are POSIX; `SEEK_DATA`/`SEEK_HOLE`
//! (Linux 3.1+) are how `cp --sparse=always`, `tar --sparse`, and
//! `qemu-img convert` walk holes in sparse files efficiently.

// ---------------------------------------------------------------------------
// Whence values
// ---------------------------------------------------------------------------

pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;
pub const SEEK_DATA: u32 = 3;
pub const SEEK_HOLE: u32 = 4;

pub const SEEK_MAX: u32 = SEEK_HOLE;

// ---------------------------------------------------------------------------
// `copy_file_range(2)` flag (currently must be zero)
// ---------------------------------------------------------------------------

pub const COPY_FILE_RANGE_FLAGS_RESERVED: u32 = 0;

// ---------------------------------------------------------------------------
// `splice(2)` / `tee(2)` / `vmsplice(2)` flags
// ---------------------------------------------------------------------------

pub const SPLICE_F_MOVE: u32 = 1 << 0;
pub const SPLICE_F_NONBLOCK: u32 = 1 << 1;
pub const SPLICE_F_MORE: u32 = 1 << 2;
pub const SPLICE_F_GIFT: u32 = 1 << 3;

pub const SPLICE_F_ALL: u32 = SPLICE_F_MOVE | SPLICE_F_NONBLOCK | SPLICE_F_MORE | SPLICE_F_GIFT;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_LSEEK: u32 = 8;
pub const NR__LLSEEK: u32 = 140; // 32-bit ABIs only; on x86_64 plain lseek suffices.
pub const NR_COPY_FILE_RANGE: u32 = 326;
pub const NR_SPLICE: u32 = 275;
pub const NR_TEE: u32 = 276;
pub const NR_VMSPLICE: u32 = 278;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whence_dense_0_to_4() {
        let w = [SEEK_SET, SEEK_CUR, SEEK_END, SEEK_DATA, SEEK_HOLE];
        for (i, &v) in w.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(SEEK_MAX, 4);
    }

    #[test]
    fn test_posix_three_anchor() {
        // POSIX-defined values are the dense 0..=2 block.
        assert_eq!(SEEK_SET, 0);
        assert_eq!(SEEK_CUR, 1);
        assert_eq!(SEEK_END, 2);
    }

    #[test]
    fn test_data_and_hole_disjoint_from_posix() {
        // DATA/HOLE were added later but stayed in the same dense range.
        assert!(SEEK_DATA > SEEK_END);
        assert!(SEEK_HOLE > SEEK_DATA);
    }

    #[test]
    fn test_splice_flags_low_4_bits_dense() {
        let f = [SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE, SPLICE_F_GIFT];
        let mut or = 0u32;
        for (i, v) in f.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0xF);
        assert_eq!(SPLICE_F_ALL, 0xF);
    }

    #[test]
    fn test_syscall_numbers() {
        // lseek is famously syscall #8.
        assert_eq!(NR_LSEEK, 8);
        // splice and tee live adjacent.
        assert_eq!(NR_TEE, NR_SPLICE + 1);
        // vmsplice is two more.
        assert_eq!(NR_VMSPLICE, NR_SPLICE + 3);
    }
}
