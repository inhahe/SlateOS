//! `<linux/comedi.h>` — Comedi data acquisition device constants.
//!
//! Comedi is a Linux kernel interface for data-acquisition hardware
//! (ADCs, DACs, digital I/O, counters). It exposes a uniform ioctl
//! interface across many vendors so userspace tools (kcomedilib, the
//! comedi_calibrate utility) can probe and configure devices.

// ---------------------------------------------------------------------------
// Subdevice types
// ---------------------------------------------------------------------------

pub const COMEDI_SUBD_UNUSED: u32 = 0;
pub const COMEDI_SUBD_AI: u32 = 1;
pub const COMEDI_SUBD_AO: u32 = 2;
pub const COMEDI_SUBD_DI: u32 = 3;
pub const COMEDI_SUBD_DO: u32 = 4;
pub const COMEDI_SUBD_DIO: u32 = 5;
pub const COMEDI_SUBD_COUNTER: u32 = 6;
pub const COMEDI_SUBD_TIMER: u32 = 7;
pub const COMEDI_SUBD_MEMORY: u32 = 8;
pub const COMEDI_SUBD_CALIB: u32 = 9;
pub const COMEDI_SUBD_PROC: u32 = 10;
pub const COMEDI_SUBD_SERIAL: u32 = 11;
pub const COMEDI_SUBD_PWM: u32 = 12;

// ---------------------------------------------------------------------------
// Subdevice flags (bitmask)
// ---------------------------------------------------------------------------

pub const SDF_BUSY: u32 = 0x0001;
pub const SDF_BUSY_OWNER: u32 = 0x0002;
pub const SDF_LOCKED: u32 = 0x0004;
pub const SDF_LOCK_OWNER: u32 = 0x0008;
pub const SDF_MAXDATA: u32 = 0x0010;
pub const SDF_FLAGS: u32 = 0x0020;
pub const SDF_RANGETYPE: u32 = 0x0040;
pub const SDF_PWM_COUNTER: u32 = 0x0080;
pub const SDF_PWM_HBRIDGE: u32 = 0x0100;
pub const SDF_CMD: u32 = 0x1000;
pub const SDF_SOFT_CALIBRATED: u32 = 0x2000;
pub const SDF_CMD_WRITE: u32 = 0x4000;
pub const SDF_CMD_READ: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Trigger sources (used in comedi_cmd.start_src/scan_begin_src/...)
// ---------------------------------------------------------------------------

pub const TRIG_NONE: u32 = 0x00000001;
pub const TRIG_NOW: u32 = 0x00000002;
pub const TRIG_FOLLOW: u32 = 0x00000004;
pub const TRIG_TIME: u32 = 0x00000008;
pub const TRIG_TIMER: u32 = 0x00000010;
pub const TRIG_COUNT: u32 = 0x00000020;
pub const TRIG_EXT: u32 = 0x00000040;
pub const TRIG_INT: u32 = 0x00000080;
pub const TRIG_OTHER: u32 = 0x00000100;

// ---------------------------------------------------------------------------
// Device character special file
// ---------------------------------------------------------------------------

pub const COMEDI_DEV_PATH_PREFIX: &str = "/dev/comedi";
pub const COMEDI_NUM_MINORS: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subd_types_dense_0_to_12() {
        let s = [
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
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_sdf_flags_distinct_single_bit() {
        let f = [
            SDF_BUSY,
            SDF_BUSY_OWNER,
            SDF_LOCKED,
            SDF_LOCK_OWNER,
            SDF_MAXDATA,
            SDF_FLAGS,
            SDF_RANGETYPE,
            SDF_PWM_COUNTER,
            SDF_PWM_HBRIDGE,
            SDF_CMD,
            SDF_SOFT_CALIBRATED,
            SDF_CMD_WRITE,
            SDF_CMD_READ,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_trig_flags_distinct_single_bit() {
        let t = [
            TRIG_NONE,
            TRIG_NOW,
            TRIG_FOLLOW,
            TRIG_TIME,
            TRIG_TIMER,
            TRIG_COUNT,
            TRIG_EXT,
            TRIG_INT,
            TRIG_OTHER,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        // OR of all = low 9 bits = 0x1FF.
        let or_all = t.iter().fold(0u32, |a, &v| a | v);
        assert_eq!(or_all, 0x1FF);
    }

    #[test]
    fn test_dev_path_and_minors() {
        assert_eq!(COMEDI_DEV_PATH_PREFIX, "/dev/comedi");
        assert!(COMEDI_DEV_PATH_PREFIX.starts_with("/dev/"));
        // 16 devices supported (comedi0..comedi15).
        assert_eq!(COMEDI_NUM_MINORS, 16);
        assert!(COMEDI_NUM_MINORS.is_power_of_two());
    }

    #[test]
    fn test_cmd_subdevice_has_both_read_and_write_capabilities() {
        // SDF_CMD_WRITE | SDF_CMD_READ together = bi-directional command-capable.
        let both = SDF_CMD_WRITE | SDF_CMD_READ;
        assert_eq!(both & SDF_CMD_WRITE, SDF_CMD_WRITE);
        assert_eq!(both & SDF_CMD_READ, SDF_CMD_READ);
        assert_eq!(both.count_ones(), 2);
    }
}
