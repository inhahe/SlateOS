//! `wipe.c`: erasing old signatures when the table is written, and noticing
//! them before.
//!
//! A new partition table can leave an old filesystem's or table's magic
//! where blkid will still find it. The areas to clean are kept as a list of
//! (start, size) in sectors; [`Context::do_wipe`] erases every signature
//! libblkid finds in each, just before the table is written.

use crate::context::Context;
use crate::errno::{EINVAL, ENOMEM};
use crate::is_undef;

impl Context {
    /// `fdisk_get_wipe_area (cxt, start, size)`: its position in the list.
    fn get_wipe_area(&self, start: u64, size: u64) -> Option<usize> {
        self.wipes
            .iter()
            .position(|&(s, z)| s == start && z == size)
    }

    /// `fdisk_free_wipe_areas (cxt)`.
    pub fn free_wipe_areas(&mut self) {
        self.wipes.clear();
    }

    /// `fdisk_has_wipe_area (cxt, start, size)`.
    #[must_use]
    pub fn has_wipe_area(&self, start: u64, size: u64) -> bool {
        self.get_wipe_area(start, size).is_some()
    }

    /// `fdisk_set_wipe_area (cxt, start, size, enable)`: the area added to
    /// or taken off the list. The old setting (1 enabled, 0 not), or
    /// -EINVAL for an undefined area.
    pub fn set_wipe_area(&mut self, start: u64, size: u64, enable: bool) -> i32 {
        if is_undef(start) || is_undef(size) {
            return -EINVAL;
        }
        let wp = self.get_wipe_area(start, size);
        if !enable {
            return match wp {
                Some(i) => {
                    self.wipes.remove(i);
                    1
                }
                None => 0,
            };
        }
        if wp.is_some() {
            return 1;
        }
        if self.wipes.try_reserve(1).is_err() {
            return -ENOMEM;
        }
        self.wipes.push((start, size));
        0
    }

    /// `fdisk_do_wipe (cxt)`: every filesystem, RAID and partition-table
    /// signature libblkid finds in each area, erased.
    pub fn do_wipe(&mut self) -> i32 {
        if self.wipes.is_empty() {
            return 0;
        }
        let mut pr = ulblkid::Probe::new();
        for &(start, size) in &self.wipes.clone() {
            let off = start.wrapping_mul(self.sector_size);
            let len = size.wrapping_mul(self.sector_size);
            let rc = pr.set_device(
                self.dev.clone(),
                i64::from_ne_bytes(off.to_ne_bytes()),
                i64::from_ne_bytes(len.to_ne_bytes()),
            );
            if rc != 0 {
                return rc;
            }
            pr.enable_superblocks(true);
            pr.set_superblocks_flags(ulblkid::SUBLKS_MAGIC | ulblkid::SUBLKS_BADCSUM);
            pr.enable_partitions(true);
            pr.set_partitions_flags(ulblkid::PARTS_MAGIC | ulblkid::PARTS_FORCE_GPT);
            while pr.do_probe() == 0 {
                let _ = pr.do_wipe(false);
            }
        }
        0
    }

    /// `fdisk_check_collisions (cxt)`: the first filesystem, RAID member or
    /// partition table libblkid finds on the device, remembered as the
    /// collision. 1 when there is one, 0 when not, <0 on error.
    pub fn check_collisions(&mut self) -> i32 {
        let mut pr = ulblkid::Probe::new();
        let rc = pr.set_device(self.dev.clone(), 0, 0);
        if rc != 0 {
            return rc;
        }
        self.pt_collision = false;
        self.collision = None;
        pr.enable_superblocks(true);
        pr.set_superblocks_flags(ulblkid::SUBLKS_TYPE | ulblkid::SUBLKS_BADCSUM);
        pr.enable_partitions(true);
        pr.set_partitions_flags(ulblkid::PARTS_FORCE_GPT);

        // The first signature found is what matters: one blkid_do_probe(),
        // not a loop and not a full probe.
        let rc = pr.do_probe();
        if rc == 0 {
            if let Some(v) = pr.lookup_value("TYPE") {
                self.collision = Some(v.as_c_str().to_vec());
            } else if let Some(v) = pr.lookup_value("PTTYPE") {
                self.collision = Some(v.as_c_str().to_vec());
                self.pt_collision = true;
            }
        }
        if rc < 0 {
            return rc;
        }
        i32::from(self.collision.is_some())
    }
}
