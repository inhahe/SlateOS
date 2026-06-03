//! `<linux/input.h>` (input_id subset) — input device bus and identification.
//!
//! Every input device reports an identity consisting of bus type,
//! vendor, product, and version fields. The bus type identifies how
//! the device is connected (USB, Bluetooth, I2C, etc.). These values
//! populate the `input_id` struct returned by `EVIOCGID` ioctl and
//! are used for device matching in udev rules and libinput.

// ---------------------------------------------------------------------------
// Bus types (input_id.bustype)
// ---------------------------------------------------------------------------

/// PCI bus.
pub const BUS_PCI: u16 = 0x01;
/// ISA bus (legacy).
pub const BUS_ISAPNP: u16 = 0x02;
/// USB bus.
pub const BUS_USB: u16 = 0x03;
/// HIL (HP-UX Human Interface Loop).
pub const BUS_HIL: u16 = 0x04;
/// Bluetooth.
pub const BUS_BLUETOOTH: u16 = 0x05;
/// Virtual device (software-generated events).
pub const BUS_VIRTUAL: u16 = 0x06;
/// ISA bus.
pub const BUS_ISA: u16 = 0x10;
/// I8042 keyboard/mouse controller.
pub const BUS_I8042: u16 = 0x11;
/// Xbox gamepad controller.
pub const BUS_XTKBD: u16 = 0x12;
/// RS-232 serial.
pub const BUS_RS232: u16 = 0x13;
/// Gameport (legacy joystick port).
pub const BUS_GAMEPORT: u16 = 0x14;
/// Parallel port.
pub const BUS_PARPORT: u16 = 0x15;
/// Amiga bus.
pub const BUS_AMIGA: u16 = 0x16;
/// ADB (Apple Desktop Bus).
pub const BUS_ADB: u16 = 0x17;
/// I2C bus.
pub const BUS_I2C: u16 = 0x18;
/// Host bus (platform device).
pub const BUS_HOST: u16 = 0x19;
/// GSC bus (HP PA-RISC).
pub const BUS_GSC: u16 = 0x1A;
/// Atari bus.
pub const BUS_ATARI: u16 = 0x1B;
/// SPI bus.
pub const BUS_SPI: u16 = 0x1C;
/// RMI (Synaptics RMI4).
pub const BUS_RMI: u16 = 0x1D;
/// CEC (HDMI CEC).
pub const BUS_CEC: u16 = 0x1E;
/// Intel iSHTP (Integrated Sensor Hub).
pub const BUS_INTEL_ISHTP: u16 = 0x1F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_types_distinct() {
        let buses = [
            BUS_PCI,
            BUS_ISAPNP,
            BUS_USB,
            BUS_HIL,
            BUS_BLUETOOTH,
            BUS_VIRTUAL,
            BUS_ISA,
            BUS_I8042,
            BUS_XTKBD,
            BUS_RS232,
            BUS_GAMEPORT,
            BUS_PARPORT,
            BUS_AMIGA,
            BUS_ADB,
            BUS_I2C,
            BUS_HOST,
            BUS_GSC,
            BUS_ATARI,
            BUS_SPI,
            BUS_RMI,
            BUS_CEC,
            BUS_INTEL_ISHTP,
        ];
        for i in 0..buses.len() {
            for j in (i + 1)..buses.len() {
                assert_ne!(buses[i], buses[j], "bus types {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_common_buses() {
        assert_eq!(BUS_USB, 0x03);
        assert_eq!(BUS_BLUETOOTH, 0x05);
        assert_eq!(BUS_I2C, 0x18);
        assert_eq!(BUS_SPI, 0x1C);
    }

    #[test]
    fn test_virtual_bus() {
        // Virtual is used for software input devices (uinput)
        assert_eq!(BUS_VIRTUAL, 0x06);
    }

    #[test]
    fn test_all_nonzero() {
        let buses = [
            BUS_PCI,
            BUS_ISAPNP,
            BUS_USB,
            BUS_HIL,
            BUS_BLUETOOTH,
            BUS_VIRTUAL,
            BUS_ISA,
            BUS_I8042,
            BUS_XTKBD,
            BUS_RS232,
            BUS_GAMEPORT,
            BUS_PARPORT,
            BUS_AMIGA,
            BUS_ADB,
            BUS_I2C,
            BUS_HOST,
            BUS_GSC,
            BUS_ATARI,
            BUS_SPI,
            BUS_RMI,
            BUS_CEC,
            BUS_INTEL_ISHTP,
        ];
        for &b in &buses {
            assert_ne!(b, 0, "bus type should be nonzero");
        }
    }
}
