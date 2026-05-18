//! `<linux/x25.h>` — X.25 packet-switching constants.
//!
//! X.25 is a packet-switched wide-area network protocol.
//! These constants define X.25 address families, socket
//! options, facilities, and IOCTL commands.

// ---------------------------------------------------------------------------
// X.25 address family
// ---------------------------------------------------------------------------

/// X.25 address family.
pub const AF_X25: u32 = 9;
/// X.25 protocol family.
pub const PF_X25: u32 = AF_X25;

// ---------------------------------------------------------------------------
// X.25 socket options
// ---------------------------------------------------------------------------

/// QBit include (Q-bit handling).
pub const X25_QBITINCL: u32 = 1;

// ---------------------------------------------------------------------------
// X.25 IOCTL commands
// ---------------------------------------------------------------------------

/// Subscribe to X.25 events.
pub const SIOCX25GSUBSCRIP: u32 = 0x8960;
/// Set subscription.
pub const SIOCX25SSUBSCRIP: u32 = 0x8961;
/// Get facilities.
pub const SIOCX25GFACILITIES: u32 = 0x8962;
/// Set facilities.
pub const SIOCX25SFACILITIES: u32 = 0x8963;
/// Get call user data.
pub const SIOCX25GCALLUSERDATA: u32 = 0x8964;
/// Set call user data.
pub const SIOCX25SCALLUSERDATA: u32 = 0x8965;
/// Get cause/diagnostic.
pub const SIOCX25GCAUSEDIAG: u32 = 0x8966;
/// Send call user data.
pub const SIOCX25SCUDMATCHLEN: u32 = 0x8967;
/// Get extended facilities.
pub const SIOCX25CALLACCPTAPPRV: u32 = 0x8968;
/// Send call clear request.
pub const SIOCX25SENDCALLACCPT: u32 = 0x8969;
/// Get DTE facilities.
pub const SIOCX25GDTEFACILITIES: u32 = 0x896A;
/// Set DTE facilities.
pub const SIOCX25SDTEFACILITIES: u32 = 0x896B;

// ---------------------------------------------------------------------------
// X.25 facility codes
// ---------------------------------------------------------------------------

/// Reverse charging.
pub const X25_FAC_REVERSE: u32 = 0x01;
/// Throughput class.
pub const X25_FAC_THROUGHPUT: u32 = 0x02;
/// Packet size.
pub const X25_FAC_PACKET_SIZE: u32 = 0x42;
/// Window size.
pub const X25_FAC_WINDOW_SIZE: u32 = 0x43;
/// Calling address extension.
pub const X25_FAC_CALLING_AE: u32 = 0xCB;
/// Called address extension.
pub const X25_FAC_CALLED_AE: u32 = 0xC9;

// ---------------------------------------------------------------------------
// X.25 call direction
// ---------------------------------------------------------------------------

/// Incoming call.
pub const X25_DIRECTION_INCOMING: u32 = 0;
/// Outgoing call.
pub const X25_DIRECTION_OUTGOING: u32 = 1;

// ---------------------------------------------------------------------------
// X.25 address length
// ---------------------------------------------------------------------------

/// Maximum X.25 address length (digits).
pub const X25_ADDR_LEN: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_x25() {
        assert_eq!(AF_X25, 9);
        assert_eq!(PF_X25, AF_X25);
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            SIOCX25GSUBSCRIP, SIOCX25SSUBSCRIP,
            SIOCX25GFACILITIES, SIOCX25SFACILITIES,
            SIOCX25GCALLUSERDATA, SIOCX25SCALLUSERDATA,
            SIOCX25GCAUSEDIAG, SIOCX25SCUDMATCHLEN,
            SIOCX25CALLACCPTAPPRV, SIOCX25SENDCALLACCPT,
            SIOCX25GDTEFACILITIES, SIOCX25SDTEFACILITIES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_facilities_distinct() {
        let facs = [
            X25_FAC_REVERSE, X25_FAC_THROUGHPUT,
            X25_FAC_PACKET_SIZE, X25_FAC_WINDOW_SIZE,
            X25_FAC_CALLING_AE, X25_FAC_CALLED_AE,
        ];
        for i in 0..facs.len() {
            for j in (i + 1)..facs.len() {
                assert_ne!(facs[i], facs[j]);
            }
        }
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(X25_DIRECTION_INCOMING, X25_DIRECTION_OUTGOING);
    }

    #[test]
    fn test_addr_len() {
        assert_eq!(X25_ADDR_LEN, 16);
    }
}
