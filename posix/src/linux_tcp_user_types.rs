//! `<netinet/tcp.h>` — TCP socket options and connection states.
//!
//! Every server uses these: nginx/HAProxy/postgres for tuning
//! latency (`TCP_NODELAY`, `TCP_CORK`), keepalives (`TCP_KEEPIDLE`),
//! TLS offload (`TCP_ULP`), and BPF inspection (`TCP_BPF_*`).
//! The values are the stable kernel-userspace ABI.

// ---------------------------------------------------------------------------
// `setsockopt`/`getsockopt` level
// ---------------------------------------------------------------------------

pub const IPPROTO_TCP: u32 = 6;
pub const SOL_TCP: u32 = IPPROTO_TCP;

// ---------------------------------------------------------------------------
// `TCP_*` socket options
// ---------------------------------------------------------------------------

pub const TCP_NODELAY: u32 = 1;
pub const TCP_MAXSEG: u32 = 2;
pub const TCP_CORK: u32 = 3;
pub const TCP_KEEPIDLE: u32 = 4;
pub const TCP_KEEPINTVL: u32 = 5;
pub const TCP_KEEPCNT: u32 = 6;
pub const TCP_SYNCNT: u32 = 7;
pub const TCP_LINGER2: u32 = 8;
pub const TCP_DEFER_ACCEPT: u32 = 9;
pub const TCP_WINDOW_CLAMP: u32 = 10;
pub const TCP_INFO: u32 = 11;
pub const TCP_QUICKACK: u32 = 12;
pub const TCP_CONGESTION: u32 = 13;
pub const TCP_MD5SIG: u32 = 14;
pub const TCP_THIN_LINEAR_TIMEOUTS: u32 = 16;
pub const TCP_THIN_DUPACK: u32 = 17;
pub const TCP_USER_TIMEOUT: u32 = 18;
pub const TCP_REPAIR: u32 = 19;
pub const TCP_REPAIR_QUEUE: u32 = 20;
pub const TCP_QUEUE_SEQ: u32 = 21;
pub const TCP_REPAIR_OPTIONS: u32 = 22;
pub const TCP_FASTOPEN: u32 = 23;
pub const TCP_TIMESTAMP: u32 = 24;
pub const TCP_NOTSENT_LOWAT: u32 = 25;
pub const TCP_CC_INFO: u32 = 26;
pub const TCP_SAVE_SYN: u32 = 27;
pub const TCP_SAVED_SYN: u32 = 28;
pub const TCP_REPAIR_WINDOW: u32 = 29;
pub const TCP_FASTOPEN_CONNECT: u32 = 30;
pub const TCP_ULP: u32 = 31;
pub const TCP_MD5SIG_EXT: u32 = 32;
pub const TCP_FASTOPEN_KEY: u32 = 33;
pub const TCP_FASTOPEN_NO_COOKIE: u32 = 34;
pub const TCP_ZEROCOPY_RECEIVE: u32 = 35;
pub const TCP_INQ: u32 = 36;
pub const TCP_TX_DELAY: u32 = 37;

// ---------------------------------------------------------------------------
// TCP connection states (`enum tcp_state`)
// ---------------------------------------------------------------------------

pub const TCP_ESTABLISHED: u8 = 1;
pub const TCP_SYN_SENT: u8 = 2;
pub const TCP_SYN_RECV: u8 = 3;
pub const TCP_FIN_WAIT1: u8 = 4;
pub const TCP_FIN_WAIT2: u8 = 5;
pub const TCP_TIME_WAIT: u8 = 6;
pub const TCP_CLOSE: u8 = 7;
pub const TCP_CLOSE_WAIT: u8 = 8;
pub const TCP_LAST_ACK: u8 = 9;
pub const TCP_LISTEN: u8 = 10;
pub const TCP_CLOSING: u8 = 11;
pub const TCP_NEW_SYN_RECV: u8 = 12;

pub const TCP_MAX_STATES: u8 = 13;

// ---------------------------------------------------------------------------
// Common congestion-control names
// ---------------------------------------------------------------------------

pub const TCP_CONG_RENO: &str = "reno";
pub const TCP_CONG_CUBIC: &str = "cubic";
pub const TCP_CONG_BBR: &str = "bbr";
pub const TCP_CONG_DCTCP: &str = "dctcp";
pub const TCP_CONG_VEGAS: &str = "vegas";

// ---------------------------------------------------------------------------
// Default tunables (from `tcp_input.c`)
// ---------------------------------------------------------------------------

pub const TCP_KEEPALIVE_TIME_S: u32 = 7200;
pub const TCP_KEEPALIVE_INTVL_S: u32 = 75;
pub const TCP_KEEPALIVE_PROBES: u32 = 9;
pub const TCP_RTO_MAX_MS: u32 = 120_000;
pub const TCP_RTO_MIN_MS: u32 = 200;
pub const TCP_MSS_DEFAULT: u32 = 536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_and_sol_match() {
        assert_eq!(IPPROTO_TCP, 6);
        assert_eq!(SOL_TCP, IPPROTO_TCP);
    }

    #[test]
    fn test_core_sockopts_dense_1_to_14() {
        let o = [
            TCP_NODELAY,
            TCP_MAXSEG,
            TCP_CORK,
            TCP_KEEPIDLE,
            TCP_KEEPINTVL,
            TCP_KEEPCNT,
            TCP_SYNCNT,
            TCP_LINGER2,
            TCP_DEFER_ACCEPT,
            TCP_WINDOW_CLAMP,
            TCP_INFO,
            TCP_QUICKACK,
            TCP_CONGESTION,
            TCP_MD5SIG,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_thin_dupack_gap() {
        // TCP_THIN_* skips opt #15 (a removed feature).
        assert_eq!(TCP_THIN_LINEAR_TIMEOUTS, 16);
        assert_eq!(TCP_THIN_DUPACK, 17);
        // Then dense to TCP_TX_DELAY=37.
        assert_eq!(TCP_TX_DELAY, 37);
    }

    #[test]
    fn test_state_machine_dense_1_to_12() {
        let s = [
            TCP_ESTABLISHED,
            TCP_SYN_SENT,
            TCP_SYN_RECV,
            TCP_FIN_WAIT1,
            TCP_FIN_WAIT2,
            TCP_TIME_WAIT,
            TCP_CLOSE,
            TCP_CLOSE_WAIT,
            TCP_LAST_ACK,
            TCP_LISTEN,
            TCP_CLOSING,
            TCP_NEW_SYN_RECV,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        assert_eq!(TCP_MAX_STATES, 13);
    }

    #[test]
    fn test_congestion_names_distinct() {
        let c = [
            TCP_CONG_RENO,
            TCP_CONG_CUBIC,
            TCP_CONG_BBR,
            TCP_CONG_DCTCP,
            TCP_CONG_VEGAS,
        ];
        for a in 0..c.len() {
            for b in (a + 1)..c.len() {
                assert_ne!(c[a], c[b]);
            }
        }
    }

    #[test]
    fn test_default_keepalive_match_legacy_unix() {
        // The Linux defaults match the classic Unix net.ipv4.tcp_keepalive_*
        // sysctls: 2h before first probe, 75s between, 9 probes.
        assert_eq!(TCP_KEEPALIVE_TIME_S, 7200);
        assert_eq!(TCP_KEEPALIVE_INTVL_S, 75);
        assert_eq!(TCP_KEEPALIVE_PROBES, 9);
        // 9 probes × 75s = 675s total grace after the idle window.
        assert_eq!(
            TCP_KEEPALIVE_TIME_S + TCP_KEEPALIVE_INTVL_S * TCP_KEEPALIVE_PROBES,
            7875
        );
        // RTO_MIN ≤ RTO_MAX, and MSS_DEFAULT from RFC 879.
        assert!(TCP_RTO_MIN_MS < TCP_RTO_MAX_MS);
        assert_eq!(TCP_MSS_DEFAULT, 536);
    }
}
