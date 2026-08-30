//! `<linux/bpf.h>` — BPF program-load / object naming constants.
//!
//! These are the small constants every `bpftool` and `libbpf`
//! caller touches when loading a program: the object-name length,
//! the license-string sentinels, the log-buffer cap, and the
//! per-object metadata limits.

// ---------------------------------------------------------------------------
// Name / size limits
// ---------------------------------------------------------------------------

/// Maximum length of an object name (program, map, link).
pub const BPF_OBJ_NAME_LEN: usize = 16;

/// Maximum verifier log-buffer size (16 MiB).
pub const BPF_LOG_BUF_MAX: u32 = 16 * 1024 * 1024;

/// Default verifier log-buffer size (64 KiB).
pub const BPF_LOG_BUF_DEFAULT: u32 = 64 * 1024;

/// Maximum BPF instructions per program (1M, post-5.2).
pub const BPF_COMPLEXITY_LIMIT_INSNS: u32 = 1_000_000;

/// Maximum jump-stack depth.
pub const BPF_COMPLEXITY_LIMIT_JMP_SEQ: u32 = 8_192;

// ---------------------------------------------------------------------------
// License strings the verifier recognises as "GPL-compatible"
// ---------------------------------------------------------------------------

pub const BPF_LICENSE_GPL: &str = "GPL";
pub const BPF_LICENSE_GPL_V2: &str = "GPL v2";
pub const BPF_LICENSE_DUAL_BSD_GPL: &str = "Dual BSD/GPL";
pub const BPF_LICENSE_DUAL_MIT_GPL: &str = "Dual MIT/GPL";
pub const BPF_LICENSE_DUAL_MPL_GPL: &str = "Dual MPL/GPL";

// ---------------------------------------------------------------------------
// Log-level bits in `BPF_PROG_LOAD.log_level`
// ---------------------------------------------------------------------------

pub const BPF_LOG_LEVEL1: u32 = 1 << 0;
pub const BPF_LOG_LEVEL2: u32 = 1 << 1;
pub const BPF_LOG_STATS: u32 = 1 << 2;

/// Combined mask of all log-level bits.
pub const BPF_LOG_LEVEL_MASK: u32 = BPF_LOG_LEVEL1 | BPF_LOG_LEVEL2 | BPF_LOG_STATS;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_obj_name_len() {
        assert_eq!(BPF_OBJ_NAME_LEN, 16);
        assert!(BPF_OBJ_NAME_LEN.is_power_of_two());
    }

    #[test]
    fn test_log_buf_bounds() {
        assert_eq!(BPF_LOG_BUF_DEFAULT, 64 * 1024);
        assert_eq!(BPF_LOG_BUF_MAX, 16 * 1024 * 1024);
        assert!(BPF_LOG_BUF_DEFAULT < BPF_LOG_BUF_MAX);
        assert!(BPF_LOG_BUF_DEFAULT.is_power_of_two());
        assert!(BPF_LOG_BUF_MAX.is_power_of_two());
        // 256x size ratio.
        assert_eq!(BPF_LOG_BUF_MAX / BPF_LOG_BUF_DEFAULT, 256);
    }

    #[test]
    fn test_complexity_bounds() {
        assert_eq!(BPF_COMPLEXITY_LIMIT_INSNS, 1_000_000);
        assert_eq!(BPF_COMPLEXITY_LIMIT_JMP_SEQ, 8_192);
        assert!(BPF_COMPLEXITY_LIMIT_JMP_SEQ.is_power_of_two());
        assert!(BPF_COMPLEXITY_LIMIT_INSNS > BPF_COMPLEXITY_LIMIT_JMP_SEQ);
    }

    #[test]
    fn test_license_strings_distinct() {
        let l = [
            BPF_LICENSE_GPL,
            BPF_LICENSE_GPL_V2,
            BPF_LICENSE_DUAL_BSD_GPL,
            BPF_LICENSE_DUAL_MIT_GPL,
            BPF_LICENSE_DUAL_MPL_GPL,
        ];
        for (i, &x) in l.iter().enumerate() {
            for &y in &l[i + 1..] {
                assert_ne!(x, y);
            }
            // All GPL-compatible licenses include "GPL".
            assert!(x.contains("GPL"));
        }
        // The three Dual-* licenses share a prefix.
        for &v in &[
            BPF_LICENSE_DUAL_BSD_GPL,
            BPF_LICENSE_DUAL_MIT_GPL,
            BPF_LICENSE_DUAL_MPL_GPL,
        ] {
            assert!(v.starts_with("Dual "));
        }
    }

    #[test]
    fn test_log_level_bits_single_and_mask() {
        let f = [BPF_LOG_LEVEL1, BPF_LOG_LEVEL2, BPF_LOG_STATS];
        let mut or = 0u32;
        for &v in &f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0b111);
        assert_eq!(BPF_LOG_LEVEL_MASK, 0b111);
    }
}
