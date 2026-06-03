//! `<linux/netfilter/xt_*.h>` — netfilter xtables target/match return codes.
//!
//! Constants returned by netfilter xtable modules (iptables, ip6tables,
//! arptables) as verdicts from match and target functions. Userspace
//! shadow modules (`libxt_*.so`) and policy parsers (firewalld,
//! nftables compat) consume these.

// ---------------------------------------------------------------------------
// Netfilter verdict codes (NF_* — returned by hooks and targets)
// ---------------------------------------------------------------------------

/// Drop the packet silently.
pub const NF_DROP: u32 = 0;
/// Accept the packet — skip further chain processing.
pub const NF_ACCEPT: u32 = 1;
/// Steal the packet (target took ownership; do not free).
pub const NF_STOLEN: u32 = 2;
/// Queue the packet to userspace (NFQUEUE).
pub const NF_QUEUE: u32 = 3;
/// Re-enter the hook with the next rule.
pub const NF_REPEAT: u32 = 4;
/// Stop traversing this chain immediately.
pub const NF_STOP: u32 = 5;
/// Maximum verdict code currently defined.
pub const NF_MAX_VERDICT: u32 = NF_STOP;

// ---------------------------------------------------------------------------
// Magic XT_RETURN sentinel — appears at the end of every rule chain
// ---------------------------------------------------------------------------

/// XT_RETURN — chain returned to its caller.
pub const XT_RETURN: i32 = -(NF_REPEAT as i32 + 1);
/// XT_CONTINUE — fall through to next rule.
pub const XT_CONTINUE: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// xt_table sizing limits (per-rule and per-chain)
// ---------------------------------------------------------------------------

/// Maximum table-name length (NUL-terminated).
pub const XT_TABLE_MAXNAMELEN: u32 = 32;
/// Maximum target-name length.
pub const XT_EXTENSION_MAXNAMELEN: u32 = 29;
/// Maximum number of hook functions per table.
pub const XT_FUNCTION_MAXNAMELEN: u32 = 30;

// ---------------------------------------------------------------------------
// Standard target sentinel sizes (used to detect "implicit" targets)
// ---------------------------------------------------------------------------

/// Size (bytes) of an empty xt_standard_target entry.
pub const XT_STANDARD_TARGET_SIZE: u32 = 64;
/// Size (bytes) of the xt_error_target trailer.
pub const XT_ERROR_TARGET_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [
            NF_DROP, NF_ACCEPT, NF_STOLEN, NF_QUEUE, NF_REPEAT, NF_STOP,
        ];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
        assert_eq!(NF_MAX_VERDICT, NF_STOP);
    }

    #[test]
    fn test_xt_return_is_negative_distinct_marker() {
        // XT_RETURN must be negative so it can never be confused
        // with a valid u32 verdict code returned by a match.
        assert!(XT_RETURN < 0);
        // CONTINUE is the all-ones marker used by the kernel.
        assert_eq!(XT_CONTINUE, !0u32);
    }

    #[test]
    fn test_name_lengths_within_typical_limits() {
        // All name limits must fit a single page entry header and
        // remain below the historical iptables limit (32 bytes).
        assert!(XT_TABLE_MAXNAMELEN <= 64);
        assert!(XT_EXTENSION_MAXNAMELEN < XT_TABLE_MAXNAMELEN);
        assert!(XT_FUNCTION_MAXNAMELEN < XT_TABLE_MAXNAMELEN);
    }

    #[test]
    fn test_target_sizes_equal() {
        // Standard and error targets must share the same size so
        // chain walkers can advance by a constant.
        assert_eq!(XT_STANDARD_TARGET_SIZE, XT_ERROR_TARGET_SIZE);
        assert!(XT_STANDARD_TARGET_SIZE >= 32);
    }
}
