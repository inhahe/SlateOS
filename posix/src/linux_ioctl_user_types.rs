//! `<asm-generic/ioctl.h>` — `_IO{R,W,WR}` ioctl-number layout.
//!
//! Every device driver, every kernel-userspace control-plane interface
//! encodes ioctl numbers with the bit-field scheme defined here:
//! direction (2) | size (14) | type (8) | nr (8). The helpers below
//! match the kernel's `_IO`/`_IOR`/`_IOW`/`_IOWR` macros so we can
//! construct and decode ioctls in a `const`-correct way.

// ---------------------------------------------------------------------------
// Field widths
// ---------------------------------------------------------------------------

pub const IOC_NRBITS: u32 = 8;
pub const IOC_TYPEBITS: u32 = 8;
pub const IOC_SIZEBITS: u32 = 14;
pub const IOC_DIRBITS: u32 = 2;

pub const IOC_NRMASK: u32 = (1 << IOC_NRBITS) - 1;
pub const IOC_TYPEMASK: u32 = (1 << IOC_TYPEBITS) - 1;
pub const IOC_SIZEMASK: u32 = (1 << IOC_SIZEBITS) - 1;
pub const IOC_DIRMASK: u32 = (1 << IOC_DIRBITS) - 1;

// ---------------------------------------------------------------------------
// Field positions
// ---------------------------------------------------------------------------

pub const IOC_NRSHIFT: u32 = 0;
pub const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
pub const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
pub const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

// ---------------------------------------------------------------------------
// Direction codes
// ---------------------------------------------------------------------------

/// No data transfer.
pub const IOC_NONE: u32 = 0;
/// Userland is writing — kernel reads from userland.
pub const IOC_WRITE: u32 = 1;
/// Userland is reading — kernel writes to userland.
pub const IOC_READ: u32 = 2;

// ---------------------------------------------------------------------------
// Encoding helper
// ---------------------------------------------------------------------------

/// `_IOC(dir, type, nr, size)` — kernel-canonical ioctl encoding.
#[must_use]
pub const fn ioc(dir: u32, type_: u32, nr: u32, size: u32) -> u32 {
    (dir << IOC_DIRSHIFT)
        | (type_ << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | (size << IOC_SIZESHIFT)
}

/// `_IO(type, nr)` — no data.
#[must_use]
pub const fn io(type_: u32, nr: u32) -> u32 {
    ioc(IOC_NONE, type_, nr, 0)
}

/// `_IOR(type, nr, T)` — kernel returns one T to userland.
#[must_use]
pub const fn ior(type_: u32, nr: u32, size: u32) -> u32 {
    ioc(IOC_READ, type_, nr, size)
}

/// `_IOW(type, nr, T)` — userland gives one T to the kernel.
#[must_use]
pub const fn iow(type_: u32, nr: u32, size: u32) -> u32 {
    ioc(IOC_WRITE, type_, nr, size)
}

/// `_IOWR(type, nr, T)` — bidirectional.
#[must_use]
pub const fn iowr(type_: u32, nr: u32, size: u32) -> u32 {
    ioc(IOC_READ | IOC_WRITE, type_, nr, size)
}

// ---------------------------------------------------------------------------
// Decoding helpers
// ---------------------------------------------------------------------------

#[must_use]
pub const fn ioc_dir(cmd: u32) -> u32 {
    (cmd >> IOC_DIRSHIFT) & IOC_DIRMASK
}

#[must_use]
pub const fn ioc_type(cmd: u32) -> u32 {
    (cmd >> IOC_TYPESHIFT) & IOC_TYPEMASK
}

#[must_use]
pub const fn ioc_nr(cmd: u32) -> u32 {
    (cmd >> IOC_NRSHIFT) & IOC_NRMASK
}

#[must_use]
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
    fn test_field_widths_sum_to_32() {
        assert_eq!(IOC_NRBITS + IOC_TYPEBITS + IOC_SIZEBITS + IOC_DIRBITS, 32);
    }

    #[test]
    fn test_field_positions_disjoint() {
        // Position fields tile the 32-bit word without overlap.
        assert_eq!(IOC_NRSHIFT, 0);
        assert_eq!(IOC_TYPESHIFT, 8);
        assert_eq!(IOC_SIZESHIFT, 16);
        assert_eq!(IOC_DIRSHIFT, 30);
    }

    #[test]
    fn test_direction_codes_distinct() {
        assert_eq!(IOC_NONE, 0);
        assert_eq!(IOC_WRITE, 1);
        assert_eq!(IOC_READ, 2);
        // IOC_READ | IOC_WRITE = 3 fits in the 2-bit direction field.
        assert_eq!(IOC_READ | IOC_WRITE, IOC_DIRMASK);
    }

    #[test]
    fn test_encoding_matches_kernel_examples() {
        // _IO('T', 1) = 0x00005401 — first VT command.
        assert_eq!(io(b'T' as u32, 1), 0x0000_5401);
        // _IOR('K', 1, int) = 0x40044B01 — like KDGETLED (1 byte for short).
        // Here use 4-byte int for clarity.
        let r = ior(b'K' as u32, 1, 4);
        assert_eq!(ioc_dir(r), IOC_READ);
        assert_eq!(ioc_type(r), u32::from(b'K'));
        assert_eq!(ioc_nr(r), 1);
        assert_eq!(ioc_size(r), 4);
        // _IOWR sets both direction bits.
        let rw = iowr(b'L' as u32, 7, 16);
        assert_eq!(ioc_dir(rw), IOC_READ | IOC_WRITE);
        assert_eq!(ioc_size(rw), 16);
    }

    #[test]
    fn test_round_trip_decode() {
        let cmd = iow(b'X' as u32, 0x2A, 8);
        assert_eq!(ioc_dir(cmd), IOC_WRITE);
        assert_eq!(ioc_type(cmd), u32::from(b'X'));
        assert_eq!(ioc_nr(cmd), 0x2A);
        assert_eq!(ioc_size(cmd), 8);
    }

    #[test]
    fn test_max_field_sizes() {
        // size field fits 14 bits ⇒ max 16383 bytes per arg.
        assert_eq!(IOC_SIZEMASK, 0x3FFF);
    }
}
