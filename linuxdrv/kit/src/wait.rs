//! The loop's wait with nothing to run (`sched`): for what can make a task
//! runnable from outside -- a device's interrupts (`pci`), a network stack
//! connecting to a card the process serves or ringing its doorbell
//! (`nicport`) -- or until `timeout` has passed. One `SYS_WAIT_MULTIPLE`
//! for all of them; each is told what fired.

use crate::sys::syscall4;

const SYS_WAIT_MULTIPLE: u64 = 1066;

/// `cap::ResourceType`'s kinds in `SYS_WAIT_MULTIPLE`: a channel end, a
/// service's listener, a device handle.
pub(crate) const KIND_CHANNEL: u32 = 1;
pub(crate) const KIND_SERVICE: u32 = 14;
pub(crate) const KIND_PCI_DEVICE: u32 = 36;
/// `poll(2)`'s `POLLIN`: a device's interrupts counted and not yet taken, a
/// message waiting, a connection waiting to be accepted.
pub(crate) const POLLIN: u32 = 0x0001;
/// `poll(2)`'s answers for a handle that cannot be waited on: a device's
/// MSI not set up (`POLLERR`), closed -- a channel's other end too
/// (`POLLHUP`) -- or not the kind said (`POLLNVAL`).
pub(crate) const POLLERR: u32 = 0x0008;
pub(crate) const POLLHUP: u32 = 0x0010;
pub(crate) const POLLNVAL: u32 = 0x0020;

/// One item of `SYS_WAIT_MULTIPLE`'s array, as the kernel documents it: 24
/// bytes, aligned to 8.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct WaitItem {
    pub(crate) handle: u64,
    pub(crate) kind: u32,
    pub(crate) events: u32,
    pub(crate) revents: u32,
    pub(crate) pad: u32,
}

/// Wait until something waited for fires, or `timeout_ns` has passed
/// (`u64::MAX`: for ever): `false`, at once, when there is nothing to wait
/// for -- the caller then sleeps, or ends.
pub(crate) fn idle(timeout_ns: u64) -> bool {
    let mut items = [WaitItem::default(); crate::pci::FUNCTIONS + crate::nicport::WAIT_ITEMS];
    let devices = crate::pci::wait_items(&mut items);
    let port = crate::nicport::wait_items(items.get_mut(devices..).unwrap_or(&mut []));
    let count = devices.saturating_add(port);
    if count == 0 {
        return false;
    }
    // A stack that submitted just as the loop said it would sleep: look at
    // everything once, and turn.
    let timeout_ns = if crate::nicport::must_not_sleep() {
        0
    } else {
        timeout_ns
    };
    // Ready, timed out or interrupted, the loop turns again and takes what
    // came; each is told what its items answered.
    let _woke = syscall4(
        SYS_WAIT_MULTIPLE,
        items.as_mut_ptr() as u64,
        count as u64,
        timeout_ns,
        0,
    );
    crate::pci::waited(items.get(..devices).unwrap_or(&[]));
    crate::nicport::waited(items.get(devices..count).unwrap_or(&[]));
    true
}
