//! Waiting on several kernel objects at once (`SYS_WAIT_MULTIPLE`): the
//! serving loop's listener alone, or with a driver process's card's channel
//! ([`super::port`]). The record the call takes is restated here from the
//! kernel's doc of it (known-issues TD-A-THREE-COPIES-OF-THE-WAIT-MULTIPLE-ITEM).

use super::syscall3;

const SYS_WAIT_MULTIPLE: u64 = 1066;
/// `SYS_WAIT_MULTIPLE`'s kinds of a channel end and a service listener
/// (`cap::ResourceType`).
pub const KIND_CHANNEL: u32 = 1;
pub const KIND_SERVICE: u32 = 14;
/// `poll(2)`'s readable bit, and its bit for an item the call did not know.
const POLLIN: u32 = 1;
const POLLNVAL: u32 = 0x20;
/// SlateOS's `Interrupted`: a wait a signal, a freeze or a stop ended.
const INTERRUPTED: i64 = -8;

/// One object `SYS_WAIT_MULTIPLE` waits on, laid out as its ABI says (24
/// bytes): the handle, its kind (`cap::ResourceType`), the `poll(2)` events
/// asked for and those that came.
#[repr(C)]
pub struct WaitItem {
    handle: u64,
    kind: u32,
    events: u32,
    revents: u32,
    _tail: u32,
}

impl WaitItem {
    /// `handle`, of `kind`, waited on until it is readable.
    pub fn readable(handle: u64, kind: u32) -> Self {
        Self {
            handle,
            kind,
            events: POLLIN,
            revents: 0,
            _tail: 0,
        }
    }
}

/// `SYS_WAIT_MULTIPLE` failed, or knew an item not (`POLLNVAL`): a handle
/// gone or wrong, which waiting again would not mend.
#[derive(Debug)]
pub struct WaitFailed;

/// `items` waited on, `timeout_ns` at most. A wait a signal, a freeze or a
/// stop ended (`Interrupted`) is a wake like any: the caller looks and
/// waits again.
pub fn wait_multiple(items: &mut [WaitItem], timeout_ns: u64) -> Result<(), WaitFailed> {
    let answer = syscall3(
        SYS_WAIT_MULTIPLE,
        items.as_mut_ptr() as u64,
        items.len() as u64,
        timeout_ns.max(1),
    );
    if (answer < 0 && answer != INTERRUPTED) || items.iter().any(|i| i.revents & POLLNVAL != 0) {
        return Err(WaitFailed);
    }
    Ok(())
}

/// A connection waiting on the service `listener` waited for, `timeout_ns`
/// at most -- the serving loop's wait over the kernel's NIC, which has no
/// wait of its own.
pub fn listener(listener: u64, timeout_ns: u64) -> Result<(), WaitFailed> {
    let mut items = [WaitItem::readable(listener, KIND_SERVICE)];
    wait_multiple(&mut items, timeout_ns)
}
