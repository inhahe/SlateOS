//! `<drm/drm_aperture.h>` — DRM framebuffer-aperture sysfs paths.
//!
//! The DRM "aperture" subsystem tracks PCI BARs that own a graphics
//! framebuffer, so a takeover driver (typically `simpledrm` or a real
//! GPU driver) can evict an earlier owner (`efifb`/`vesafb`). Userspace
//! observes the result via sysfs and via the `/dev/dri/*` device nodes.

// ---------------------------------------------------------------------------
// DRM device-node prefixes
// ---------------------------------------------------------------------------

pub const DEV_DRI: &str = "/dev/dri";
pub const DRM_CARD_PREFIX: &str = "card";
pub const DRM_RENDER_PREFIX: &str = "renderD";

/// First minor allocated to render-only nodes.
pub const DRM_RENDER_MINOR_BASE: u32 = 128;

// ---------------------------------------------------------------------------
// `efifb` / `vesafb` simple framebuffer fallback drivers
// ---------------------------------------------------------------------------

pub const FB_NAME_EFIFB: &str = "efifb";
pub const FB_NAME_VESAFB: &str = "vesafb";
pub const FB_NAME_SIMPLEDRM: &str = "simpledrm";
pub const FB_NAME_VGACON: &str = "vgacon";

// ---------------------------------------------------------------------------
// /sys/class/drm paths
// ---------------------------------------------------------------------------

pub const SYS_CLASS_DRM: &str = "/sys/class/drm";
pub const DRM_SYSFS_VERSION_ATTR: &str = "version";

// ---------------------------------------------------------------------------
// PCI class codes that own framebuffer apertures (DRM matches against these)
// ---------------------------------------------------------------------------

pub const PCI_CLASS_DISPLAY_VGA: u32 = 0x0300_00;
pub const PCI_CLASS_DISPLAY_XGA: u32 = 0x0301_00;
pub const PCI_CLASS_DISPLAY_3D: u32 = 0x0302_00;
pub const PCI_CLASS_DISPLAY_OTHER: u32 = 0x0380_00;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_dri_layout() {
        assert_eq!(DEV_DRI, "/dev/dri");
        assert!(DEV_DRI.starts_with("/dev/"));
        assert_eq!(DRM_CARD_PREFIX, "card");
        // Render-node prefix has the capital D — case-sensitive ABI.
        assert!(DRM_RENDER_PREFIX.ends_with('D'));
    }

    #[test]
    fn test_render_minor_base_128() {
        // Render minor allocation starts at 128 to leave 0..63 for primary
        // and 64..127 for control nodes (unused since 2014).
        assert_eq!(DRM_RENDER_MINOR_BASE, 128);
    }

    #[test]
    fn test_fb_driver_names_distinct() {
        let n = [FB_NAME_EFIFB, FB_NAME_VESAFB, FB_NAME_SIMPLEDRM, FB_NAME_VGACON];
        for (i, &a) in n.iter().enumerate() {
            for &b in &n[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // All lowercase, no path separators.
        for s in n {
            assert!(!s.contains('/'));
            assert!(s.chars().all(|c| !c.is_uppercase()));
        }
    }

    #[test]
    fn test_sysfs_paths_under_sys() {
        assert_eq!(SYS_CLASS_DRM, "/sys/class/drm");
        assert!(SYS_CLASS_DRM.starts_with("/sys/"));
        assert_eq!(DRM_SYSFS_VERSION_ATTR, "version");
    }

    #[test]
    fn test_pci_classes_in_display_block() {
        // Display PCI base class is 0x03; subclass varies.
        let classes = [
            PCI_CLASS_DISPLAY_VGA,
            PCI_CLASS_DISPLAY_XGA,
            PCI_CLASS_DISPLAY_3D,
            PCI_CLASS_DISPLAY_OTHER,
        ];
        for v in classes {
            assert_eq!(v >> 16, 0x03);
        }
        // VGA=0, XGA=1, 3D=2, Other=80 — subclasses.
        assert_eq!((PCI_CLASS_DISPLAY_VGA >> 8) & 0xFF, 0x00);
        assert_eq!((PCI_CLASS_DISPLAY_XGA >> 8) & 0xFF, 0x01);
        assert_eq!((PCI_CLASS_DISPLAY_3D >> 8) & 0xFF, 0x02);
        assert_eq!((PCI_CLASS_DISPLAY_OTHER >> 8) & 0xFF, 0x80);
    }
}
