//! `<drm/drm_mode.h>` — DRM property type and flag constants.
//!
//! DRM properties attach typed metadata to objects (connectors, CRTCs,
//! planes). Properties control display configuration: rotation, scaling,
//! color space, HDR metadata. Each property has a type defining its
//! value domain (range, enum, blob, bitmask).

// ---------------------------------------------------------------------------
// Property types (drm_property.flags & DRM_MODE_PROP_TYPE_MASK)
// ---------------------------------------------------------------------------

/// Range property (integer with min/max bounds).
pub const DRM_MODE_PROP_RANGE: u32 = 1 << 1;
/// Enum property (one of several named values).
pub const DRM_MODE_PROP_ENUM: u32 = 1 << 3;
/// Blob property (arbitrary binary data).
pub const DRM_MODE_PROP_BLOB: u32 = 1 << 4;
/// Bitmask property (OR-able named bits).
pub const DRM_MODE_PROP_BITMASK: u32 = 1 << 5;
/// Object property (reference to another DRM object).
pub const DRM_MODE_PROP_OBJECT: u32 = 1 << 6;
/// Signed range property.
pub const DRM_MODE_PROP_SIGNED_RANGE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Property flags (combined with type)
// ---------------------------------------------------------------------------

/// Property is immutable (read-only).
pub const DRM_MODE_PROP_IMMUTABLE: u32 = 1 << 2;
/// Property is atomic-only (not visible to legacy ioctls).
pub const DRM_MODE_PROP_ATOMIC: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Standard rotation property values
// ---------------------------------------------------------------------------

/// No rotation (identity).
pub const DRM_MODE_ROTATE_0: u32 = 1 << 0;
/// 90-degree rotation.
pub const DRM_MODE_ROTATE_90: u32 = 1 << 1;
/// 180-degree rotation.
pub const DRM_MODE_ROTATE_180: u32 = 1 << 2;
/// 270-degree rotation.
pub const DRM_MODE_ROTATE_270: u32 = 1 << 3;
/// Horizontal flip (reflect X).
pub const DRM_MODE_REFLECT_X: u32 = 1 << 4;
/// Vertical flip (reflect Y).
pub const DRM_MODE_REFLECT_Y: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Standard scaling mode values (connector property)
// ---------------------------------------------------------------------------

/// No scaling (native resolution).
pub const DRM_MODE_SCALE_NONE: u32 = 0;
/// Scale to fill the entire display.
pub const DRM_MODE_SCALE_FULLSCREEN: u32 = 1;
/// Scale maintaining aspect ratio (center, black bars).
pub const DRM_MODE_SCALE_CENTER: u32 = 2;
/// Aspect-ratio-preserving scale.
pub const DRM_MODE_SCALE_ASPECT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_types_no_overlap() {
        let types = [
            DRM_MODE_PROP_RANGE,
            DRM_MODE_PROP_ENUM,
            DRM_MODE_PROP_BLOB,
            DRM_MODE_PROP_BITMASK,
            DRM_MODE_PROP_OBJECT,
            DRM_MODE_PROP_SIGNED_RANGE,
        ];
        for i in 0..types.len() {
            assert!(types[i].is_power_of_two());
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_rotation_no_overlap() {
        let rots = [
            DRM_MODE_ROTATE_0,
            DRM_MODE_ROTATE_90,
            DRM_MODE_ROTATE_180,
            DRM_MODE_ROTATE_270,
            DRM_MODE_REFLECT_X,
            DRM_MODE_REFLECT_Y,
        ];
        for i in 0..rots.len() {
            assert!(rots[i].is_power_of_two());
            for j in (i + 1)..rots.len() {
                assert_eq!(rots[i] & rots[j], 0);
            }
        }
    }

    #[test]
    fn test_scale_modes_distinct() {
        let modes = [
            DRM_MODE_SCALE_NONE,
            DRM_MODE_SCALE_FULLSCREEN,
            DRM_MODE_SCALE_CENTER,
            DRM_MODE_SCALE_ASPECT,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
