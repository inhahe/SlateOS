//! `<sys/syslog.h>` — kernel `syslog(2)` and RFC 5424 priorities.
//!
//! Two unrelated APIs sit under the same `<sys/syslog.h>` header:
//! the kernel `syslog(2)` action codes (used by `dmesg` and
//! `klogd`) and the userspace `openlog/syslog` priority/facility
//! encoding (used by every daemon). Both are stable wire formats.

// ---------------------------------------------------------------------------
// `syslog(2)` kernel commands (`SYSLOG_ACTION_*`)
// ---------------------------------------------------------------------------

pub const SYSLOG_ACTION_CLOSE: u32 = 0;
pub const SYSLOG_ACTION_OPEN: u32 = 1;
pub const SYSLOG_ACTION_READ: u32 = 2;
pub const SYSLOG_ACTION_READ_ALL: u32 = 3;
pub const SYSLOG_ACTION_READ_CLEAR: u32 = 4;
pub const SYSLOG_ACTION_CLEAR: u32 = 5;
pub const SYSLOG_ACTION_CONSOLE_OFF: u32 = 6;
pub const SYSLOG_ACTION_CONSOLE_ON: u32 = 7;
pub const SYSLOG_ACTION_CONSOLE_LEVEL: u32 = 8;
pub const SYSLOG_ACTION_SIZE_UNREAD: u32 = 9;
pub const SYSLOG_ACTION_SIZE_BUFFER: u32 = 10;

// ---------------------------------------------------------------------------
// RFC 5424 severities (`LOG_*`)
// ---------------------------------------------------------------------------

pub const LOG_EMERG: u32 = 0;
pub const LOG_ALERT: u32 = 1;
pub const LOG_CRIT: u32 = 2;
pub const LOG_ERR: u32 = 3;
pub const LOG_WARNING: u32 = 4;
pub const LOG_NOTICE: u32 = 5;
pub const LOG_INFO: u32 = 6;
pub const LOG_DEBUG: u32 = 7;

pub const LOG_PRIMASK: u32 = 0x07;

// ---------------------------------------------------------------------------
// RFC 5424 facilities (`LOG_KERN` … `LOG_LOCAL7`)
// ---------------------------------------------------------------------------

pub const LOG_KERN: u32 = 0 << 3;
pub const LOG_USER: u32 = 1 << 3;
pub const LOG_MAIL: u32 = 2 << 3;
pub const LOG_DAEMON: u32 = 3 << 3;
pub const LOG_AUTH: u32 = 4 << 3;
pub const LOG_SYSLOG: u32 = 5 << 3;
pub const LOG_LPR: u32 = 6 << 3;
pub const LOG_NEWS: u32 = 7 << 3;
pub const LOG_UUCP: u32 = 8 << 3;
pub const LOG_CRON: u32 = 9 << 3;
pub const LOG_AUTHPRIV: u32 = 10 << 3;
pub const LOG_FTP: u32 = 11 << 3;
pub const LOG_LOCAL0: u32 = 16 << 3;
pub const LOG_LOCAL1: u32 = 17 << 3;
pub const LOG_LOCAL2: u32 = 18 << 3;
pub const LOG_LOCAL3: u32 = 19 << 3;
pub const LOG_LOCAL4: u32 = 20 << 3;
pub const LOG_LOCAL5: u32 = 21 << 3;
pub const LOG_LOCAL6: u32 = 22 << 3;
pub const LOG_LOCAL7: u32 = 23 << 3;

pub const LOG_FACMASK: u32 = 0x03F8;
pub const LOG_NFACILITIES: u32 = 24;

// ---------------------------------------------------------------------------
// `openlog(3)` option flags
// ---------------------------------------------------------------------------

pub const LOG_PID: u32 = 0x01;
pub const LOG_CONS: u32 = 0x02;
pub const LOG_ODELAY: u32 = 0x04;
pub const LOG_NDELAY: u32 = 0x08;
pub const LOG_NOWAIT: u32 = 0x10;
pub const LOG_PERROR: u32 = 0x20;

// ---------------------------------------------------------------------------
// Syscall
// ---------------------------------------------------------------------------

pub const NR_SYSLOG: u32 = 103;

/// `/dev/log` — Unix-domain socket every libc `syslog()` connects to.
pub const DEV_LOG: &str = "/dev/log";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_dense_0_to_10() {
        let a = [
            SYSLOG_ACTION_CLOSE,
            SYSLOG_ACTION_OPEN,
            SYSLOG_ACTION_READ,
            SYSLOG_ACTION_READ_ALL,
            SYSLOG_ACTION_READ_CLEAR,
            SYSLOG_ACTION_CLEAR,
            SYSLOG_ACTION_CONSOLE_OFF,
            SYSLOG_ACTION_CONSOLE_ON,
            SYSLOG_ACTION_CONSOLE_LEVEL,
            SYSLOG_ACTION_SIZE_UNREAD,
            SYSLOG_ACTION_SIZE_BUFFER,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_severity_dense_0_to_7() {
        let s = [
            LOG_EMERG, LOG_ALERT, LOG_CRIT, LOG_ERR, LOG_WARNING, LOG_NOTICE, LOG_INFO, LOG_DEBUG,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // PRIMASK is exactly the low 3 bits.
        assert_eq!(LOG_PRIMASK, 0x07);
    }

    #[test]
    fn test_facility_shifted_left_3() {
        // Facilities live in bits 3..9 — multiples of 8.
        let f = [
            LOG_KERN,
            LOG_USER,
            LOG_MAIL,
            LOG_DAEMON,
            LOG_AUTH,
            LOG_SYSLOG,
            LOG_LPR,
            LOG_NEWS,
            LOG_UUCP,
            LOG_CRON,
            LOG_AUTHPRIV,
            LOG_FTP,
            LOG_LOCAL0,
            LOG_LOCAL1,
            LOG_LOCAL2,
            LOG_LOCAL3,
            LOG_LOCAL4,
            LOG_LOCAL5,
            LOG_LOCAL6,
            LOG_LOCAL7,
        ];
        for v in f {
            assert_eq!(v & 0x07, 0);
            assert!(v <= LOG_FACMASK);
        }
        // LOG_LOCAL7 sits at the high end of the facility space.
        assert_eq!(LOG_LOCAL7 >> 3, 23);
        assert_eq!(LOG_NFACILITIES, 24);
    }

    #[test]
    fn test_priority_facility_disjoint() {
        // PRIMASK (low 3 bits) and FACMASK (bits 3..9) don't overlap.
        assert_eq!(LOG_PRIMASK & LOG_FACMASK, 0);
        // Their union covers bits 0..9.
        assert_eq!(LOG_PRIMASK | LOG_FACMASK, 0x03FF);
    }

    #[test]
    fn test_openlog_flags_low_6_bits_dense() {
        let o = [LOG_PID, LOG_CONS, LOG_ODELAY, LOG_NDELAY, LOG_NOWAIT, LOG_PERROR];
        let mut or = 0u32;
        for (i, v) in o.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0x3F);
    }

    #[test]
    fn test_syscall_number_and_dev_log_path() {
        // syslog(2) is syscall 103 on x86_64.
        assert_eq!(NR_SYSLOG, 103);
        assert_eq!(DEV_LOG, "/dev/log");
    }
}
