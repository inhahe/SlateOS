//! The part of libblkid libmount calls: evaluating a tag to the device that
//! carries it, and reading a device's own tags.
//!
//! **Evaluating** follows `/etc/blkid.conf` (`EVALUATE=`), and by default
//! is upstream's `udev,scan`: the tag's udev link, `/dev/disk/by-uuid/X` and
//! its siblings, with the value encoded as udev encodes it -- then
//! libblkid's cache ([`crate::blkid_cache`]), which probes every device if
//! the tag is in neither. The link must be a block device and is
//! canonicalized.
//!
//! **Probing** reads the device's superblock, which needs read access to
//! it: for anyone but root it fails with `EACCES`, as upstream's does.
//! Of libblkid's hundred-odd probers only ext2, ext3, ext4 (and ext4dev
//! and jbd), SlateOS's filesystems, are here; a device holding anything
//! else probes as holding nothing, and its tags are found only through
//! udev (see known-issues TD-B-ULMOUNT-PROBES-ONLY-EXT).

use crate::blkid_cache::{BlkCache, Config, Eval, cache_filename};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A path from bytes.
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(b))
}

/// `S_ISBLK` of `stat(path)`.
#[must_use]
pub fn is_block_device(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(path).is_ok_and(|m| m.file_type().is_block_device())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// `utf8_encoded_expected_len(str)`.
fn utf8_expected_len(c: u8) -> usize {
    match c {
        0..=0x7f => 1,
        _ if c & 0xe0 == 0xc0 => 2,
        _ if c & 0xf0 == 0xe0 => 3,
        _ if c & 0xf8 == 0xf0 => 4,
        _ if c & 0xfc == 0xf8 => 5,
        _ if c & 0xfe == 0xfc => 6,
        _ => 0,
    }
}

/// `utf8_encoded_valid_unichar(str)`: the length of a valid multibyte
/// character at the front of `s`, 1 for ASCII, `None` for anything else.
fn utf8_valid_len(s: &[u8]) -> Option<usize> {
    let &c = s.first()?;
    let len = utf8_expected_len(c);
    match len {
        0 => return None,
        1 => return Some(1),
        _ => {}
    }
    let bytes = s.get(..len)?;
    if bytes.iter().any(|&b| b & 0x80 != 0x80) {
        return None;
    }
    let lead_bits = match len {
        2 => 0x1f,
        3 => 0x0f,
        4 => 0x07,
        5 => 0x03,
        _ => 0x01,
    };
    let mut unichar = u32::from(c & lead_bits);
    for &b in bytes.get(1..)? {
        if b & 0xc0 != 0x80 {
            return None;
        }
        unichar = (unichar << 6) | u32::from(b & 0x3f);
    }
    let encoded_len = match unichar {
        0..0x80 => 1,
        0x80..0x800 => 2,
        0x800..0x1_0000 => 3,
        0x1_0000..0x20_0000 => 4,
        0x20_0000..0x400_0000 => 5,
        _ => 6,
    };
    if encoded_len != len {
        return None;
    }
    let valid = unichar <= 0x10_ffff
        && unichar & 0xffff_f800 != 0xd800
        && !(unichar > 0xfdcf && unichar < 0xfdf0)
        && unichar & 0xffff != 0xffff;
    valid.then_some(len)
}

/// `blkid_encode_string(str, enc, len)` with udev's buffer: valid UTF-8
/// characters as they are, `[0-9A-Za-z#+-.:=@_]` as they are, everything
/// else -- a backslash included -- as `\xHH`. `None` when it would not fit
/// in `room` bytes with its NUL.
#[must_use]
pub fn encode_string(s: &[u8], room: usize) -> Option<Vec<u8>> {
    let s = crate::c_str(s);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let rest = s.get(i..).unwrap_or_default();
        let c = rest.first().copied().unwrap_or(0);
        match utf8_valid_len(rest) {
            Some(n) if n > 1 => {
                if room.saturating_sub(out.len()) < n {
                    return None;
                }
                out.extend_from_slice(rest.get(..n).unwrap_or_default());
                i = i.saturating_add(n);
            }
            _ => {
                let whitelisted = c.is_ascii_alphanumeric() || b"#+-.:=@_".contains(&c);
                if c == b'\\' || !whitelisted {
                    if room.saturating_sub(out.len()) < 4 {
                        return None;
                    }
                    out.extend_from_slice(format!("\\x{c:02x}").as_bytes());
                } else {
                    if room.saturating_sub(out.len()) < 1 {
                        return None;
                    }
                    out.push(c);
                }
                i = i.saturating_add(1);
            }
        }
        if out.len().saturating_add(3) >= room {
            return None;
        }
    }
    if room.saturating_sub(out.len()) < 1 {
        return None;
    }
    Some(out)
}

/// `PATH_MAX`.
const PATH_MAX: usize = 4096;

/// `evaluate_by_udev(token, value)`: the tag's udev link, if it is there
/// and a block device, canonicalized.
fn evaluate_by_udev(token: &[u8], value: &[u8]) -> Option<Vec<u8>> {
    let dir: &[u8] = match token {
        b"UUID" => b"/dev/disk/by-uuid/",
        b"LABEL" => b"/dev/disk/by-label/",
        b"PARTLABEL" => b"/dev/disk/by-partlabel/",
        b"PARTUUID" => b"/dev/disk/by-partuuid/",
        b"ID" => b"/dev/disk/by-id/",
        _ => return None,
    };
    let enc = encode_string(value, PATH_MAX.saturating_sub(dir.len()))?;
    let mut dev = dir.to_vec();
    dev.extend_from_slice(&enc);
    if !is_block_device(&path_of(&dev)) {
        return None;
    }
    crate::cache::canonicalize_path(&dev)
}

/// `evaluate_by_scan(token, value, &cache, conf)`: the device libblkid's
/// cache knows with the tag -- probing new devices, then all of them, if
/// it knows none. `bc` is the caller's cache, made from the configured
/// file if it has none yet; without one, a cache is made for the lookup
/// and put (written back) after it.
fn evaluate_by_scan(
    token: &[u8],
    value: &[u8],
    bc: Option<&mut Option<BlkCache>>,
    conf: &Config,
) -> Option<Vec<u8>> {
    let cachefile = cache_filename(Some(conf));
    match bc {
        Some(slot) => slot
            .get_or_insert_with(|| BlkCache::get_cache(cachefile.as_deref()))
            .get_devname(token, value),
        None => BlkCache::get_cache(cachefile.as_deref()).get_devname(token, value),
    }
}

/// `blkid_evaluate_tag(token, value, &cache)`: the device a tag names, by
/// each of the configured methods in turn (`EVALUATE=` in
/// `/etc/blkid.conf`; udev's links, then libblkid's cache, by default).
/// `None` when the configuration does not parse, as upstream gives up then.
#[must_use]
pub fn evaluate_tag(
    token: &[u8],
    value: &[u8],
    mut bc: Option<&mut Option<BlkCache>>,
) -> Option<Vec<u8>> {
    let conf = crate::blkid_cache::read_config(None)?;
    for method in &conf.evals {
        let found = match method {
            Eval::Udev => evaluate_by_udev(token, value),
            Eval::Scan => evaluate_by_scan(token, value, bc.as_deref_mut(), &conf),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// CRC-32C (Castagnoli), reflected, without the final inversion: libblkid's
/// `crc32c(seed, buf, len)`.
fn crc32c(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    crc
}

/// A little-endian `u32` of `buf` at `at`.
fn le32(buf: &[u8], at: usize) -> u32 {
    let b = |k: usize| buf.get(at.saturating_add(k)).copied().unwrap_or(0);
    u32::from_le_bytes([b(0), b(1), b(2), b(3)])
}

/// `EXT3_FEATURE_COMPAT_HAS_JOURNAL`.
const HAS_JOURNAL: u32 = 0x0004;
/// `EXT3_FEATURE_INCOMPAT_JOURNAL_DEV`.
const JOURNAL_DEV: u32 = 0x0008;
/// `EXT2_FLAGS_TEST_FILESYS`.
const TEST_FILESYS: u32 = 0x0004;
/// `EXT4_FEATURE_RO_COMPAT_METADATA_CSUM`.
const METADATA_CSUM: u32 = 0x0400;
/// `EXT2_FEATURE_RO_COMPAT_SUPP` (ext3's is the same).
const RO_COMPAT_SUPP: u32 = 0x0001 | 0x0002 | 0x0004;
/// `EXT2_FEATURE_INCOMPAT_SUPP`.
const EXT2_INCOMPAT_SUPP: u32 = 0x0002 | 0x0010;
/// `EXT3_FEATURE_INCOMPAT_SUPP`.
const EXT3_INCOMPAT_SUPP: u32 = 0x0002 | 0x0004 | 0x0010;

/// `BLKID_SUBLKS_LABEL`: report LABEL.
pub const SUBLKS_LABEL: u32 = 1 << 1;
/// `BLKID_SUBLKS_UUID`: report UUID.
pub const SUBLKS_UUID: u32 = 1 << 3;
/// `BLKID_SUBLKS_TYPE`: report TYPE.
pub const SUBLKS_TYPE: u32 = 1 << 5;
/// `BLKID_SUBLKS_SECTYPE`: report SEC_TYPE.
pub const SUBLKS_SECTYPE: u32 = 1 << 6;

/// `blkid_unparse_uuid`: `8-4-4-4-12` lowercase hex; `None` for the all-zero
/// UUID, which `blkid_probe_set_uuid_as` does not report.
fn unparse_uuid(uuid: &[u8]) -> Option<Vec<u8>> {
    if uuid.iter().all(|&b| b == 0) {
        return None;
    }
    let hex: Vec<String> = uuid.iter().map(|b| format!("{b:02x}")).collect();
    let s = format!(
        "{}-{}-{}-{}-{}",
        hex.get(..4).unwrap_or_default().concat(),
        hex.get(4..6).unwrap_or_default().concat(),
        hex.get(6..8).unwrap_or_default().concat(),
        hex.get(8..10).unwrap_or_default().concat(),
        hex.get(10..16).unwrap_or_default().concat()
    );
    Some(s.into_bytes())
}

/// What libblkid's ext probers report for an ext superblock --
/// `probe_ext4dev`, `probe_ext4`, `probe_ext3`, `probe_ext2` and
/// `probe_jbd`, each deciding for itself, then `ext_get_info` -- in
/// libblkid's order: LABEL, UUID, EXT_JOURNAL, SEC_TYPE, BLOCK_SIZE (never
/// gated), LOGUUID (a journal device's), then TYPE. `flags` are the
/// `BLKID_SUBLKS_*` that gate LABEL, UUID, SEC_TYPE and TYPE. `sb` is the
/// 1024 bytes at offset 1024.
///
/// # Errors
///
/// [`ProbeFail::Nothing`] when no ext prober claims it,
/// [`ProbeFail::Ambivalent`] when two do (`blkid_do_safeprobe` refuses a
/// superblock more than one prober claims).
pub fn probe_ext(sb: &[u8], flags: u32) -> Result<Values, ProbeFail> {
    if sb.get(0x38..0x3a) != Some(&[0x53, 0xef][..]) {
        return Err(ProbeFail::Nothing);
    }
    let fc = le32(sb, 0x5c);
    let fi = le32(sb, 0x60);
    let frc = le32(sb, 0x64);
    let s_flags = le32(sb, 0x160);
    if frc & METADATA_CSUM != 0 {
        // The checksum covers the superblock up to `s_checksum`, the last
        // four of its 1024 bytes.
        let body = sb.get(..0x3fc).ok_or(ProbeFail::Nothing)?;
        if crc32c(!0, body) != le32(sb, 0x3fc) {
            return Err(ProbeFail::Nothing);
        }
    }
    let ext3_unsupported = frc & !RO_COMPAT_SUPP != 0 || fi & !EXT3_INCOMPAT_SUPP != 0;
    // Each prober, in libblkid's list order, with the version it hands
    // `ext_get_info`.
    let claims: [(&'static [u8], u32, bool); 5] = [
        (
            b"ext4dev",
            4,
            fi & JOURNAL_DEV == 0 && s_flags & TEST_FILESYS != 0,
        ),
        (
            b"ext4",
            4,
            fi & JOURNAL_DEV == 0 && ext3_unsupported && s_flags & TEST_FILESYS == 0,
        ),
        (b"ext3", 3, fc & HAS_JOURNAL != 0 && !ext3_unsupported),
        (
            b"ext2",
            2,
            fc & HAS_JOURNAL == 0 && frc & !RO_COMPAT_SUPP == 0 && fi & !EXT2_INCOMPAT_SUPP == 0,
        ),
        (b"jbd", 2, fi & JOURNAL_DEV != 0),
    ];
    let mut matched = claims.iter().filter(|(_, _, m)| *m);
    let &(ty, ver, _) = matched.next().ok_or(ProbeFail::Nothing)?;
    if matched.next().is_some() {
        return Err(ProbeFail::Ambivalent);
    }
    let mut values: Values = Vec::new();
    // `ext_get_info`. The label only when its first byte is not NUL; then
    // `blkid_probe_set_label`: a C string, trailing white space off, and
    // nothing if that leaves nothing.
    if flags & SUBLKS_LABEL != 0 && sb.get(0x78).is_some_and(|&b| b != 0) {
        let label = crate::c_str(sb.get(0x78..0x88).unwrap_or_default());
        let end = label
            .iter()
            .rposition(|&b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
            .map_or(0, |i| i.saturating_add(1));
        if end > 0 {
            values.push((b"LABEL", label.get(..end).unwrap_or_default().to_vec()));
        }
    }
    let uuid = sb.get(0x68..0x78).unwrap_or_default();
    if flags & SUBLKS_UUID != 0
        && let Some(u) = unparse_uuid(uuid)
    {
        values.push((b"UUID", u));
    }
    if fc & HAS_JOURNAL != 0
        && let Some(j) = unparse_uuid(sb.get(0xd0..0xe0).unwrap_or_default())
    {
        values.push((b"EXT_JOURNAL", j));
    }
    if ver != 2 && flags & SUBLKS_SECTYPE != 0 && fi & !EXT2_INCOMPAT_SUPP == 0 {
        values.push((b"SEC_TYPE", b"ext2".to_vec()));
    }
    let log_block_size = le32(sb, 0x18);
    if log_block_size < 32 {
        values.push((
            b"BLOCK_SIZE",
            (1024u64 << log_block_size).to_string().into_bytes(),
        ));
    }
    if ty == b"jbd"
        && let Some(u) = unparse_uuid(uuid)
    {
        values.push((b"LOGUUID", u));
    }
    if flags & SUBLKS_TYPE != 0 {
        values.push((b"TYPE", ty.to_vec()));
    }
    Ok(values)
}

/// `EINVAL`.
const EINVAL: i32 = 22;
/// `EIO`, for an error without an `errno`.
const EIO: i32 = 5;
/// `O_NONBLOCK`: Linux's value, which SlateOS's C library shares -- so a
/// FIFO named as a device does not hang the open.
#[cfg(unix)]
const O_NONBLOCK: i32 = 0o4000;

/// What a probe found: each value's name and its bytes, in libblkid's
/// order.
pub type Values = Vec<(&'static [u8], Vec<u8>)>;

/// Why a probe has nothing to report: libblkid's return codes, and the
/// `errno` it leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeFail {
    /// `open` failed (`EACCES` for a user who may not read the device).
    Open(i32),
    /// `blkid_probe_set_device` refused it: a directory, a character device
    /// other than UBI (`EINVAL`), or one whose size cannot be read.
    Device(i32),
    /// A read failed while probing (`-errno`).
    Io(i32),
    /// Nothing on it was recognised (`blkid_do_safeprobe` returned 1).
    Nothing,
    /// More than one prober claimed it (`-2`).
    Ambivalent,
}

impl ProbeFail {
    /// The `errno` the failure leaves, where it left one.
    #[must_use]
    pub fn errno(self) -> Option<i32> {
        match self {
            ProbeFail::Open(e) | ProbeFail::Device(e) | ProbeFail::Io(e) => Some(e),
            ProbeFail::Nothing | ProbeFail::Ambivalent => None,
        }
    }
}

/// The `errno` of an I/O error.
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EIO)
}

/// A device open for probing: `open(O_RDONLY|O_CLOEXEC|O_NONBLOCK)`.
///
/// # Errors
///
/// The `open`'s `errno`, as [`ProbeFail::Open`].
pub fn open_nonblock(devname: &[u8]) -> Result<std::fs::File, ProbeFail> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(O_NONBLOCK);
    }
    opts.open(path_of(devname))
        .map_err(|e| ProbeFail::Open(errno_of(&e)))
}

/// `blkid_probe_set_device(pr, fd, 0, 0)`: the size to probe -- a block
/// device's, a regular file's, a UBI volume's 1 -- or why not.
///
/// # Errors
///
/// [`ProbeFail::Device`]: `EINVAL` for anything else, or the `errno` of
/// an `fstat` or a size that could not be read.
pub fn device_size(file: &std::fs::File) -> Result<u64, ProbeFail> {
    let meta = file
        .metadata()
        .map_err(|e| ProbeFail::Device(errno_of(&e)))?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let ft = meta.file_type();
        if ft.is_block_device() {
            // `blkdev_get_size`.
            let mut f = file;
            return f
                .seek(SeekFrom::End(0))
                .map_err(|e| ProbeFail::Device(errno_of(&e)));
        }
        if ft.is_char_device() {
            // Only UBI volumes, which are character devices, are probed.
            let rdev = meta.rdev();
            let link = format!(
                "/sys/dev/char/{}:{}",
                crate::fs::major(rdev),
                crate::fs::minor(rdev)
            );
            let ubi = std::fs::read_link(link)
                .ok()
                .and_then(|t| {
                    t.file_name()
                        .map(|n| quoting::os_bytes(n).starts_with(b"ubi"))
                })
                .unwrap_or(false);
            return if ubi {
                Ok(1)
            } else {
                Err(ProbeFail::Device(EINVAL))
            };
        }
    }
    Err(ProbeFail::Device(EINVAL))
}

/// `blkid_do_safeprobe` with the superblocks chain (as `flags` allow) and
/// the partitions chain: every value found, in libblkid's order. Only the
/// ext family is recognised here (known-issues
/// TD-B-ULMOUNT-PROBES-ONLY-EXT), and no partition table.
///
/// # Errors
///
/// A read that failed, nothing recognised, or more than one thing.
pub fn probe_file(file: &std::fs::File, size: u64, flags: u32) -> Result<Values, ProbeFail> {
    // The ext superblock is the 1024 bytes at 1024.
    if size < 2048 {
        return Err(ProbeFail::Nothing);
    }
    let mut f = file;
    let mut sb = vec![0u8; 1024];
    f.seek(SeekFrom::Start(1024))
        .map_err(|e| ProbeFail::Io(errno_of(&e)))?;
    f.read_exact(&mut sb).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            ProbeFail::Nothing
        } else {
            ProbeFail::Io(errno_of(&e))
        }
    })?;
    probe_ext(&sb, flags)
}

/// `mnt_cache_read_tags`' probe (`blkid_new_probe_from_filename`, then
/// superblocks with LABEL, UUID and TYPE, and partitions): the device's
/// LABEL, UUID, TYPE, PARTUUID and PARTLABEL (libblkid's `PART_ENTRY_UUID`
/// and `PART_ENTRY_NAME`), in that order, those it has.
///
/// # Errors
///
/// Why there are none (see [`ProbeFail`]).
pub fn probe_tags(devname: &[u8]) -> Result<Values, ProbeFail> {
    let file = open_nonblock(devname)?;
    let size = device_size(&file)?;
    let values = probe_file(&file, size, SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_TYPE)?;
    // `tags[]` and `blktags[]`: libmount's names for libblkid's values.
    let names: [(&[u8], &'static [u8]); 5] = [
        (b"LABEL", b"LABEL"),
        (b"UUID", b"UUID"),
        (b"TYPE", b"TYPE"),
        (b"PART_ENTRY_UUID", b"PARTUUID"),
        (b"PART_ENTRY_NAME", b"PARTLABEL"),
    ];
    let mut tags = Vec::new();
    for (blk, name) in names {
        if let Some((_, v)) = values.iter().find(|(n, _)| *n == blk) {
            tags.push((name, v.clone()));
        }
    }
    Ok(tags)
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests write a buffer they sized at fixed offsets"
)]
mod tests {
    use super::*;

    fn superblock(fc: u32, fi: u32, frc: u32, label: &[u8], uuid: [u8; 16]) -> Vec<u8> {
        let mut sb = vec![0u8; 1024];
        sb[0x38] = 0x53;
        sb[0x39] = 0xef;
        sb[0x5c..0x60].copy_from_slice(&fc.to_le_bytes());
        sb[0x60..0x64].copy_from_slice(&fi.to_le_bytes());
        sb[0x64..0x68].copy_from_slice(&frc.to_le_bytes());
        sb[0x68..0x78].copy_from_slice(&uuid);
        sb[0x78..0x78 + label.len()].copy_from_slice(label);
        sb
    }

    #[test]
    #[allow(
        clippy::indexing_slicing,
        reason = "fixed offsets in a buffer the test sized"
    )]
    fn ext_types_as_libblkid() {
        let uuid = [
            0xf3, 0xac, 0x64, 0x74, 0xba, 0x9c, 0x46, 0x8c, 0xb6, 0xd0, 0xe2, 0x1c, 0xa6, 0xbc,
            0x1d, 0xca,
        ];
        let mut sb = superblock(HAS_JOURNAL, 0x0002 | 0x0040, 0x0001, b"root  ", uuid);
        // A 4096-byte block, and an external journal.
        sb[0x18] = 2;
        sb[0xd0] = 0xab;
        let v = probe_ext(&sb, SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_TYPE).unwrap_or_default();
        assert_eq!(
            v,
            vec![
                (&b"LABEL"[..], b"root".to_vec()),
                (
                    &b"UUID"[..],
                    b"f3ac6474-ba9c-468c-b6d0-e21ca6bc1dca".to_vec()
                ),
                (
                    &b"EXT_JOURNAL"[..],
                    b"ab000000-0000-0000-0000-000000000000".to_vec()
                ),
                (&b"BLOCK_SIZE"[..], b"4096".to_vec()),
                (&b"TYPE"[..], b"ext4".to_vec()),
            ]
        );
        // SEC_TYPE only when asked for, and only for a filesystem ext2 could
        // mount: ext3 without ext4's extents feature.
        let ext3 = superblock(HAS_JOURNAL, 0x0002, 0x0001, b"", [0; 16]);
        let names = |sb: &[u8], flags| {
            probe_ext(sb, flags)
                .unwrap_or_default()
                .into_iter()
                .map(|(n, _)| n)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&ext3, SUBLKS_TYPE | SUBLKS_SECTYPE),
            vec![&b"SEC_TYPE"[..], b"BLOCK_SIZE", b"TYPE"]
        );
        assert_eq!(names(&ext3, SUBLKS_TYPE), vec![&b"BLOCK_SIZE"[..], b"TYPE"]);
        assert_eq!(
            names(&sb, SUBLKS_SECTYPE),
            vec![&b"EXT_JOURNAL"[..], b"BLOCK_SIZE"]
        );
        let t = |fc, fi, frc| {
            probe_ext(&superblock(fc, fi, frc, b"", [0; 16]), SUBLKS_TYPE)
                .ok()
                .and_then(|v| v.last().map(|(_, t)| t.clone()))
        };
        assert_eq!(t(HAS_JOURNAL, 0x0002, 0x0001), Some(b"ext3".to_vec()));
        assert_eq!(t(0, 0x0002, 0x0001), Some(b"ext2".to_vec()));
        assert_eq!(t(0, JOURNAL_DEV, 0), Some(b"jbd".to_vec()));
        let mut bad = superblock(0, 0, METADATA_CSUM, b"", [0; 16]);
        bad[0x3fc] = 1;
        assert_eq!(probe_ext(&bad, SUBLKS_TYPE), Err(ProbeFail::Nothing));
        let mut good = superblock(0, 0, METADATA_CSUM, b"", [0; 16]);
        let csum = crc32c(!0, &good[..0x3fc]);
        good[0x3fc..0x400].copy_from_slice(&csum.to_le_bytes());
        assert!(probe_ext(&good, SUBLKS_TYPE).is_ok());
    }

    #[test]
    fn crc32c_is_castagnolis() {
        // The standard check value, with the final inversion applied here.
        assert_eq!(!crc32c(!0, b"123456789"), 0xe306_9283);
    }

    #[test]
    fn strings_encode_as_udev() {
        assert_eq!(
            encode_string(b"My Disk", 100),
            Some(b"My\\x20Disk".to_vec())
        );
        assert_eq!(encode_string(b"a\\b", 100), Some(b"a\\x5cb".to_vec()));
        assert_eq!(
            encode_string("é".as_bytes(), 100),
            Some("é".as_bytes().to_vec())
        );
        assert_eq!(encode_string(&[0xff], 100), Some(b"\\xff".to_vec()));
        assert_eq!(encode_string(b"abcdef", 5), None);
    }
}
