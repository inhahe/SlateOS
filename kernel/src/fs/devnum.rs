//! Device numbers: the `major:minor` pair that names a device -- what `stat`
//! reports as `st_rdev` for a character or block device node, what
//! `/proc/<pid>/stat` field 7 reports as a process's terminal, and what
//! `/sys/devices/block/<name>/dev` reports for a disk.
//!
//! SlateOS uses **Linux's numbering**, so a program that decodes a number
//! (`ls -l`'s `1, 3`, `ps`'s TTY column, `lsblk`) or compares two (`w`
//! matching a process's terminal with `/dev/pts/3`'s `st_rdev`) works
//! unchanged. The fixed majors are Linux's
//! (`Documentation/admin-guide/devices.txt`). Linux numbers two of our
//! drivers dynamically -- virtio-blk and NVMe -- and for those this fixes
//! the major a typical Linux system ends up with: 254 for virtio-blk (the
//! first dynamic block major, which it takes), 259 (`BLOCK_EXT_MAJOR`) for
//! NVMe namespaces.
//!
//! Until 2026-10-03 nothing in SlateOS had a device number: every `st_rdev`
//! was 0 and every process's `tty_nr` 0 -- the value that means "no
//! terminal" (requests/b-ad-proc-stat-reports-no-controlling-terminal.md).

/// A device number, `major:minor`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DevNum {
    /// The driver: which kind of device.
    pub major: u32,
    /// Which one of the driver's devices.
    pub minor: u32,
}

impl DevNum {
    /// No device: what a file that is not a device node reports.
    pub const NONE: Self = Self::new(0, 0);

    /// `major:minor`.
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Linux's `new_encode_dev`, the 32-bit form `/proc/<pid>/stat` and
    /// `stat`'s `st_rdev` carry: the minor's low byte in bits 0-7, the major
    /// in bits 8-19, the rest of the minor from bit 20. For a major below
    /// 4096 and a minor below 2^20 it equals glibc's 64-bit `makedev`.
    #[must_use]
    pub const fn linux_encode(self) -> u32 {
        (self.minor & 0xff) | ((self.major & 0xfff) << 8) | ((self.minor & !0xff) << 12)
    }
}

/// `/dev/null` 1:3, `zero` 1:5, `full` 1:7, `random` 1:8, `urandom` 1:9,
/// `kmsg` 1:11; RAM disk `ramN` is 1:N.
pub const MEM_MAJOR: u32 = 1;
/// `/dev/tty` 5:0, `/dev/console` 5:1, `/dev/ptmx` 5:2.
pub const TTYAUX_MAJOR: u32 = 5;
/// Loop devices: `loopN` is 7:N.
pub const LOOP_MAJOR: u32 = 7;
/// The first SCSI-disk major: `sda` is 8:0, `sdb` 8:16, sixteen minors each
/// (the disk and fifteen partitions). Disks past the sixteenth use
/// [`SCSI_DISK_MAJORS`].
pub const SCSI_DISK0_MAJOR: u32 = 8;
/// Input: `/dev/input/eventN` is 13:(64 + N).
pub const INPUT_MAJOR: u32 = 13;
/// ALSA: card C's control is 116:(32C), its PCM device D 116:(32C + 16 + D)
/// for playback and 116:(32C + 24 + D) for capture (the static minors).
pub const ALSA_MAJOR: u32 = 116;
/// Pseudo-terminal slaves: `/dev/pts/N` is 136:N.
pub const UNIX98_PTY_SLAVE_MAJOR: u32 = 136;
/// DRM: `/dev/dri/cardN` is 226:N, `renderD128 + N` is 226:(128 + N).
pub const DRM_MAJOR: u32 = 226;
/// virtio-blk, as a typical Linux system numbers it: `vda` 254:0, `vdb`
/// 254:16, sixteen minors each.
pub const VIRTBLK_MAJOR: u32 = 254;
/// Linux's extended block major, which NVMe namespaces take: `nvme0n1` is
/// 259:0 here.
pub const BLOCK_EXT_MAJOR: u32 = 259;

/// The sixteen SCSI-disk majors in order, sixteen disks each: 8, then 65-71,
/// then 128-135.
pub const SCSI_DISK_MAJORS: [u32; 16] = [
    SCSI_DISK0_MAJOR,
    65,
    66,
    67,
    68,
    69,
    70,
    71,
    128,
    129,
    130,
    131,
    132,
    133,
    134,
    135,
];

/// `name` split into its base and a trailing decimal number (`"vda12"` →
/// `("vda", Some(12))`, `"vda"` → `("vda", None)`).
fn split_number(name: &str) -> (&str, Option<u32>) {
    let digits = name.bytes().rev().take_while(u8::is_ascii_digit).count();
    // The cut falls before an ASCII digit, so on a character boundary; the
    // `get`s cannot fail, and fall back to "no number" if they ever did.
    let at = name.len().saturating_sub(digits);
    match (name.get(..at), name.get(at..)) {
        (Some(base), Some(tail)) => (base, tail.parse().ok()),
        _ => (name, None),
    }
}

/// The disk index of `letters` -- `a` is 0, `z` 25, `aa` 26, as Linux
/// names disks -- or `None` for an empty or non-letter name.
fn disk_index(letters: &str) -> Option<u32> {
    if letters.is_empty() {
        return None;
    }
    let mut index: u32 = 0;
    for b in letters.bytes() {
        if !b.is_ascii_lowercase() {
            return None;
        }
        index = index
            .checked_mul(26)?
            .checked_add(u32::from(b.saturating_sub(b'a')))?
            .checked_add(1)?;
    }
    index.checked_sub(1)
}

/// A disk with sixteen minors per disk (`sdX`, `vdX`): disk `index`,
/// partition `part` (0 for the whole disk), or `None` past fifteen
/// partitions.
fn sixteen_per_disk(index: u32, part: u32) -> Option<u32> {
    if part > 15 {
        return None;
    }
    index.checked_mul(16)?.checked_add(part)
}

/// The device number of the block device named `name`, as `stat` reports
/// it for `/dev/<name>`: `sdX[N]`, `vdX[N]`, `nvmeCnS[pN]`, `ramN`,
/// `loopN`. [`DevNum::NONE`] for a name none of these describe -- a test
/// device, say -- rather than a number some other device has.
#[must_use]
pub fn for_block(name: &str) -> DevNum {
    block_number(name).unwrap_or(DevNum::NONE)
}

fn block_number(name: &str) -> Option<DevNum> {
    if let Some(rest) = name.strip_prefix("nvme") {
        // nvme<controller>n<namespace>[p<partition>]
        let (disk, part) = match rest.split_once('p') {
            Some((disk, part)) => (disk, part.parse::<u32>().ok()?),
            None => (rest, 0),
        };
        let (ctrl, ns) = disk.split_once('n')?;
        let ctrl: u32 = ctrl.parse().ok()?;
        let ns: u32 = ns.parse().ok()?;
        if ctrl > 15 || !(1..=16).contains(&ns) || part > 15 {
            return None;
        }
        let minor = ctrl
            .checked_mul(16)?
            .checked_add(ns.checked_sub(1)?)?
            .checked_mul(16)?
            .checked_add(part)?;
        return Some(DevNum::new(BLOCK_EXT_MAJOR, minor));
    }
    let (base, number) = split_number(name);
    match base {
        "ram" => number.map(|n| DevNum::new(MEM_MAJOR, n)),
        "loop" => number.map(|n| DevNum::new(LOOP_MAJOR, n)),
        _ => {
            let part = number.unwrap_or(0);
            if let Some(letters) = base.strip_prefix("sd") {
                let index = disk_index(letters)?;
                let major = *SCSI_DISK_MAJORS.get(usize::try_from(index / 16).ok()?)?;
                Some(DevNum::new(major, sixteen_per_disk(index % 16, part)?))
            } else if let Some(letters) = base.strip_prefix("vd") {
                Some(DevNum::new(
                    VIRTBLK_MAJOR,
                    sixteen_per_disk(disk_index(letters)?, part)?,
                ))
            } else {
                None
            }
        }
    }
}

/// Boot self-test: the encoding, and the block names against the numbers
/// Linux gives the same disks.
///
/// # Errors
///
/// `InternalError` on the first mismatch, after a serial line naming it.
pub fn self_test() -> crate::error::KernelResult<()> {
    let checks: [(&str, DevNum, DevNum); 18] = [
        ("sda", for_block("sda"), DevNum::new(8, 0)),
        ("sda1", for_block("sda1"), DevNum::new(8, 1)),
        ("sdb", for_block("sdb"), DevNum::new(8, 16)),
        ("sdp15", for_block("sdp15"), DevNum::new(8, 255)),
        ("sdq, the 17th disk", for_block("sdq"), DevNum::new(65, 0)),
        (
            "sdaa, the 27th disk",
            for_block("sdaa"),
            DevNum::new(65, 160),
        ),
        ("vda", for_block("vda"), DevNum::new(254, 0)),
        ("vdb2", for_block("vdb2"), DevNum::new(254, 18)),
        ("nvme0n1", for_block("nvme0n1"), DevNum::new(259, 0)),
        ("nvme0n1p3", for_block("nvme0n1p3"), DevNum::new(259, 3)),
        ("nvme1n2", for_block("nvme1n2"), DevNum::new(259, 272)),
        ("ram3", for_block("ram3"), DevNum::new(1, 3)),
        ("loop2", for_block("loop2"), DevNum::new(7, 2)),
        ("a 16th partition", for_block("sda16"), DevNum::NONE),
        ("a test device", for_block("fat-test"), DevNum::NONE),
        ("no disk letters", for_block("sd"), DevNum::NONE),
        ("nvme namespace 0", for_block("nvme0n0"), DevNum::NONE),
        ("a bare ram", for_block("ram"), DevNum::NONE),
    ];
    for (what, got, want) in checks {
        if got != want {
            crate::serial_println!(
                "[devnum]   FAIL: {} is {}:{}, want {}:{}",
                what,
                got.major,
                got.minor,
                want.major,
                want.minor
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }
    // Linux's encoding: /dev/console 5:1 is 0x501, /dev/pts/3 0x8803, a minor
    // past 255 carries its high bits from bit 20.
    let encodings = [
        (DevNum::new(5, 1), 0x501),
        (DevNum::new(136, 3), 0x8803),
        (DevNum::new(136, 256), 0x10_8800),
        (DevNum::new(254, 16), 0xFE10),
    ];
    for (dev, want) in encodings {
        if dev.linux_encode() != want {
            crate::serial_println!(
                "[devnum]   FAIL: {}:{} encodes as {:#x}, want {:#x}",
                dev.major,
                dev.minor,
                dev.linux_encode(),
                want
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }
    crate::serial_println!(
        "[devnum] Self-test PASSED: {} block names, {} encodings",
        checks.len(),
        encodings.len()
    );
    Ok(())
}
