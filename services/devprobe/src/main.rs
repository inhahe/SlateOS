//! devprobe — the device interface's first ring-3 user (design-decisions
//! 1579, roadmap.md 2.1).
//!
//! A native probe driver with no Linux code in it: what the Linux driver
//! layer will stand on, used from userspace with real system calls before
//! anything depends on it. The kernel's boot self-test
//! (`devhandle::self_test_ring3`) spawns it holding a `PciDevice` capability
//! for exactly one PCI function -- the boot test's second e1000e, on bus 0,
//! which no kernel driver binds -- and it:
//!
//! 1. tries to open every function on bus 0, and must open exactly one, with
//!    every other refused by the capability check (`PERMISSION_DENIED`);
//! 2. reads the function's vendor and device ids through the handle;
//! 3. turns on memory decoding and bus mastering through the command
//!    register, as a driver does, maps BAR0, and reads the MAC address the
//!    boot test gave the card (`52:54:00:12:34:59`), from the receive-address
//!    registers or, failing those, the EEPROM through EERD;
//! 4. allocates a DMA arena and writes it;
//! 5. finds the device `POLLERR` in `SYS_WAIT_MULTIPLE` while its MSI is
//!    not set up; sets up the card's MSI and takes its (zero) pending
//!    interrupts; finds
//!    the device not ready in `SYS_WAIT_MULTIPLE`; has the card raise an
//!    interrupt (IMS, ICS); waits for it with `SYS_WAIT_MULTIPLE`, as a
//!    driver's event loop waits on its device beside its other handles; and
//!    takes it;
//! 6. exits *without* closing the handle, so that its exit's release of the
//!    device is what the kernel test checks next.
//!
//! It exits with 42 when every step worked, and otherwise with the failed
//! step's own code (`devprobe::Exit`), saying which on the console. What it
//! decodes is in `lib.rs`, where the host tests it.

#![no_std]
#![no_main]

use devprobe::{
    COMMAND_BUS_MASTER, COMMAND_MEMORY_SPACE, E1000E_82574, EERD, EXPECTED_MAC, Exit, ICR, ICS,
    IMC, IMS, INTEL, KIND_PCI_DEVICE, MIN_BAR0, PCI_COMMAND, POLLERR, POLLIN, RAH0, RAL0,
    TEST_CAUSE, WaitItem, eerd_data, eerd_read_command, mac_from_eeprom_words,
    mac_from_receive_address, note_open, register_inside,
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
const SYS_PCI_DEVICE_CONFIG_WRITE: u64 = 1183;
const SYS_PCI_DEVICE_MAP_BAR: u64 = 1184;
const SYS_PCI_DEVICE_DMA_ALLOC: u64 = 1185;
const SYS_PCI_DEVICE_IRQ_SETUP: u64 = 1186;
const SYS_PCI_DEVICE_IRQ_WAIT: u64 = 1187;
const SYS_WAIT_MULTIPLE: u64 = 1066;

/// How long the probe waits for the interrupt it has the card raise: 1 s.
/// On QEMU it lands the moment ICS is written, before the wait begins, so
/// the wait finds it at once: what this checks is the readiness answer,
/// through the system call. The wake of a waiter already parked is checked
/// by the kernel's own test, which parks its waiter first
/// (`devhandle::self_test`).
const INTERRUPT_WAIT_NS: u64 = 1_000_000_000;

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

/// BAR0, mapped into this process: [`MIN_BAR0`] bytes or more from `base`.
/// Made only by [`Bar0::map`], so every register access through it is
/// inside a live mapping.
struct Bar0 {
    base: usize,
}

impl Bar0 {
    /// Map BAR0 of the device `handle` names, if it is a memory BAR of at
    /// least [`MIN_BAR0`] bytes; failing the probe otherwise.
    fn map(handle: u64) -> Bar0 {
        let size = syscall4(SYS_PCI_DEVICE_BAR, handle, 0, 0, 0);
        if !u64::try_from(size).is_ok_and(|size| size >= MIN_BAR0) {
            fail(
                Exit::BarUnusable,
                "BAR0 is not a memory BAR of at least 128 KiB",
            );
        }
        match usize::try_from(syscall4(SYS_PCI_DEVICE_MAP_BAR, handle, 0, 0, 0)) {
            Ok(base) if base != 0 => Bar0 { base },
            _ => fail(Exit::MapFailed, "BAR0 could not be mapped"),
        }
    }

    /// The address of the register at `offset`, which must lie inside the
    /// first [`MIN_BAR0`] bytes; a register outside is a bug in the probe.
    fn register(&self, offset: usize) -> *mut u32 {
        if !register_inside(offset, MIN_BAR0) {
            fail(Exit::Panicked, "a register outside BAR0");
        }
        self.base.wrapping_add(offset) as *mut u32
    }

    fn read(&self, offset: usize) -> u32 {
        // SAFETY: `register` answers only for a 4-aligned register inside
        // the mapping `map` made, which lives as long as the process.
        unsafe { self.register(offset).read_volatile() }
    }

    fn write(&self, offset: usize, value: u32) {
        // SAFETY: as for `read`.
        unsafe { self.register(offset).write_volatile(value) }
    }

    /// The MAC address from receive address 0, if valid, else from the
    /// EEPROM's first three words through EERD; `None` if an EEPROM read
    /// never finished.
    fn mac(&self) -> Option<[u8; 6]> {
        if let Some(mac) = mac_from_receive_address(self.read(RAL0), self.read(RAH0)) {
            return Some(mac);
        }
        let mut words = [0u16; 3];
        for (word, slot) in (0u16..).zip(words.iter_mut()) {
            self.write(EERD, eerd_read_command(word));
            *slot = (0..EERD_POLLS).find_map(|_| eerd_data(self.read(EERD)))?;
        }
        Some(mac_from_eeprom_words(words))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // 1. Exactly one function on bus 0 opens -- the one granted -- and every
    //    other is refused by the capability check.
    let mut found = None;
    for device in 0..32u64 {
        for function in 0..8u64 {
            let result = syscall4(SYS_PCI_DEVICE_OPEN, (device << 3) | function, 0, 0, 0);
            match note_open(&mut found, result) {
                Ok(()) => {}
                Err(Exit::OpenedMore) => {
                    fail(Exit::OpenedMore, "more than the granted function opened");
                }
                Err(code) => fail(code, "an open failed other than by the capability check"),
            }
        }
    }
    let Some(handle) = found else {
        fail(Exit::OpenedNone, "no function opened");
    };

    // 2. It is the card the test attached.
    let vendor = syscall4(SYS_PCI_DEVICE_CONFIG_READ, handle, 0, 2, 0);
    let device = syscall4(SYS_PCI_DEVICE_CONFIG_READ, handle, 2, 2, 0);
    if vendor != i64::from(INTEL) || device != i64::from(E1000E_82574) {
        fail(Exit::WrongDevice, "the opened function is not an 82574");
    }

    // 3. Decoding and bus mastering on, as a driver turns them on, then BAR0
    //    mapped and the MAC read through it.
    let on = COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER;
    let Ok(command) = u64::try_from(syscall4(
        SYS_PCI_DEVICE_CONFIG_READ,
        handle,
        PCI_COMMAND,
        2,
        0,
    )) else {
        fail(
            Exit::CommandRefused,
            "the command register could not be read",
        );
    };
    if syscall4(
        SYS_PCI_DEVICE_CONFIG_WRITE,
        handle,
        PCI_COMMAND,
        2,
        command | on,
    ) != 0
        || !u64::try_from(syscall4(
            SYS_PCI_DEVICE_CONFIG_READ,
            handle,
            PCI_COMMAND,
            2,
            0,
        ))
        .is_ok_and(|now| now & on == on)
    {
        fail(
            Exit::CommandRefused,
            "decoding and bus mastering could not be turned on",
        );
    }
    let bar0 = Bar0::map(handle);
    if bar0.mac() != Some(EXPECTED_MAC) {
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

    // 5. Its MSI, and an interrupt waited for and taken.
    interrupts(handle, &bar0);

    // 6. Out without closing: the exit releases the device.
    print("[devprobe] opened, read the MAC, made an arena, and waited for an interrupt\n");
    exit(Exit::Ok);
}

/// Step 5: `SYS_WAIT_MULTIPLE` answering `POLLERR` before the card's MSI is
/// set up; then the MSI set up, with nothing pending; the device not ready;
/// an interrupt the card is made to raise (IMS, ICS), waited for there as a
/// driver's event loop waits, and taken.
fn interrupts(handle: u64, bar0: &Bar0) {
    let mut item = WaitItem {
        handle,
        kind: KIND_PCI_DEVICE,
        events: POLLIN,
        ..WaitItem::default()
    };
    if wait_for(&mut item, 0) != 1 || item.revents & POLLERR == 0 {
        fail(
            Exit::NoErrorBeforeSetUp,
            "a device with no MSI set up was not POLLERR",
        );
    }
    if syscall4(SYS_PCI_DEVICE_IRQ_SETUP, handle, 0, 0, 0) != 0 {
        fail(Exit::IrqSetupFailed, "the card's MSI could not be set up");
    }
    if syscall4(SYS_PCI_DEVICE_IRQ_WAIT, handle, 0, 0, 0) < 0 {
        fail(
            Exit::IrqTakeFailed,
            "the card's interrupts could not be taken",
        );
    }
    if wait_for(&mut item, 0) != 0 {
        fail(
            Exit::ReadyTooSoon,
            "the device was ready before it interrupted",
        );
    }
    bar0.write(IMS, TEST_CAUSE);
    bar0.write(ICS, TEST_CAUSE);
    let ready = wait_for(&mut item, INTERRUPT_WAIT_NS);
    let taken = syscall4(SYS_PCI_DEVICE_IRQ_WAIT, handle, 0, 0, 0);
    // Quiet again: every cause masked, and the one raised read away.
    bar0.write(IMC, u32::MAX);
    let _cleared = bar0.read(ICR);
    if ready != 1 || item.revents & POLLIN == 0 {
        fail(
            Exit::InterruptLost,
            "the card's interrupt did not make the device ready",
        );
    }
    if taken < 1 {
        fail(Exit::InterruptLost, "the card's interrupt was not counted");
    }
}

/// `SYS_WAIT_MULTIPLE` on the one item: how many items are ready (0 or 1),
/// waiting up to `timeout_ns` (0 looks once); negative on failure.
fn wait_for(item: &mut WaitItem, timeout_ns: u64) -> i64 {
    item.revents = 0;
    syscall4(
        SYS_WAIT_MULTIPLE,
        core::ptr::from_mut(item) as u64,
        1,
        timeout_ns,
        0,
    )
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    fail(Exit::Panicked, "panicked");
}
