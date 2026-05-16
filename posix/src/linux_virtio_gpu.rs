//! `<linux/virtio_gpu.h>` — Virtio GPU device constants.
//!
//! Virtio-gpu provides 2D/3D GPU rendering in VMs. Supports
//! scanout display, cursor, resource management, and optional
//! 3D (virgl) acceleration.

pub use crate::linux_virtio_types::VIRTIO_ID_GPU;

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// 3D acceleration (virgl).
pub const VIRTIO_GPU_F_VIRGL: u32 = 0;
/// EDID support.
pub const VIRTIO_GPU_F_EDID: u32 = 1;
/// Resource UUID support.
pub const VIRTIO_GPU_F_RESOURCE_UUID: u32 = 2;
/// Resource blob support.
pub const VIRTIO_GPU_F_RESOURCE_BLOB: u32 = 3;
/// Context init support.
pub const VIRTIO_GPU_F_CONTEXT_INIT: u32 = 4;

// ---------------------------------------------------------------------------
// Command types (control queue)
// ---------------------------------------------------------------------------

/// Get display info.
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
/// Create 2D resource.
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
/// Unref resource.
pub const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
/// Set scanout.
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
/// Flush resource to display.
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
/// Transfer data to host.
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
/// Attach resource backing.
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
/// Detach resource backing.
pub const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
/// Get EDID.
pub const VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010A;

// ---------------------------------------------------------------------------
// 3D commands
// ---------------------------------------------------------------------------

/// Create 3D context.
pub const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0200;
/// Destroy 3D context.
pub const VIRTIO_GPU_CMD_CTX_DESTROY: u32 = 0x0201;
/// Attach resource to context.
pub const VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE: u32 = 0x0202;
/// Detach resource from context.
pub const VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE: u32 = 0x0203;
/// Create 3D resource.
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0204;
/// Transfer to host 3D.
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D: u32 = 0x0205;
/// Transfer from host 3D.
pub const VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D: u32 = 0x0206;
/// Submit 3D command buffer.
pub const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0207;

// ---------------------------------------------------------------------------
// Cursor commands
// ---------------------------------------------------------------------------

/// Update cursor.
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;
/// Move cursor.
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0301;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// OK (no data).
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
/// OK (display info).
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
/// OK (EDID).
pub const VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
/// Error (unspecified).
pub const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
/// Error (out of memory).
pub const VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
/// Error (invalid scanout).
pub const VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
/// Error (invalid resource).
pub const VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
/// Error (invalid context).
pub const VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
/// Error (invalid parameter).
pub const VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

// ---------------------------------------------------------------------------
// Pixel formats
// ---------------------------------------------------------------------------

/// BGRA 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
/// BGRX 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;
/// ARGB 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM: u32 = 3;
/// XRGB 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM: u32 = 4;
/// RGBA 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;
/// RGBX 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM: u32 = 68;
/// ABGR 8888 (unorm).
pub const VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM: u32 = 121;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_distinct() {
        let feats = [
            VIRTIO_GPU_F_VIRGL, VIRTIO_GPU_F_EDID,
            VIRTIO_GPU_F_RESOURCE_UUID, VIRTIO_GPU_F_RESOURCE_BLOB,
            VIRTIO_GPU_F_CONTEXT_INIT,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_2d_cmds_distinct() {
        let cmds = [
            VIRTIO_GPU_CMD_GET_DISPLAY_INFO, VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
            VIRTIO_GPU_CMD_RESOURCE_UNREF, VIRTIO_GPU_CMD_SET_SCANOUT,
            VIRTIO_GPU_CMD_RESOURCE_FLUSH, VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
            VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING,
            VIRTIO_GPU_CMD_GET_EDID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_resp_types_distinct() {
        let resps = [
            VIRTIO_GPU_RESP_OK_NODATA, VIRTIO_GPU_RESP_OK_DISPLAY_INFO,
            VIRTIO_GPU_RESP_OK_EDID, VIRTIO_GPU_RESP_ERR_UNSPEC,
            VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY,
            VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID,
            VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID,
        ];
        for i in 0..resps.len() {
            for j in (i + 1)..resps.len() {
                assert_ne!(resps[i], resps[j]);
            }
        }
    }

    #[test]
    fn test_formats_distinct() {
        let fmts = [
            VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM, VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM,
            VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM, VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM,
            VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM, VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM,
            VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_GPU, 16);
    }
}
