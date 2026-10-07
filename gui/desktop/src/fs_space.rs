//! How full a file system is: the system monitor widget's disk meter.
//!
//! The kernel answers it -- `SYS_FS_STATVFS` (608,
//! `kernel/src/syscall/number.rs`): the block size, how many blocks the file
//! system has and how many are free, as one 64-byte record. The desktop asks
//! it for the file system the system runs from, `/`, at the meter's second
//! (`DesktopShell::sample_system`). Until this the meter said "not measured":
//! `/sys/devices/block` gives a disk's capacity and nothing how much of it is
//! in use, and the call that does had no caller outside the C library.
//!
//! Made as the desktop makes its other call of the kernel's own
//! (`crate::backlight`): directly, on SlateOS; anywhere else there is no such
//! call, and the meter goes on saying it is not measured. The call wants the
//! right to read a file's metadata; refused it, the meter says the same.

/// What `SYS_FS_STATVFS` says of a file system: its block size, and how many
/// blocks it has and has free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    /// The size of the blocks the counts are in, in bytes.
    pub block_size: u64,
    /// How many blocks the file system has.
    pub total_blocks: u64,
    /// How many are free -- available to be written.
    pub free_blocks: u64,
}

/// The size of the record `SYS_FS_STATVFS` writes.
pub const RECORD_SIZE: usize = 64;

impl Space {
    /// Read the kernel's record: three little-endian `u64`s at its head --
    /// the block size, the total and the free -- and the rest, inodes and a
    /// name's length, not wanted here.
    #[must_use]
    pub fn from_record(record: &[u8; RECORD_SIZE]) -> Self {
        let word = |at: usize| {
            record
                .get(at..at.saturating_add(8))
                .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
                .map_or(0, u64::from_le_bytes)
        };
        Self {
            block_size: word(0),
            total_blocks: word(8),
            free_blocks: word(16),
        }
    }

    /// How much of it is in use, 0.0 to 1.0: `None` for a file system with no
    /// blocks -- a share of nothing is no share. More free than there are,
    /// which no file system means, is read as empty.
    #[must_use]
    pub fn used_fraction(&self) -> Option<f32> {
        if self.total_blocks == 0 {
            return None;
        }
        let used = self.total_blocks.saturating_sub(self.free_blocks);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a ratio of block counts; f32's precision is far finer than a meter's"
        )]
        let fraction = used as f32 / self.total_blocks as f32;
        Some(fraction.clamp(0.0, 1.0))
    }
}

/// How full the file system `path` is on is, asked of the kernel.
///
/// # Errors
///
/// The kernel's errno: `EPERM` without the right to read metadata, `ENOENT`
/// for a path that is not there -- and `ENOSYS` anywhere but SlateOS, where
/// there is no such call.
pub fn of(path: &[u8]) -> Result<Space, i32> {
    sys::statvfs(path).map(|record| Space::from_record(&record))
}

/// `ENOSYS`: no such call here.
#[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
const ENOSYS: i32 = 38;

/// Anywhere but SlateOS: no such call.
#[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
mod sys {
    pub fn statvfs(_path: &[u8]) -> Result<[u8; super::RECORD_SIZE], i32> {
        Err(super::ENOSYS)
    }
}

/// The system call, on SlateOS.
#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
mod sys {
    use std::arch::asm;

    /// `SYS_FS_STATVFS` (`kernel/src/syscall/number.rs`), as the `rax` it
    /// goes in and comes back out of.
    const SYS_FS_STATVFS: i64 = 608;

    /// `ENOSYS`, for an errno the kernel's answer does not hold.
    const ENOSYS: i32 = 38;

    /// `SYS_FS_STATVFS(path, len, record)`.
    pub fn statvfs(path: &[u8]) -> Result<[u8; super::RECORD_SIZE], i32> {
        let mut record = [0u8; super::RECORD_SIZE];
        let ret: i64;
        // SAFETY: the `syscall` instruction clobbers `rcx` and `r11`, declared
        // below, and returns in `rax`; the arguments go in `rdi`, `rsi` and
        // `rdx`, the convention this system's calls share with Linux's (as
        // `crate::backlight` makes its own). The kernel reads `path.len()`
        // bytes from `path`'s start -- a live slice, borrowed for the call --
        // and writes at most `RECORD_SIZE` bytes to `record`, a live array of
        // exactly that size that nothing else holds; it validates both ranges
        // itself before touching them.
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") SYS_FS_STATVFS => ret,
                in("rdi") path.as_ptr(),
                in("rsi") path.len(),
                in("rdx") record.as_mut_ptr(),
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        if (-4095..0).contains(&ret) {
            Err(ret
                .checked_neg()
                .and_then(|e| i32::try_from(e).ok())
                .unwrap_or(ENOSYS))
        } else {
            Ok(record)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::float_cmp)]

    use super::*;

    /// A record as the kernel lays it out: the three counts, then inodes, a
    /// name's length and the read-only flag, which are not read.
    fn record(block_size: u64, total: u64, free: u64) -> [u8; RECORD_SIZE] {
        let mut rec = [0u8; RECORD_SIZE];
        rec[0..8].copy_from_slice(&block_size.to_le_bytes());
        rec[8..16].copy_from_slice(&total.to_le_bytes());
        rec[16..24].copy_from_slice(&free.to_le_bytes());
        rec[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        rec[40..48].copy_from_slice(&255_u64.to_le_bytes());
        rec[48] = 1;
        rec
    }

    /// **The record is read as the kernel writes it**: the block size, the
    /// total and the free, little-endian, from its head.
    #[test]
    fn the_record_is_read_as_the_kernel_writes_it() {
        assert_eq!(
            Space::from_record(&record(4096, 1_000_000, 250_000)),
            Space {
                block_size: 4096,
                total_blocks: 1_000_000,
                free_blocks: 250_000,
            }
        );
    }

    /// **How full it is, is the share of its blocks not free** -- none of an
    /// empty one, a whole one of a full one, nothing at all of one with no
    /// blocks, and an empty one of one that says it has more free than it
    /// has.
    #[test]
    fn how_full_it_is_is_the_share_of_its_blocks_not_free() {
        let space = |total, free| Space {
            block_size: 4096,
            total_blocks: total,
            free_blocks: free,
        };
        assert_eq!(space(1000, 250).used_fraction(), Some(0.75));
        assert_eq!(space(1000, 1000).used_fraction(), Some(0.0));
        assert_eq!(space(1000, 0).used_fraction(), Some(1.0));
        assert_eq!(space(0, 0).used_fraction(), None);
        assert_eq!(space(1000, 4000).used_fraction(), Some(0.0));
        let huge = space(u64::MAX, u64::MAX / 4).used_fraction().unwrap();
        assert!((huge - 0.75).abs() < 0.001, "{huge}");
    }

    /// **Off SlateOS there is no such call**, and the answer says so rather
    /// than a made-up share.
    #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
    #[test]
    fn off_slateos_there_is_no_such_call() {
        assert_eq!(of(b"/"), Err(ENOSYS));
    }
}
