//! Tests for how full a file system is, which of what is mounted are disks,
//! and how the mount table is read.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]

use super::*;

/// A record as the kernel lays it out: the three counts, then inodes, a
/// name's length and the read-only flag.
fn record(block_size: u64, total: u64, free: u64, read_only: bool) -> [u8; RECORD_SIZE] {
    let mut rec = [0u8; RECORD_SIZE];
    rec[0..8].copy_from_slice(&block_size.to_le_bytes());
    rec[8..16].copy_from_slice(&total.to_le_bytes());
    rec[16..24].copy_from_slice(&free.to_le_bytes());
    rec[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    rec[40..48].copy_from_slice(&255_u64.to_le_bytes());
    rec[48] = u8::from(read_only);
    rec
}

/// A file system that can be written, with `total` blocks of 4 KiB and
/// `free` of them free.
fn space(total: u64, free: u64) -> Space {
    Space {
        block_size: 4096,
        total_blocks: total,
        free_blocks: free,
        read_only: false,
    }
}

/// **The record is read as the kernel writes it**: the block size, the
/// total and the free, little-endian, from its head, and the read-only
/// flag after the inodes and a name's length.
#[test]
fn the_record_is_read_as_the_kernel_writes_it() {
    assert_eq!(
        Space::from_record(&record(4096, 1_000_000, 250_000, true)),
        Space {
            block_size: 4096,
            total_blocks: 1_000_000,
            free_blocks: 250_000,
            read_only: true,
        }
    );
    assert!(!Space::from_record(&record(512, 10, 5, false)).read_only);
}

/// **How full it is, is the share of its blocks not free** -- none of an
/// empty one, a whole one of a full one, nothing at all of one with no
/// blocks, and an empty one of one that says it has more free than it
/// has.
#[test]
fn how_full_it_is_is_the_share_of_its_blocks_not_free() {
    assert_eq!(space(1000, 250).used_fraction(), Some(0.75));
    assert_eq!(space(1000, 1000).used_fraction(), Some(0.0));
    assert_eq!(space(1000, 0).used_fraction(), Some(1.0));
    assert_eq!(space(0, 0).used_fraction(), None);
    assert_eq!(space(1000, 4000).used_fraction(), Some(0.0));
    let huge = space(u64::MAX, u64::MAX / 4).used_fraction().unwrap();
    assert!((huge - 0.75).abs() < 0.001, "{huge}");
}

/// **Its percentage is truncated**: 100 only when it is full, and a
/// hair under nine tenths is not 90.
#[test]
fn its_percentage_reaches_a_mark_only_past_it() {
    assert_eq!(space(1000, 1).used_percent(), Some(99));
    assert_eq!(space(1000, 0).used_percent(), Some(100));
    assert_eq!(space(1000, 101).used_percent(), Some(89));
    assert_eq!(space(1000, 100).used_percent(), Some(90));
    assert_eq!(space(0, 0).used_percent(), None);
}

/// **What it holds and what is free are its blocks at their size**: the
/// free never more than it holds, and neither past what a `u64` counts.
#[test]
fn what_it_holds_and_has_free_are_its_blocks_at_their_size() {
    assert_eq!(space(1000, 250).total_bytes(), 4_096_000);
    assert_eq!(space(1000, 250).free_bytes(), 1_024_000);
    assert_eq!(space(1000, 4000).free_bytes(), 4_096_000);
    assert_eq!(space(u64::MAX, u64::MAX).total_bytes(), u64::MAX);
    assert_eq!(space(u64::MAX, u64::MAX).free_bytes(), u64::MAX);
}

/// **Nearly full is nine tenths and more in use of one that can be
/// written**; a read-only one is as full as it was made, which is no
/// warning, and one with no blocks is not full of anything.
#[test]
fn nearly_full_is_nine_tenths_of_one_that_can_be_written() {
    assert!(space(1000, 100).nearly_full());
    assert!(space(1000, 0).nearly_full());
    assert!(!space(1000, 101).nearly_full());
    assert!(!space(0, 0).nearly_full());
    let disc = Space {
        read_only: true,
        ..space(1000, 0)
    };
    assert!(!disc.nearly_full());
}

/// **Off SlateOS there is no such call**, and the answer says so rather
/// than a made-up share.
#[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
#[test]
fn off_slateos_there_is_no_such_call() {
    assert_eq!(of(b"/"), Err(ENOSYS));
}

/// A mount at `point` of type `fs_type`.
fn mount(point: &[u8], fs_type: &[u8]) -> Mount {
    Mount {
        point: point.to_vec(),
        fs_type: fs_type.to_vec(),
    }
}

/// **The mount table is read as the kernel writes it**: the point and the
/// type of each line, in its order, the source and the options passed
/// over; a short line is none.
#[test]
fn the_mount_table_is_read_as_the_kernel_writes_it() {
    let table = b"none / fat32 rw 0 0\n\
                  none /proc procfs rw 0 0\n\
                  none /mnt ext4 rw,noatime 0 0\n\
                  half-a-line\n\
                  \n\
                  /dev/sda1\t/boot\tvfat\trw\t0\t0\n";
    assert_eq!(
        parse_mounts(table),
        [
            mount(b"/", b"fat32"),
            mount(b"/proc", b"procfs"),
            mount(b"/mnt", b"ext4"),
            mount(b"/boot", b"vfat"),
        ]
    );
}

/// **A field's escapes are undone**: a space, a backslash, a byte that is
/// not text, each as the kernel writes it; and a backslash the escape does
/// not follow -- too few digits, a digit that is not octal, a number past a
/// byte -- is kept as it is.
#[test]
fn a_fields_escapes_are_undone() {
    let table = b"none /media/My\\040Disk ext4 rw 0 0\n\
                  none /back\\134slash ext4 rw 0 0\n\
                  none /caf\\351 ext4 rw 0 0\n\
                  none /odd\\089 ext4 rw 0 0\n\
                  none /big\\400 ext4 rw 0 0\n\
                  none /end\\04 ext4 rw 0 0\n";
    let points: Vec<Vec<u8>> = parse_mounts(table).into_iter().map(|m| m.point).collect();
    assert_eq!(
        points,
        [
            b"/media/My Disk".to_vec(),
            b"/back\\slash".to_vec(),
            b"/caf\xe9".to_vec(),
            b"/odd\\089".to_vec(),
            b"/big\\400".to_vec(),
            b"/end\\04".to_vec(),
        ]
    );
}

/// **A point mounted over is the file system mounted last**, in the first
/// one's place.
#[test]
fn a_point_mounted_over_is_the_last_mounted() {
    let table = b"none / fat32 rw 0 0\n\
                  none /mnt memfs rw 0 0\n\
                  none /home ext4 rw 0 0\n\
                  none /mnt ext4 rw 0 0\n";
    assert_eq!(
        parse_mounts(table),
        [
            mount(b"/", b"fat32"),
            mount(b"/mnt", b"ext4"),
            mount(b"/home", b"ext4"),
        ]
    );
}

/// **The disks are what is mounted less the kernel's views and memory**,
/// and less what has no blocks -- a type nobody listed is a disk if it has
/// some; one that cannot be measured is still a disk, not measured.
#[test]
fn the_disks_are_what_is_mounted_less_what_keeps_nothing_on_a_disk() {
    let mounts = [
        mount(b"/", b"fat32"),
        mount(b"/proc", b"procfs"),
        mount(b"/dev", b"devfs"),
        mount(b"/tmp", b"memfs"),
        mount(b"/mnt", b"ext4"),
        mount(b"/odd", b"newfs"),
        mount(b"/empty", b"newfs"),
        mount(b"/gone", b"ext4"),
    ];
    let measure = |point: &[u8]| match point {
        b"/" => Ok(space(1000, 500)),
        b"/mnt" => Ok(space(2000, 100)),
        b"/odd" => Ok(space(10, 10)),
        b"/empty" => Ok(space(0, 0)),
        b"/gone" => Err(2),
        other => panic!("{other:?} was measured"),
    };
    let found = disks(&mounts, measure);
    let points: Vec<&[u8]> = found.iter().map(|d| d.point.as_slice()).collect();
    assert_eq!(points, [&b"/"[..], b"/mnt", b"/odd", b"/gone"]);
    assert_eq!(found[1].space, Ok(space(2000, 100)));
    assert_eq!(found[3].space, Err(2));
}

/// **Each of SlateOS's views is known not to be a disk** -- every type the
/// kernel's `procfs`, `sysfs`, `devfs` and `memfs` call themselves, and a
/// container's overlay.
#[test]
fn slateos_views_are_not_disks() {
    for name in [&b"procfs"[..], b"sysfs", b"devfs", b"memfs", b"overlay"] {
        assert!(NOT_DISKS.contains(&name), "{name:?}");
    }
    for name in [&b"ext4"[..], b"fat32", b"iso9660", b"btrfs", b"ntfs"] {
        assert!(!NOT_DISKS.contains(&name), "{name:?}");
    }
}

/// **A mount table that will not read is said**, not taken for no disks.
#[test]
fn a_mount_table_that_will_not_read_is_said() {
    let dir = scratchdir::ScratchDir::new("fs-space-missing-table");
    assert!(mounted_disks(&dir.path("mounts")).is_err());
}

/// **The mount table's disks are measured by the kernel** -- off SlateOS,
/// not at all, so each is listed as not measured.
#[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
#[test]
fn the_tables_disks_are_measured_by_the_kernel() {
    let dir = scratchdir::ScratchDir::new("fs-space-table");
    let table = dir.path("mounts");
    std::fs::write(&table, b"none / ext4 rw 0 0\nnone /proc procfs rw 0 0\n").unwrap();
    assert_eq!(
        mounted_disks(&table).unwrap(),
        [Disk {
            point: b"/".to_vec(),
            space: Err(ENOSYS),
        }]
    );
}
