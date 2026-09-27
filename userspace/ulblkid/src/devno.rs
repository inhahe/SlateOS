//! libblkid's `devno.c`: a device number to the `/dev` path that names it.

use crate::blkdev::{is_block, rdev};

/// A path from bytes.
fn path_of(b: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(quoting::os_from_bytes(b))
}

/// `blkid_strconcat(a, "/", b)`.
fn join(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut p = a.to_vec();
    p.push(b'/');
    p.extend_from_slice(b);
    p
}

/// `blkid__scan_dir(dirname, devno, list, &devname)`: the block device in
/// `dirname` with this number; with `list`, each real subdirectory (not a
/// link, not a dot-name, not `shm`) is pushed onto it.
#[must_use]
pub fn scan_dir(
    dirname: &[u8],
    devno: u64,
    mut list: Option<&mut Vec<Vec<u8>>>,
) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(path_of(dirname)).ok()?;
    for e in entries {
        let Ok(e) = e else {
            break;
        };
        let name = quoting::os_bytes(&e.file_name()).into_owned();
        let d_type = e.file_type().ok();
        if let Some(ft) = d_type {
            let blk = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::FileTypeExt;
                    ft.is_block_device()
                }
                #[cfg(not(unix))]
                {
                    false
                }
            };
            if !(blk || ft.is_symlink() || ft.is_dir()) {
                continue;
            }
        }
        let path = join(dirname, &name);
        let Ok(st) = std::fs::metadata(path_of(&path)) else {
            continue;
        };
        if is_block(&st) && rdev(&st) == devno {
            return Some(path);
        }
        let Some(l) = list.as_deref_mut() else {
            continue;
        };
        if !st.is_dir() {
            continue;
        }
        match d_type {
            Some(ft) if ft.is_symlink() => continue,
            None => {
                // DT_UNKNOWN: look without following a link.
                match std::fs::symlink_metadata(path_of(&path)) {
                    Ok(m) if m.is_dir() => {}
                    _ => continue,
                }
            }
            Some(_) => {}
        }
        let is_dir_type = d_type.is_some_and(|t| t.is_dir());
        if name.first() == Some(&b'.') || (is_dir_type && name == b"shm") {
            continue;
        }
        l.push(path);
    }
    None
}

/// `devdirs[]`, pushed in this order onto a stack -- so `/dev` is searched
/// first.
const DEVDIRS: [&[u8]; 3] = [b"/devices", b"/devfs", b"/dev"];

/// `scandev_devno_to_devpath`: a breadth-first search of `/dev` (and the
/// others) for the number; each level is searched in reverse of the order
/// its directories were found in, as upstream's stack does.
fn scandev_devno_to_devpath(devno: u64) -> Option<Vec<u8>> {
    let mut list: Vec<Vec<u8>> = DEVDIRS.iter().map(|d| d.to_vec()).collect();
    let mut new_list: Vec<Vec<u8>> = Vec::new();
    while let Some(current) = list.pop() {
        if let Some(name) = scan_dir(&current, devno, Some(&mut new_list)) {
            return Some(name);
        }
        if list.is_empty() {
            list = std::mem::take(&mut new_list);
        }
    }
    None
}

/// `blkid_devno_to_devname(devno)`: through sysfs, else the search.
#[must_use]
pub fn devno_to_devname(devno: u64) -> Option<Vec<u8>> {
    ulsysfs::devno_to_devpath(devno).or_else(|| scandev_devno_to_devpath(devno))
}

/// `blkid_devno_to_wholedisk(dev, diskname, len, &diskdevno)`.
#[must_use]
pub fn devno_to_wholedisk(devno: u64, len: usize) -> Option<(Vec<u8>, u64)> {
    ulsysfs::devno_to_wholedisk(devno, len)
}
