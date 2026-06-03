//! `<linux/ppdev.h>` — `/dev/parport*` userspace ioctls.
//!
//! Although physical parallel ports are now rare, ppdev is still
//! used to drive USB→parport adapters, EEPROM programmers, and
//! legacy industrial controllers. The constants below cover the
//! claim/release, data/control/status I/O, and ECP/EPP-mode ioctls.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for ppdev ioctls ('p').
pub const PP_IOCTL: u8 = b'p';

// ---------------------------------------------------------------------------
// Claim / release
// ---------------------------------------------------------------------------

/// `PPCLAIM` — claim the port.
pub const PPCLAIM: u32 = 0x0000_708b;
/// `PPRELEASE` — release the port.
pub const PPRELEASE: u32 = 0x0000_708c;
/// `PPYIELD` — yield the port (let another process claim).
pub const PPYIELD: u32 = 0x0000_708d;
/// `PPEXCL` — request exclusive access.
pub const PPEXCL: u32 = 0x0000_708f;

// ---------------------------------------------------------------------------
// Data / control / status lines
// ---------------------------------------------------------------------------

/// `PPRDATA` — read the data bus.
pub const PPRDATA: u32 = 0x8001_7083;
/// `PPWDATA` — write the data bus.
pub const PPWDATA: u32 = 0x4001_7084;
/// `PPRCONTROL` — read the control register.
pub const PPRCONTROL: u32 = 0x8001_7081;
/// `PPWCONTROL` — write the control register.
pub const PPWCONTROL: u32 = 0x4001_7082;
/// `PPRSTATUS` — read the status register.
pub const PPRSTATUS: u32 = 0x8001_7085;
/// `PPFCONTROL` — frame the control register (mask+value).
pub const PPFCONTROL: u32 = 0x4002_708e;

// ---------------------------------------------------------------------------
// Mode / phase selection
// ---------------------------------------------------------------------------

/// `PPDATADIR` — set the data direction (0=out, 1=in).
pub const PPDATADIR: u32 = 0x4004_7090;
/// `PPNEGOT` — negotiate to a 1284 mode.
pub const PPNEGOT: u32 = 0x4004_7091;
/// `PPWCTLONIRQ` — wait for an IRQ then write control.
pub const PPWCTLONIRQ: u32 = 0x4001_7092;
/// `PPCLRIRQ` — clear the pending IRQ count.
pub const PPCLRIRQ: u32 = 0x8004_7093;
/// `PPSETMODE` — choose IEEE-1284 mode (compat/byte/nibble/ECP/EPP).
pub const PPSETMODE: u32 = 0x4004_7080;
/// `PPGETMODE` — get current IEEE-1284 mode.
pub const PPGETMODE: u32 = 0x8004_7098;
/// `PPSETPHASE` — choose ECP phase.
pub const PPSETPHASE: u32 = 0x4004_7094;
/// `PPGETPHASE` — query ECP phase.
pub const PPGETPHASE: u32 = 0x8004_7099;
/// `PPGETMODES` — query supported modes mask.
pub const PPGETMODES: u32 = 0x8004_7097;
/// `PPSETFLAGS` — set per-fd flags (PP_FASTREAD/WRITE).
pub const PPSETFLAGS: u32 = 0x4004_709c;
/// `PPGETFLAGS` — get per-fd flags.
pub const PPGETFLAGS: u32 = 0x8004_709b;

// ---------------------------------------------------------------------------
// Modes (IEEE-1284) — argument to PPSETMODE / value from PPGETMODE
// ---------------------------------------------------------------------------

/// Compatibility (SPP) mode.
pub const IEEE1284_MODE_COMPAT: u32 = 0x0000;
/// Nibble mode.
pub const IEEE1284_MODE_NIBBLE: u32 = 0x0001;
/// Byte mode.
pub const IEEE1284_MODE_BYTE: u32 = 0x0002;
/// EPP mode.
pub const IEEE1284_MODE_EPP: u32 = 0x0040;
/// ECP mode.
pub const IEEE1284_MODE_ECP: u32 = 0x0010;
/// ECPRLE mode.
pub const IEEE1284_MODE_ECPRLE: u32 = 0x0030;
/// EPP-SWE (software-emulated EPP).
pub const IEEE1284_MODE_EPPSWE: u32 = 0x0c40;
/// Data-direction flag.
pub const IEEE1284_DATA: u32 = 0x0008;

// ---------------------------------------------------------------------------
// Per-fd flag bits (PPSETFLAGS / PPGETFLAGS)
// ---------------------------------------------------------------------------

/// Fast read (skip status polling).
pub const PP_FASTREAD: u32 = 1 << 0;
/// Fast write (skip status polling).
pub const PP_FASTWRITE: u32 = 1 << 1;
/// Drive WriteEvent on every byte.
pub const PP_W91284PIC: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_p() {
        assert_eq!(PP_IOCTL, b'p');
    }

    #[test]
    fn test_claim_release_distinct_and_use_p() {
        let ops = [PPCLAIM, PPRELEASE, PPYIELD, PPEXCL];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'p' (0x70).
            assert_eq!((ops[i] >> 8) & 0xff, b'p' as u32);
        }
    }

    #[test]
    fn test_io_ioctls_distinct() {
        let ops = [
            PPRDATA, PPWDATA, PPRCONTROL, PPWCONTROL, PPRSTATUS, PPFCONTROL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_mode_phase_ioctls_distinct() {
        let ops = [
            PPDATADIR, PPNEGOT, PPWCTLONIRQ, PPCLRIRQ, PPSETMODE, PPGETMODE,
            PPSETPHASE, PPGETPHASE, PPGETMODES, PPSETFLAGS, PPGETFLAGS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let m = [
            IEEE1284_MODE_COMPAT,
            IEEE1284_MODE_NIBBLE,
            IEEE1284_MODE_BYTE,
            IEEE1284_MODE_EPP,
            IEEE1284_MODE_ECP,
            IEEE1284_MODE_ECPRLE,
            IEEE1284_MODE_EPPSWE,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
        // COMPAT==0 — zeroed mode means SPP/compat mode.
        assert_eq!(IEEE1284_MODE_COMPAT, 0);
    }

    #[test]
    fn test_pp_flag_bits_pow2_distinct() {
        let f = [PP_FASTREAD, PP_FASTWRITE, PP_W91284PIC];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }
}
