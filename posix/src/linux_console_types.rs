//! `<linux/console.h>` — Linux console constants.
//!
//! The Linux console subsystem manages text-mode display output.
//! Multiple virtual consoles (VT1-VT63) can exist simultaneously,
//! each with its own screen buffer. The console driver handles text
//! rendering, cursor management, scrolling, and color attributes.
//! Console output goes to the active VT and to any registered
//! console devices (serial console, netconsole, etc.).

// ---------------------------------------------------------------------------
// Console types
// ---------------------------------------------------------------------------

/// VGA text-mode console.
pub const CONSOLE_TYPE_VGA: u32 = 0;
/// Framebuffer console (fbcon).
pub const CONSOLE_TYPE_FB: u32 = 1;
/// Serial console (UART).
pub const CONSOLE_TYPE_SERIAL: u32 = 2;
/// Network console (netconsole).
pub const CONSOLE_TYPE_NET: u32 = 3;
/// Dummy console (no output).
pub const CONSOLE_TYPE_DUMMY: u32 = 4;

// ---------------------------------------------------------------------------
// Console flags
// ---------------------------------------------------------------------------

/// Console is enabled (receives output).
pub const CON_ENABLED: u32 = 0x0001;
/// Console can be used during boot (before full init).
pub const CON_BOOT: u32 = 0x0002;
/// Console is the preferred one (receives printk output).
pub const CON_CONSDEV: u32 = 0x0004;
/// Console supports ANSI escape sequences.
pub const CON_ANSI: u32 = 0x0008;
/// Console can be used in atomic/NMI context.
pub const CON_NBCON: u32 = 0x0010;
/// Console output is buffered (ring buffer backed).
pub const CON_BRL: u32 = 0x0020;
/// Console is registered.
pub const CON_REGISTERED: u32 = 0x0040;

// ---------------------------------------------------------------------------
// Console log levels (for console_loglevel)
// ---------------------------------------------------------------------------

/// Emergency messages only.
pub const CONSOLE_LOGLEVEL_EMERGENCY: u32 = 0;
/// Alerts and above.
pub const CONSOLE_LOGLEVEL_ALERT: u32 = 1;
/// Critical and above.
pub const CONSOLE_LOGLEVEL_CRITICAL: u32 = 2;
/// Errors and above.
pub const CONSOLE_LOGLEVEL_ERROR: u32 = 3;
/// Warnings and above.
pub const CONSOLE_LOGLEVEL_WARNING: u32 = 4;
/// Notice and above.
pub const CONSOLE_LOGLEVEL_NOTICE: u32 = 5;
/// Informational and above.
pub const CONSOLE_LOGLEVEL_INFO: u32 = 6;
/// Debug and above (all messages).
pub const CONSOLE_LOGLEVEL_DEBUG: u32 = 7;
/// Default console loglevel (warnings and above).
pub const CONSOLE_LOGLEVEL_DEFAULT: u32 = 4;

// ---------------------------------------------------------------------------
// Maximum virtual consoles
// ---------------------------------------------------------------------------

/// Maximum number of virtual consoles.
pub const MAX_NR_CONSOLES: u32 = 63;
/// First virtual console minor device number.
pub const FIRST_VC_MINOR: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_types_distinct() {
        let types = [
            CONSOLE_TYPE_VGA,
            CONSOLE_TYPE_FB,
            CONSOLE_TYPE_SERIAL,
            CONSOLE_TYPE_NET,
            CONSOLE_TYPE_DUMMY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_console_flags_no_overlap() {
        let flags = [
            CON_ENABLED,
            CON_BOOT,
            CON_CONSDEV,
            CON_ANSI,
            CON_NBCON,
            CON_BRL,
            CON_REGISTERED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_loglevels_ordered() {
        assert!(CONSOLE_LOGLEVEL_EMERGENCY < CONSOLE_LOGLEVEL_DEBUG);
        assert_eq!(CONSOLE_LOGLEVEL_DEFAULT, CONSOLE_LOGLEVEL_WARNING);
    }

    #[test]
    fn test_vc_limits() {
        assert!(MAX_NR_CONSOLES > 0);
        assert_eq!(FIRST_VC_MINOR, 1);
    }
}
