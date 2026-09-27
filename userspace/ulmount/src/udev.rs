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

/// What udev knows of a device, from its database: `udev_device_new_from_
/// subsystem_sysname` succeeded, whether or not udev has written anything
/// about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Device {
    /// The `E:KEY=VALUE` properties, the last of a key winning -- and one
    /// with an empty value removing the key, as systemd's
    /// `device_add_property_internal_from_string` does.
    properties: Vec<(Vec<u8>, Vec<u8>)>,
    /// `udev_device_get_devlinks_list_entry`: each `S:` link as a `/dev/`
    /// path, once each, sorted as libudev's unique lists are (`strcmp`).
    pub devlinks: Vec<Vec<u8>>,
}

impl Device {
    /// `udev_device_get_property_value(dev, key)`.
    #[must_use]
    pub fn property(&self, key: &[u8]) -> Option<&[u8]> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
    }
}

/// `udev_device_new_from_subsystem_sysname(udev, "block", sysname)` and the
/// database read behind it: `None` when there is no such device (or no
/// number for it), a [`Device`] with nothing in it when udev has no
/// database for it. The database is named for the device number, `b` for a
/// device of the `block` subsystem and `c` for any other.
#[must_use]
pub fn device(sysname: &[u8]) -> Option<Device> {
    let dir = syspath(sysname)?;
    let (major, minor) = devnum(&dir)?;
    let mut dev = Device::default();
    if major == 0 {
        return Some(dev);
    }
    let Ok(subsystem) = std::fs::read_link(dir.join("subsystem")) else {
        return Some(dev);
    };
    let kind = if subsystem.file_name().is_some_and(|n| n == "block") {
        'b'
    } else {
        'c'
    };
    let Ok(text) = std::fs::read(format!("/run/udev/data/{kind}{major}:{minor}")) else {
        return Some(dev);
    };
    parse_db(&text, &mut dev);
    Some(dev)
}

/// A udev database file's lines into `dev`: `E:KEY=VALUE` properties and
/// `S:LINK` device links.
fn parse_db(text: &[u8], dev: &mut Device) {
    for line in text.split(|&b| b == b'\n') {
        if let Some(kv) = line.strip_prefix(b"E:") {
            let Some(eq) = kv.iter().position(|&b| b == b'=') else {
                continue;
            };
            let key = kv.get(..eq).unwrap_or_default().to_vec();
            let value = kv.get(eq.saturating_add(1)..).unwrap_or_default().to_vec();
            let at = dev.properties.iter().position(|(k, _)| *k == key);
            match (at, value.is_empty()) {
                (Some(i), true) => {
                    dev.properties.remove(i);
                }
                (Some(i), false) => {
                    if let Some(slot) = dev.properties.get_mut(i) {
                        slot.1 = value;
                    }
                }
                (None, true) => {}
                (None, false) => dev.properties.push((key, value)),
            }
        } else if let Some(link) = line.strip_prefix(b"S:") {
            let mut path = b"/dev/".to_vec();
            path.extend_from_slice(link);
            dev.devlinks.push(path);
        }
    }
    dev.devlinks.sort();
    dev.devlinks.dedup();
}

/// `udev_device_get_property_value(dev, key)` for the block device named
/// `sysname` -- the device name without `/dev/`, as `findmnt` passes it.
#[must_use]
pub fn property(sysname: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    device(sysname)?.property(key).map(<[u8]>::to_vec)
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
    fn a_database_is_read_as_systemd_reads_it() {
        let mut dev = Device::default();
        let db: &[&[u8]] = &[
            b"S:disk/by-uuid/x",
            b"E:ID_FS_TYPE=ext4",
            b"E:ID_FS_LABEL=old",
            b"S:disk/by-id/b",
            b"E:ID_FS_LABEL=",
            b"E:ID_FS_TYPE=xfs",
            b"S:disk/by-id/b",
            b"I:123",
        ];
        parse_db(&db.join(&b'\n'), &mut dev);
        assert_eq!(dev.property(b"ID_FS_TYPE"), Some(&b"xfs"[..]));
        // An empty value removes the key.
        assert_eq!(dev.property(b"ID_FS_LABEL"), None);
        assert_eq!(
            dev.devlinks,
            vec![b"/dev/disk/by-id/b".to_vec(), b"/dev/disk/by-uuid/x".to_vec()]
        );
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
