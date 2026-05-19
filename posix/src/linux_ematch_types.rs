//! `<linux/pkt_cls.h>` — TC extended match constants.
//!
//! Traffic control extended match (ematch) constants covering
//! match kinds, operations, and layer definitions.

// ---------------------------------------------------------------------------
// TC ematch kinds
// ---------------------------------------------------------------------------

/// Container.
pub const TCF_EM_CONTAINER: u32 = 0;
/// Comparison.
pub const TCF_EM_CMP: u32 = 1;
/// Number hash.
pub const TCF_EM_NBYTE: u32 = 2;
/// U32.
pub const TCF_EM_U32: u32 = 3;
/// Meta.
pub const TCF_EM_META: u32 = 4;
/// Text.
pub const TCF_EM_TEXT: u32 = 5;
/// Virtual mark.
pub const TCF_EM_VLAN: u32 = 6;
/// Canid.
pub const TCF_EM_CANID: u32 = 7;
/// IPset.
pub const TCF_EM_IPSET: u32 = 8;
/// IPT (iptables).
pub const TCF_EM_IPT: u32 = 9;

// ---------------------------------------------------------------------------
// TC ematch operations
// ---------------------------------------------------------------------------

/// Logical AND.
pub const TCF_EM_REL_AND: u32 = 1;
/// Logical OR.
pub const TCF_EM_REL_OR: u32 = 2;
/// End marker.
pub const TCF_EM_REL_END: u32 = 0;

// ---------------------------------------------------------------------------
// TC ematch layers
// ---------------------------------------------------------------------------

/// Link layer.
pub const TCF_LAYER_LINK: u32 = 0;
/// Network layer.
pub const TCF_LAYER_NETWORK: u32 = 1;
/// Transport layer.
pub const TCF_LAYER_TRANSPORT: u32 = 2;

// ---------------------------------------------------------------------------
// TC ematch flags
// ---------------------------------------------------------------------------

/// Invert match.
pub const TCF_EM_INVERT: u32 = 1 << 0;
/// Simple payload.
pub const TCF_EM_SIMPLE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kinds_distinct() {
        let kinds = [
            TCF_EM_CONTAINER, TCF_EM_CMP, TCF_EM_NBYTE,
            TCF_EM_U32, TCF_EM_META, TCF_EM_TEXT,
            TCF_EM_VLAN, TCF_EM_CANID, TCF_EM_IPSET, TCF_EM_IPT,
        ];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }

    #[test]
    fn test_operations_distinct() {
        let ops = [TCF_EM_REL_AND, TCF_EM_REL_OR, TCF_EM_REL_END];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_layers_distinct() {
        let layers = [TCF_LAYER_LINK, TCF_LAYER_NETWORK, TCF_LAYER_TRANSPORT];
        for i in 0..layers.len() {
            for j in (i + 1)..layers.len() {
                assert_ne!(layers[i], layers[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(TCF_EM_INVERT & TCF_EM_SIMPLE, 0);
    }

    #[test]
    fn test_flags_power_of_two() {
        assert!(TCF_EM_INVERT.is_power_of_two());
        assert!(TCF_EM_SIMPLE.is_power_of_two());
    }
}
