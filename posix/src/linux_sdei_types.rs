//! `<linux/arm_sdei.h>` — Software Delegated Exception Interface (SDEI) constants.
//!
//! SDEI is an ARM specification that allows firmware to deliver
//! asynchronous events to the OS. It's used primarily for GHES error
//! delivery on ARM platforms (replacing NMI, which ARM traditionally
//! lacks). The OS registers event handlers with the firmware, and
//! when a hardware error occurs, firmware invokes the handler via
//! a special exception entry. SDEI is also used for watchdog timeout
//! notification and firmware-first error handling.

// ---------------------------------------------------------------------------
// SDEI event types
// ---------------------------------------------------------------------------

/// Shared event (system-wide, any PE can handle).
pub const SDEI_EVENT_TYPE_SHARED: u32 = 0;
/// Private event (per-PE, only the specific PE handles).
pub const SDEI_EVENT_TYPE_PRIVATE: u32 = 1;

// ---------------------------------------------------------------------------
// SDEI event priority
// ---------------------------------------------------------------------------

/// Normal priority event.
pub const SDEI_EVENT_PRIORITY_NORMAL: u32 = 0;
/// Critical priority event (higher priority than normal).
pub const SDEI_EVENT_PRIORITY_CRITICAL: u32 = 1;

// ---------------------------------------------------------------------------
// SDEI function IDs (SMC/HVC calls)
// ---------------------------------------------------------------------------

/// Get SDEI version.
pub const SDEI_FN_VERSION: u32 = 0xC400_0020;
/// Register an event handler.
pub const SDEI_FN_REGISTER: u32 = 0xC400_0021;
/// Enable an event.
pub const SDEI_FN_ENABLE: u32 = 0xC400_0022;
/// Disable an event.
pub const SDEI_FN_DISABLE: u32 = 0xC400_0023;
/// Get event context (during handler execution).
pub const SDEI_FN_CONTEXT: u32 = 0xC400_0024;
/// Complete event handling.
pub const SDEI_FN_COMPLETE: u32 = 0xC400_0025;
/// Complete and resume from previous context.
pub const SDEI_FN_COMPLETE_AND_RESUME: u32 = 0xC400_0026;
/// Unregister an event handler.
pub const SDEI_FN_UNREGISTER: u32 = 0xC400_0027;
/// Get event status.
pub const SDEI_FN_STATUS: u32 = 0xC400_0028;
/// Get event info.
pub const SDEI_FN_INFO: u32 = 0xC400_0029;
/// PE unmask (allow events on this PE).
pub const SDEI_FN_PE_UNMASK: u32 = 0xC400_002A;
/// PE mask (block events on this PE).
pub const SDEI_FN_PE_MASK: u32 = 0xC400_002B;
/// Interrupt bind (bind SDEI event to a hardware interrupt).
pub const SDEI_FN_INTERRUPT_BIND: u32 = 0xC400_002C;
/// Interrupt release (unbind SDEI event from interrupt).
pub const SDEI_FN_INTERRUPT_RELEASE: u32 = 0xC400_002D;
/// Reset (unregister all events).
pub const SDEI_FN_RESET: u32 = 0xC400_0031;

// ---------------------------------------------------------------------------
// SDEI return codes
// ---------------------------------------------------------------------------

/// Success.
pub const SDEI_SUCCESS: i32 = 0;
/// Not supported.
pub const SDEI_NOT_SUPPORTED: i32 = -1;
/// Invalid parameters.
pub const SDEI_INVALID_PARAMETERS: i32 = -2;
/// Denied (insufficient privilege).
pub const SDEI_DENIED: i32 = -3;
/// Pending (event is being processed).
pub const SDEI_PENDING: i32 = -5;
/// Out of resource (too many registrations).
pub const SDEI_OUT_OF_RESOURCE: i32 = -10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        assert_ne!(SDEI_EVENT_TYPE_SHARED, SDEI_EVENT_TYPE_PRIVATE);
    }

    #[test]
    fn test_priorities_distinct() {
        assert_ne!(SDEI_EVENT_PRIORITY_NORMAL, SDEI_EVENT_PRIORITY_CRITICAL);
    }

    #[test]
    fn test_function_ids_distinct() {
        let fns = [
            SDEI_FN_VERSION, SDEI_FN_REGISTER, SDEI_FN_ENABLE,
            SDEI_FN_DISABLE, SDEI_FN_CONTEXT, SDEI_FN_COMPLETE,
            SDEI_FN_COMPLETE_AND_RESUME, SDEI_FN_UNREGISTER,
            SDEI_FN_STATUS, SDEI_FN_INFO, SDEI_FN_PE_UNMASK,
            SDEI_FN_PE_MASK, SDEI_FN_INTERRUPT_BIND,
            SDEI_FN_INTERRUPT_RELEASE, SDEI_FN_RESET,
        ];
        for i in 0..fns.len() {
            for j in (i + 1)..fns.len() {
                assert_ne!(fns[i], fns[j]);
            }
        }
    }

    #[test]
    fn test_return_codes_distinct() {
        let codes = [
            SDEI_SUCCESS, SDEI_NOT_SUPPORTED,
            SDEI_INVALID_PARAMETERS, SDEI_DENIED,
            SDEI_PENDING, SDEI_OUT_OF_RESOURCE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
