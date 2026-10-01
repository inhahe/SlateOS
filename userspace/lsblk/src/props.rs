//! `lsblk-properties.c`: what is known of a device's contents -- its
//! filesystem, partition table and partition entry, and the disk's own
//! identity -- from one of three places:
//!
//! * with `--sysroot`, a text file standing in for the device node (the
//!   `udev` properties of the machine the snapshot was taken on, and
//!   lsblk's own MODE, OWNER and GROUP);
//! * otherwise udev's database, which every user may read;
//! * and, when udev knows nothing of the device, libblkid probing it,
//!   which needs root.
//!
//! # Where udev is
//!
//! Upstream is built with libudev, which finds a device in sysfs whether or
//! not udev is running -- and a device found that way, with nothing in its
//! database, has properties, all empty, so libblkid is never asked. On a
//! system without udev that leaves `lsblk -f` blank, which is why util-linux
//! built without libudev asks libblkid instead. SlateOS has no udev, and the
//! reference this is measured against has one; so udev is asked only where
//! it runs -- where `/run/udev/data` is a directory -- which is upstream's
//! behaviour with libudev on a system running udev, and without it on one
//! that is not (todo.txt, Judgment Calls; design-decisions §1041).

use crate::devtree::{DevId, Devtree};
use crate::parttypes::{GPT_TYPES, MBR_TYPES};

/// `struct lsblk_devprop`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DevProp {
    pub(crate) fstype: Option<Vec<u8>>,
    pub(crate) fsversion: Option<Vec<u8>>,
    pub(crate) uuid: Option<Vec<u8>>,
    pub(crate) ptuuid: Option<Vec<u8>>,
    pub(crate) pttype: Option<Vec<u8>>,
    pub(crate) label: Option<Vec<u8>>,
    pub(crate) parttype: Option<Vec<u8>>,
    pub(crate) partuuid: Option<Vec<u8>>,
    pub(crate) partlabel: Option<Vec<u8>>,
    pub(crate) partflags: Option<Vec<u8>>,
    pub(crate) partn: Option<Vec<u8>>,
    pub(crate) wwn: Option<Vec<u8>>,
    pub(crate) serial: Option<Vec<u8>>,
    pub(crate) model: Option<Vec<u8>>,
    pub(crate) idlink: Option<Vec<u8>>,
    pub(crate) revision: Option<Vec<u8>>,
    pub(crate) owner: Option<Vec<u8>>,
    pub(crate) group: Option<Vec<u8>>,
    pub(crate) mode: Option<Vec<u8>>,
}

/// `LSBLK_UDEV_BYID_PREFIX`.
const BYID_PREFIX: &[u8] = b"/dev/disk/by-id/";

/// `normalize_whitespace(str)`: leading white space dropped, each run of it
/// cut to its first byte, and a trailing one dropped.
pub(crate) fn normalize_whitespace(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let (mut nsp, mut intext) = (0usize, false);
    for &c in s {
        if ulstrutils::c_isspace(c) {
            nsp = nsp.saturating_add(1);
        } else {
            nsp = 0;
            intext = true;
        }
        if nsp > 1 || (nsp > 0 && !intext) {
            continue;
        }
        out.push(c);
    }
    if nsp > 0 && !out.is_empty() {
        out.pop();
    }
    out
}

/// Whether udev runs here: see the module's documentation.
fn udev_is_running() -> bool {
    std::fs::metadata("/run/udev/data").is_ok_and(|m| m.is_dir())
}

/// `get_properties_by_udev(ld)`: the device's properties from udev's
/// database -- none if udev has no device of that name.
fn get_properties_by_udev(tr: &mut Devtree, id: DevId) -> bool {
    if tr.dev(id).udev_requested {
        return tr.dev(id).properties.is_some();
    }
    tr.dev_mut(id).udev_requested = true;
    if !udev_is_running() {
        return tr.dev(id).properties.is_some();
    }
    let Some(dev) = ulmount::udev::device(&tr.dev(id).name) else {
        return tr.dev(id).properties.is_some();
    };
    let get = |key: &[u8]| dev.property(key).map(<[u8]>::to_vec);
    let unhex = |v: Vec<u8>| ulmount::mangle::unhexmangle(&v);
    let mut prop = DevProp {
        label: get(b"ID_FS_LABEL_ENC").map(unhex),
        uuid: get(b"ID_FS_UUID_ENC").map(unhex),
        ptuuid: get(b"ID_PART_TABLE_UUID"),
        pttype: get(b"ID_PART_TABLE_TYPE"),
        partlabel: get(b"ID_PART_ENTRY_NAME").map(unhex),
        fstype: get(b"ID_FS_TYPE"),
        fsversion: get(b"ID_FS_VERSION"),
        parttype: get(b"ID_PART_ENTRY_TYPE"),
        partuuid: get(b"ID_PART_ENTRY_UUID"),
        partn: get(b"ID_PART_ENTRY_NUMBER"),
        partflags: get(b"ID_PART_ENTRY_FLAGS"),
        wwn: get(b"ID_WWN_WITH_EXTENSION").or_else(|| get(b"ID_WWN")),
        // sg3_utils's own name for it has no ID_ prefix.
        serial: get(b"SCSI_IDENT_SERIAL")
            .or_else(|| get(b"ID_SCSI_SERIAL"))
            .or_else(|| get(b"ID_SERIAL_SHORT"))
            .or_else(|| get(b"ID_SERIAL"))
            .map(|s| normalize_whitespace(&s)),
        revision: get(b"ID_REVISION"),
        model: match get(b"ID_MODEL_ENC") {
            Some(m) => Some(normalize_whitespace(&unhex(m))),
            None => get(b"ID_MODEL").map(|m| normalize_whitespace(&m)),
        },
        ..DevProp::default()
    };
    // The shortest by-id link, the first of that length in libudev's
    // (sorted) order.
    let mut len = 0usize;
    for name in &dev.devlinks {
        let Some(rest) = name.strip_prefix(BYID_PREFIX) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        if len == 0 || rest.len() < len {
            len = rest.len();
            prop.idlink = Some(rest.to_vec());
        }
    }
    tr.dev_mut(id).properties = Some(prop);
    true
}

/// `lookup(buf, pattern, &value)`: `PATTERN=value` fills an empty slot
/// with the value, up to the newline; a slot already filled, another key,
/// or an empty value leaves it.
fn lookup(line: &[u8], pattern: &[u8], value: &mut Option<Vec<u8>>) -> bool {
    if value.is_some() {
        return false;
    }
    let Some(rest) = line.strip_prefix(pattern) else {
        return false;
    };
    let Some(v) = rest.strip_prefix(b"=") else {
        return false;
    };
    let v = ulsysfs::c_str(v);
    let end = v.iter().position(|&b| b == b'\n').unwrap_or(v.len());
    if end == 0 {
        return false;
    }
    *value = Some(v.get(..end).unwrap_or_default().to_vec());
    true
}

/// `fgets(buf, BUFSIZ, fp)` over a file's bytes: its lines, each through
/// its newline or cut at 8191 bytes.
fn fgets_lines(text: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let line_end = rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |i| i.saturating_add(1));
        let take = line_end.min(8191);
        let (line, tail) = rest.split_at(take);
        out.push(line);
        rest = tail;
    }
    out
}

/// `get_properties_by_file(ld)`: with `--sysroot`, the properties the
/// snapshot's stand-in for the device node holds.
fn get_properties_by_file(tr: &mut Devtree, id: DevId, sysroot: &[u8]) -> bool {
    if tr.dev(id).file_requested {
        return tr.dev(id).properties.is_some();
    }
    tr.dev_mut(id).file_requested = true;
    // Upstream frees them first whenever there is a file name, which there
    // always is.
    tr.dev_mut(id).properties = None;
    let Some(filename) = tr.dev(id).filename.clone() else {
        return false;
    };
    let mut pc = ulsysfs::PathCxt::new(Some(b"/"));
    pc.set_prefix(Some(sysroot));
    if !pc.stat(&filename).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let Ok(mut f) = pc.open(&filename) else {
        return false;
    };
    let mut text = Vec::new();
    if std::io::Read::read_to_end(&mut f, &mut text).is_err() {
        // What was read before the error stays, as `fgets` would have
        // handed it out.
    }
    let mut p = DevProp::default();
    let unhex = |v: &mut Option<Vec<u8>>| {
        if let Some(s) = v.as_mut() {
            *s = ulmount::mangle::unhexmangle(s);
        }
    };
    for buf in fgets_lines(&text) {
        if lookup(buf, b"ID_FS_LABEL_ENC", &mut p.label) {
            unhex(&mut p.label);
        } else if lookup(buf, b"ID_FS_UUID_ENC", &mut p.uuid) {
            unhex(&mut p.uuid);
        } else if lookup(buf, b"ID_PART_ENTRY_NAME", &mut p.partlabel) {
            unhex(&mut p.partlabel);
        } else {
            // The rest are taken as they are, the first key that matches
            // and whose slot is empty winning.
            let _ = lookup(buf, b"ID_PART_TABLE_UUID", &mut p.ptuuid)
                || lookup(buf, b"ID_PART_TABLE_TYPE", &mut p.pttype)
                || lookup(buf, b"ID_FS_TYPE", &mut p.fstype)
                || lookup(buf, b"ID_FS_VERSION", &mut p.fsversion)
                || lookup(buf, b"ID_PART_ENTRY_TYPE", &mut p.parttype)
                || lookup(buf, b"ID_PART_ENTRY_UUID", &mut p.partuuid)
                || lookup(buf, b"ID_PART_ENTRY_FLAGS", &mut p.partflags)
                || lookup(buf, b"ID_PART_ENTRY_NUMBER", &mut p.partn)
                || lookup(buf, b"ID_MODEL", &mut p.model)
                || lookup(buf, b"ID_WWN_WITH_EXTENSION", &mut p.wwn)
                || lookup(buf, b"ID_WWN", &mut p.wwn)
                || lookup(buf, b"SCSI_IDENT_SERIAL", &mut p.serial)
                || lookup(buf, b"ID_SCSI_SERIAL", &mut p.serial)
                || lookup(buf, b"ID_SERIAL_SHORT", &mut p.serial)
                || lookup(buf, b"ID_SERIAL", &mut p.serial)
                || lookup(buf, b"ID_REVISION", &mut p.revision)
                || lookup(buf, b"MODE", &mut p.mode)
                || lookup(buf, b"OWNER", &mut p.owner)
                || lookup(buf, b"GROUP", &mut p.group);
        }
    }
    tr.dev_mut(id).properties = Some(p);
    true
}

/// `get_properties_by_blkid(dev)`: probed by libblkid, for root only (no
/// one else may read the device), and only a device with a size.
fn get_properties_by_blkid(tr: &mut Devtree, id: DevId) -> bool {
    if tr.dev(id).blkid_requested {
        return tr.dev(id).properties.is_some();
    }
    tr.dev_mut(id).blkid_requested = true;
    if tr.dev(id).size == 0 || crate::sys::getuid() != 0 {
        return tr.dev(id).properties.is_some();
    }
    let Some(filename) = tr.dev(id).filename.clone() else {
        return tr.dev(id).properties.is_some();
    };
    let Ok(mut pr) = ulblkid::Probe::from_filename(&filename) else {
        return tr.dev(id).properties.is_some();
    };
    pr.enable_superblocks(true);
    pr.set_superblocks_flags(ulblkid::SUBLKS_LABEL | ulblkid::SUBLKS_UUID | ulblkid::SUBLKS_TYPE);
    pr.enable_partitions(true);
    pr.set_partitions_flags(ulblkid::PARTS_ENTRY_DETAILS);
    if pr.do_safeprobe() == 0 {
        let get = |name: &str| pr.lookup_value(name).map(|v| v.as_c_str().to_vec());
        let prop = DevProp {
            fstype: get("TYPE"),
            uuid: get("UUID"),
            ptuuid: get("PTUUID"),
            pttype: get("PTTYPE"),
            label: get("LABEL"),
            fsversion: get("VERSION"),
            parttype: get("PART_ENTRY_TYPE"),
            partuuid: get("PART_ENTRY_UUID"),
            partlabel: get("PART_ENTRY_NAME"),
            partflags: get("PART_ENTRY_FLAGS"),
            partn: get("PART_ENTRY_NUMBER"),
            ..DevProp::default()
        };
        tr.dev_mut(id).properties = Some(prop);
    }
    tr.dev(id).properties.is_some()
}

/// `lsblk_device_get_properties(dev)`: from the snapshot with `--sysroot`;
/// otherwise from udev, else from libblkid.
pub(crate) fn get_properties<'t>(
    tr: &'t mut Devtree,
    id: DevId,
    sysroot: Option<&[u8]>,
) -> Option<&'t DevProp> {
    let found = match sysroot {
        Some(root) => get_properties_by_file(tr, id, root),
        None => get_properties_by_udev(tr, id) || get_properties_by_blkid(tr, id),
    };
    if found {
        tr.dev(id).properties.as_ref()
    } else {
        None
    }
}

/// `lsblk_parttype_code_to_string(code, pttype)`: a DOS type byte's (read
/// as `strtol` reads it, in hex) or a GPT type GUID's (in any case) name.
pub(crate) fn parttype_code_to_string(code: &[u8], pttype: &[u8]) -> Option<&'static str> {
    if pttype == b"dos" || pttype == b"mbr" {
        // `strtol(code, &end, 16)`, refused on ERANGE or anything after it;
        // the long cut to the unsigned int it is compared as.
        let xcode: u32 = match ulstrutils::scan_integer(code, 16) {
            None => {
                if !code.is_empty() {
                    return None;
                }
                0
            }
            Some(sc) => {
                if sc.end != code.len()
                    || sc.saturated
                    || sc.magnitude
                        > u128::from(i64::MAX.unsigned_abs())
                            .saturating_add(u128::from(sc.negative))
                {
                    return None;
                }
                let m = u64::try_from(sc.magnitude).unwrap_or(u64::MAX);
                let v = if sc.negative { m.wrapping_neg() } else { m };
                // `unsigned int xcode = strtol(...)`: the low 32 bits.
                v as u32
            }
        };
        MBR_TYPES
            .iter()
            .find(|&&(c, name)| name.is_some() && c == xcode)
            .and_then(|&(_, name)| name)
    } else if pttype == b"gpt" {
        GPT_TYPES
            .iter()
            .find(|(guid, _)| guid.as_bytes().eq_ignore_ascii_case(code))
            .map(|&(_, name)| name)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "tests index what they built")]
mod tests {
    use super::*;

    #[test]
    fn white_space_is_normalized_as_upstream_does() {
        assert_eq!(normalize_whitespace(b"  a  b\tc  "), b"a b\tc");
        assert_eq!(normalize_whitespace(b"a\t\tb"), b"a\tb");
        assert_eq!(normalize_whitespace(b"   "), b"");
        assert_eq!(normalize_whitespace(b"x "), b"x");
    }

    #[test]
    fn a_property_line_fills_only_an_empty_slot() {
        let mut v = None;
        assert!(!lookup(b"ID_WWN_WITH_EXTENSION=0x1\n", b"ID_WWN", &mut v));
        assert!(!lookup(b"ID_WWN=\n", b"ID_WWN", &mut v));
        assert!(lookup(b"ID_WWN=0x5\n", b"ID_WWN", &mut v));
        assert_eq!(v.as_deref(), Some(&b"0x5"[..]));
        assert!(!lookup(b"ID_WWN=0x6\n", b"ID_WWN", &mut v));
        assert_eq!(v.as_deref(), Some(&b"0x5"[..]));
    }

    #[test]
    fn partition_types_are_named_by_table() {
        assert_eq!(parttype_code_to_string(b"0x83", b"dos"), Some("Linux"));
        assert_eq!(parttype_code_to_string(b"83", b"dos"), Some("Linux"));
        assert_eq!(parttype_code_to_string(b"0x83x", b"dos"), None);
        assert_eq!(
            parttype_code_to_string(b"0fc63daf-8483-4772-8e79-3d69d8477de4", b"gpt"),
            Some("Linux filesystem")
        );
        assert_eq!(parttype_code_to_string(b"0x83", b"gpt"), None);
        assert_eq!(parttype_code_to_string(b"0x83", b"sun"), None);
        // `strtol("")` converts nothing, ends at the end, and is 0.
        assert_eq!(parttype_code_to_string(b"", b"dos"), Some("Empty"));
    }

    #[test]
    fn stand_in_file_lines_are_fgets_lines() {
        assert_eq!(fgets_lines(b"a\nb"), vec![&b"a\n"[..], b"b"]);
        let long = vec![b'x'; 9000];
        let lines = fgets_lines(&long);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 8191);
    }
}
