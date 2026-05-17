//! `<linux/psp-sev.h>` — AMD SEV (Secure Encrypted Virtualization) constants.
//!
//! AMD SEV encrypts VM memory with per-VM keys that the hypervisor
//! cannot access. SEV-ES additionally encrypts register state on
//! VMEXIT. SEV-SNP (Secure Nested Paging) adds integrity protection
//! preventing the hypervisor from replaying, remapping, or corrupting
//! guest memory pages. Guests can request attestation reports from the
//! AMD Secure Processor to prove they're running in a genuine SEV
//! environment.

// ---------------------------------------------------------------------------
// SEV platform commands (issued to /dev/sev)
// ---------------------------------------------------------------------------

/// Initialize the SEV platform.
pub const SEV_CMD_INIT: u32 = 0x001;
/// Shut down the SEV platform.
pub const SEV_CMD_SHUTDOWN: u32 = 0x002;
/// Factory reset the platform.
pub const SEV_CMD_FACTORY_RESET: u32 = 0x003;
/// Get platform status.
pub const SEV_CMD_PLATFORM_STATUS: u32 = 0x004;
/// Generate a new PEK (Platform Endorsement Key).
pub const SEV_CMD_PEK_GEN: u32 = 0x005;
/// Sign the PEK with the CEK.
pub const SEV_CMD_PEK_CSR: u32 = 0x006;
/// Import a PEK certificate chain.
pub const SEV_CMD_PEK_CERT_IMPORT: u32 = 0x007;
/// Generate a new PDH (Platform Diffie-Hellman key).
pub const SEV_CMD_PDH_GEN: u32 = 0x008;
/// Export the PDH certificate.
pub const SEV_CMD_PDH_CERT_EXPORT: u32 = 0x009;
/// Get the platform certificate chain.
pub const SEV_CMD_GET_ID: u32 = 0x00A;

// ---------------------------------------------------------------------------
// SEV guest commands (issued to KVM on a running VM)
// ---------------------------------------------------------------------------

/// Launch a new SEV guest.
pub const SEV_CMD_LAUNCH_START: u32 = 0x020;
/// Update guest memory pages during launch.
pub const SEV_CMD_LAUNCH_UPDATE_DATA: u32 = 0x021;
/// Update guest VMSA during launch.
pub const SEV_CMD_LAUNCH_UPDATE_VMSA: u32 = 0x022;
/// Finalize launch and get measurement.
pub const SEV_CMD_LAUNCH_MEASURE: u32 = 0x023;
/// Provide a launch secret (sealed to measurement).
pub const SEV_CMD_LAUNCH_SECRET: u32 = 0x024;
/// Finish launch.
pub const SEV_CMD_LAUNCH_FINISH: u32 = 0x025;

// ---------------------------------------------------------------------------
// SEV guest migration commands
// ---------------------------------------------------------------------------

/// Begin sending a guest (migration source).
pub const SEV_CMD_SEND_START: u32 = 0x030;
/// Send guest memory pages.
pub const SEV_CMD_SEND_UPDATE_DATA: u32 = 0x031;
/// Send guest VMSA.
pub const SEV_CMD_SEND_UPDATE_VMSA: u32 = 0x032;
/// Finish sending.
pub const SEV_CMD_SEND_FINISH: u32 = 0x033;
/// Begin receiving a guest (migration destination).
pub const SEV_CMD_RECEIVE_START: u32 = 0x040;
/// Receive guest memory pages.
pub const SEV_CMD_RECEIVE_UPDATE_DATA: u32 = 0x041;
/// Receive guest VMSA.
pub const SEV_CMD_RECEIVE_UPDATE_VMSA: u32 = 0x042;
/// Finish receiving.
pub const SEV_CMD_RECEIVE_FINISH: u32 = 0x043;

// ---------------------------------------------------------------------------
// SEV-SNP guest request types (via /dev/sev-guest)
// ---------------------------------------------------------------------------

/// Get SNP attestation report.
pub const SNP_GET_REPORT: u32 = 0x100;
/// Get derived key (from VCEK or VLEK).
pub const SNP_GET_DERIVED_KEY: u32 = 0x101;
/// Get extended report (includes certificate chain).
pub const SNP_GET_EXT_REPORT: u32 = 0x102;

// ---------------------------------------------------------------------------
// SEV platform states
// ---------------------------------------------------------------------------

/// Platform is uninitialized.
pub const SEV_STATE_UNINIT: u32 = 0;
/// Platform is initialized.
pub const SEV_STATE_INIT: u32 = 1;
/// Platform is working (processing a command).
pub const SEV_STATE_WORKING: u32 = 2;

// ---------------------------------------------------------------------------
// SEV guest policy flags
// ---------------------------------------------------------------------------

/// Guest must not be debugged.
pub const SEV_POLICY_NO_DEBUG: u32 = 1 << 0;
/// Guest must not be migrated to another platform.
pub const SEV_POLICY_NO_MIGRATE: u32 = 1 << 1;
/// Guest requires SEV-ES.
pub const SEV_POLICY_ES: u32 = 1 << 2;
/// Guest requires SEV-SNP.
pub const SEV_POLICY_SNP: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_commands_distinct() {
        let cmds = [
            SEV_CMD_INIT, SEV_CMD_SHUTDOWN, SEV_CMD_FACTORY_RESET,
            SEV_CMD_PLATFORM_STATUS, SEV_CMD_PEK_GEN, SEV_CMD_PEK_CSR,
            SEV_CMD_PEK_CERT_IMPORT, SEV_CMD_PDH_GEN,
            SEV_CMD_PDH_CERT_EXPORT, SEV_CMD_GET_ID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_guest_commands_distinct() {
        let cmds = [
            SEV_CMD_LAUNCH_START, SEV_CMD_LAUNCH_UPDATE_DATA,
            SEV_CMD_LAUNCH_UPDATE_VMSA, SEV_CMD_LAUNCH_MEASURE,
            SEV_CMD_LAUNCH_SECRET, SEV_CMD_LAUNCH_FINISH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_migration_commands_distinct() {
        let cmds = [
            SEV_CMD_SEND_START, SEV_CMD_SEND_UPDATE_DATA,
            SEV_CMD_SEND_UPDATE_VMSA, SEV_CMD_SEND_FINISH,
            SEV_CMD_RECEIVE_START, SEV_CMD_RECEIVE_UPDATE_DATA,
            SEV_CMD_RECEIVE_UPDATE_VMSA, SEV_CMD_RECEIVE_FINISH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_snp_requests_distinct() {
        let reqs = [SNP_GET_REPORT, SNP_GET_DERIVED_KEY, SNP_GET_EXT_REPORT];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_policy_flags_no_overlap() {
        let flags = [
            SEV_POLICY_NO_DEBUG, SEV_POLICY_NO_MIGRATE,
            SEV_POLICY_ES, SEV_POLICY_SNP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
