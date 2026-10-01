//! `lib/blkdev.c`: the part of it that is about names rather than
//! devices -- what a SCSI peripheral type is called.

/// `blkdev_scsi_type_to_name(type)`: the name of a SCSI peripheral device
/// type, as `/sys/block/NAME/device/type` gives it (`include/blkdev.h`'s
/// `SCSI_TYPE_*`); `None` for one upstream does not name.
#[must_use]
pub fn scsi_type_to_name(ty: i32) -> Option<&'static str> {
    Some(match ty {
        0x00 => "disk",
        0x01 => "tape",
        0x02 => "printer",
        0x03 => "processor",
        0x04 => "worm",
        0x05 => "rom",
        0x06 => "scanner",
        0x07 => "mo-disk",
        0x08 => "changer",
        0x09 => "comm",
        0x0c => "raid",
        0x0d => "enclosure",
        0x0e => "rbc",
        0x11 => "osd",
        0x7f => "no-lun",
        _ => return None,
    })
}
