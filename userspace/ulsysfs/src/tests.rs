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

/// A fake sysfs holding one SATA disk, `sda` on SCSI host 0 behind a PCI
/// controller, and a USB stick, `sdb`, whose USB device says it is
/// removable: the subsystems each hangs from, its SCSI address and host,
/// and whether it can be unplugged.
#[cfg(unix)]
#[test]
fn a_disks_subsystems_and_scsi_host_are_found_up_its_chain() {
    use std::os::unix::fs::symlink;
    let dir = scratchdir::ScratchDir::new("ulsysfs-chain");
    let root = dir.path("");
    let w = |p: &str, text: &str| {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    };
    let l = |target: &str, p: &str| {
        let full = root.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        symlink(target, full).unwrap();
    };
    let pci = "sys/devices/pci0000:00/0000:00:1f.2";
    let lun = format!("{pci}/ata1/host0/target0:0:0/0:0:0:0");
    w(&format!("{lun}/block/sda/dev"), "8:0\n");
    l(
        "../../../../../../../../../class/block",
        &format!("{lun}/block/sda/subsystem"),
    );
    l("../../../0:0:0:0", &format!("{lun}/block/sda/device"));
    l("../../../../../../../bus/scsi", &format!("{lun}/subsystem"));
    // A second SCSI link on the way up: "block:scsi:scsi" is not repeated
    // by the caller, but the chain does hold it.
    l(
        "../../../../../../bus/scsi",
        &format!("{pci}/ata1/host0/target0:0:0/subsystem"),
    );
    l("../../../bus/pci", &format!("{pci}/subsystem"));
    w(&format!("{pci}/removable"), "fixed\n");
    w("sys/class/scsi_host/host0/proc_name", "ahci\n");
    w(&format!("{lun}/vpd_pg80"), "x");
    // The same directories as links from the top of sysfs see them.
    let lun_rel = lun.strip_prefix("sys/").unwrap();
    l(
        &format!("../../../{lun_rel}"),
        "sys/bus/scsi/devices/0:0:0:0",
    );
    l(&format!("../../{lun_rel}/block/sda"), "sys/dev/block/8:0");

    let usb = "sys/devices/pci0000:00/0000:00:14.0/usb1/1-1";
    let ulun = format!("{usb}/1-1:1.0/host6/target6:0:0/6:0:0:0");
    w(&format!("{ulun}/block/sdb/dev"), "8:16\n");
    l("../../../6:0:0:0", &format!("{ulun}/block/sdb/device"));
    w(&format!("{usb}/removable"), "removable\n");
    let ulun_rel = ulun.strip_prefix("sys/").unwrap();
    l(&format!("../../{ulun_rel}/block/sdb"), "sys/dev/block/8:16");

    let prefix = quoting::os_bytes(root.as_os_str()).into_owned();
    let prefix = prefix.strip_suffix(b"/").unwrap_or(&prefix).to_vec();
    let sda = crate::new_sysfs_path(makedev(8, 0), None, Some(&prefix)).unwrap();

    let mut chain = sda.blkdev_devchain().unwrap();
    let mut expect = prefix.clone();
    expect.extend_from_slice(format!("/sys/dev/block/../../{lun_rel}/block/sda").as_bytes());
    assert_eq!(chain, expect);
    let mut subs = Vec::new();
    while let Some(s) = crate::next_subsystem(&mut chain) {
        subs.push(String::from_utf8(s).unwrap());
    }
    assert_eq!(subs, ["block", "scsi", "scsi", "pci"]);
    assert!(chain.is_empty());

    assert_eq!(sda.blkdev_scsi_hctl(), Ok([0, 0, 0, 0]));
    assert!(sda.blkdev_scsi_host_is(b"scsi"));
    assert!(!sda.blkdev_scsi_host_is(b"fc"));
    assert_eq!(
        sda.blkdev_scsi_host_attribute(b"scsi", b"proc_name"),
        Some(b"ahci".to_vec())
    );
    assert_eq!(sda.blkdev_scsi_host_attribute(b"scsi", b"nosuch"), None);
    assert!(sda.blkdev_scsi_has_attribute(b"vpd_pg80"));
    assert!(!sda.blkdev_scsi_has_attribute(b"sas_device"));
    assert!(sda.blkdev_scsi_path_contains(b"pci0000"));
    assert!(!sda.blkdev_scsi_path_contains(b"usb"));
    // The controller says fixed before anything says removable.
    assert!(!sda.blkdev_is_hotpluggable());

    let sdb = crate::new_sysfs_path(makedev(8, 16), None, Some(&prefix)).unwrap();
    assert_eq!(sdb.blkdev_scsi_hctl(), Ok([6, 0, 0, 0]));
    assert!(sdb.blkdev_is_hotpluggable());
    // No sys/bus/scsi entry for it.
    assert!(!sdb.blkdev_scsi_path_contains(b"usb"));
}

#[cfg(unix)]
#[test]
fn a_device_without_a_scsi_address_is_asked_once() {
    use std::os::unix::fs::symlink;
    let dir = scratchdir::ScratchDir::new("ulsysfs-nohctl");
    let root = dir.path("");
    std::fs::create_dir_all(root.join("sys/devices/virtual/block/loop0")).unwrap();
    std::fs::write(root.join("sys/devices/virtual/block/loop0/dev"), "7:0\n").unwrap();
    std::fs::create_dir_all(root.join("sys/dev/block")).unwrap();
    symlink(
        "../../devices/virtual/block/loop0",
        root.join("sys/dev/block/7:0"),
    )
    .unwrap();
    let prefix = quoting::os_bytes(root.as_os_str()).into_owned();
    let prefix = prefix.strip_suffix(b"/").unwrap_or(&prefix).to_vec();
    let pc = crate::new_sysfs_path(makedev(7, 0), None, Some(&prefix)).unwrap();
    assert_eq!(pc.blkdev_scsi_hctl(), Err(crate::ENOENT));
    // Even once a `device` link appears, the answer stays no.
    symlink(
        "../../../x/0:0:0:0",
        root.join("sys/devices/virtual/block/loop0/device"),
    )
    .unwrap();
    assert_eq!(pc.blkdev_scsi_hctl(), Err(crate::EINVAL));
    assert!(!pc.blkdev_is_hotpluggable());
}

#[test]
fn scsi_types_have_upstreams_names() {
    assert_eq!(crate::blkdev::scsi_type_to_name(0), Some("disk"));
    assert_eq!(crate::blkdev::scsi_type_to_name(5), Some("rom"));
    assert_eq!(crate::blkdev::scsi_type_to_name(0x7f), Some("no-lun"));
    assert_eq!(crate::blkdev::scsi_type_to_name(0x0a), None);
    assert_eq!(crate::blkdev::scsi_type_to_name(-1), None);
}

#[test]
fn hctl_numbers_scan_as_sscanf_u_does() {
    use crate::sysfs::scan_hctl;
    assert_eq!(scan_hctl(b"1:2:3:4"), Some([1, 2, 3, 4]));
    assert_eq!(scan_hctl(b"1:2:3"), None);
    assert_eq!(scan_hctl(b"1: 2:3:4"), Some([1, 2, 3, 4]));
    // `%u` takes a sign, and the low 32 bits are printed back signed.
    assert_eq!(scan_hctl(b"-1:0:0:4294967296"), Some([-1, 0, 0, 0]));
}
