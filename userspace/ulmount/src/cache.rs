//! libmount's `cache.c` and `lib/canonicalize.c`: canonical paths and
//! evaluated tags, each looked up once and remembered.
//!
//! A path is canonicalized with `realpath` (a path that does not resolve
//! stays as it is), and a `/dev/dm-N` it resolves to is named by its
//! `/dev/mapper` name. A tag (`LABEL=root`) is evaluated the way libblkid's
//! default configuration does -- through udev's `/dev/disk/by-*` links
//! first, then the blkid cache file -- and a device's own tags are read by
//! probing it ([`crate::blkid`]), which needs read access to the device:
//! for anyone but root that fails with `EACCES`, and upstream then falls
//! back to the links.

use crate::blkid;
use crate::tab::Table;
use crate::utils;
use std::path::Path;

/// `MNT_CACHE_ISTAG`.
const ISTAG: u32 = 1 << 1;
/// `MNT_CACHE_ISPATH`.
const ISPATH: u32 = 1 << 2;
/// `MNT_CACHE_TAGREAD`.
const TAGREAD: u32 = 1 << 3;

/// `struct mnt_cache_entry`: for a path, the path and its canonical form;
/// for a tag, `(name, value)` and the device.
#[derive(Clone, Debug)]
struct Entry {
    key: Vec<u8>,
    /// A tag's value (the key is its name).
    tagval: Vec<u8>,
    value: Vec<u8>,
    flag: u32,
}

/// `struct libmnt_cache`.
#[derive(Debug, Default)]
pub struct Cache {
    ents: Vec<Entry>,
    /// `mnt_cache_set_targets`: the kernel's table, whose mount points are
    /// already canonical and so are not canonicalized again.
    pub mountinfo: Option<Table>,
    /// `cache->bc`: libblkid's cache, made the first time a tag is
    /// evaluated by scanning, and written back when this cache goes.
    pub bc: Option<crate::blkid_cache::BlkCache>,
}

/// A path from bytes.
fn path_of(b: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(quoting::os_from_bytes(b))
}

/// Bytes of a path.
fn bytes_of(p: &Path) -> Vec<u8> {
    quoting::os_bytes(p.as_os_str()).into_owned()
}

/// `is_dm_devname(canonical, &name)`: `/dev/dm-N`, a block device.
fn dm_name(canonical: &[u8]) -> Option<&[u8]> {
    let slash = canonical.iter().rposition(|&b| b == b'/')?;
    let name = canonical.get(slash.saturating_add(1)..)?;
    if !name.starts_with(b"dm-") || !name.get(3).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    if !blkid::is_block_device(&path_of(canonical)) {
        return None;
    }
    Some(name)
}

/// `canonicalize_dm_name(ptname)`: `/dev/mapper/NAME`, from
/// `/sys/block/dm-N/dm/name`, if that device exists.
pub(crate) fn canonicalize_dm_name(ptname: &[u8]) -> Option<Vec<u8>> {
    let mut sys = b"/sys/block/".to_vec();
    sys.extend_from_slice(ptname);
    sys.extend_from_slice(b"/dm/name");
    let text = std::fs::read(path_of(&sys)).ok()?;
    // `fgets(name, 256 - sizeof "/dev/mapper")`: at most 243 bytes of the
    // first line; then its last byte (the newline) off.
    let line_end = text
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text.len(), |i| i.saturating_add(1));
    let line = crate::c_str(text.get(..line_end.min(243)).unwrap_or_default());
    if line.len() <= 1 {
        return None;
    }
    let mut path = b"/dev/mapper/".to_vec();
    path.extend_from_slice(line.get(..line.len().saturating_sub(1)).unwrap_or_default());
    path.truncate(255);
    std::fs::metadata(path_of(&path)).ok().map(|_| path)
}

/// `canonicalize_path(path)`: `realpath`, or the path as it is when it does
/// not resolve; `/dev/dm-N` as its `/dev/mapper` name.
#[must_use]
pub fn canonicalize_path(path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    let Ok(canonical) = std::fs::canonicalize(path_of(path)) else {
        return Some(path.to_vec());
    };
    let canonical = bytes_of(&canonical);
    if let Some(dm) = dm_name(&canonical).and_then(canonicalize_dm_name) {
        return Some(dm);
    }
    Some(canonical)
}

/// `is_relative_path(path)` and `absolute_path(path)`: the path made
/// absolute against the working directory, `./` and a lone `.` cleaned.
#[must_use]
pub fn absolute_path(path: &[u8]) -> Option<Vec<u8>> {
    if path.first() == Some(&b'/') || path.is_empty() {
        return None;
    }
    let cwd = bytes_of(&std::env::current_dir().ok()?);
    let rest: &[u8] = if let Some(r) = path.strip_prefix(b"./") {
        r
    } else if path == b"." {
        b""
    } else {
        path
    };
    if rest.is_empty() {
        return Some(cwd);
    }
    let mut res = cwd;
    res.push(b'/');
    res.extend_from_slice(rest);
    Some(res)
}

impl Cache {
    /// `mnt_new_cache()`.
    #[must_use]
    pub fn new() -> Self {
        Cache::default()
    }

    /// `cache_find_path(cache, path)`.
    fn find_path(&self, path: &[u8]) -> Option<Vec<u8>> {
        self.ents
            .iter()
            .find(|e| e.flag & ISPATH != 0 && utils::streq_paths(Some(path), Some(&e.key)))
            .map(|e| e.value.clone())
    }

    /// `cache_find_tag(cache, token, value)`: the device with that tag.
    fn find_tag(&self, token: &[u8], value: &[u8]) -> Option<Vec<u8>> {
        self.ents
            .iter()
            .find(|e| e.flag & ISTAG != 0 && e.key == token && e.tagval == value)
            .map(|e| e.value.clone())
    }

    /// `cache_find_tag_value(cache, devname, token)`.
    fn find_tag_value_cached(&self, devname: &[u8], token: &[u8]) -> Option<Vec<u8>> {
        self.ents
            .iter()
            .find(|e| e.flag & ISTAG != 0 && e.value == devname && e.key == token)
            .map(|e| e.tagval.clone())
    }

    /// `canonicalize_path_and_cache(path, cache)`.
    fn canonicalize_and_cache(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        let p = canonicalize_path(path)?;
        self.ents.push(Entry {
            key: path.to_vec(),
            tagval: Vec::new(),
            value: p.clone(),
            flag: ISPATH,
        });
        Some(p)
    }

    /// `mnt_resolve_path(path, cache)`.
    pub fn resolve_path(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        self.find_path(path)
            .or_else(|| self.canonicalize_and_cache(path))
    }

    /// `mnt_resolve_target(path, cache)`: a mount point the kernel's table
    /// names is taken as it is.
    pub fn resolve_target(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        if self.mountinfo.is_none() {
            return self.resolve_path(path);
        }
        if let Some(p) = self.find_path(path) {
            return Some(p);
        }
        let known = self.mountinfo.as_ref().is_some_and(|mi| {
            mi.ents
                .iter()
                .rev()
                .any(|fs| fs.is_kernel() && !fs.is_swaparea() && fs.streq_target(Some(path)))
        });
        if known {
            self.ents.push(Entry {
                key: path.to_vec(),
                tagval: Vec::new(),
                value: path.to_vec(),
                flag: ISPATH,
            });
            return Some(path.to_vec());
        }
        self.canonicalize_and_cache(path)
    }

    /// `mnt_resolve_tag(token, value, cache)`: the device a tag names.
    pub fn resolve_tag(&mut self, token: &[u8], value: &[u8]) -> Option<Vec<u8>> {
        if let Some(p) = self.find_tag(token, value) {
            return Some(p);
        }
        let dev = blkid::evaluate_tag(token, value, Some(&mut self.bc))?;
        self.ents.push(Entry {
            key: token.to_vec(),
            tagval: value.to_vec(),
            value: dev.clone(),
            flag: ISTAG,
        });
        Some(dev)
    }

    /// `mnt_resolve_spec(spec, cache)`: a tag's device, or a canonical path.
    pub fn resolve_spec(&mut self, spec: &[u8]) -> Option<Vec<u8>> {
        match utils::parse_tag_string(spec) {
            Some((t, v)) if utils::valid_tagname(&t) => self.resolve_tag(&t, &v),
            _ => self.resolve_path(spec),
        }
    }

    /// `mnt_cache_read_tags(cache, devname)`: the device's LABEL, UUID,
    /// TYPE, PARTUUID and PARTLABEL, probed once. `Ok(true)` when it has
    /// any (or they were read before), `Ok(false)` when it has none.
    ///
    /// # Errors
    ///
    /// The device could not be probed -- an `errno`, `EACCES` for a user
    /// who may not read it -- or nothing (or too much) was found on it.
    pub fn read_tags(&mut self, devname: &[u8]) -> Result<bool, blkid::ProbeFail> {
        if self
            .ents
            .iter()
            .any(|e| e.flag & TAGREAD != 0 && e.value == devname)
        {
            return Ok(true);
        }
        let values = blkid::probe_tags(devname)?;
        let mut n = 0usize;
        for (tag, value) in values {
            if self.find_tag_value_cached(devname, tag).is_some() {
                continue;
            }
            self.ents.push(Entry {
                key: tag.to_vec(),
                tagval: value,
                value: devname.to_vec(),
                flag: TAGREAD | ISTAG,
            });
            n = n.saturating_add(1);
        }
        Ok(n > 0)
    }

    /// `mnt_cache_device_has_tag(cache, devname, token, value)`.
    #[must_use]
    pub fn device_has_tag(&self, devname: &[u8], token: &[u8], value: &[u8]) -> bool {
        self.find_tag(token, value).is_some_and(|p| p == devname)
    }

    /// `__mnt_cache_find_tag_value(cache, devname, token)`: the device's
    /// tag, probing it if it has not been.
    ///
    /// # Errors
    ///
    /// The probe failed ([`Cache::read_tags`]), or found no tags
    /// (`Ok(None)` is a device probed that lacks this one).
    pub fn find_tag_value_rc(
        &mut self,
        devname: &[u8],
        token: &[u8],
    ) -> Result<Option<Vec<u8>>, blkid::ProbeFail> {
        if self.read_tags(devname)? {
            Ok(self.find_tag_value_cached(devname, token))
        } else {
            Err(blkid::ProbeFail::Nothing)
        }
    }

    /// `mnt_cache_find_tag_value(cache, devname, token)`: the device's tag,
    /// probing it if it has not been.
    pub fn find_tag_value(&mut self, devname: &[u8], token: &[u8]) -> Option<Vec<u8>> {
        self.find_tag_value_rc(devname, token).ok().flatten()
    }

    /// `mnt_get_fstype(devname, &ambi, cache)`: the filesystem type probed
    /// on the device.
    ///
    /// # Errors
    ///
    /// Why there is none: [`blkid::ProbeFail::Ambivalent`] is upstream's
    /// `ambi`; an `errno` is what `errno` then holds.
    pub fn get_fstype(&mut self, devname: &[u8]) -> Result<Vec<u8>, blkid::ProbeFail> {
        self.find_tag_value_rc(devname, b"TYPE")?
            .ok_or(blkid::ProbeFail::Nothing)
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_do_not_resolve_stay() {
        assert_eq!(
            canonicalize_path(b"/nonexistent-ulmount/x"),
            Some(b"/nonexistent-ulmount/x".to_vec())
        );
        assert_eq!(canonicalize_path(b""), None);
    }

    #[test]
    fn relative_paths_are_made_absolute() {
        let cwd = bytes_of(&std::env::current_dir().unwrap_or_default());
        let mut want = cwd.clone();
        want.extend_from_slice(b"/x");
        assert_eq!(absolute_path(b"./x"), Some(want.clone()));
        assert_eq!(absolute_path(b"x"), Some(want));
        assert_eq!(absolute_path(b"."), Some(cwd));
        assert_eq!(absolute_path(b"/x"), None);
    }

    #[test]
    fn resolved_paths_are_remembered() {
        let mut c = Cache::new();
        let a = c.resolve_path(b"/nonexistent-ulmount/y");
        assert_eq!(a.as_deref(), Some(&b"/nonexistent-ulmount/y"[..]));
        assert_eq!(c.ents.len(), 1);
        let b = c.resolve_path(b"/nonexistent-ulmount//y/");
        assert_eq!(a, b);
        assert_eq!(c.ents.len(), 1);
    }
}
