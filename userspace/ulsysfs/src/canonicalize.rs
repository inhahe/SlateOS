//! util-linux 2.39.3's `lib/canonicalize.c`: a path made canonical the way
//! libmount and libblkid both make it -- `realpath`, with a device-mapper
//! node named by its `/dev/mapper` name -- and a relative path made
//! absolute.

use crate::{c_str, path_of};
use std::path::Path;

/// Bytes of a path.
fn bytes_of(p: &Path) -> Vec<u8> {
    quoting::os_bytes(p.as_os_str()).into_owned()
}

/// `S_ISBLK` of `stat(path)`.
fn is_block_device(path: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(path_of(path)).is_ok_and(|m| m.file_type().is_block_device())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// `is_dm_devname(canonical, &name)`: the `dm-N` of a path naming
/// `/dev/dm-N` (any directory), when it is a block device.
#[must_use]
pub fn is_dm_devname(canonical: &[u8]) -> Option<&[u8]> {
    let slash = canonical.iter().rposition(|&b| b == b'/')?;
    let name = canonical.get(slash.saturating_add(1)..)?;
    if !name.starts_with(b"dm-") || !name.get(3).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    if !is_block_device(canonical) {
        return None;
    }
    Some(name)
}

/// `canonicalize_dm_name(ptname)`: `/dev/mapper/NAME`, from
/// `/sys/block/dm-N/dm/name`, if that path exists.
#[must_use]
pub fn canonicalize_dm_name(ptname: &[u8]) -> Option<Vec<u8>> {
    canonicalize_dm_name_in(None, ptname)
}

/// `__canonicalize_dm_name(prefix, ptname)`: as [`canonicalize_dm_name`],
/// with sysfs read under `prefix` -- and then, the prefix standing for
/// another system's root, the `/dev/mapper` path is not required to exist
/// in this one. Both paths are cut, as upstream's 256-byte buffer cuts
/// them, to 255 bytes.
#[must_use]
pub fn canonicalize_dm_name_in(prefix: Option<&[u8]>, ptname: &[u8]) -> Option<Vec<u8>> {
    if ptname.is_empty() {
        return None;
    }
    let prefix = prefix.unwrap_or_default();
    let mut sys = prefix.to_vec();
    sys.extend_from_slice(b"/sys/block/");
    sys.extend_from_slice(ptname);
    sys.extend_from_slice(b"/dm/name");
    sys.truncate(255);
    let text = std::fs::read(path_of(c_str(&sys))).ok()?;
    // `fgets(name, 256 - sizeof "/dev/mapper")`: at most 243 bytes of the
    // first line; then its last byte (the newline) off.
    let line_end = text
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text.len(), |i| i.saturating_add(1));
    let line = c_str(text.get(..line_end.min(243)).unwrap_or_default());
    if line.len() <= 1 {
        return None;
    }
    let mut path = b"/dev/mapper/".to_vec();
    path.extend_from_slice(line.get(..line.len().saturating_sub(1)).unwrap_or_default());
    path.truncate(255);
    if !prefix.is_empty() {
        return Some(path);
    }
    std::fs::metadata(path_of(&path)).ok().map(|_| path)
}

/// `canonicalize_path(path)`: `realpath`, or the path as it is when it does
/// not resolve; `/dev/dm-N` as its `/dev/mapper` name. `None` for an empty
/// path.
#[must_use]
pub fn canonicalize_path(path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    let Ok(canonical) = std::fs::canonicalize(path_of(path)) else {
        return Some(path.to_vec());
    };
    let canonical = bytes_of(&canonical);
    if let Some(dm) = is_dm_devname(&canonical).and_then(canonicalize_dm_name) {
        return Some(dm);
    }
    Some(canonical)
}

/// `is_relative_path(path)` and `absolute_path(path)`: the path made
/// absolute against the working directory, `./` and a lone `.` cleaned.
/// `None` for an absolute or empty path, or when the working directory
/// cannot be read.
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests unwrap what they built")]
mod tests {
    use super::*;

    #[test]
    fn paths_that_do_not_resolve_stay() {
        assert_eq!(
            canonicalize_path(b"/nonexistent-ulsysfs/x"),
            Some(b"/nonexistent-ulsysfs/x".to_vec())
        );
        assert_eq!(canonicalize_path(b""), None);
    }

    #[test]
    fn relative_paths_are_made_absolute() {
        let cwd = bytes_of(&std::env::current_dir().unwrap());
        let mut want = cwd.clone();
        want.extend_from_slice(b"/x");
        assert_eq!(absolute_path(b"./x"), Some(want.clone()));
        assert_eq!(absolute_path(b"x"), Some(want));
        assert_eq!(absolute_path(b"."), Some(cwd));
        assert_eq!(absolute_path(b"/x"), None);
    }

    #[test]
    fn only_block_dm_nodes_are_dm_names() {
        assert_eq!(is_dm_devname(b"/nonexistent/dm-0"), None);
        assert_eq!(is_dm_devname(b"/dev/dmx"), None);
        assert_eq!(is_dm_devname(b"/dev/dm-"), None);
    }
}
