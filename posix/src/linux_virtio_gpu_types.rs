//! `<linux/virtio_gpu.h>` — VirtIO GPU device constants.
//!
//! virtio-gpu provides a paravirtualized 3D GPU for guest VMs.
//! It supports 2D framebuffer operations (scanout, transfer,
//! cursor) and optional 3D rendering via virgl (OpenGL) or
//! venus (Vulkan). The guest sends GPU commands through
//! virtqueues; the host renders using its native GPU. Used by
//! QEMU/KVM, crosvm (ChromeOS), and cloud gaming/desktop VMs.

// ---------------------------------------------------------------------------
// VirtIO GPU command types (VIRTIO_GPU_CMD_*)
// ---------------------------------------------------------------------------

/// Get display info (resolution, enabled outputs).
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
/// Create a 2D resource (buffer).
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
/// Destroy a resource.
pub const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
/// Set scanout (attach resource to display output).
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
/// Flush resource to display.
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
/// Transfer data from guest to host resource.
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
/// Attach backing pages to resource.
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
/// Detach backing pages.
pub const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
/// Get capabilities (3D extensions).
pub const VIRTIO_GPU_CMD_GET_CAPSET_INFO: u32 = 0x0108;
/// Get capability set data.
pub const VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0109;
/// Get EDID for an output.
pub const VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010A;
/// Update cursor position and image.
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;
/// Move cursor position.
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0301;

// ---------------------------------------------------------------------------
// VirtIO GPU 3D commands
// ---------------------------------------------------------------------------

/// Submit 3D command buffer.
pub const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0200;
/// Create 3D resource.
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0201;
/// Transfer to host (3D).
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D: u32 = 0x0202;
/// Transfer from host (3D).
pub const VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D: u32 = 0x0203;
/// Create 3D rendering context.
pub const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0204;
/// Destroy 3D rendering context.
pub const VIRTIO_GPU_CMD_CTX_DESTROY: u32 = 0x0205;
/// Attach resource to context.
pub const VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE: u32 = 0x0206;
/// Detach resource from context.
pub const VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE: u32 = 0x0207;
/// Create a blob resource (virtio-gpu blob).
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB: u32 = 0x0208;
/// Set scanout for blob resource.
pub const VIRTIO_GPU_CMD_SET_SCANOUT_BLOB: u32 = 0x0209;

// ---------------------------------------------------------------------------
// VirtIO GPU response types
// ---------------------------------------------------------------------------

/// Success (no error, no data).
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
/// Success with display info.
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
/// Success with capset info.
pub const VIRTIO_GPU_RESP_OK_CAPSET_INFO: u32 = 0x1102;
/// Success with capset data.
pub const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1103;
/// Success with EDID.
pub const VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
/// Error: unspecified.
pub const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
/// Error: out of memory.
pub const VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
/// Error: invalid scanout ID.
pub const VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
/// Error: invalid resource ID.
pub const VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
/// Error: invalid context ID.
pub const VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
/// Error: invalid parameter.
pub const VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

// ---------------------------------------------------------------------------
// VirtIO GPU feature bits
// ---------------------------------------------------------------------------

/// Device supports 3D rendering (virgl/venus).
pub const VIRTIO_GPU_F_VIRGL: u32 = 0;
/// Device supports EDID.
pub const VIRTIO_GPU_F_EDID: u32 = 1;
/// Device supports resource blob.
pub const VIRTIO_GPU_F_RESOURCE_BLOB: u32 = 3;
/// Device supports context init (multiple 3D contexts).
pub const VIRTIO_GPU_F_CONTEXT_INIT: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_2d_commands_distinct() {
        let cmds = [
            VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
            VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
            VIRTIO_GPU_CMD_RESOURCE_UNREF,
            VIRTIO_GPU_CMD_SET_SCANOUT,
            VIRTIO_GPU_CMD_RESOURCE_FLUSH,
            VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
            VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
            VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING,
            VIRTIO_GPU_CMD_GET_CAPSET_INFO,
            VIRTIO_GPU_CMD_GET_CAPSET,
            VIRTIO_GPU_CMD_GET_EDID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_3d_commands_distinct() {
        let cmds = [
            VIRTIO_GPU_CMD_SUBMIT_3D,
            VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
            VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D,
            VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D,
            VIRTIO_GPU_CMD_CTX_CREATE,
            VIRTIO_GPU_CMD_CTX_DESTROY,
            VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
            VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE,
            VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB,
            VIRTIO_GPU_CMD_SET_SCANOUT_BLOB,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_responses_distinct() {
        let resps = [
            VIRTIO_GPU_RESP_OK_NODATA,
            VIRTIO_GPU_RESP_OK_DISPLAY_INFO,
            VIRTIO_GPU_RESP_OK_CAPSET_INFO,
            VIRTIO_GPU_RESP_OK_CAPSET,
            VIRTIO_GPU_RESP_OK_EDID,
            VIRTIO_GPU_RESP_ERR_UNSPEC,
            VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY,
            VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID,
            VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID,
            VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID,
            VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER,
        ];
        for i in 0..resps.len() {
            for j in (i + 1)..resps.len() {
                assert_ne!(resps[i], resps[j]);
            }
        }
    }

    #[test]
    fn test_features_distinct() {
        let feats = [
            VIRTIO_GPU_F_VIRGL,
            VIRTIO_GPU_F_EDID,
            VIRTIO_GPU_F_RESOURCE_BLOB,
            VIRTIO_GPU_F_CONTEXT_INIT,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }
}
