//! `<linux/netfilter.h>` (verdict subset) — netfilter verdict codes.
//!
//! Every netfilter hook callback returns a verdict that tells the
//! network stack what to do with the packet. `NF_ACCEPT` continues
//! normal processing, `NF_DROP` silently discards the packet,
//! `NF_QUEUE` sends it to userspace for inspection, etc.

// ---------------------------------------------------------------------------
// Verdict codes (NF_*)
// ---------------------------------------------------------------------------

/// Drop the packet silently.
pub const NF_DROP: u32 = 0;
/// Accept the packet (continue processing).
pub const NF_ACCEPT: u32 = 1;
/// Packet was stolen by the hook (don't free it).
pub const NF_STOLEN: u32 = 2;
/// Queue the packet to userspace (NFQUEUE).
pub const NF_QUEUE: u32 = 3;
/// Repeat the hook (call this hook again).
pub const NF_REPEAT: u32 = 4;
/// Stop processing (used internally).
pub const NF_STOP: u32 = 5;
/// Inject the packet (from NFQUEUE).
pub const NF_QUEUE_NR_BASE: u32 = 0x10;
/// Maximum verdict value.
pub const NF_MAX_VERDICT: u32 = NF_STOP;

// ---------------------------------------------------------------------------
// nftables verdict codes (NFT_*)
// ---------------------------------------------------------------------------

/// Continue to next rule in the chain.
pub const NFT_CONTINUE: i32 = -1;
/// Terminate chain evaluation with drop.
pub const NFT_BREAK: i32 = -2;
/// Jump to another chain.
pub const NFT_JUMP: i32 = -3;
/// Go to another chain (no return).
pub const NFT_GOTO: i32 = -4;
/// Return to calling chain.
pub const NFT_RETURN: i32 = -5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [NF_DROP, NF_ACCEPT, NF_STOLEN, NF_QUEUE, NF_REPEAT, NF_STOP];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }

    #[test]
    fn test_verdicts_sequential() {
        assert_eq!(NF_DROP, 0);
        assert_eq!(NF_ACCEPT, 1);
        assert_eq!(NF_STOLEN, 2);
        assert_eq!(NF_QUEUE, 3);
        assert_eq!(NF_REPEAT, 4);
        assert_eq!(NF_STOP, 5);
    }

    #[test]
    fn test_max_verdict() {
        assert_eq!(NF_MAX_VERDICT, NF_STOP);
    }

    #[test]
    fn test_nft_verdicts_distinct() {
        let nft = [NFT_CONTINUE, NFT_BREAK, NFT_JUMP, NFT_GOTO, NFT_RETURN];
        for i in 0..nft.len() {
            for j in (i + 1)..nft.len() {
                assert_ne!(nft[i], nft[j]);
            }
        }
    }

    #[test]
    fn test_nft_verdicts_negative() {
        // All nftables verdicts are negative to distinguish from NF_* verdicts
        assert!(NFT_CONTINUE < 0);
        assert!(NFT_BREAK < 0);
        assert!(NFT_JUMP < 0);
        assert!(NFT_GOTO < 0);
        assert!(NFT_RETURN < 0);
    }

    #[test]
    fn test_queue_nr_base() {
        assert!(NF_QUEUE_NR_BASE > NF_STOP);
    }
}
