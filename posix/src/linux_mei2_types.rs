//! `<linux/mei.h>` — Additional MEI (Management Engine Interface) constants.
//!
//! Supplementary MEI constants covering client properties,
//! connection types, and ioctl commands.

// ---------------------------------------------------------------------------
// MEI connection types
// ---------------------------------------------------------------------------

/// Normal connection.
pub const MEI_CL_CONNECT_SUCCESS: u32 = 0;
/// Not found.
pub const MEI_CL_NOT_FOUND: u32 = 1;
/// Already started.
pub const MEI_CL_ALREADY_STARTED: u32 = 2;
/// Out of resources.
pub const MEI_CL_OUT_OF_RESOURCES: u32 = 3;
/// Message too small.
pub const MEI_CL_MESSAGE_SMALL: u32 = 4;
/// Not allowed.
pub const MEI_CL_NOT_ALLOWED: u32 = 5;

// ---------------------------------------------------------------------------
// MEI client properties flags
// ---------------------------------------------------------------------------

/// Fixed address.
pub const MEI_CL_FLAG_FIXED_ADDRESS: u32 = 1 << 0;
/// Single receive buffer.
pub const MEI_CL_FLAG_SINGLE_RECV_BUF: u32 = 1 << 1;
/// Can send vtag.
pub const MEI_CL_FLAG_VTAG: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// MEI ioctl commands
// ---------------------------------------------------------------------------

/// Connect to ME client.
pub const MEI_CONNECT_CLIENT_IOCTL: u32 = 0xC0104801;
/// Connect to ME client by vtag.
pub const MEI_CONNECT_CLIENT_VTAG_IOCTL: u32 = 0xC0104802;
/// Notify set.
pub const MEI_NOTIFY_SET: u32 = 0x40044803;
/// Notify get.
pub const MEI_NOTIFY_GET: u32 = 0x80044804;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_results_distinct() {
        let results = [
            MEI_CL_CONNECT_SUCCESS,
            MEI_CL_NOT_FOUND,
            MEI_CL_ALREADY_STARTED,
            MEI_CL_OUT_OF_RESOURCES,
            MEI_CL_MESSAGE_SMALL,
            MEI_CL_NOT_ALLOWED,
        ];
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(results[i], results[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            MEI_CL_FLAG_FIXED_ADDRESS,
            MEI_CL_FLAG_SINGLE_RECV_BUF,
            MEI_CL_FLAG_VTAG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            MEI_CONNECT_CLIENT_IOCTL,
            MEI_CONNECT_CLIENT_VTAG_IOCTL,
            MEI_NOTIFY_SET,
            MEI_NOTIFY_GET,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
