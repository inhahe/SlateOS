//! `<net/9p/9p.h>` — Plan 9 Filesystem Protocol (9P) constants.
//!
//! 9P is a simple, message-based network protocol for distributed
//! file systems, originating from Plan 9 from Bell Labs. Linux's
//! v9fs uses it for guest↔host file sharing in VMs (virtio-9p),
//! container file access, and accessing Plan 9 servers.

// ---------------------------------------------------------------------------
// 9P message types (T-messages: requests, R-messages: responses)
// ---------------------------------------------------------------------------

/// Version negotiation (request).
pub const P9_TVERSION: u8 = 100;
/// Version negotiation (response).
pub const P9_RVERSION: u8 = 101;
/// Authentication (request).
pub const P9_TAUTH: u8 = 102;
/// Authentication (response).
pub const P9_RAUTH: u8 = 103;
/// Attach to filesystem (request).
pub const P9_TATTACH: u8 = 104;
/// Attach to filesystem (response).
pub const P9_RATTACH: u8 = 105;
/// Error (response only).
pub const P9_RERROR: u8 = 107;
/// Flush pending request (request).
pub const P9_TFLUSH: u8 = 108;
/// Flush (response).
pub const P9_RFLUSH: u8 = 109;
/// Walk path (request).
pub const P9_TWALK: u8 = 110;
/// Walk path (response).
pub const P9_RWALK: u8 = 111;
/// Open file (request).
pub const P9_TOPEN: u8 = 112;
/// Open file (response).
pub const P9_ROPEN: u8 = 113;
/// Create file (request).
pub const P9_TCREATE: u8 = 114;
/// Create file (response).
pub const P9_RCREATE: u8 = 115;
/// Read (request).
pub const P9_TREAD: u8 = 116;
/// Read (response).
pub const P9_RREAD: u8 = 117;
/// Write (request).
pub const P9_TWRITE: u8 = 118;
/// Write (response).
pub const P9_RWRITE: u8 = 119;
/// Clunk fid (request).
pub const P9_TCLUNK: u8 = 120;
/// Clunk fid (response).
pub const P9_RCLUNK: u8 = 121;
/// Remove file (request).
pub const P9_TREMOVE: u8 = 122;
/// Remove file (response).
pub const P9_RREMOVE: u8 = 123;
/// Stat file (request).
pub const P9_TSTAT: u8 = 124;
/// Stat file (response).
pub const P9_RSTAT: u8 = 125;
/// Write stat (request).
pub const P9_TWSTAT: u8 = 126;
/// Write stat (response).
pub const P9_RWSTAT: u8 = 127;

// ---------------------------------------------------------------------------
// 9P open/create modes
// ---------------------------------------------------------------------------

/// Open for read.
pub const P9_OREAD: u8 = 0x00;
/// Open for write.
pub const P9_OWRITE: u8 = 0x01;
/// Open for read/write.
pub const P9_ORDWR: u8 = 0x02;
/// Open for execute.
pub const P9_OEXEC: u8 = 0x03;
/// Truncate on open.
pub const P9_OTRUNC: u8 = 0x10;
/// Remove on close.
pub const P9_ORCLOSE: u8 = 0x40;

// ---------------------------------------------------------------------------
// 9P QID types
// ---------------------------------------------------------------------------

/// Directory.
pub const P9_QTDIR: u8 = 0x80;
/// Append-only.
pub const P9_QTAPPEND: u8 = 0x40;
/// Exclusive use (lock).
pub const P9_QTEXCL: u8 = 0x20;
/// Authentication file.
pub const P9_QTAUTH: u8 = 0x08;
/// Temporary file.
pub const P9_QTTMP: u8 = 0x04;
/// Plain file.
pub const P9_QTFILE: u8 = 0x00;

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

/// Maximum message size (default).
pub const P9_DEFAULT_MSIZE: u32 = 8192;
/// No fid (invalid).
pub const P9_NOFID: u32 = 0xFFFF_FFFF;
/// No tag.
pub const P9_NOTAG: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_types_distinct() {
        let msgs = [
            P9_TVERSION,
            P9_RVERSION,
            P9_TAUTH,
            P9_RAUTH,
            P9_TATTACH,
            P9_RATTACH,
            P9_RERROR,
            P9_TFLUSH,
            P9_RFLUSH,
            P9_TWALK,
            P9_RWALK,
            P9_TOPEN,
            P9_ROPEN,
            P9_TCREATE,
            P9_RCREATE,
            P9_TREAD,
            P9_RREAD,
            P9_TWRITE,
            P9_RWRITE,
            P9_TCLUNK,
            P9_RCLUNK,
            P9_TREMOVE,
            P9_RREMOVE,
            P9_TSTAT,
            P9_RSTAT,
            P9_TWSTAT,
            P9_RWSTAT,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_open_modes_basic_distinct() {
        let modes = [P9_OREAD, P9_OWRITE, P9_ORDWR, P9_OEXEC];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_qid_types_distinct() {
        let types = [
            P9_QTDIR,
            P9_QTAPPEND,
            P9_QTEXCL,
            P9_QTAUTH,
            P9_QTTMP,
            P9_QTFILE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_protocol_constants() {
        assert_eq!(P9_DEFAULT_MSIZE, 8192);
        assert_eq!(P9_NOFID, 0xFFFF_FFFF);
        assert_eq!(P9_NOTAG, 0xFFFF);
    }
}
