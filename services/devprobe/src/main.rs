//! devprobe — the device interface's first ring-3 user (design-decisions
//! 1579, roadmap.md 2.1).
//!
//! A native probe driver with no Linux code in it: what the Linux driver
//! layer will stand on, used from userspace with real system calls before
//! anything depends on it. The kernel's boot self-test
//! (`devhandle::self_test_ring3`) spawns it holding a `PciDevice` capability
//! for exactly one PCI function -- a QEMU e1000e no kernel driver binds -- and
//! it:
//!
//! 1. tries to open every function on bus 0, and must open exactly one: every
//!    other open is refused, which is the capability check seen from outside;
//! 2. reads the function's vendor and device ids through the handle;
//! 3. maps BAR0 and reads the MAC address the boot test gave the card
//!    (`52:54:00:12:34:59`), from the receive-address registers or, failing
//!    those, the EEPROM through EERD;
//! 4. allocates a DMA arena and writes it;
//! 5. sets up the card's MSI, and takes its (zero) pending interrupts;
//! 6. exits *without* closing the handle, so that its exit's release of the
//!    device is what the kernel test checks next.
//!
//! It exits with 42 when every step worked, and otherwise with the failed
//! step's own code (`devprobe::Exit`), saying which on the console. What it
//! decodes is in `lib.rs`, where the host tests it.

#![no_std]
#![no_main]
#![deny(clippy::all, clippy::pedantic)]

use devprobe::{
    E1000E_82574, EERD, EXPECTED_MAC, Exit, INTEL, MIN_BAR0, RAH0, RAL0, eerd_data,
    eerd_read_command, mac_from_eeprom_words, mac_from_receive_address, only_granted_opened,
};

// The SlateOS ABI note, as `services/hello` carries it (see there for why it
// is assembled rather than a `#[used]` static).
core::arch::global_asm!(
    ".pushsection .note.slateos, \"a\", @note",
    ".balign 4",
    ".long 8",
    ".long 4",
    ".long 1",
    ".asciz \"SlateOS\"",
    ".long 1",
    ".popsection",
);

const SYS_EXIT: u64 = 1;
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_PCI_DEVICE_OPEN: u64 = 1179;
const SYS_PCI_DEVICE_BAR: u64 = 1181;
const SYS_PCI_DEVICE_CONFIG_READ: u64 = 1182;
const SYS_PCI_DEVICE_MAP_BAR: u64 = 1184;
const SYS_PCI_DEVICE_DMA_ALLOC: u64 = 1185;
const SYS_PCI_DEVICE_IRQ_SETUP: u64 = 1186;
const SYS_PCI_DEVICE_IRQ_WAIT: u64 = 1187;

/// How many times to read EERD for one word before giving up on it.
const EERD_POLLS: u32 = 100_000;

#[inline]
fn syscall4(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: the SlateOS system-call convention: number in rax, arguments in
    // rdi/rsi/rdx/r10, result in rax; rcx and r11 are clobbered.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr.cast_signed() => ret,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            in("r10") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn print(s: &str) {
    syscall4(SYS_CONSOLE_WRITE, s.as_ptr() as u64, s.len() as u64, 0, 0);
}

fn exit(code: Exit) -> ! {
    syscall4(SYS_EXIT, code.code(), 0, 0, 0);
    loop {
        // SAFETY: unreachable after SYS_EXIT; a halt faults the task if it is.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
}

fn fail(code: Exit, why: &str) -> ! {
    print("[devprobe] FAIL: ");
    print(why);
    print("\n");
    exit(code);
}

/// A 32-bit register of the mapped BAR.
fn reg(base: usize, offset: usize) -> u32 {
    // SAFETY: `base` is BAR0's mapping, at least `MIN_BAR0` bytes, and every
    // offset read here is a 4-aligned register inside it.
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn set_reg(base: usize, offset: usize, value: u32) {
    // SAFETY: as for `reg`.
    unsafe { ((base + offset) as *mut u32).write_volatile(value) }
}

/// The MAC address from receive address 0, if valid, else from the EEPROM's
/// first three words through EERD; `None` if an EEPROM read never finished.
fn mac_of(base: usize) -> Option<[u8; 6]> {
    if let Some(mac) = mac_from_receive_address(reg(base, RAL0), reg(base, RAH0)) {
        return Some(mac);
    }
    let mut words = [0u16; 3];
    for (word, slot) in (0u16..).zip(words.iter_mut()) {
        set_reg(base, EERD, eerd_read_command(word));
        *slot = (0..EERD_POLLS).find_map(|_| eerd_data(reg(base, EERD)))?;
    }
    Some(mac_from_eeprom_words(words))
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // 1. Exactly one function on bus 0 opens: the one granted.
    let mut last = None;
    let mut opened = 0u32;
    for device in 0..32u64 {
        for function in 0..8u64 {
            let r = syscall4(SYS_PCI_DEVICE_OPEN, device << 3 | function, 0, 0, 0);
            if r > 0 {
                opened += 1;
                last = Some(r.cast_unsigned());
            }
        }
    }
    let handle = match only_granted_opened(opened, last) {
        Ok(handle) => handle,
        Err(Exit::OpenedNone) => fail(Exit::OpenedNone, "no function opened"),
        Err(code) => fail(code, "more than the granted function opened"),
    };

    // 2. It is the card the test attached.
    let vendor = syscall4(SYS_PCI_DEVICE_CONFIG_READ, handle, 0, 2, 0);
    let device = syscall4(SYS_PCI_DEVICE_CONFIG_READ, handle, 2, 2, 0);
    if vendor != i64::from(INTEL) || device != i64::from(E1000E_82574) {
        fail(Exit::WrongDevice, "the opened function is not an 82574");
    }

    // 3. BAR0 mapped, and the MAC read through it.
    let bar = syscall4(SYS_PCI_DEVICE_BAR, handle, 0, 0, 0);
    if !u64::try_from(bar).is_ok_and(|size| size >= MIN_BAR0) {
        fail(
            Exit::BarUnusable,
            "BAR0 is not a memory BAR of at least 128 KiB",
        );
    }
    let base = match usize::try_from(syscall4(SYS_PCI_DEVICE_MAP_BAR, handle, 0, 0, 0)) {
        Ok(base) if base != 0 => base,
        _ => fail(Exit::MapFailed, "BAR0 could not be mapped"),
    };
    if mac_of(base) != Some(EXPECTED_MAC) {
        fail(
            Exit::WrongMac,
            "the MAC read through BAR0 is not the one the test gave",
        );
    }

    // 4. A DMA arena, written.
    let mut device_addr: u64 = 0;
    let arena = syscall4(
        SYS_PCI_DEVICE_DMA_ALLOC,
        handle,
        0x4000,
        &raw mut device_addr as u64,
        64,
    );
    if arena <= 0 || device_addr == 0 {
        fail(Exit::DmaFailed, "no DMA arena");
    }
    // SAFETY: the arena just mapped into this process, 16 KiB, writable.
    unsafe { (arena as *mut u64).write_volatile(0x5EED_DA7A) };

    // 5. Its MSI, set up, with nothing pending.
    if syscall4(SYS_PCI_DEVICE_IRQ_SETUP, handle, 0, 0, 0) != 0 {
        fail(Exit::IrqSetupFailed, "the card's MSI could not be set up");
    }
    if syscall4(SYS_PCI_DEVICE_IRQ_WAIT, handle, 0, 0, 0) < 0 {
        fail(
            Exit::IrqTakeFailed,
            "the card's interrupts could not be taken",
        );
    }

    // 6. Out without closing: the exit releases the device.
    print("[devprobe] opened, read the MAC, made an arena and set up MSI\n");
    exit(Exit::Ok);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    fail(Exit::Panicked, "panicked");
}
