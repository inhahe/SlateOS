//! How full a file system is: the system monitor widget's disk meter, and the
//! disk usage widget's disks.
//!
//! The kernel answers it -- `SYS_FS_STATVFS` (608,
//! `kernel/src/syscall/number.rs`): the block size, how many blocks the file
//! system has and how many are free, and whether it is mounted read-only, as
//! one 64-byte record. The desktop asks it for the file system the system
//! runs from, `/`, at the meter's second (`DesktopShell::sample_system`), and
//! for each disk mounted while a disk usage widget is out
//! (`widgets::disks::Sampler`). Until the meter asked, it said "not
//! measured": `/sys/devices/block` gives a disk's capacity and nothing how
//! much of it is in use, and the call that does had no caller outside the C
//! library.
//!
//! Made as the desktop makes its other call of the kernel's own
//! (`crate::backlight`): directly, on SlateOS; anywhere else there is no such
//! call, and the meter goes on saying it is not measured. The call wants the
//! right to read a file's metadata; refused it, the meter says the same.
//!
//! # Which file systems are disks
//!
//! What is mounted is the mount table's ([`MOUNT_TABLE`], [`parse_mounts`]),
//! and not all of it is a disk: there are the kernel's views of itself
//! (`procfs`, `sysfs`, `devfs`), memory (`memfs`), a container's `overlay`.
//! [`disks`] leaves out a file system of a type known to keep nothing on a
//! disk ([`NOT_DISKS`]), and then -- as `df` does -- any whose kernel says it
//! has no blocks at all, which on SlateOS each of those does too.
//!
//! The list of what is *not* a disk, rather than of what is, so that a disk of
//! a type nobody listed is shown, as it should be, rather than hidden until
//! someone adds its name; and a list at all, beside the blocks, so that a disk
//! that cannot be measured is still listed -- as not measured -- without the
//! kernel's views listed beside it, which cannot be told from it by their
//! blocks when nothing can be measured.

use std::path::Path;

/// What `SYS_FS_STATVFS` says of a file system: its block size, how many
/// blocks it has and has free, and whether it is mounted read-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    /// The size of the blocks the counts are in, in bytes.
    pub block_size: u64,
    /// How many blocks the file system has.
    pub total_blocks: u64,
    /// How many are free -- available to be written.
    pub free_blocks: u64,
    /// Whether it is mounted read-only: a disc, an image. As full as it was
    /// made, which is no warning ([`nearly_full`](Self::nearly_full)).
    pub read_only: bool,
}

/// The size of the record `SYS_FS_STATVFS` writes.
pub const RECORD_SIZE: usize = 64;

/// Where in the record the read-only flag is: a byte after the counts, the
/// inodes and the longest name's length.
const READ_ONLY_AT: usize = 48;

/// How much of a file system that can be written must be in use, in whole
/// percent, for it to be nearly full: its bar drawn red, and tools told.
pub const NEARLY_FULL_PERCENT: u32 = 90;

impl Space {
    /// Read the kernel's record: three little-endian `u64`s at its head --
    /// the block size, the total and the free -- and the read-only flag
    /// after the inodes and a name's length, which are not wanted here.
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
            read_only: record.get(READ_ONLY_AT).is_some_and(|&flag| flag != 0),
        }
    }

    /// How many of its blocks are in use: never fewer than none, whatever the
    /// free count says.
    fn used_blocks(&self) -> u64 {
        self.total_blocks.saturating_sub(self.free_blocks)
    }

    /// How much of it is in use, 0.0 to 1.0: `None` for a file system with no
    /// blocks -- a share of nothing is no share. More free than there are,
    /// which no file system means, is read as empty.
    #[must_use]
    pub fn used_fraction(&self) -> Option<f32> {
        if self.total_blocks == 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a ratio of block counts; f32's precision is far finer than a meter's"
        )]
        let fraction = self.used_blocks() as f32 / self.total_blocks as f32;
        Some(fraction.clamp(0.0, 1.0))
    }

    /// How much of it is in use in whole percent, truncated -- 100 only when
    /// it is full, 90 only when it is past nine tenths. `None` for a file
    /// system with no blocks.
    #[must_use]
    pub fn used_percent(&self) -> Option<u32> {
        guitk::ratio::percent_whole(self.used_blocks(), self.total_blocks)
    }

    /// How many bytes it holds: its blocks at their size -- no more than a
    /// `u64` counts.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.total_blocks.saturating_mul(self.block_size)
    }

    /// How many bytes of it are free: never more than it holds.
    #[must_use]
    pub fn free_bytes(&self) -> u64 {
        self.free_blocks
            .min(self.total_blocks)
            .saturating_mul(self.block_size)
    }

    /// Whether it is nearly full: [`NEARLY_FULL_PERCENT`] or more of it in
    /// use, and it can be written -- a read-only one cannot be filled further
    /// and is as full as it was made.
    #[must_use]
    pub fn nearly_full(&self) -> bool {
        !self.read_only
            && self
                .used_percent()
                .is_some_and(|used| used >= NEARLY_FULL_PERCENT)
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

/// Where the desktop reads what is mounted: its own view of the mount
/// table, which in a container is the container's.
pub const MOUNT_TABLE: &str = "/proc/self/mounts";

/// A line of the mount table: where a file system is mounted, and its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    /// Where: bytes, as every path is -- a name need not be text.
    pub point: Vec<u8>,
    /// Its type as the kernel names it: `ext4`, `fat32`, `procfs`.
    pub fs_type: Vec<u8>,
}

/// The file systems the mount table `table` lists, in its order: a line
/// each, `source point type options dump pass`, each field with the bytes a
/// field cannot hold escaped as a backslash and three octal digits, undone
/// here.
///
/// A point mounted over is listed once, in its first place, as the file
/// system mounted last -- the one a path there reaches. A line of fewer
/// than three fields is no mount, and is passed over.
#[must_use]
pub fn parse_mounts(table: &[u8]) -> Vec<Mount> {
    let mut mounts: Vec<Mount> = Vec::new();
    for line in table.split(|&b| b == b'\n') {
        let mut fields = line
            .split(|&b| b == b' ' || b == b'\t')
            .filter(|field| !field.is_empty());
        let (Some(_source), Some(point), Some(fs_type)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let mount = Mount {
            point: unmangle(point),
            fs_type: unmangle(fs_type),
        };
        match mounts.iter_mut().find(|m| m.point == mount.point) {
            Some(under) => *under = mount,
            None => mounts.push(mount),
        }
    }
    mounts
}

/// A mount table field as it was before the kernel escaped it: each
/// backslash and three octal digits is the byte they spell -- how a space, a
/// tab, a newline, a backslash and, on SlateOS, any byte that is not
/// printable ASCII are written (Linux's `mangle()`;
/// `kernel/src/fs/escape.rs`). A backslash not so followed is kept as it is,
/// as libmount keeps it.
///
/// Not `ulmount::mangle`, which reads the same escape: that crate is the
/// util-linux port, and libblkid and sysfs come with it.
fn unmangle(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&b, after)) = rest.split_first() {
        if b == b'\\'
            && let Some(byte) = after.get(..3).and_then(octal_byte)
        {
            out.push(byte);
            rest = after.get(3..).unwrap_or_default();
            continue;
        }
        out.push(b);
        rest = after;
    }
    out
}

/// Three octal digits as the byte they spell; `None` for a digit that is not
/// octal, or for a number past a byte's (`\400` and up).
fn octal_byte(digits: &[u8]) -> Option<u8> {
    let value = digits.iter().try_fold(0_u16, |value, &digit| {
        let digit = digit.checked_sub(b'0').filter(|&d| d < 8)?;
        value.checked_mul(8)?.checked_add(u16::from(digit))
    })?;
    u8::try_from(value).ok()
}

/// File system types that keep nothing on a disk: SlateOS's views of the
/// kernel, memory and a container's overlay (`kernel/src/fs`); then the same
/// and their kin under the names a Linux kernel mounts them by, for a
/// desktop run where it is built and tested; and the read-only images Linux
/// installs packages as, which nobody fills.
pub const NOT_DISKS: &[&[u8]] = &[
    // SlateOS.
    b"procfs",
    b"sysfs",
    b"devfs",
    b"memfs",
    b"overlay",
    // Linux: the kernel's views of itself,
    b"proc",
    b"devtmpfs",
    b"devpts",
    b"securityfs",
    b"cgroup",
    b"cgroup2",
    b"pstore",
    b"efivarfs",
    b"bpf",
    b"debugfs",
    b"tracefs",
    b"configfs",
    b"fusectl",
    b"mqueue",
    b"hugetlbfs",
    b"binfmt_misc",
    b"autofs",
    b"nsfs",
    b"rpc_pipefs",
    b"selinuxfs",
    // memory,
    b"tmpfs",
    b"ramfs",
    // and packages' images.
    b"squashfs",
];

/// A disk the system has mounted, and what its kernel says of its space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Where it is mounted.
    pub point: Vec<u8>,
    /// Its space -- or the errno the kernel answered the asking with.
    pub space: Result<Space, i32>,
}

/// The disks among `mounts`, each with its space as `measure` gives it, in
/// the table's order: less any of a type that keeps nothing on a disk
/// ([`NOT_DISKS`]), and any measured as having no blocks at all.
///
/// One that cannot be measured is kept, with the errno: it is a disk not
/// measured, not no disk.
#[must_use]
pub fn disks(mounts: &[Mount], measure: impl Fn(&[u8]) -> Result<Space, i32>) -> Vec<Disk> {
    mounts
        .iter()
        .filter(|mount| !NOT_DISKS.contains(&mount.fs_type.as_slice()))
        .filter_map(|mount| {
            let space = measure(&mount.point);
            match space {
                Ok(measured) if measured.total_blocks == 0 => None,
                _ => Some(Disk {
                    point: mount.point.clone(),
                    space,
                }),
            }
        })
        .collect()
}

/// The disks mounted, as the mount table at `table` lists them -- the
/// kernel's is [`MOUNT_TABLE`] -- each with its space, asked of the kernel.
///
/// # Errors
///
/// The table's, when it will not read: where there is no `/proc`, as on the
/// host the tests run on, or no right to read it.
pub fn mounted_disks(table: &Path) -> std::io::Result<Vec<Disk>> {
    let read = std::fs::read(table)?;
    Ok(disks(&parse_mounts(&read), of))
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
#[path = "fs_space_tests.rs"]
mod tests;
