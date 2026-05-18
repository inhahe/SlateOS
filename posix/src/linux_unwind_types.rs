//! `<unwind.h>` — Stack unwinding constants.
//!
//! The Itanium ABI stack unwinding interface is used by C++
//! exception handling and DWARF-based debuggers.  These constants
//! define reason codes, action flags, and frame register numbers.

// ---------------------------------------------------------------------------
// _Unwind_Reason_Code values
// ---------------------------------------------------------------------------

/// No reason (unused).
pub const URC_NO_REASON: u32 = 0;
/// Foreign exception caught.
pub const URC_FOREIGN_EXCEPTION_CAUGHT: u32 = 1;
/// Phase 2 error (fatal).
pub const URC_FATAL_PHASE2_ERROR: u32 = 2;
/// Phase 1 error (fatal).
pub const URC_FATAL_PHASE1_ERROR: u32 = 3;
/// Normal stop (phase 1 search found handler).
pub const URC_NORMAL_STOP: u32 = 4;
/// End of stack reached (no handler found).
pub const URC_END_OF_STACK: u32 = 5;
/// Handler found (phase 1 success).
pub const URC_HANDLER_FOUND: u32 = 6;
/// Install context (phase 2 transfer to handler).
pub const URC_INSTALL_CONTEXT: u32 = 7;
/// Continue unwinding (phase 2, skip this frame).
pub const URC_CONTINUE_UNWIND: u32 = 8;

// ---------------------------------------------------------------------------
// _Unwind_Action flags (personality routine actions)
// ---------------------------------------------------------------------------

/// Phase 1: search for handler.
pub const UA_SEARCH_PHASE: u32 = 1;
/// Phase 2: cleanup.
pub const UA_CLEANUP_PHASE: u32 = 2;
/// Handler frame (personality should install context).
pub const UA_HANDLER_FRAME: u32 = 4;
/// Force unwinding (longjmp, thread cancellation).
pub const UA_FORCE_UNWIND: u32 = 8;
/// End of stack (used with force unwind).
pub const UA_END_OF_STACK: u32 = 16;

// ---------------------------------------------------------------------------
// DWARF register numbers (x86_64)
// ---------------------------------------------------------------------------

/// RAX register.
pub const DW_REG_RAX: u32 = 0;
/// RDX register.
pub const DW_REG_RDX: u32 = 1;
/// RCX register.
pub const DW_REG_RCX: u32 = 2;
/// RBX register.
pub const DW_REG_RBX: u32 = 3;
/// RSI register.
pub const DW_REG_RSI: u32 = 4;
/// RDI register.
pub const DW_REG_RDI: u32 = 5;
/// RBP register.
pub const DW_REG_RBP: u32 = 6;
/// RSP register.
pub const DW_REG_RSP: u32 = 7;
/// Return address (RIP).
pub const DW_REG_RETURN_ADDR: u32 = 16;

// ---------------------------------------------------------------------------
// Exception class identifiers
// ---------------------------------------------------------------------------

/// Rust exception class ("RUST\0\0\0\0" encoded as u64).
pub const EXCEPTION_CLASS_RUST: u64 = 0x525553_5400_000000;
/// GNU C++ exception class ("GNUCC++\0" encoded as u64).
pub const EXCEPTION_CLASS_GNUCXX: u64 = 0x474E55_4343_2B2B00;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reason_codes_distinct() {
        let codes = [
            URC_NO_REASON, URC_FOREIGN_EXCEPTION_CAUGHT,
            URC_FATAL_PHASE2_ERROR, URC_FATAL_PHASE1_ERROR,
            URC_NORMAL_STOP, URC_END_OF_STACK,
            URC_HANDLER_FOUND, URC_INSTALL_CONTEXT,
            URC_CONTINUE_UNWIND,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_no_reason_is_zero() {
        assert_eq!(URC_NO_REASON, 0);
    }

    #[test]
    fn test_action_flags_powers_of_two() {
        let flags = [
            UA_SEARCH_PHASE, UA_CLEANUP_PHASE,
            UA_HANDLER_FRAME, UA_FORCE_UNWIND,
            UA_END_OF_STACK,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_action_flags_no_overlap() {
        let flags = [
            UA_SEARCH_PHASE, UA_CLEANUP_PHASE,
            UA_HANDLER_FRAME, UA_FORCE_UNWIND,
            UA_END_OF_STACK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dwarf_regs_distinct() {
        let regs = [
            DW_REG_RAX, DW_REG_RDX, DW_REG_RCX, DW_REG_RBX,
            DW_REG_RSI, DW_REG_RDI, DW_REG_RBP, DW_REG_RSP,
            DW_REG_RETURN_ADDR,
        ];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
        }
    }

    #[test]
    fn test_rax_is_zero() {
        assert_eq!(DW_REG_RAX, 0);
    }

    #[test]
    fn test_return_addr_is_sixteen() {
        assert_eq!(DW_REG_RETURN_ADDR, 16);
    }

    #[test]
    fn test_exception_classes_distinct() {
        assert_ne!(EXCEPTION_CLASS_RUST, EXCEPTION_CLASS_GNUCXX);
    }
}
