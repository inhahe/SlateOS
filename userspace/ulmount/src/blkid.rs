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
//! **Probing** is `ulblkid`'s -- the port of libblkid's, every
//! superblock and partition-table prober -- called as libmount and
//! libblkid's cache call it. It reads the device, which needs read access
//! to it: for anyone but root it fails with `EACCES`, as upstream's does.

use crate::blkid_cache::{BlkCache, Config, Eval, cache_filename};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub use ulblkid::{PARTS_ENTRY_DETAILS, SUBLKS_LABEL, SUBLKS_SECTYPE, SUBLKS_TYPE, SUBLKS_UUID};

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

/// `blkid_probe_set_device(pr, fd, 0, 0)` on the device, the superblocks
/// chain with `sb_flags`, the partitions chain -- if `pt_flags` asks for it
/// -- with those, and `blkid_do_safeprobe`: every value found, in
/// libblkid's order, each string without its NUL. Probing is `ulblkid`'s,
/// the port of libblkid's.
///
/// # Errors
///
/// [`ProbeFail::Device`] when the device cannot be probed at all (a
/// directory, a character device other than UBI -- `EINVAL` -- or one
/// whose size cannot be read); [`ProbeFail::Io`] for a read that failed;
/// [`ProbeFail::Nothing`] or [`ProbeFail::Ambivalent`] for what
/// `blkid_do_safeprobe` found.
pub fn probe_file(
    file: std::fs::File,
    sb_flags: u32,
    pt_flags: Option<u32>,
) -> Result<Values, ProbeFail> {
    let mut pr = ulblkid::Probe::new();
    if pr.set_device(Some(Rc::new(file)), 0, 0) != 0 {
        return Err(ProbeFail::Device(if pr.errno != 0 {
            pr.errno
        } else {
            EINVAL
        }));
    }
    pr.enable_superblocks(true);
    pr.set_superblocks_flags(sb_flags);
    if let Some(flags) = pt_flags {
        pr.enable_partitions(true);
        pr.set_partitions_flags(flags);
    }
    match pr.do_safeprobe() {
        ulblkid::PROBE_OK => Ok(pr
            .values()
            .iter()
            .map(|v| (v.name.as_bytes(), v.as_c_str().to_vec()))
            .collect()),
        ulblkid::PROBE_NONE => Err(ProbeFail::Nothing),
        ulblkid::PROBE_AMBIGUOUS => Err(ProbeFail::Ambivalent),
        _ => Err(ProbeFail::Io(if pr.errno != 0 { pr.errno } else { EIO })),
    }
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
    let values = probe_file(
        file,
        SUBLKS_LABEL | SUBLKS_UUID | SUBLKS_TYPE,
        Some(PARTS_ENTRY_DETAILS),
    )?;
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
mod tests {
    use super::*;

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
