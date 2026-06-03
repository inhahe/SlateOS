//! `<linux/comedi.h>` — Comedi data-acquisition userspace constants.
//!
//! Constants for the Comedi (COntrol and MEasurement Device Interface)
//! framework — used to drive lab DAQ cards (analog/digital I/O,
//! counter/timer, calibration) from userspace.

// ---------------------------------------------------------------------------
// Subdevice types
// ---------------------------------------------------------------------------

/// Unused subdevice slot.
pub const COMEDI_SUBD_UNUSED: u32 = 0;
/// Analog input.
pub const COMEDI_SUBD_AI: u32 = 1;
/// Analog output.
pub const COMEDI_SUBD_AO: u32 = 2;
/// Digital input.
pub const COMEDI_SUBD_DI: u32 = 3;
/// Digital output.
pub const COMEDI_SUBD_DO: u32 = 4;
/// Bidirectional digital I/O.
pub const COMEDI_SUBD_DIO: u32 = 5;
/// Counter.
pub const COMEDI_SUBD_COUNTER: u32 = 6;
/// Timer.
pub const COMEDI_SUBD_TIMER: u32 = 7;
/// Memory access (debug/EEPROM).
pub const COMEDI_SUBD_MEMORY: u32 = 8;
/// Calibration.
pub const COMEDI_SUBD_CALIB: u32 = 9;
/// Process-wide.
pub const COMEDI_SUBD_PROC: u32 = 10;
/// Serial I/O.
pub const COMEDI_SUBD_SERIAL: u32 = 11;
/// Pulse-width modulation.
pub const COMEDI_SUBD_PWM: u32 = 12;

// ---------------------------------------------------------------------------
// Subdevice flags (bitfield)
// ---------------------------------------------------------------------------

/// Subdevice supports asynchronous (streaming) commands.
pub const SDF_CMD: u32 = 0x1000;
/// Subdevice supports asynchronous commands for read.
pub const SDF_CMD_READ: u32 = 0x2000;
/// Subdevice supports asynchronous commands for write.
pub const SDF_CMD_WRITE: u32 = 0x4000;
/// Subdevice is busy.
pub const SDF_BUSY: u32 = 0x0001;
/// Subdevice is busy by the calling process.
pub const SDF_BUSY_OWNER: u32 = 0x0002;
/// Subdevice is locked.
pub const SDF_LOCKED: u32 = 0x0004;
/// Subdevice is locked by the calling process.
pub const SDF_LOCK_OWNER: u32 = 0x0008;
/// Maxdata varies by channel.
pub const SDF_MAXDATA: u32 = 0x0010;
/// Flags-arg setting is supported.
pub const SDF_FLAGS: u32 = 0x0020;
/// Range-table varies by channel.
pub const SDF_RANGETYPE: u32 = 0x0040;
/// Subdevice runs commands.
pub const SDF_PWM_COUNTER: u32 = 0x0080;
/// Subdevice supports software triggers.
pub const SDF_PWM_HBRIDGE: u32 = 0x0100;

// ---------------------------------------------------------------------------
// Trigger sources (cmd.start_src / scan_begin_src / convert_src etc.)
// ---------------------------------------------------------------------------

/// No trigger source.
pub const TRIG_NONE: u32 = 0x0001;
/// Trigger now.
pub const TRIG_NOW: u32 = 0x0002;
/// Trigger immediately, no delay.
pub const TRIG_FOLLOW: u32 = 0x0004;
/// Trigger on internal timer.
pub const TRIG_TIMER: u32 = 0x0008;
/// Trigger on count.
pub const TRIG_COUNT: u32 = 0x0010;
/// Trigger on external signal.
pub const TRIG_EXT: u32 = 0x0020;
/// Trigger from internal channel.
pub const TRIG_INT: u32 = 0x0040;
/// Trigger on other subdevice's count.
pub const TRIG_OTHER: u32 = 0x0080;

// ---------------------------------------------------------------------------
// Insn types (synchronous instruction commands)
// ---------------------------------------------------------------------------

/// Read instruction.
pub const INSN_READ: u32 = 0;
/// Write instruction.
pub const INSN_WRITE: u32 = 1;
/// Bits read/write for digital I/O.
pub const INSN_BITS: u32 = 2;
/// Configure subdevice.
pub const INSN_CONFIG: u32 = 3;
/// GTOD-style query.
pub const INSN_GTOD: u32 = 4;
/// Wait for trigger.
pub const INSN_WAIT: u32 = 5;
/// Inttrig (internal trigger).
pub const INSN_INTTRIG: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subdevice_types_distinct() {
        let subs = [
            COMEDI_SUBD_UNUSED,
            COMEDI_SUBD_AI,
            COMEDI_SUBD_AO,
            COMEDI_SUBD_DI,
            COMEDI_SUBD_DO,
            COMEDI_SUBD_DIO,
            COMEDI_SUBD_COUNTER,
            COMEDI_SUBD_TIMER,
            COMEDI_SUBD_MEMORY,
            COMEDI_SUBD_CALIB,
            COMEDI_SUBD_PROC,
            COMEDI_SUBD_SERIAL,
            COMEDI_SUBD_PWM,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_sdf_flags_single_bits() {
        let flags = [
            SDF_CMD,
            SDF_CMD_READ,
            SDF_CMD_WRITE,
            SDF_BUSY,
            SDF_BUSY_OWNER,
            SDF_LOCKED,
            SDF_LOCK_OWNER,
            SDF_MAXDATA,
            SDF_FLAGS,
            SDF_RANGETYPE,
            SDF_PWM_COUNTER,
            SDF_PWM_HBRIDGE,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two(), "{f:#x} not single-bit");
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_trigger_sources_single_bits() {
        let trigs = [
            TRIG_NONE, TRIG_NOW, TRIG_FOLLOW, TRIG_TIMER, TRIG_COUNT, TRIG_EXT, TRIG_INT,
            TRIG_OTHER,
        ];
        for &t in &trigs {
            assert!(t.is_power_of_two());
        }
        for i in 0..trigs.len() {
            for j in (i + 1)..trigs.len() {
                assert_ne!(trigs[i], trigs[j]);
            }
        }
    }

    #[test]
    fn test_insn_types_distinct() {
        let ins = [
            INSN_READ,
            INSN_WRITE,
            INSN_BITS,
            INSN_CONFIG,
            INSN_GTOD,
            INSN_WAIT,
            INSN_INTTRIG,
        ];
        for i in 0..ins.len() {
            for j in (i + 1)..ins.len() {
                assert_ne!(ins[i], ins[j]);
            }
        }
    }
}
