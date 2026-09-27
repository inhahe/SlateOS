#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests index and unwrap what they built"
)]

use crate::path::{scan_int, scan_long, scan_majmin, scan_ulong};
use crate::{
    PathCxt, devname_dev_to_sys, devname_sys_to_dev, majmin, major, makedev, minor,
    stripoff_last_component,
};

#[test]
fn device_numbers_are_glibcs() {
    let d = makedev(8, 1);
    assert_eq!(d, 0x801);
    assert_eq!((major(d), minor(d)), (8, 1));
    let big = makedev(0x1234, 0x56789);
    assert_eq!((major(big), minor(big)), (0x1234, 0x56789));
    assert_eq!(majmin(makedev(259, 3)), "259:3");
    // `%d` of an unsigned int past INT_MAX prints negative, as C does.
    assert_eq!(majmin(makedev(0xffff_ffff, 0)), "-1:0");
}

#[test]
fn numbers_scan_as_fscanf_does() {
    assert_eq!(scan_majmin(b"8:1\n"), Some(makedev(8, 1)));
    // `%d` skips white space, the literal `:` does not.
    assert_eq!(scan_majmin(b"  8: 1"), Some(makedev(8, 1)));
    assert_eq!(scan_majmin(b"8 :1"), None);
    assert_eq!(scan_majmin(b"8"), None);
    assert_eq!(scan_majmin(b""), None);
    assert_eq!(scan_ulong(b"1024\n", &mut 0), Some(1024));
    // strtoul's `-`: modulo 2^64.
    assert_eq!(scan_ulong(b"-1", &mut 0), Some(u64::MAX));
    assert_eq!(
        scan_ulong(b"99999999999999999999999", &mut 0),
        Some(u64::MAX)
    );
    assert_eq!(scan_ulong(b"x1", &mut 0), None);
    assert_eq!(scan_long(b"-9223372036854775809", &mut 0), Some(i64::MIN));
    assert_eq!(scan_long(b"-9223372036854775808", &mut 0), Some(i64::MIN));
    assert_eq!(scan_long(b"9223372036854775808", &mut 0), Some(i64::MAX));
    // `%d` stores the long in an int: truncated.
    assert_eq!(scan_int(b"4294967297", &mut 0), Some(1));
    let mut pos = 0;
    assert_eq!(scan_int(b" 12 34", &mut pos), Some(12));
    assert_eq!(pos, 3);
}

#[test]
fn names_change_their_slashes() {
    assert_eq!(devname_sys_to_dev(b"cciss!c0d0"), b"cciss/c0d0");
    assert_eq!(devname_dev_to_sys(b"cciss/c0d0"), b"cciss!c0d0");
    let mut p = b"../../block/sda/sda1".to_vec();
    assert_eq!(stripoff_last_component(&mut p), Some(b"sda1".to_vec()));
    assert_eq!(stripoff_last_component(&mut p), Some(b"sda".to_vec()));
    assert_eq!(p, b"../../block");
    let mut none = b"sda".to_vec();
    assert_eq!(stripoff_last_component(&mut none), None);
    assert_eq!(none, b"sda");
}

/// A scratch directory as a path context.
fn scratch(tag: &str) -> (scratchdir::ScratchDir, PathCxt) {
    let dir = scratchdir::ScratchDir::new(tag);
    let root = quoting::os_bytes(dir.path("").as_os_str()).into_owned();
    let root = root.strip_suffix(b"/").unwrap_or(&root).to_vec();
    let root = root.strip_suffix(b"\\").unwrap_or(&root).to_vec();
    (dir, PathCxt::new(Some(&root)))
}

#[test]
fn strings_read_as_ul_path_read_string() {
    let (dir, pc) = scratch("ulsysfs-str");
    std::fs::write(dir.path("a"), b"value\n").unwrap();
    std::fs::write(dir.path("b"), b"").unwrap();
    std::fs::write(dir.path("c"), b"\n").unwrap();
    std::fs::write(dir.path("d"), b"ab\0cd\n").unwrap();
    std::fs::write(dir.path("e"), b"two\n\n").unwrap();
    assert_eq!(pc.read_string(b"a"), Ok(Some(b"value".to_vec())));
    assert_eq!(pc.read_string(b"/a"), Ok(Some(b"value".to_vec())));
    assert_eq!(pc.read_string(b"b"), Ok(None));
    assert_eq!(pc.read_string(b"c"), Ok(None));
    assert_eq!(pc.read_string(b"d"), Ok(Some(b"ab".to_vec())));
    // One newline off, not all of them.
    assert_eq!(pc.read_string(b"e"), Ok(Some(b"two\n".to_vec())));
    assert_eq!(pc.read_string(b"missing"), Err(crate::ENOENT));
    // `ul_path_read_buffer` without a newline loses the last byte read.
    std::fs::write(dir.path("f"), b"4096").unwrap();
    assert_eq!(pc.read_buffer(64, b"f"), Ok((4, b"409".to_vec())));
    assert_eq!(pc.read_buffer(64, b"a"), Ok((5, b"value".to_vec())));
    std::fs::write(dir.path("n"), b" 512\n").unwrap();
    assert_eq!(pc.read_u64(b"n"), Some(512));
    assert_eq!(pc.read_s32(b"n"), Some(512));
    assert_eq!(pc.read_u64(b"b"), None);
    assert!(pc.access(crate::F_OK, b"a").is_ok());
    assert_eq!(pc.access(crate::F_OK, b"zz"), Err(crate::ENOENT));
    assert_eq!(pc.count_dirents(None), 7);
}

#[test]
fn a_prefix_goes_in_front_of_the_directory() {
    let mut pc = PathCxt::new(Some(b"/sys/dev/block/8:1"));
    assert_eq!(pc.absdir(), Ok(b"/sys/dev/block/8:1".to_vec()));
    pc.set_prefix(Some(b"/tmp/fake"));
    assert_eq!(pc.absdir(), Ok(b"/tmp/fake/sys/dev/block/8:1".to_vec()));
    let bare = {
        let mut p = PathCxt::new(None);
        p.set_prefix(Some(b"/tmp/fake"));
        p
    };
    assert_eq!(bare.absdir(), Ok(b"/tmp/fake".to_vec()));
    let mut long = PathCxt::new(Some(&[b'a'; 5000]));
    long.set_prefix(Some(b"/p"));
    assert_eq!(long.absdir(), Err(crate::ENAMETOOLONG));
}

/// A fake sysfs: `sda` with partition `sda1`, and `dm-0`, a partition
/// device-mapper made of `sdb`.
#[cfg(unix)]
#[test]
fn a_partition_reads_its_disks_attributes_and_finds_its_disk() {
    use std::os::unix::fs::symlink;
    let dir = scratchdir::ScratchDir::new("ulsysfs-tree");
    let root = dir.path("");
    let w = |p: &str, text: &str| {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    };
    w("sys/devices/virtual/block/sda/dev", "8:0\n");
    w(
        "sys/devices/virtual/block/sda/queue/logical_block_size",
        "512\n",
    );
    w("sys/devices/virtual/block/sda/sda1/dev", "8:1\n");
    w("sys/devices/virtual/block/sda/sda1/partition", "1\n");
    w("sys/devices/virtual/block/sda/sda1/start", "2048\n");
    w("sys/devices/virtual/block/sdb/dev", "8:16\n");
    w("sys/devices/virtual/block/dm-0/dev", "253:0\n");
    w("sys/devices/virtual/block/dm-0/dm/uuid", "part1-LVM-abc\n");
    std::fs::create_dir_all(root.join("sys/devices/virtual/block/dm-0/slaves/sdb")).unwrap();
    std::fs::create_dir_all(root.join("sys/dev/block")).unwrap();
    std::fs::create_dir_all(root.join("sys/block")).unwrap();
    symlink(
        "../../devices/virtual/block/sda",
        root.join("sys/dev/block/8:0"),
    )
    .unwrap();
    symlink(
        "../../devices/virtual/block/sda/sda1",
        root.join("sys/dev/block/8:1"),
    )
    .unwrap();
    symlink(
        "../../devices/virtual/block/sdb",
        root.join("sys/dev/block/8:16"),
    )
    .unwrap();
    symlink(
        "../../devices/virtual/block/dm-0",
        root.join("sys/dev/block/253:0"),
    )
    .unwrap();
    symlink("../devices/virtual/block/sda", root.join("sys/block/sda")).unwrap();
    symlink("../devices/virtual/block/sdb", root.join("sys/block/sdb")).unwrap();
    symlink("../devices/virtual/block/dm-0", root.join("sys/block/dm-0")).unwrap();
    let prefix = quoting::os_bytes(root.as_os_str()).into_owned();
    let prefix = prefix.strip_suffix(b"/").unwrap_or(&prefix).to_vec();

    let disk = std::rc::Rc::new(crate::new_sysfs_path(makedev(8, 0), None, Some(&prefix)).unwrap());
    let part = crate::new_sysfs_path(makedev(8, 1), Some(disk.clone()), Some(&prefix)).unwrap();
    assert_eq!(part.blkdev_name(64), Some(b"sda1".to_vec()));
    assert_eq!(part.blkdev_name(4), None);
    assert_eq!(part.read_u64(b"start"), Some(2048));
    // Not in the partition's directory: read from the disk's.
    assert_eq!(part.read_u64(b"queue/logical_block_size"), Some(512));
    assert_eq!(
        part.blkdev_wholedisk(32),
        Some((b"sda".to_vec(), makedev(8, 0)))
    );
    assert_eq!(
        part.blkdev_wholedisk(3),
        Some((b"sd".to_vec(), makedev(8, 0)))
    );
    assert_eq!(
        disk.blkdev_wholedisk(32),
        Some((b"sda".to_vec(), makedev(8, 0)))
    );
    assert_eq!(disk.blkdev_count_partitions(Some(b"sda")), 1);
    assert_eq!(disk.blkdev_partno_to_devno(1), makedev(8, 1));
    assert_eq!(disk.blkdev_partno_to_devno(2), 0);
    let dm = crate::new_sysfs_path(makedev(253, 0), None, Some(&prefix)).unwrap();
    assert_eq!(dm.blkdev_slave(), Some(b"sdb".to_vec()));
    assert_eq!(
        dm.blkdev_wholedisk(32),
        Some((b"sdb".to_vec(), makedev(8, 16)))
    );
    assert_eq!(
        crate::devname_to_devno_in(Some(&prefix), b"sda", None),
        makedev(8, 0)
    );
    assert_eq!(
        crate::devname_to_devno_in(Some(&prefix), b"sda1", Some(b"sda")),
        makedev(8, 1)
    );
    assert_eq!(
        crate::devname_to_devno_in(Some(&prefix), b"nosuch", None),
        0
    );
    assert!(crate::new_sysfs_path(makedev(9, 9), None, Some(&prefix)).is_none());
}
