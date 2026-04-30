//! `x86_64` port I/O primitives.
//!
//! Provides safe wrappers around the `in` and `out` CPU instructions
//! for communicating with hardware devices via I/O ports.  Every wrapper
//! is `unsafe` at the hardware level (writing to an arbitrary port can
//! crash the system), so callers must ensure the port address is valid
//! for the intended device.

/// Write a single byte to an I/O port.
///
/// # Safety
///
/// The caller must ensure `port` is a valid I/O port for the intended
/// device and that writing `value` to it is a safe hardware operation
/// in the current system state.
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    // SAFETY: Caller guarantees the port and value are valid.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Read a single byte from an I/O port.
///
/// # Safety
///
/// The caller must ensure `port` is a valid I/O port for the intended
/// device and that reading from it is safe in the current system state.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: Caller guarantees the port is valid.
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }
    value
}

/// Write a 16-bit word to an I/O port.
///
/// # Safety
///
/// Same requirements as [`outb`] but for a 16-bit write.
#[inline]
pub unsafe fn outw(port: u16, value: u16) {
    // SAFETY: Caller guarantees the port and value are valid.
    unsafe {
        core::arch::asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Read a 16-bit word from an I/O port.
///
/// # Safety
///
/// Same requirements as [`inb`] but for a 16-bit read.
#[inline]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: Caller guarantees the port is valid.
    unsafe {
        core::arch::asm!(
            "in ax, dx",
            out("ax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }
    value
}

/// Write a 32-bit doubleword to an I/O port.
///
/// # Safety
///
/// Same requirements as [`outb`] but for a 32-bit write.
#[inline]
pub unsafe fn outl(port: u16, value: u32) {
    // SAFETY: Caller guarantees the port and value are valid.
    unsafe {
        core::arch::asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Read a 32-bit doubleword from an I/O port.
///
/// # Safety
///
/// Same requirements as [`inb`] but for a 32-bit read.
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: Caller guarantees the port is valid.
    unsafe {
        core::arch::asm!(
            "in eax, dx",
            out("eax") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }
    value
}

/// Small busy-wait by writing to port 0x80 (POST diagnostic port).
///
/// Used after certain I/O operations that need a short delay for
/// hardware to catch up (e.g., PIC programming).
///
/// # Safety
///
/// Port 0x80 is the standard POST code port and writing to it is
/// safe on all standard PC-compatible hardware.
#[inline]
pub unsafe fn io_wait() {
    // SAFETY: Port 0x80 is the POST diagnostic port; writes are
    // harmless and provide a ~1us delay on most hardware.
    unsafe {
        outb(0x80, 0);
    }
}
