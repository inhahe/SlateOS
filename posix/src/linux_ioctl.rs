//! `<linux/ioctl.h>` + `<asm-generic/ioctl.h>` — ioctl encoding constants.
//!
//! Provides the building blocks for encoding and decoding ioctl
//! command numbers. Each ioctl is encoded as: direction (2 bits) |
//! size (14 bits) | type (8 bits) | number (8 bits).

// ---------------------------------------------------------------------------
// Direction bits
// ---------------------------------------------------------------------------

/// No data transfer.
pub const IOC_NONE: u32 = 0;
/// Writing to device (userspace → kernel).
pub const IOC_WRITE: u32 = 1;
/// Reading from device (kernel → userspace).
pub const IOC_READ: u32 = 2;

// ---------------------------------------------------------------------------
// Bit widths and shifts
// ---------------------------------------------------------------------------

/// Number field width (8 bits).
pub const IOC_NRBITS: u32 = 8;
/// Type field width (8 bits).
pub const IOC_TYPEBITS: u32 = 8;
/// Size field width (14 bits).
pub const IOC_SIZEBITS: u32 = 14;
/// Direction field width (2 bits).
pub const IOC_DIRBITS: u32 = 2;

/// Number field shift.
pub const IOC_NRSHIFT: u32 = 0;
/// Type field shift.
pub const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
/// Size field shift.
pub const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
/// Direction field shift.
pub const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

// ---------------------------------------------------------------------------
// Masks
// ---------------------------------------------------------------------------

/// Number field mask.
pub const IOC_NRMASK: u32 = (1 << IOC_NRBITS) - 1;
/// Type field mask.
pub const IOC_TYPEMASK: u32 = (1 << IOC_TYPEBITS) - 1;
/// Size field mask.
pub const IOC_SIZEMASK: u32 = (1 << IOC_SIZEBITS) - 1;
/// Direction field mask.
pub const IOC_DIRMASK: u32 = (1 << IOC_DIRBITS) - 1;

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

/// Encode an ioctl command number.
pub const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (ty << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | (size << IOC_SIZESHIFT)
}

/// Encode an ioctl with no data (_IO).
pub const fn io(ty: u32, nr: u32) -> u32 {
    ioc(IOC_NONE, ty, nr, 0)
}

/// Encode an ioctl that reads data (_IOR).
pub const fn ior(ty: u32, nr: u32, size: u32) -> u32 {
    ioc(IOC_READ, ty, nr, size)
}

/// Encode an ioctl that writes data (_IOW).
pub const fn iow(ty: u32, nr: u32, size: u32) -> u32 {
    ioc(IOC_WRITE, ty, nr, size)
}

/// Encode an ioctl that reads and writes data (_IOWR).
pub const fn iowr(ty: u32, nr: u32, size: u32) -> u32 {
    ioc(IOC_READ | IOC_WRITE, ty, nr, size)
}

// ---------------------------------------------------------------------------
// Decoding helpers
// ---------------------------------------------------------------------------

/// Extract direction from an ioctl command.
pub const fn ioc_dir(cmd: u32) -> u32 {
    (cmd >> IOC_DIRSHIFT) & IOC_DIRMASK
}

/// Extract type from an ioctl command.
pub const fn ioc_type(cmd: u32) -> u32 {
    (cmd >> IOC_TYPESHIFT) & IOC_TYPEMASK
}

/// Extract number from an ioctl command.
pub const fn ioc_nr(cmd: u32) -> u32 {
    (cmd >> IOC_NRSHIFT) & IOC_NRMASK
}

/// Extract size from an ioctl command.
pub const fn ioc_size(cmd: u32) -> u32 {
    (cmd >> IOC_SIZESHIFT) & IOC_SIZEMASK
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_values() {
        assert_eq!(IOC_NONE, 0);
        assert_eq!(IOC_WRITE, 1);
        assert_eq!(IOC_READ, 2);
    }

    #[test]
    fn test_shifts() {
        assert_eq!(IOC_NRSHIFT, 0);
        assert_eq!(IOC_TYPESHIFT, 8);
        assert_eq!(IOC_SIZESHIFT, 16);
        assert_eq!(IOC_DIRSHIFT, 30);
    }

    #[test]
    fn test_masks() {
        assert_eq!(IOC_NRMASK, 0xFF);
        assert_eq!(IOC_TYPEMASK, 0xFF);
        assert_eq!(IOC_SIZEMASK, 0x3FFF);
        assert_eq!(IOC_DIRMASK, 0x3);
    }

    #[test]
    fn test_io_encoding() {
        let cmd = io(b'T' as u32, 1);
        assert_eq!(ioc_dir(cmd), IOC_NONE);
        assert_eq!(ioc_type(cmd), b'T' as u32);
        assert_eq!(ioc_nr(cmd), 1);
        assert_eq!(ioc_size(cmd), 0);
    }

    #[test]
    fn test_ior_encoding() {
        let cmd = ior(b'V' as u32, 5, 16);
        assert_eq!(ioc_dir(cmd), IOC_READ);
        assert_eq!(ioc_type(cmd), b'V' as u32);
        assert_eq!(ioc_nr(cmd), 5);
        assert_eq!(ioc_size(cmd), 16);
    }

    #[test]
    fn test_iow_encoding() {
        let cmd = iow(b'F' as u32, 3, 8);
        assert_eq!(ioc_dir(cmd), IOC_WRITE);
        assert_eq!(ioc_type(cmd), b'F' as u32);
        assert_eq!(ioc_nr(cmd), 3);
        assert_eq!(ioc_size(cmd), 8);
    }

    #[test]
    fn test_iowr_encoding() {
        let cmd = iowr(b'X' as u32, 7, 32);
        assert_eq!(ioc_dir(cmd), IOC_READ | IOC_WRITE);
        assert_eq!(ioc_type(cmd), b'X' as u32);
        assert_eq!(ioc_nr(cmd), 7);
        assert_eq!(ioc_size(cmd), 32);
    }

    #[test]
    fn test_roundtrip() {
        for dir in [IOC_NONE, IOC_WRITE, IOC_READ, IOC_READ | IOC_WRITE] {
            for ty in [0u32, 42, 255] {
                for nr in [0u32, 1, 127, 255] {
                    for sz in [0u32, 4, 1024, 16383] {
                        let cmd = ioc(dir, ty, nr, sz);
                        assert_eq!(ioc_dir(cmd), dir);
                        assert_eq!(ioc_type(cmd), ty);
                        assert_eq!(ioc_nr(cmd), nr);
                        assert_eq!(ioc_size(cmd), sz);
                    }
                }
            }
        }
    }
}
