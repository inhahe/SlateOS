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

/// `lib/canonicalize.c`, which libmount's cache is where callers reach it.
pub use ulsysfs::canonicalize::{absolute_path, canonicalize_path};

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
    pub bc: Option<ulblkid::cache::BlkCache>,
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
        let dev = ulblkid::evaluate::evaluate_tag(token, Some(value), Some(&mut self.bc))?;
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
