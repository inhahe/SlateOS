//! libblkid's `evaluate.c`: a tag (`LABEL=root`, `UUID=...`) to the device
//! that carries it.
//!
//! The methods and their order come from `/etc/blkid.conf` (`EVALUATE=`);
//! upstream's default, and this port's, is `udev,scan`: first the tag's
//! udev link -- `/dev/disk/by-uuid/X` and its siblings, the value encoded
//! as udev encodes it -- then libblkid's cache ([`crate::cache`]), which
//! probes every device if the tag is in neither. A udev link must be a
//! block device, and is canonicalized. (Upstream can be built to verify
//! the link's tag by probing; its default build does not, and neither does
//! this.)

use crate::cache::{BlkCache, Config, Eval, cache_filename, read_config};
use crate::encode::encode_string;
use crate::probe::parse_tag_string;
use ulsysfs::canonicalize::canonicalize_path;

/// `PATH_MAX`.
const PATH_MAX: usize = 4096;

/// `S_ISBLK` of `stat(path)`.
fn is_block_device(path: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(std::path::PathBuf::from(quoting::os_from_bytes(path)))
            .is_ok_and(|m| m.file_type().is_block_device())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// `evaluate_by_udev(token, value, uevent)`: the tag's udev link, if it is
/// there and a block device, canonicalized.
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
    // A link that does not exist would send a "change" uevent -- if the path
    // had been canonicalized by then, which on this path it never is.
    if !is_block_device(&dev) {
        return None;
    }
    canonicalize_path(&dev)
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
/// each configured method in turn. With no `value`, `token` is either the
/// whole `NAME=value` or -- with no `=` in it -- a device name, returned as
/// it is. `None` for a tag that does not parse, a configuration that does
/// not, or a tag no method finds.
#[must_use]
pub fn evaluate_tag(
    token: &[u8],
    value: Option<&[u8]>,
    mut bc: Option<&mut Option<BlkCache>>,
) -> Option<Vec<u8>> {
    let (token, value) = match value {
        Some(v) => (token.to_vec(), v.to_vec()),
        None => {
            if !token.contains(&b'=') {
                return Some(token.to_vec());
            }
            parse_tag_string(token)?
        }
    };
    let conf = read_config(None)?;
    for method in &conf.evals {
        let found = match method {
            Eval::Udev => evaluate_by_udev(&token, &value),
            Eval::Scan => evaluate_by_scan(&token, &value, bc.as_deref_mut(), &conf),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// `blkid_evaluate_spec(spec, &cache)`: a `NAME=value` tag evaluated, or a
/// path canonicalized.
#[must_use]
pub fn evaluate_spec(spec: &[u8], bc: Option<&mut Option<BlkCache>>) -> Option<Vec<u8>> {
    if spec.contains(&b'=') {
        let (t, v) = parse_tag_string(spec)?;
        return evaluate_tag(&t, Some(&v), bc);
    }
    canonicalize_path(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_without_a_tag_is_itself() {
        assert_eq!(
            evaluate_tag(b"/dev/sda1", None, None),
            Some(b"/dev/sda1".to_vec())
        );
        // A tag that does not parse is nothing.
        assert_eq!(evaluate_tag(b"LABEL=\"open", None, None), None);
        assert_eq!(evaluate_tag(b"LABEL=", None, None), None);
        // A spec that is not a tag is a path.
        assert_eq!(
            evaluate_spec(b"/nonexistent-ulblkid/x", None),
            Some(b"/nonexistent-ulblkid/x".to_vec())
        );
    }

    #[test]
    fn unsupported_udev_tokens_are_nothing() {
        assert_eq!(evaluate_by_udev(b"TYPE", b"ext4"), None);
        assert_eq!(evaluate_by_udev(b"UUID", b"no-such-uuid-anywhere"), None);
    }
}
