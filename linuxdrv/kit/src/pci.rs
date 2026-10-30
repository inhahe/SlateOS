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
//! And its interrupts: its MSI, MSI-X or legacy line (INTx) set up (the
//! kernel programs them), which of its vectors have had interrupts since
//! last asked, whether any has without taking them, and whether the loop,
//! with nothing to run, waits for them too ([`wait_items`]) -- while the C
//! half has a handler requested and enabled for one.
//!
//! Results are the system call's: a value of 0 or more, or a negative
//! SlateOS error code (`PermissionDenied`, -400, for a function the process
//! holds no capability for, or a configuration register the kernel manages).

use crate::bars::{self, Bars};
use crate::sys::{Single, syscall4};
use crate::wait::{KIND_PCI_DEVICE, POLLERR, POLLHUP, POLLIN, POLLNVAL, WaitItem};

const SYS_PCI_DEVICE_OPEN: u64 = 1179;
const SYS_PCI_DEVICE_BAR: u64 = 1181;
const SYS_PCI_DEVICE_CONFIG_READ: u64 = 1182;
const SYS_PCI_DEVICE_CONFIG_WRITE: u64 = 1183;
const SYS_PCI_DEVICE_MAP_BAR: u64 = 1184;
const SYS_PCI_DEVICE_IRQ_SETUP: u64 = 1186;
/// `SYS_PCI_DEVICE_IRQ_SETUP`'s kinds: messages (MSI or MSI-X), or the
/// legacy line.
const IRQ_SETUP_MESSAGE: u64 = 0;
const IRQ_SETUP_INTX: u64 = 1;
/// A DMA arena placed where the caller says (`sys.rs`'s DMA window).
pub(crate) const SYS_PCI_DEVICE_DMA_ALLOC_AT: u64 = 1188;
const SYS_PCI_DEVICE_IRQ_PENDING: u64 = 1189;
/// `SYS_PCI_DEVICE_IRQ_PENDING`'s ways: take the interrupts, or look.
const IRQ_PENDING_TAKE: u64 = 0;
const IRQ_PENDING_LOOK: u64 = 1;

const SYS_PCI_DEVICE_CLOSE: u64 = 1180;

/// SlateOS's `OutOfMemory`: more functions opened than are kept.
const OUT_OF_MEMORY: i64 = -100;
/// SlateOS's `InvalidArgument`: a configuration offset no register has.
const INVALID_ARGUMENT: i64 = -3;
/// SlateOS's `ResourceExhausted` and `InternalError`: the interrupt set-ups
/// that fail having taken the old one away.
const RESOURCE_EXHAUSTED: i64 = -304;
const INTERNAL_ERROR: i64 = -1;

/// How many functions one process may be given: the C half asks
/// (`slx_pci_functions`), and sizes its own tables by it.
pub(crate) const FUNCTIONS: usize = 8;

/// `slx_pci_functions`: how many functions one process may be given.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_functions() -> usize {
    FUNCTIONS
}

/// A function opened: its handle, its BARs, whether its interrupts (MSI,
/// MSI-X or INTx) are set up, whether the loop waits for them, and whether
/// the kernel has said it cannot be waited on.
struct Function {
    handle: i64,
    bars: Bars,
    irqs: bool,
    listen: bool,
    unwaitable: bool,
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
        irqs: false,
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

/// `slx_pci_irq_setup`: the function's interrupts set up -- the kernel
/// programs them: `vectors` 0 for its MSI, answering 0; more for its MSI-X,
/// answering how many vectors it has. A set-up the function has already is
/// kept; another replaces it. Or an error: one the kernel answers before it
/// changes anything (no such capability, a set-up in progress) leaves the
/// function as it was; running out of vectors, or an internal error, leaves
/// it with none, the old set-up gone.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_setup(handle: i64, vectors: u32) -> i64 {
    irq_setup(handle, vectors, IRQ_SETUP_MESSAGE)
}

/// `slx_pci_intx_setup`: the function's legacy line set up -- routed as the
/// firmware routed it, its interrupts then taken as one MSI vector's, the
/// kernel quenching the function at each until it is taken -- answering 0.
/// Or an error, as [`slx_pci_irq_setup`]'s: `NotSupported` for a function
/// with no pin or routed line, which changes nothing.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_intx_setup(handle: i64) -> i64 {
    irq_setup(handle, 0, IRQ_SETUP_INTX)
}

/// The function's interrupts set up as `kind` (`count` vectors, for MSI-X)
/// and the answer recorded: set up, and waitable again -- what made it not
/// was its last set-up's -- or, after a failure that took the old set-up
/// away, none.
fn irq_setup(handle: i64, count: u32, kind: u64) -> i64 {
    let answer = syscall4(
        SYS_PCI_DEVICE_IRQ_SETUP,
        handle.cast_unsigned(),
        u64::from(count),
        kind,
        0,
    );
    with_function(handle, |f| {
        if answer >= 0 {
            f.irqs = true;
            f.unwaitable = false;
        } else if matches!(answer, RESOURCE_EXHAUSTED | INTERNAL_ERROR) {
            f.irqs = false;
        }
    });
    answer
}

/// `slx_pci_irq_pending`: which of the function's vectors have had
/// interrupts since last asked, taken now without waiting -- MSI-X vector
/// `i` as bit `i`, MSI as bit 0; an error while none are set up.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_pending(handle: i64) -> i64 {
    syscall4(
        SYS_PCI_DEVICE_IRQ_PENDING,
        handle.cast_unsigned(),
        IRQ_PENDING_TAKE,
        0,
        0,
    )
}

/// `slx_pci_irq_ready`: whether the function has had interrupts not yet
/// taken, looked at without taking them -- so that a legacy line, which
/// the kernel quenches until its interrupts are taken, is re-armed only
/// once its handler has run. False while none are set up.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_ready(handle: i64) -> bool {
    with_function(handle, |f| f.irqs) == Some(true)
        && syscall4(
            SYS_PCI_DEVICE_IRQ_PENDING,
            handle.cast_unsigned(),
            IRQ_PENDING_LOOK,
            0,
            0,
        ) > 0
}

/// `slx_pci_irq_listen`: whether the loop, with nothing to run, waits for
/// the function's interrupts too -- only once they are set up, the handle
/// being an error to wait on before.
#[unsafe(no_mangle)]
pub extern "C" fn slx_pci_irq_listen(handle: i64, on: bool) {
    with_function(handle, |f| f.listen = on && f.irqs);
}

/// The waits for the functions the loop listens to, while it has nothing to
/// run ([`crate::wait`]): written into `items`, as many as fit; how many.
/// A function the kernel answered cannot be waited on is not waited for
/// again: waiting on it would return at once, every time.
pub(crate) fn wait_items(items: &mut [WaitItem]) -> usize {
    // SAFETY: as in `with_function`.
    let open = unsafe { OPEN.get() };
    let mut count = 0usize;
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
    count
}

/// After the wait: a function `items` says cannot be waited on -- its MSI
/// gone, or itself closed -- is said, and marked so. Ready or not, the
/// loop turns and takes what came.
pub(crate) fn waited(items: &[WaitItem]) {
    for item in items {
        if item.revents & (POLLERR | POLLHUP | POLLNVAL) == 0 {
            continue;
        }
        let handle = item.handle.cast_signed();
        if with_function(handle, |f| core::mem::replace(&mut f.unwaitable, true)) == Some(false) {
            crate::sys::log(b"[lx] a device can no longer be waited on: its interrupts are not\n");
        }
    }
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
