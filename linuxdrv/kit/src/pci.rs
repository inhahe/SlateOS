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
//! And its interrupts: its MSI set up once (the kernel programs it), the
//! interrupts counted since last taken, and whether the loop, with nothing
//! to run, waits for them too ([`wait_for_interrupts`]) -- while the C half
//! has a handler requested and enabled for them.
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
const SYS_PCI_DEVICE_IRQ_SETUP: u64 = 1186;
const SYS_PCI_DEVICE_IRQ_WAIT: u64 = 1187;
/// A DMA arena placed where the caller says (`sys.rs`'s DMA window).
pub(crate) const SYS_PCI_DEVICE_DMA_ALLOC_AT: u64 = 1188;
const SYS_WAIT_MULTIPLE: u64 = 1066;
/// `cap::ResourceType::PciDevice`: a device handle's kind in
/// `SYS_WAIT_MULTIPLE`.
const KIND_PCI_DEVICE: u32 = 36;
/// `poll(2)`'s `POLLIN`: a device's interrupts counted and not yet taken.
const POLLIN: u32 = 0x0001;
/// `poll(2)`'s answers for a handle that cannot be waited on: its MSI not
/// set up (`POLLERR`), closed (`POLLHUP`), or no device's (`POLLNVAL`).
const POLLERR: u32 = 0x0008;
const POLLHUP: u32 = 0x0010;
const POLLNVAL: u32 = 0x0020;

const SYS_PCI_DEVICE_CLOSE: u64 = 1180;

/// SlateOS's `OutOfMemory`: more functions opened than are kept.
const OUT_OF_MEMORY: i64 = -100;
/// SlateOS's `InvalidArgument`: a configuration offset no register has.
const INVALID_ARGUMENT: i64 = -3;

/// How many functions one process may be given: the C half asks
/// (`slx_pci_functions`), and sizes its own tables by it.
const FUNCTIONS: usize = 8;

/// `slx_pci_functions`: how many functions one process may be given.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_functions() -> usize {
    FUNCTIONS
}

/// A function opened: its handle, its BARs, whether its MSI is set up,
/// whether the loop waits for its interrupts, and whether the kernel has
/// said it cannot be waited on.
struct Function {
    handle: i64,
    bars: Bars,
    msi: bool,
    listen: bool,
    unwaitable: bool,
}

/// One item of `SYS_WAIT_MULTIPLE`'s array, as the kernel documents it: 24
/// bytes, aligned to 8.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct WaitItem {
    handle: u64,
    kind: u32,
    events: u32,
    revents: u32,
    pad: u32,
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
        let read = config_read(handle, offset, 4);
        let Ok(value) = u32::try_from(read) else {
            // A function whose BARs cannot be read cannot be shown to Linux
            // as it is: the open fails, and the handle goes. Its close's own
            // answer adds nothing to the read's.
            let _closed = syscall4(SYS_PCI_DEVICE_CLOSE, handle.cast_unsigned(), 0, 0, 0);
            return read.min(INVALID_ARGUMENT);
        };
        *r = value;
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
        msi: false,
        listen: false,
        unwaitable: false,
    });
    handle
}

/// `slx_pci_close`: the function `handle` names given back -- its handle
/// closed, the kernel releasing the device -- for a function the C half
/// took and then could not use. 0, or the close's error; `INVALID_ARGUMENT`
/// for a handle [`slx_pci_open`] did not give.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_close(handle: i64) -> i64 {
    // SAFETY: as in `with_function`.
    let open = unsafe { OPEN.get() };
    let Some(slot) = open
        .iter_mut()
        .find(|f| f.as_ref().is_some_and(|f| f.handle == handle))
    else {
        return INVALID_ARGUMENT;
    };
    *slot = None;
    syscall4(SYS_PCI_DEVICE_CLOSE, handle.cast_unsigned(), 0, 0, 0)
}

/// `slx_pci_irq_setup`: the function's MSI set up -- the kernel programs
/// it, once for the handle's life, so a second call finds it done: 0, or an
/// error.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_setup(handle: i64) -> i64 {
    if with_function(handle, |f| f.msi) == Some(true) {
        return 0;
    }
    let answer = syscall4(SYS_PCI_DEVICE_IRQ_SETUP, handle.cast_unsigned(), 0, 0, 0);
    if answer == 0 {
        with_function(handle, |f| f.msi = true);
    }
    answer
}

/// `slx_pci_irq_take`: the function's interrupts counted since they were
/// last taken, now, without waiting; an error while its MSI is not set up.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_take(handle: i64) -> i64 {
    syscall4(SYS_PCI_DEVICE_IRQ_WAIT, handle.cast_unsigned(), 0, 0, 0)
}

/// `slx_pci_irq_listen`: whether the loop, with nothing to run, waits for
/// the function's interrupts too -- only once its MSI is set up, the
/// handle being an error to wait on before.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_listen(handle: i64, on: bool) {
    with_function(handle, |f| f.listen = on && f.msi);
}

/// The loop with nothing to run: wait until a function it listens to
/// interrupts, or `timeout_ns` has passed (`u64::MAX`: for ever). `false`,
/// at once, when it listens to none -- the caller then sleeps, or ends. A
/// function the kernel answers cannot be waited on is said, and not waited
/// for again: waiting on it would return at once, every time.
pub(crate) fn wait_for_interrupts(timeout_ns: u64) -> bool {
    let mut items = [WaitItem::default(); FUNCTIONS];
    let mut count = 0usize;
    // SAFETY: as in `with_function`.
    let open = unsafe { OPEN.get() };
    for (item, f) in items
        .iter_mut()
        .zip(open.iter().flatten().filter(|f| f.listen && !f.unwaitable))
    {
        *item = WaitItem {
            handle: f.handle.cast_unsigned(),
            kind: KIND_PCI_DEVICE,
            events: POLLIN,
            ..WaitItem::default()
        };
        count = count.saturating_add(1);
    }
    if count == 0 {
        return false;
    }
    // Ready, timed out or interrupted, the loop turns again and takes what
    // came; only a handle answered as unwaitable needs more.
    let _woke = syscall4(
        SYS_WAIT_MULTIPLE,
        items.as_mut_ptr() as u64,
        count as u64,
        timeout_ns,
        0,
    );
    for item in items.iter().take(count) {
        if item.revents & (POLLERR | POLLHUP | POLLNVAL) == 0 {
            continue;
        }
        let handle = item.handle.cast_signed();
        if with_function(handle, |f| core::mem::replace(&mut f.unwaitable, true)) == Some(false) {
            crate::sys::log(b"[lx] a device can no longer be waited on: its interrupts are not\n");
        }
    }
    true
}

/// `slx_pci_config_read`: `width` bytes (1, 2 or 4, naturally aligned) of
/// configuration space at `offset`, below 256 -- a BAR's or the ROM's as
/// the hardware would answer.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_config_read(handle: i64, offset: u32, width: u32) -> i64 {
    let Ok(offset) = u16::try_from(offset) else {
        return INVALID_ARGUMENT;
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
/// names is -- where the kernel placed it -- how large, and where Linux's
/// view of it is, into `base`, `size` and `seen`: 1 while the two agree, -1
/// when Linux has written it elsewhere (which the device does not follow),
/// 0 for no memory BAR, the three then untouched.
///
/// # Safety
///
/// `base`, `size` and `seen` point at writable words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slx_pci_bar_window(
    handle: i64,
    bar: u32,
    base: *mut u64,
    size: *mut u64,
    seen: *mut u64,
) -> i32 {
    let window = usize::try_from(bar)
        .ok()
        .and_then(|i| with_function(handle, |f| f.bars.window(i)))
        .flatten();
    let Some(w) = window else {
        return 0;
    };
    // SAFETY: the caller's.
    unsafe {
        base.write(w.base);
        size.write(w.size);
        seen.write(w.seen);
    }
    if w.moved { -1 } else { 1 }
}

/// `slx_pci_map_bar`: memory BAR `bar` mapped in the process, whole and
/// uncached: where, or the kernel's error -- `NotSupported` (-2) for a BAR
/// smaller than a 4 KiB page or not aligned to one, which cannot be mapped
/// without exposing the registers beside it.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_map_bar(handle: i64, bar: u32) -> i64 {
    syscall4(
        SYS_PCI_DEVICE_MAP_BAR,
        handle.cast_unsigned(),
        u64::from(bar),
        0,
        0,
    )
}
