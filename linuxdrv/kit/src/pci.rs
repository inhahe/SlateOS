//! The PCI functions a driver process was given, through the kernel's device
//! handle (design-decisions 1579, 1584): what the C half's PCI bus
//! (`linuxdrv/emul/lx_pci.c`) opens a function by, reads and writes its
//! configuration space through, and maps its memory BARs with.
//!
//! Each function's BARs are kept here ([`crate::bars`]), so that a
//! configuration access to them is answered as the hardware would answer,
//! the handle refusing to write them; every other access is the handle's
//! system call.
//!
//! Results are the system call's: a value of 0 or more, or a negative
//! SlateOS error code (`PermissionDenied`, -400, for a function the process
//! holds no capability for, or a configuration register the kernel manages).

use crate::bars::{self, Bars};
use crate::sys::{Single, syscall4};

const SYS_PCI_DEVICE_OPEN: u64 = 1179;
const SYS_PCI_DEVICE_BAR: u64 = 1181;
const SYS_PCI_DEVICE_CONFIG_READ: u64 = 1182;
const SYS_PCI_DEVICE_CONFIG_WRITE: u64 = 1183;
const SYS_PCI_DEVICE_MAP_BAR: u64 = 1184;

/// SlateOS's `OutOfMemory`: more functions opened than are kept.
const OUT_OF_MEMORY: i64 = -100;

/// How many functions one process may be given.
const FUNCTIONS: usize = 8;

/// A function opened: its handle and its BARs.
struct Function {
    handle: i64,
    bars: Bars,
}

static OPEN: Single<[Option<Function>; FUNCTIONS]> = Single::new([const { None }; FUNCTIONS]);

/// `f` run on the function `handle` names, if this process opened it.
fn with_function<R>(handle: i64, f: impl FnOnce(&mut Function) -> R) -> Option<R> {
    // SAFETY: the one thread; these calls do not re-enter one another, and
    // `f` makes no system call that reaches them.
    let open = unsafe { OPEN.get() };
    open.iter_mut()
        .flatten()
        .find(|g| g.handle == handle)
        .map(f)
}

fn config_read(handle: i64, offset: u16, width: u32) -> i64 {
    syscall4(
        SYS_PCI_DEVICE_CONFIG_READ,
        handle.cast_unsigned(),
        u64::from(offset),
        u64::from(width),
        0,
    )
}

/// `slx_pci_open`: open the function at `bus`, `devfn` (device << 3 |
/// function), and read its BARs: its handle, or an error.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_open(bus: u8, devfn: u8) -> i64 {
    // SAFETY: as in `with_function`.
    let open = unsafe { OPEN.get() };
    let Some(slot) = open.iter_mut().find(|f| f.is_none()) else {
        return OUT_OF_MEMORY;
    };
    let handle = syscall4(
        SYS_PCI_DEVICE_OPEN,
        (u64::from(bus) << 8) | u64::from(devfn),
        0,
        0,
        0,
    );
    if handle < 0 {
        return handle;
    }
    let mut real = [0u32; bars::COUNT];
    let mut size = [0u64; bars::COUNT];
    for (i, (r, s)) in (0u16..).zip(real.iter_mut().zip(size.iter_mut())) {
        let offset = bars::FIRST.saturating_add(i.saturating_mul(4));
        *r = u32::try_from(config_read(handle, offset, 4)).unwrap_or(0);
        // A memory BAR's size, bit 0 its prefetchability; an error for any
        // other kind, which has none here.
        let answer = syscall4(
            SYS_PCI_DEVICE_BAR,
            handle.cast_unsigned(),
            u64::from(i),
            0,
            0,
        );
        *s = u64::try_from(answer).map_or(0, |a| a & !1);
    }
    *slot = Some(Function {
        handle,
        bars: Bars::new(real, size),
    });
    handle
}

/// `slx_pci_config_read`: `width` bytes (1, 2 or 4, naturally aligned) of
/// configuration space at `offset`, below 256 -- a BAR's or the ROM's as
/// the hardware would answer.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_config_read(handle: i64, offset: u32, width: u32) -> i64 {
    let Ok(offset) = u16::try_from(offset) else {
        return config_read(handle, u16::MAX, width);
    };
    match with_function(handle, |f| f.bars.read(offset, width)).flatten() {
        Some(value) => i64::from(value),
        None => config_read(handle, offset, width),
    }
}

/// `slx_pci_config_write`: write `value`, `width` bytes at `offset`: 0, or
/// an error. A BAR's or the ROM's is kept, never reaching the device.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_config_write(handle: i64, offset: u32, width: u32, value: u32) -> i64 {
    if let Ok(at) = u16::try_from(offset)
        && with_function(handle, |f| f.bars.write(at, width, value)) == Some(true)
    {
        return 0;
    }
    syscall4(
        SYS_PCI_DEVICE_CONFIG_WRITE,
        handle.cast_unsigned(),
        u64::from(offset),
        u64::from(width),
        u64::from(value),
    )
}

/// `slx_pci_bar_window`: where memory BAR `bar` of the function `handle`
/// names is, and how large, into `base` and `size`: 1 while Linux's view of
/// it is where the kernel placed it, 0 for no memory BAR, -1 for one Linux
/// has written elsewhere -- which the device does not follow.
///
/// # Safety
///
/// `base` and `size` point at writable words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_pci_bar_window(
    handle: i64,
    bar: u32,
    base: *mut u64,
    size: *mut u64,
) -> i32 {
    let window = usize::try_from(bar)
        .ok()
        .and_then(|i| with_function(handle, |f| f.bars.window(i)))
        .unwrap_or(Ok(None));
    match window {
        Ok(Some((at, bytes))) => {
            // SAFETY: the caller's.
            unsafe {
                base.write(at);
                size.write(bytes);
            }
            1
        }
        Ok(None) => 0,
        Err(()) => -1,
    }
}

/// `slx_pci_map_bar`: where memory BAR `bar` is mapped in the process,
/// uncached, or NULL when it cannot be.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_map_bar(handle: i64, bar: u32) -> *mut u8 {
    let at = syscall4(
        SYS_PCI_DEVICE_MAP_BAR,
        handle.cast_unsigned(),
        u64::from(bar),
        0,
        0,
    );
    usize::try_from(at).map_or(core::ptr::null_mut(), |at| at as *mut u8)
}
