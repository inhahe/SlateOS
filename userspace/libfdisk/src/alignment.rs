//! `alignment.c`: the device's topology and geometry, and where partitions
//! may start and end.
//!
//! Partitions are aligned to the "grain" -- 1 MiB, or the optimal I/O size
//! when that is larger -- rather than to the physical sector, so that a
//! layout copied to a disk with larger sectors stays aligned. The first
//! partition starts at 1 MiB (LBA 2048 on 512-byte sectors) unless the
//! device's topology says otherwise or the device is too small for it.

use crate::context::Context;
use crate::errno::EINVAL;
use crate::utils::{div, rem};
use crate::{ALIGN_DOWN, ALIGN_NEAREST, ALIGN_UP, Sector};

/// `DEFAULT_SECTOR_SIZE`.
pub const DEFAULT_SECTOR_SIZE: u64 = 512;

impl Context {
    /// `lba_is_aligned`: on a grain (or larger physical) boundary, the
    /// alignment offset counted in.
    fn lba_is_aligned(&self, lba: u64) -> bool {
        let mut granularity = self.phy_sector_size.max(self.min_io_size);
        if self.grain > granularity {
            granularity = self.grain;
        }
        let offset = rem(lba.wrapping_mul(self.sector_size), granularity);
        rem(
            granularity
                .wrapping_add(self.alignment_offset)
                .wrapping_sub(offset),
            granularity,
        ) == 0
    }

    /// `fdisk_lba_is_phy_aligned`: on a physical sector (or minimal I/O)
    /// boundary.
    #[must_use]
    pub fn lba_is_phy_aligned(&self, lba: Sector) -> bool {
        let granularity = self.phy_sector_size.max(self.min_io_size);
        let offset = rem(lba.wrapping_mul(self.sector_size), granularity);
        rem(
            granularity
                .wrapping_add(self.alignment_offset)
                .wrapping_sub(offset),
            granularity,
        ) == 0
    }

    /// `fdisk_align_lba (cxt, lba, direction)`: `lba` moved to a grain
    /// boundary, up, down or to the nearer (`ALIGN_*`); not below the first
    /// LBA.
    #[must_use]
    pub fn align_lba(&self, lba: Sector, direction: i32) -> Sector {
        if self.lba_is_aligned(lba) {
            return lba;
        }
        let sects_in_phy = div(self.grain, self.sector_size);
        let mut res = if lba < self.first_lba {
            self.first_lba
        } else if direction == ALIGN_UP {
            div(lba.wrapping_add(sects_in_phy), sects_in_phy).wrapping_mul(sects_in_phy)
        } else if direction == ALIGN_DOWN {
            div(lba, sects_in_phy).wrapping_mul(sects_in_phy)
        } else {
            div(lba.wrapping_add(sects_in_phy / 2), sects_in_phy).wrapping_mul(sects_in_phy)
        };
        if self.alignment_offset != 0
            && !self.lba_is_aligned(res)
            && res > div(self.alignment_offset, self.sector_size)
        {
            // Physical blocks start below LBA 0 on a disk with alignment
            // compensation: move to the physical boundary.
            let back = div(
                self.phy_sector_size
                    .max(self.min_io_size)
                    .wrapping_sub(self.alignment_offset),
                self.sector_size,
            );
            res = res.wrapping_sub(back);
            if direction == ALIGN_UP && res < lba {
                res = res.wrapping_add(sects_in_phy);
            }
        }
        res
    }

    /// `fdisk_align_lba_in_range (cxt, lba, start, stop)`: `lba` aligned,
    /// but kept within `start`..`stop` (themselves aligned inward when the
    /// range is at least a grain).
    #[must_use]
    pub fn align_lba_in_range(&self, lba: Sector, start: Sector, stop: Sector) -> Sector {
        let g = div(self.grain, self.sector_size);
        let (mut start, mut stop) = (start, stop);
        if start.wrapping_add(g) <= stop {
            start = self.align_lba(start, ALIGN_UP);
            stop = self.align_lba(stop, ALIGN_DOWN);
        }
        if start.wrapping_add(g) > stop {
            // The area is smaller than the grain: no alignment.
            return lba;
        }
        let lba = self.align_lba(lba, ALIGN_NEAREST);
        if lba < start {
            start
        } else if lba > stop {
            stop
        } else {
            lba
        }
    }

    /// `get_sector_size` (alignment.c's): the device's logical sector size,
    /// 512 for a file.
    fn device_sector_size(&self) -> u64 {
        if !self.is_regfile() {
            if let Some(dev) = self.dev.as_ref() {
                if let Ok(s) = ulblkid::blkdev::get_sector_size(dev) {
                    return u64::from(s);
                }
            }
        }
        DEFAULT_SECTOR_SIZE
    }

    /// `recount_geometry`: 255 heads and 63 sectors unless known, and the
    /// cylinders they make.
    fn recount_geometry(&mut self) {
        if self.geom.heads == 0 {
            self.geom.heads = 255;
        }
        if self.geom.sectors == 0 {
            self.geom.sectors = 63;
        }
        self.geom.cylinders = div(
            self.total_sectors,
            u64::from(self.geom.heads).wrapping_mul(self.geom.sectors),
        );
    }

    /// `fdisk_override_geometry (cxt, cylinders, heads, sectors)`: the
    /// geometry as given (zero: keep), alignment reset.
    pub fn override_geometry(&mut self, cylinders: u32, heads: u32, sectors: u32) -> i32 {
        if heads != 0 {
            self.geom.heads = heads;
        }
        if sectors != 0 {
            self.geom.sectors = u64::from(sectors);
        }
        if cylinders != 0 {
            self.geom.cylinders = u64::from(cylinders);
        } else {
            self.recount_geometry();
        }
        let _ = self.reset_alignment();
        0
    }

    /// `fdisk_save_user_geometry (cxt, cylinders, heads, sectors)`: a
    /// geometry applied whenever a device is assigned. More than 256 heads
    /// or 63 sectors is taken as none.
    pub fn save_user_geometry(&mut self, cylinders: u32, heads: u32, sectors: u32) -> i32 {
        if heads != 0 {
            self.user_geom.heads = if heads > 256 { 0 } else { heads };
        }
        if sectors != 0 {
            self.user_geom.sectors = if sectors >= 64 { 0 } else { u64::from(sectors) };
        }
        if cylinders != 0 {
            self.user_geom.cylinders = u64::from(cylinders);
        }
        0
    }

    /// `fdisk_save_user_sector_size (cxt, phy, log)`.
    pub fn save_user_sector_size(&mut self, phy: u32, log: u32) -> i32 {
        self.user_pyh_sector = u64::from(phy);
        self.user_log_sector = u64::from(log);
        0
    }

    /// `fdisk_save_user_grain (cxt, grain)`: -EINVAL unless a multiple of
    /// 512.
    pub fn save_user_grain(&mut self, grain: u64) -> i32 {
        if rem(grain, 512) != 0 {
            return -EINVAL;
        }
        self.user_grain = grain;
        0
    }

    /// `fdisk_has_user_device_properties`.
    #[must_use]
    pub fn has_user_device_properties(&self) -> bool {
        self.user_pyh_sector != 0
            || self.user_log_sector != 0
            || self.user_grain != 0
            || self.has_user_device_geometry()
    }

    /// `fdisk_has_user_device_geometry`.
    #[must_use]
    pub fn has_user_device_geometry(&self) -> bool {
        self.user_geom.heads != 0 || self.user_geom.sectors != 0 || self.user_geom.cylinders != 0
    }

    /// `fdisk_apply_user_device_properties (cxt)`: the user's sector sizes,
    /// geometry and grain over what was discovered.
    pub fn apply_user_device_properties(&mut self) -> i32 {
        if self.user_pyh_sector != 0 {
            self.phy_sector_size = self.user_pyh_sector;
        }
        if self.user_log_sector != 0 {
            let old_total = self.total_sectors;
            let old_secsz = self.sector_size;
            self.sector_size = self.user_log_sector;
            self.min_io_size = self.user_log_sector;
            self.io_size = self.user_log_sector;
            if self.sector_size != old_secsz {
                self.total_sectors = div(
                    old_total.wrapping_mul(old_secsz / 512),
                    self.sector_size >> 9,
                );
            }
        }
        if self.user_geom.heads != 0 {
            self.geom.heads = self.user_geom.heads;
        }
        if self.user_geom.sectors != 0 {
            self.geom.sectors = self.user_geom.sectors;
        }
        if self.user_geom.cylinders != 0 {
            self.geom.cylinders = self.user_geom.cylinders;
        } else if self.user_geom.heads != 0 || self.user_geom.sectors != 0 {
            self.recount_geometry();
        }
        let _ = self.reset_alignment();
        if self.user_grain != 0 {
            let granularity = self.phy_sector_size.max(self.min_io_size);
            self.grain = if self.user_grain < granularity {
                granularity
            } else {
                self.user_grain
            };
        }
        if self.firstsector_bufsz() != self.sector_size {
            let _ = self.read_firstsector();
        }
        0
    }

    /// The in-memory first sector's size (`firstsector_bufsz`).
    pub(crate) fn firstsector_bufsz(&self) -> u64 {
        self.firstsector
            .as_ref()
            .map_or(0, |b| u64::try_from(b.borrow().len()).unwrap_or(0))
    }

    /// `fdisk_zeroize_device_properties (cxt)`.
    pub fn zeroize_device_properties(&mut self) {
        self.io_size = 0;
        self.optimal_io_size = 0;
        self.min_io_size = 0;
        self.phy_sector_size = 0;
        self.sector_size = 0;
        self.alignment_offset = 0;
        self.grain = 0;
        self.first_lba = 0;
        self.last_lba = 0;
        self.total_sectors = 0;
        self.geom = crate::Geometry::default();
    }

    /// `fdisk_reset_device_properties (cxt)`: topology, geometry and first
    /// sector discovered again, the user's settings applied over them.
    pub fn reset_device_properties(&mut self) -> i32 {
        self.zeroize_device_properties();
        self.discover_topology();
        self.discover_geometry();
        let rc = self.read_firstsector();
        if rc != 0 {
            return rc;
        }
        self.apply_user_device_properties();
        0
    }

    /// `fdisk_discover_geometry (cxt)`: the size in sectors, and the
    /// kernel's idea of heads and sectors per track.
    pub fn discover_geometry(&mut self) -> i32 {
        let mut h = 0;
        let mut s = 0;
        if self.is_regfile() {
            self.total_sectors = div(self.dev_st.size, self.sector_size);
        } else if let Some(dev) = self.dev.clone() {
            if let Ok(nsects) = ulblkid::blkdev::get_sectors(&dev) {
                self.total_sectors = div(nsects, self.sector_size >> 9);
            }
            if let Some((gh, gs)) = blkdev_get_geometry(&dev) {
                h = gh;
                s = gs;
            }
        }
        self.geom.cylinders = 0;
        self.geom.heads = h;
        self.geom.sectors = u64::from(s);
        self.recount_geometry();
        0
    }

    /// `fdisk_discover_topology (cxt)`: I/O sizes and alignment offset from
    /// libblkid, the logical sector size from the device, defaults for the
    /// rest.
    pub fn discover_topology(&mut self) -> i32 {
        let mut pr = ulblkid::Probe::new();
        if pr.set_device(self.dev.clone(), 0, 0) == 0 {
            if let Some(tp) = pr.topology() {
                self.min_io_size = tp.minimum_io_size;
                self.optimal_io_size = tp.optimal_io_size;
                self.phy_sector_size = tp.physical_sector_size;
                self.alignment_offset = tp.alignment_offset;
                self.io_size = self.optimal_io_size;
                if self.io_size == 0 {
                    // Optimal I/O is optional; the minimum by default.
                    self.io_size = self.min_io_size;
                }
                if self.io_size != 0 && self.phy_sector_size != 0 {
                    if self.io_size == 33_553_920 {
                        // 32 MiB - 512 is always a controller error.
                        self.io_size = self.phy_sector_size;
                    } else if rem(self.io_size, self.phy_sector_size) != 0 {
                        // Not aligned to the physical sector: ignored.
                        self.io_size = self.phy_sector_size;
                    }
                }
            }
        }
        self.sector_size = self.device_sector_size();
        if self.phy_sector_size == 0 {
            self.phy_sector_size = self.sector_size;
        }
        if self.min_io_size == 0 {
            self.min_io_size = self.sector_size;
        }
        if self.io_size == 0 {
            self.io_size = self.sector_size;
        }
        0
    }

    /// `has_topology`: whether the device reports topology worth following.
    fn has_topology(&self) -> bool {
        self.optimal_io_size != 0
            || self.alignment_offset != 0
            || !self.min_io_size.is_power_of_two()
    }

    /// `topology_get_first_lba`: where the first partition should start.
    fn topology_get_first_lba(&mut self) -> Sector {
        if self.io_size == 0 {
            self.discover_topology();
        }
        let mut x = 0;
        if self.has_topology() {
            if self.alignment_offset != 0 {
                x = self.alignment_offset;
            } else if self.io_size > 2048 * 512 {
                x = self.io_size;
            }
        }
        if x == 0 {
            x = 2048 * 512;
        }
        let mut res = div(x, self.sector_size);
        // No huge offset on a small device.
        if self.total_sectors <= res.wrapping_mul(4) {
            res = div(self.phy_sector_size, self.sector_size);
        }
        res
    }

    /// `topology_get_grain`: 1 MiB, the I/O size when larger, the physical
    /// sector on a small device.
    fn topology_get_grain(&mut self) -> u64 {
        if self.io_size == 0 {
            self.discover_topology();
        }
        let mut res = self.io_size;
        if res < 2048 * 512 {
            res = 2048 * 512;
        }
        if self.total_sectors <= div(res.wrapping_mul(4), self.sector_size) {
            res = self.phy_sector_size;
        }
        res
    }

    /// `fdisk_apply_label_device_properties (cxt)`: the current label's
    /// own alignment over the defaults.
    pub fn apply_label_device_properties(&mut self) -> i32 {
        match self.label_ref().and_then(|l| l.op.reset_alignment) {
            Some(f) => f(self),
            None => 0,
        }
    }

    /// `fdisk_reset_alignment (cxt)`: the grain, first and last LBA to the
    /// defaults, then the label's.
    pub fn reset_alignment(&mut self) -> i32 {
        self.grain = self.topology_get_grain();
        self.first_lba = self.topology_get_first_lba();
        self.last_lba = self.total_sectors.wrapping_sub(1);
        self.apply_label_device_properties()
    }

    /// `fdisk_scround (cxt, num)`: sectors to units, rounded up.
    #[must_use]
    pub fn scround(&self, num: Sector) -> Sector {
        let un = u64::from(self.get_units_per_sector());
        div(num.wrapping_add(un).wrapping_sub(1), un)
    }

    /// `fdisk_cround (cxt, num)`: a sector as a display unit (a cylinder
    /// counts from 1).
    #[must_use]
    pub fn cround(&self, num: Sector) -> Sector {
        if self.use_cylinders() {
            div(num, u64::from(self.get_units_per_sector())).wrapping_add(1)
        } else {
            num
        }
    }
}

/// `blkdev_get_geometry (fd, &h, &s)`: heads and sectors per track from
/// `HDIO_GETGEO`.
fn blkdev_get_geometry(dev: &std::fs::File) -> Option<(u32, u32)> {
    /// `struct hd_geometry`.
    #[repr(C)]
    #[derive(Default)]
    struct HdGeometry {
        heads: u8,
        sectors: u8,
        cylinders: u16,
        start: u64,
    }
    /// `HDIO_GETGEO`.
    const HDIO_GETGEO: u64 = 0x0301;
    let mut g = HdGeometry::default();
    // SAFETY: HDIO_GETGEO writes one `struct hd_geometry`, which `g` is,
    // laid out as the kernel's.
    let rc = unsafe { ulblkid::blkdev::ioctl_ptr(dev, HDIO_GETGEO, &mut g) };
    rc.ok().map(|_| (u32::from(g.heads), u32::from(g.sectors)))
}
