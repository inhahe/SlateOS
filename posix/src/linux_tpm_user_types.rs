//! `<linux/tpm.h>` — `/dev/tpm0` and `/dev/tpmrm0` character-device ABI.
//!
//! Two device nodes are exposed: `/dev/tpm0` (raw, blocking) and
//! `/dev/tpmrm0` (resource manager — multiplexes the chip across
//! processes using virtual handles). Userspace (tpm2-tss, swtpm,
//! systemd-cryptsetup with `tpm2-device=`) writes TPM2_* command
//! bodies and reads responses with the headers below.

// ---------------------------------------------------------------------------
// Buffer sizes
// ---------------------------------------------------------------------------

/// Minimum TPM2 command/response buffer (header size: tag u16 +
/// size u32 + code u32 = 10 bytes).
pub const TPM_HEADER_SIZE: usize = 10;
/// Maximum I/O buffer the kernel accepts (4 KiB).
pub const TPM_BUFSIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Command tags (TPMI_ST_COMMAND_TAG)
// ---------------------------------------------------------------------------

/// `TPM_ST_NO_SESSIONS` — bare command, no sessions in the body.
pub const TPM_ST_NO_SESSIONS: u16 = 0x8001;
/// `TPM_ST_SESSIONS` — command body includes a session area.
pub const TPM_ST_SESSIONS: u16 = 0x8002;

// ---------------------------------------------------------------------------
// Common command codes (TPMI_CC_*)
// ---------------------------------------------------------------------------

/// `TPM2_CC_Startup`.
pub const TPM2_CC_STARTUP: u32 = 0x0000_0144;
/// `TPM2_CC_Shutdown`.
pub const TPM2_CC_SHUTDOWN: u32 = 0x0000_0145;
/// `TPM2_CC_SelfTest`.
pub const TPM2_CC_SELF_TEST: u32 = 0x0000_0143;
/// `TPM2_CC_GetRandom`.
pub const TPM2_CC_GET_RANDOM: u32 = 0x0000_017b;
/// `TPM2_CC_PCR_Read`.
pub const TPM2_CC_PCR_READ: u32 = 0x0000_017e;
/// `TPM2_CC_PCR_Extend`.
pub const TPM2_CC_PCR_EXTEND: u32 = 0x0000_0182;
/// `TPM2_CC_GetCapability`.
pub const TPM2_CC_GET_CAPABILITY: u32 = 0x0000_017a;
/// `TPM2_CC_FlushContext`.
pub const TPM2_CC_FLUSH_CONTEXT: u32 = 0x0000_0165;

// ---------------------------------------------------------------------------
// Common return codes (TPM_RC_*)
// ---------------------------------------------------------------------------

/// Success.
pub const TPM_RC_SUCCESS: u32 = 0x0000_0000;
/// `TPM_RC_INITIALIZE` — Startup() not yet called.
pub const TPM_RC_INITIALIZE: u32 = 0x0000_0100;
/// `TPM_RC_FAILURE` — generic failure.
pub const TPM_RC_FAILURE: u32 = 0x0000_0101;
/// `TPM_RC_BAD_TAG` — header tag invalid.
pub const TPM_RC_BAD_TAG: u32 = 0x0000_001e;

// ---------------------------------------------------------------------------
// Startup type (parameter to TPM2_Startup)
// ---------------------------------------------------------------------------

/// `TPM_SU_CLEAR` — power-on reset (clear state).
pub const TPM_SU_CLEAR: u16 = 0x0000;
/// `TPM_SU_STATE` — restore saved state (after Shutdown(STATE)).
pub const TPM_SU_STATE: u16 = 0x0001;

// ---------------------------------------------------------------------------
// Capability categories (TPM_CAP_*)
// ---------------------------------------------------------------------------

/// `TPM_CAP_ALGS` — algorithm properties.
pub const TPM_CAP_ALGS: u32 = 0x0000_0000;
/// `TPM_CAP_HANDLES` — handles of a given type.
pub const TPM_CAP_HANDLES: u32 = 0x0000_0001;
/// `TPM_CAP_COMMANDS` — supported command codes.
pub const TPM_CAP_COMMANDS: u32 = 0x0000_0002;
/// `TPM_CAP_TPM_PROPERTIES` — fixed/var TPM properties.
pub const TPM_CAP_TPM_PROPERTIES: u32 = 0x0000_0006;
/// `TPM_CAP_PCRS` — defined PCRs.
pub const TPM_CAP_PCRS: u32 = 0x0000_0005;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_and_buffer_sizes() {
        // tag(2) + size(4) + code(4) = 10.
        assert_eq!(TPM_HEADER_SIZE, 10);
        assert!(TPM_BUFSIZE.is_power_of_two());
        assert_eq!(TPM_BUFSIZE, 4096);
        assert!(TPM_BUFSIZE > TPM_HEADER_SIZE);
    }

    #[test]
    fn test_command_tags_distinct_and_high_bit_set() {
        // Standard TPM tags live in 0x8000-range to be distinct from
        // legacy TPM1.2 0x00C1/0x00C2 tags.
        assert_ne!(TPM_ST_NO_SESSIONS, TPM_ST_SESSIONS);
        assert!(TPM_ST_NO_SESSIONS & 0x8000 != 0);
        assert!(TPM_ST_SESSIONS & 0x8000 != 0);
    }

    #[test]
    fn test_command_codes_distinct() {
        let c = [
            TPM2_CC_STARTUP,
            TPM2_CC_SHUTDOWN,
            TPM2_CC_SELF_TEST,
            TPM2_CC_GET_RANDOM,
            TPM2_CC_PCR_READ,
            TPM2_CC_PCR_EXTEND,
            TPM2_CC_GET_CAPABILITY,
            TPM2_CC_FLUSH_CONTEXT,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // All command codes live in the 0x0000_01xx range
            // (TPM-allocated baseline commands).
            assert!(c[i] >= 0x0000_0100 && c[i] < 0x0000_0200);
        }
    }

    #[test]
    fn test_return_codes_distinct() {
        let r = [
            TPM_RC_SUCCESS,
            TPM_RC_INITIALIZE,
            TPM_RC_FAILURE,
            TPM_RC_BAD_TAG,
        ];
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
        // 0 == success is a wire-level requirement.
        assert_eq!(TPM_RC_SUCCESS, 0);
    }

    #[test]
    fn test_startup_types() {
        assert_eq!(TPM_SU_CLEAR, 0);
        assert_eq!(TPM_SU_STATE, 1);
    }

    #[test]
    fn test_capabilities_distinct() {
        let c = [
            TPM_CAP_ALGS,
            TPM_CAP_HANDLES,
            TPM_CAP_COMMANDS,
            TPM_CAP_TPM_PROPERTIES,
            TPM_CAP_PCRS,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }
}
