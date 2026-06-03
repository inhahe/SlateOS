//! `<net/9p/9p.h>` — Plan 9 Filesystem Protocol (9P).
//!
//! 9P is Plan 9's network filesystem and Linux's transport for shared
//! folders with QEMU/KVM via `virtio-9p` and with WSL2's plan9 mounts.
//! Userspace tools (`mount -t 9p`, `diod`, `nfs-ganesha 9P plugin`)
//! speak the wire protocol whose constants are defined here.

// ---------------------------------------------------------------------------
// Protocol versions (negotiated in `Tversion`)
// ---------------------------------------------------------------------------

pub const NINE_P_VER_2000: &str = "9P2000";
pub const NINE_P_VER_2000_U: &str = "9P2000.u"; // Unix extensions
pub const NINE_P_VER_2000_L: &str = "9P2000.L"; // Linux extensions
pub const NINE_P_VER_UNKNOWN: &str = "unknown";

// ---------------------------------------------------------------------------
// Default message size (negotiated, used as cap)
// ---------------------------------------------------------------------------

/// `msize` lower bound — anything smaller can't carry a stat reply.
pub const NINE_P_MIN_MSIZE: u32 = 4096;

/// Linux 9p client default.
pub const NINE_P_DEFAULT_MSIZE: u32 = 8192;

/// Maximum reasonable msize (virtio-9p uses 512 KiB).
pub const NINE_P_MAX_MSIZE: u32 = 512 * 1024;

// ---------------------------------------------------------------------------
// Message types (`enum p9_msg_t`) — paired T-request / R-reply
// ---------------------------------------------------------------------------

pub const P9_TLERROR: u8 = 6;
pub const P9_RLERROR: u8 = 7;
pub const P9_TSTATFS: u8 = 8;
pub const P9_RSTATFS: u8 = 9;
pub const P9_TLOPEN: u8 = 12;
pub const P9_RLOPEN: u8 = 13;
pub const P9_TLCREATE: u8 = 14;
pub const P9_RLCREATE: u8 = 15;
pub const P9_TSYMLINK: u8 = 16;
pub const P9_RSYMLINK: u8 = 17;
pub const P9_TMKNOD: u8 = 18;
pub const P9_RMKNOD: u8 = 19;
pub const P9_TRENAME: u8 = 20;
pub const P9_RRENAME: u8 = 21;
pub const P9_TVERSION: u8 = 100;
pub const P9_RVERSION: u8 = 101;
pub const P9_TAUTH: u8 = 102;
pub const P9_RAUTH: u8 = 103;
pub const P9_TATTACH: u8 = 104;
pub const P9_RATTACH: u8 = 105;
pub const P9_TFLUSH: u8 = 108;
pub const P9_RFLUSH: u8 = 109;
pub const P9_TWALK: u8 = 110;
pub const P9_RWALK: u8 = 111;
pub const P9_TREAD: u8 = 116;
pub const P9_RREAD: u8 = 117;
pub const P9_TWRITE: u8 = 118;
pub const P9_RWRITE: u8 = 119;
pub const P9_TCLUNK: u8 = 120;
pub const P9_RCLUNK: u8 = 121;
pub const P9_TREMOVE: u8 = 122;
pub const P9_RREMOVE: u8 = 123;
pub const P9_TSTAT: u8 = 124;
pub const P9_RSTAT: u8 = 125;
pub const P9_TWSTAT: u8 = 126;
pub const P9_RWSTAT: u8 = 127;

// ---------------------------------------------------------------------------
// Qid file-type bits (top byte of the 13-byte qid)
// ---------------------------------------------------------------------------

pub const P9_QTDIR: u8 = 0x80;
pub const P9_QTAPPEND: u8 = 0x40;
pub const P9_QTEXCL: u8 = 0x20;
pub const P9_QTMOUNT: u8 = 0x10;
pub const P9_QTAUTH: u8 = 0x08;
pub const P9_QTTMP: u8 = 0x04;
pub const P9_QTSYMLINK: u8 = 0x02;
pub const P9_QTLINK: u8 = 0x01;
pub const P9_QTFILE: u8 = 0x00;

// ---------------------------------------------------------------------------
// Special fids and tags
// ---------------------------------------------------------------------------

/// 0xFFFF_FFFF — "no fid" / "no authentication".
pub const P9_NOFID: u32 = u32::MAX;

/// 0xFFFF — Tversion always uses this tag, before any session is set up.
pub const P9_NOTAG: u16 = u16::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_strings_distinct() {
        let v = [
            NINE_P_VER_2000,
            NINE_P_VER_2000_U,
            NINE_P_VER_2000_L,
            NINE_P_VER_UNKNOWN,
        ];
        for i in 0..v.len() {
            for j in (i + 1)..v.len() {
                assert_ne!(v[i], v[j]);
            }
        }
        // The three real versions share the "9P2000" prefix.
        assert!(NINE_P_VER_2000_U.starts_with("9P2000"));
        assert!(NINE_P_VER_2000_L.starts_with("9P2000"));
    }

    #[test]
    fn test_msize_ordering() {
        assert!(NINE_P_MIN_MSIZE < NINE_P_DEFAULT_MSIZE);
        assert!(NINE_P_DEFAULT_MSIZE < NINE_P_MAX_MSIZE);
        // Power-of-two friendly.
        assert!(NINE_P_MIN_MSIZE.is_power_of_two());
        assert!(NINE_P_DEFAULT_MSIZE.is_power_of_two());
        assert!(NINE_P_MAX_MSIZE.is_power_of_two());
    }

    #[test]
    fn test_t_r_pairs_adjacent_odd_even() {
        // Every request type R is even, every reply is request+1.
        let pairs = [
            (P9_TLERROR, P9_RLERROR),
            (P9_TSTATFS, P9_RSTATFS),
            (P9_TLOPEN, P9_RLOPEN),
            (P9_TLCREATE, P9_RLCREATE),
            (P9_TSYMLINK, P9_RSYMLINK),
            (P9_TMKNOD, P9_RMKNOD),
            (P9_TRENAME, P9_RRENAME),
            (P9_TVERSION, P9_RVERSION),
            (P9_TAUTH, P9_RAUTH),
            (P9_TATTACH, P9_RATTACH),
            (P9_TFLUSH, P9_RFLUSH),
            (P9_TWALK, P9_RWALK),
            (P9_TREAD, P9_RREAD),
            (P9_TWRITE, P9_RWRITE),
            (P9_TCLUNK, P9_RCLUNK),
            (P9_TREMOVE, P9_RREMOVE),
            (P9_TSTAT, P9_RSTAT),
            (P9_TWSTAT, P9_RWSTAT),
        ];
        for (t, r) in pairs {
            assert_eq!(t % 2, 0);
            assert_eq!(r, t + 1);
        }
    }

    #[test]
    fn test_qid_type_bits_single_or_zero() {
        let q = [
            P9_QTDIR,
            P9_QTAPPEND,
            P9_QTEXCL,
            P9_QTMOUNT,
            P9_QTAUTH,
            P9_QTTMP,
            P9_QTSYMLINK,
            P9_QTLINK,
        ];
        for v in q {
            assert!(v.is_power_of_two());
        }
        // FILE has no type bits set — it's the "ordinary regular file"
        // implicit base case.
        assert_eq!(P9_QTFILE, 0);
    }

    #[test]
    fn test_special_sentinels() {
        // Both NOFID and NOTAG are all-ones in their respective widths.
        assert_eq!(P9_NOFID, 0xFFFF_FFFF);
        assert_eq!(P9_NOTAG, 0xFFFF);
    }
}
