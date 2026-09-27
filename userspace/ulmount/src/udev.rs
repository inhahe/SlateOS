//! The part of libudev `findmnt` reads a block device's LABEL and UUID
//! with, when libblkid may not probe the device: udev's database.
//!
//! `udev_device_new_from_subsystem_sysname(udev, "block", name)` is
//! systemd 255's `sd_device_new_from_subsystem_sysname`: the name must be a
//! normalized path (no `.` or `..` component, no `//`), a `/` in it is
//! spelled `!` as sysfs spells it, and the device is the first of
//! `/sys/bus/block/devices/NAME`, `/sys/class/block/NAME` and
//! `/sys/firmware/block/NAME` that exists (following links) and is a
//! directory -- one under `/sys/devices/` must also have a `uevent` file.
//! Its `MAJOR`/`MINOR` come from that `uevent` file, and udev keeps what it
//! learned about the device in `/run/udev/data/bMAJOR:MINOR` as
//! `E:KEY=VALUE` lines, the last of a key winning. The database is readable
//! by everyone, which is how an ordinary user's `findmnt` shows UUIDs it
//! could not probe; where udev is not running it is not there, and nothing
//! is found, as with upstream.

use std::path::PathBuf;

/// A path from bytes.
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(b))
}

/// systemd's `path_is_normalized(p)`: a valid path (not empty, shorter
/// than `PATH_MAX`, no NUL) with no `.` or `..` component and no `//`.
fn path_is_normalized(p: &[u8]) -> bool {
    if p.is_empty() || p.len() >= 4096 || p.contains(&0) {
        return false;
    }
    if p.windows(2).any(|w| w == b"//") {
        return false;
    }
    !p.split(|&b| b == b'/').any(|c| c == b"." || c == b"..")
}

/// The device's sysfs directory, as `sd_device_new_from_subsystem_sysname`
/// finds it for the `block` subsystem.
fn syspath(sysname: &[u8]) -> Option<PathBuf> {
    if !path_is_normalized(sysname) {
        return None;
    }
    let name: Vec<u8> = sysname
        .iter()
        .map(|&b| if b == b'/' { b'!' } else { b })
        .collect();
    for base in [
        &b"/sys/bus/block/devices/"[..],
        b"/sys/class/block/",
        b"/sys/firmware/block/",
    ] {
        let mut p = base.to_vec();
        p.extend_from_slice(&name);
        let path = path_of(&p);
        // `access(p, F_OK)`: follows links; a missing one is the next place.
        if std::fs::metadata(&path).is_err() {
            continue;
        }
        // `sd_device_new_from_syspath` with verification: the resolved
        // directory, which must be under `/sys` (or where a linked `/sys`
        // leads, spelled `/sys`) and, for a real device, have a `uevent`
        // file.
        let real = std::fs::canonicalize(&path).ok()?;
        let mut real_b = quoting::os_bytes(real.as_os_str()).into_owned();
        if !real_b.starts_with(b"/sys") {
            let sys = std::fs::canonicalize("/sys").ok()?;
            let sys = quoting::os_bytes(sys.as_os_str()).into_owned();
            let rest = real_b.strip_prefix(sys.as_slice())?.to_vec();
            real_b = b"/sys".to_vec();
            real_b.extend_from_slice(&rest);
        }
        let real = path_of(&real_b);
        if real_b.starts_with(b"/sys/devices/") {
            if std::fs::metadata(real.join("uevent")).is_err() {
                return None;
            }
        } else if !std::fs::metadata(&real).is_ok_and(|m| m.is_dir()) {
            return None;
        }
        return Some(real);
    }
    None
}

/// `MAJOR=` and `MINOR=` from the device's `uevent` file.
fn devnum(dir: &std::path::Path) -> Option<(u32, u32)> {
    let text = std::fs::read(dir.join("uevent")).ok()?;
    let mut major = None;
    let mut minor = None;
    for line in text.split(|&b| b == b'\n') {
        let num = |v: &[u8]| {
            std::str::from_utf8(v)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
        };
        if let Some(v) = line.strip_prefix(b"MAJOR=") {
            major = num(v);
        } else if let Some(v) = line.strip_prefix(b"MINOR=") {
            minor = num(v);
        }
    }
    Some((major?, minor?))
}

/// The device's udev database, parsed: its `E:` properties, the last of a
/// key winning. The database is named for the device number, `b` for a
/// device of the `block` subsystem and `c` for any other.
fn properties(sysname: &[u8]) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    let dir = syspath(sysname)?;
    let (major, minor) = devnum(&dir)?;
    if major == 0 {
        return None;
    }
    let subsystem = std::fs::read_link(dir.join("subsystem")).ok()?;
    let kind = if subsystem.file_name().is_some_and(|n| n == "block") {
        'b'
    } else {
        'c'
    };
    let text = std::fs::read(format!("/run/udev/data/{kind}{major}:{minor}")).ok()?;
    let mut props: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for line in text.split(|&b| b == b'\n') {
        let Some(kv) = line.strip_prefix(b"E:") else {
            continue;
        };
        let Some(eq) = kv.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = kv.get(..eq).unwrap_or_default().to_vec();
        let value = kv.get(eq.saturating_add(1)..).unwrap_or_default().to_vec();
        match props.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => props.push((key, value)),
        }
    }
    Some(props)
}

/// `udev_device_get_property_value(dev, key)` for the block device named
/// `sysname` -- the device name without `/dev/`, as `findmnt` passes it.
#[must_use]
pub fn property(sysname: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    properties(sysname)?
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
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
    fn names_must_be_normalized() {
        assert!(path_is_normalized(b"sda1"));
        assert!(path_is_normalized(b"cciss/c0d0"));
        assert!(!path_is_normalized(b""));
        assert!(!path_is_normalized(b"../sda"));
        assert!(!path_is_normalized(b"a//b"));
        assert!(!path_is_normalized(b"a/./b"));
    }

    #[test]
    fn a_device_that_is_not_there_has_no_properties() {
        assert_eq!(
            property(b"nonexistent-ulmount-device", b"ID_FS_UUID_ENC"),
            None
        );
        assert_eq!(property(b"..", b"ID_FS_UUID_ENC"), None);
    }
}
