//! `<sys/un.h>` / `<linux/un.h>` — Unix-domain socket address ABI.
//!
//! `AF_UNIX` (a.k.a. `AF_LOCAL`) sockets carry data between processes
//! on the same host with full credential passing and fd passing. The
//! address is a filesystem path (or an "abstract" name in Linux's
//! `\0`-prefixed namespace), and ancillary messages (`SCM_*`) move
//! fds and credentials between peers.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

pub const AF_UNIX: u32 = 1;
pub const AF_LOCAL: u32 = AF_UNIX;
pub const PF_UNIX: u32 = AF_UNIX;
pub const PF_LOCAL: u32 = AF_UNIX;

// ---------------------------------------------------------------------------
// Path length — `struct sockaddr_un.sun_path[]`
// ---------------------------------------------------------------------------

/// Linux `sun_path` is 108 bytes (much longer than the BSD 104).
pub const UNIX_PATH_MAX: usize = 108;

// ---------------------------------------------------------------------------
// `setsockopt(SOL_SOCKET, …)` levels & SCM control message types
// ---------------------------------------------------------------------------

pub const SOL_SOCKET: u32 = 1;

pub const SCM_RIGHTS: u32 = 0x01;
pub const SCM_CREDENTIALS: u32 = 0x02;
pub const SCM_SECURITY: u32 = 0x03;
pub const SCM_PIDFD: u32 = 0x04;
pub const SCM_TIMESTAMP: u32 = 29;
pub const SCM_TIMESTAMPNS: u32 = 35;
pub const SCM_TIMESTAMPING: u32 = 37;

// ---------------------------------------------------------------------------
// `getsockopt` queries unique to AF_UNIX
// ---------------------------------------------------------------------------

pub const SO_PEERCRED: u32 = 17;
pub const SO_PASSCRED: u32 = 16;
pub const SO_PEERSEC: u32 = 31;
pub const SO_PASSSEC: u32 = 34;
pub const SO_PEERGROUPS: u32 = 59;
pub const SO_PEERPIDFD: u32 = 77;

// ---------------------------------------------------------------------------
// Abstract-namespace marker — first byte of sun_path is NUL.
// ---------------------------------------------------------------------------

/// First byte of `sun_path` is 0 for abstract sockets (Linux extension).
pub const UNIX_ABSTRACT_FIRST_BYTE: u8 = 0;

// ---------------------------------------------------------------------------
// Per-connection backlog cap defaults
// ---------------------------------------------------------------------------

/// `net.unix.max_dgram_qlen` default — datagrams queued per receiver.
pub const UNIX_DEFAULT_MAX_DGRAM_QLEN: u32 = 10;

// ---------------------------------------------------------------------------
// Common bind paths used by system services
// ---------------------------------------------------------------------------

pub const DEV_LOG_SOCKET: &str = "/dev/log";
pub const VAR_RUN_DBUS_SYSTEM: &str = "/var/run/dbus/system_bus_socket";
pub const RUN_DBUS_SYSTEM: &str = "/run/dbus/system_bus_socket";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_unix_is_1_and_aliases_match() {
        assert_eq!(AF_UNIX, 1);
        assert_eq!(AF_LOCAL, AF_UNIX);
        assert_eq!(PF_UNIX, AF_UNIX);
        assert_eq!(PF_LOCAL, AF_UNIX);
    }

    #[test]
    fn test_sun_path_max_is_108() {
        // Linux raised the BSD 104 to 108. A NUL-terminated path can be
        // at most UNIX_PATH_MAX - 1 = 107 characters.
        assert_eq!(UNIX_PATH_MAX, 108);
    }

    #[test]
    fn test_scm_rights_credentials_security_dense_1_to_4() {
        // The "transfer kernel resource" SCM types are 1-4 in order
        // they were added.
        let s = [SCM_RIGHTS, SCM_CREDENTIALS, SCM_SECURITY, SCM_PIDFD];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_so_peercred_after_so_passcred() {
        // SO_PASSCRED=16 enables the feature; SO_PEERCRED=17 queries it.
        // They were added together.
        assert_eq!(SO_PASSCRED, 16);
        assert_eq!(SO_PEERCRED, SO_PASSCRED + 1);
    }

    #[test]
    fn test_abstract_first_byte_is_nul() {
        // Linux's "abstract" namespace is indicated by sun_path[0] == 0,
        // then the remaining bytes are the name (no filesystem inode).
        assert_eq!(UNIX_ABSTRACT_FIRST_BYTE, 0);
    }

    #[test]
    fn test_default_dgram_qlen_is_10() {
        // The historical default — small to push back on unbounded
        // memory growth in the receiver.
        assert_eq!(UNIX_DEFAULT_MAX_DGRAM_QLEN, 10);
    }

    #[test]
    fn test_dbus_path_layout() {
        // Both classic and modern paths end with the same basename.
        assert!(VAR_RUN_DBUS_SYSTEM.ends_with("system_bus_socket"));
        assert!(RUN_DBUS_SYSTEM.ends_with("system_bus_socket"));
        assert_eq!(DEV_LOG_SOCKET, "/dev/log");
    }
}
