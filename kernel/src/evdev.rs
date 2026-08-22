//! Linux `evdev` input event devices — `/dev/input/event0` (keyboard) and
//! `/dev/input/event1` (mouse).
//!
//! ## Why this exists
//!
//! The PS/2 drivers ([`crate::keyboard`], [`crate::mouse`]) have always had
//! the data; what they lacked was a door to userspace of the shape every
//! graphical client expects. The only keyboard entry point was
//! `SYS_CONSOLE_READ_CHAR`, which hands back a decoded ASCII byte — no key
//! release, no keycode, and nothing at all for F1, the arrows, or the keypad.
//! The mouse had no userspace entry point of any kind. A compositor cannot be
//! built on that, and neither can SDL, an X server, or anything else ported in
//! future, because all of them assume the Linux `evdev` protocol.
//!
//! So this module implements that protocol rather than inventing a parallel
//! one. Lane C's request (`requests/c-a-userspace-cannot-read-the-keyboard-or-
//! the-mouse-at-all.md`) offered to own the scancode→keycode table in
//! userspace if the kernel would rather ship raw scancodes. The kernel owns it
//! here instead, for the reason lane C itself gave: a node that speaks real
//! evdev is usable by *any* client, whereas one that ships raw set-1 codes is
//! usable only by a client we also wrote.
//!
//! ## The protocol
//!
//! A read yields whole 24-byte [`InputEvent`] records and never a partial one.
//! Each coherent group of events is terminated by `EV_SYN`/`SYN_REPORT`: a PS/2
//! mouse packet carries dx, dy and the button state together, and delivered
//! without that terminator a client can observe the pointer mid-update.
//!
//! ## One source ring, many cursors
//!
//! Each device has a single ring written by its ISR, and every open file
//! description holds its own *cursor* into that ring rather than its own copy
//! of the events. Two consequences, both wanted:
//!
//! * Two readers of `/dev/input/event0` each see every event. A shared
//!   consume-once queue would let one process steal the other's keystrokes.
//! * The ISR stays single-producer and lock-free. Fanning out to a list of
//!   per-client buffers would mean taking a lock in interrupt context and
//!   allocating there on a burst.
//!
//! A client that falls more than [`RING_CAP`] events behind has its oldest
//! events overwritten. It is told so — the next record it reads is
//! `EV_SYN`/`SYN_DROPPED`, exactly as Linux signals it — and then resumes from
//! the oldest event that still exists. That is why the ring drops *oldest*: a
//! compositor 200 ms late must not be made to replay 200 ms of stale pointer
//! motion to catch up.

#![allow(dead_code)]

use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// ABI — Linux `struct input_event` and the event codes a desktop needs
// ---------------------------------------------------------------------------

/// Linux `struct input_event`, exactly as userspace reads it on x86-64.
///
/// 24 bytes with no padding: a 16-byte `struct timeval` (`__kernel_time_t sec`,
/// `suseconds_t usec`, both 64-bit), then `type`, `code` and a signed `value`.
/// The layout is asserted at compile time below, because a client memcpy's this
/// out of a read buffer and a silently different size would misparse every
/// record after the first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct InputEvent {
    /// Seconds since the epoch (`CLOCK_REALTIME`).
    pub sec: i64,
    /// Microseconds within the second.
    pub usec: i64,
    /// `EV_*` — which kind of event this is.
    pub etype: u16,
    /// Meaning depends on `etype`: a `KEY_*`/`BTN_*` code, a `REL_*` axis, ...
    pub code: u16,
    /// Meaning depends on `etype`: 0/1/2 for a key, a signed delta for an axis.
    pub value: i32,
}

/// Size of one record on the wire. A `read` always transfers a whole multiple
/// of this and never splits a record across two calls.
pub const INPUT_EVENT_SIZE: usize = core::mem::size_of::<InputEvent>();

const _: () = assert!(INPUT_EVENT_SIZE == 24, "input_event must be 24 bytes on x86-64");
const _: () = assert!(core::mem::align_of::<InputEvent>() == 8);

// Event types.
/// Synchronisation / grouping.
pub const EV_SYN: u16 = 0x00;
/// A key or button changed state.
pub const EV_KEY: u16 = 0x01;
/// A relative axis moved.
pub const EV_REL: u16 = 0x02;
/// Miscellaneous; we use it only for `MSC_SCAN`.
pub const EV_MSC: u16 = 0x04;

// `EV_SYN` codes.
/// End of one coherent packet.
pub const SYN_REPORT: u16 = 0;
/// Events were lost; the client should re-read any state it caches.
pub const SYN_DROPPED: u16 = 3;

// `EV_REL` codes.
/// Horizontal motion; positive is right.
pub const REL_X: u16 = 0;
/// Vertical motion; positive is **down**, per the evdev convention.
pub const REL_Y: u16 = 1;
/// Vertical scroll; positive is away from the user.
pub const REL_WHEEL: u16 = 8;

// `EV_MSC` codes.
/// The raw hardware scancode behind an `EV_KEY`.
pub const MSC_SCAN: u16 = 4;

// Mouse button codes (`EV_KEY`).
/// Left mouse button.
pub const BTN_LEFT: u16 = 0x110;
/// Right mouse button.
pub const BTN_RIGHT: u16 = 0x111;
/// Middle mouse button.
pub const BTN_MIDDLE: u16 = 0x112;

// `EV_KEY` values.
/// Key released.
pub const KEY_VALUE_UP: i32 = 0;
/// Key pressed.
pub const KEY_VALUE_DOWN: i32 = 1;
/// Key held; the kernel does not generate these (see [`push_key`]).
pub const KEY_VALUE_REPEAT: i32 = 2;

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/// Which input device a ring or a client belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InputDevice {
    /// `/dev/input/event0` — the PS/2 keyboard.
    Keyboard,
    /// `/dev/input/event1` — the PS/2 mouse.
    Mouse,
}

impl InputDevice {
    /// The device behind `/dev/input/eventN`, or `None` for any other `N`.
    #[must_use]
    pub const fn from_minor(n: u32) -> Option<Self> {
        match n {
            0 => Some(Self::Keyboard),
            1 => Some(Self::Mouse),
            _ => None,
        }
    }

    /// The `N` in `/dev/input/eventN`.
    #[must_use]
    pub const fn minor(self) -> u32 {
        match self {
            Self::Keyboard => 0,
            Self::Mouse => 1,
        }
    }

    /// The `EVIOCGNAME` string a client sees.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Keyboard => "AT Translated Set 2 keyboard",
            Self::Mouse => "PS/2 Generic Mouse",
        }
    }

    fn ring(self) -> &'static SourceRing {
        match self {
            Self::Keyboard => &KEYBOARD_RING,
            Self::Mouse => &MOUSE_RING,
        }
    }
}

// ---------------------------------------------------------------------------
// Source ring — single producer (ISR), many independent cursors
// ---------------------------------------------------------------------------

/// Events retained per device before the oldest is overwritten.
///
/// A PS/2 mouse at its 100 Hz default emits at most 6 records per packet, so
/// 256 is a little over two fifths of a second of continuous motion — an
/// eternity for a client that polls once per frame, and small enough that the
/// three arrays cost 4 KiB per device rather than being sized by guesswork.
pub const RING_CAP: usize = 256;
const RING_MASK: u64 = (RING_CAP as u64) - 1;
const _: () = assert!(RING_CAP.is_power_of_two());

/// One device's event ring.
///
/// ## Concurrency
///
/// **Exactly one producer.** `head` is a monotonically increasing sequence
/// number, not an index, and is stored with a plain `store` rather than
/// `fetch_add` — which is only sound because a device's ISR cannot run
/// concurrently with itself. The i8042 will not raise a line again until the
/// pending byte is read from port 0x60, and the keyboard and the mouse are
/// separate lines writing separate rings. If a third producer is ever added to
/// a ring, this must become a `fetch_add` reservation and the slot writes must
/// be published per-slot.
///
/// **Any number of consumers**, each holding a private cursor, none of which
/// write anything here. A consumer detects that it was lapped mid-read by
/// re-reading `head` afterwards; see [`EvdevClient::next_event`].
struct SourceRing {
    /// `CLOCK_REALTIME` nanoseconds at which the event was recorded.
    ts_ns: [AtomicU64; RING_CAP],
    /// `(etype << 16) | code`, packed so one store publishes both.
    packed: [AtomicU32; RING_CAP],
    /// The event's `value`.
    value: [AtomicI32; RING_CAP],
    /// Sequence number of the *next* slot to be written. Never wraps in any
    /// practical uptime: at 10^6 events/second a `u64` lasts ~580 000 years.
    head: AtomicU64,
    /// Total events ever pushed, for `evdev status`. Same as `head`, kept
    /// separately so diagnostics never perturb the ordering above.
    pushed: AtomicU64,
}

impl SourceRing {
    const fn new() -> Self {
        Self {
            ts_ns: [const { AtomicU64::new(0) }; RING_CAP],
            packed: [const { AtomicU32::new(0) }; RING_CAP],
            value: [const { AtomicI32::new(0) }; RING_CAP],
            head: AtomicU64::new(0),
            pushed: AtomicU64::new(0),
        }
    }

    /// Append one event. Callable from interrupt context: no locks, no
    /// allocation, three relaxed stores and one release store.
    fn push(&self, etype: u16, code: u16, value: i32, ts_ns: u64) {
        let seq = self.head.load(Ordering::Relaxed);
        #[allow(clippy::cast_possible_truncation)]
        let idx = (seq & RING_MASK) as usize;

        // `idx < RING_CAP` because it was masked, so all three `get`s hit. The
        // `else` arms cannot be reached; returning is still the right thing to
        // do there, since dropping an event beats indexing out of bounds in an
        // ISR.
        let (Some(ts), Some(pk), Some(vl)) = (
            self.ts_ns.get(idx),
            self.packed.get(idx),
            self.value.get(idx),
        ) else {
            return;
        };

        ts.store(ts_ns, Ordering::Relaxed);
        pk.store((u32::from(etype) << 16) | u32::from(code), Ordering::Relaxed);
        vl.store(value, Ordering::Relaxed);

        // Release: everything above must be visible to a consumer that sees
        // the new head.
        self.head.store(seq.wrapping_add(1), Ordering::Release);
        self.pushed.fetch_add(1, Ordering::Relaxed);
    }

    /// Read slot `seq` without checking whether it is still valid; the caller
    /// validates by re-reading `head`.
    fn load_slot(&self, seq: u64) -> Option<InputEvent> {
        #[allow(clippy::cast_possible_truncation)]
        let idx = (seq & RING_MASK) as usize;
        let ts = self.ts_ns.get(idx)?.load(Ordering::Relaxed);
        let packed = self.packed.get(idx)?.load(Ordering::Relaxed);
        let value = self.value.get(idx)?.load(Ordering::Relaxed);

        #[allow(clippy::cast_possible_truncation)]
        let etype = (packed >> 16) as u16;
        #[allow(clippy::cast_possible_truncation)]
        let code = (packed & 0xFFFF) as u16;

        #[allow(clippy::cast_possible_wrap)]
        let sec = (ts / 1_000_000_000) as i64;
        #[allow(clippy::cast_possible_wrap)]
        let usec = ((ts % 1_000_000_000) / 1_000) as i64;

        Some(InputEvent {
            sec,
            usec,
            etype,
            code,
            value,
        })
    }
}

static KEYBOARD_RING: SourceRing = SourceRing::new();
static MOUSE_RING: SourceRing = SourceRing::new();

/// Current `CLOCK_REALTIME` in nanoseconds, or zero before timekeeping is up.
///
/// `clock_realtime` is lock-free (two relaxed atomic loads plus a TSC read), so
/// this is safe from interrupt context. A zero timestamp during early boot is
/// preferable to not recording the event at all; evdev clients treat the
/// timestamp as advisory and none refuse a record for having one.
fn now_ns() -> u64 {
    crate::timekeeping::clock_realtime()
}

// ---------------------------------------------------------------------------
// Producer API — called from the PS/2 ISRs
// ---------------------------------------------------------------------------

/// Record a key or button transition, with the raw scancode that produced it.
///
/// Emits the Linux three-record group: `EV_MSC`/`MSC_SCAN` carrying the raw
/// hardware code, then the `EV_KEY` itself, then `EV_SYN`/`SYN_REPORT`. A
/// client that wants the physical key uses the keycode; one doing its own
/// keymapping (an X server with a custom layout) uses `MSC_SCAN`.
///
/// **No autorepeat.** `value` is only ever 0 or 1 here. The PS/2 controller's
/// own typematic repeat does generate repeated make codes, and Linux
/// deliberately suppresses them in favour of a software repeat with a
/// client-settable delay and rate; forwarding hardware repeat as `value == 1`
/// would give a compositor a repeat it cannot turn off or retime. Callers
/// filter it (see [`crate::keyboard::handle_scancode`]).
pub fn push_key(dev: InputDevice, keycode: u16, scancode: u16, pressed: bool) {
    let ts = now_ns();
    let ring = dev.ring();
    if scancode != 0 {
        ring.push(EV_MSC, MSC_SCAN, i32::from(scancode), ts);
    }
    ring.push(
        EV_KEY,
        keycode,
        if pressed { KEY_VALUE_DOWN } else { KEY_VALUE_UP },
        ts,
    );
    ring.push(EV_SYN, SYN_REPORT, 0, ts);
}

/// Record one complete mouse packet as a single synchronised group.
///
/// `buttons` is the PS/2 bitmask (bit 0 left, bit 1 right, bit 2 middle) and
/// `prev_buttons` the previous one, so that only the buttons that actually
/// changed produce an `EV_KEY`. `dy` is in the PS/2 sense (positive is *up*)
/// and is negated here, because `REL_Y` is positive-down.
///
/// Zero axes are omitted — Linux does the same, and a client that receives
/// `REL_X` alone must already handle the absent axis as "no change". The
/// `EV_SYN` is emitted unconditionally, even for a packet in which nothing
/// changed, because its absence is what tells a client a group is incomplete.
pub fn push_mouse_packet(buttons: u8, prev_buttons: u8, dx: i16, dy: i16, dz: i8) {
    let ts = now_ns();
    let ring = InputDevice::Mouse.ring();

    if dx != 0 {
        ring.push(EV_REL, REL_X, i32::from(dx), ts);
    }
    if dy != 0 {
        // PS/2 reports +Y as up; evdev's REL_Y is +down.
        ring.push(EV_REL, REL_Y, i32::from(dy).saturating_neg(), ts);
    }
    if dz != 0 {
        ring.push(EV_REL, REL_WHEEL, i32::from(dz), ts);
    }

    let changed = buttons ^ prev_buttons;
    for (bit, btn) in [(0u8, BTN_LEFT), (1, BTN_RIGHT), (2, BTN_MIDDLE)] {
        let mask = 1u8 << bit;
        if changed & mask != 0 {
            let down = buttons & mask != 0;
            ring.push(
                EV_KEY,
                btn,
                if down { KEY_VALUE_DOWN } else { KEY_VALUE_UP },
                ts,
            );
        }
    }

    ring.push(EV_SYN, SYN_REPORT, 0, ts);
}

/// Total events ever pushed for `dev`, for diagnostics.
#[must_use]
pub fn events_pushed(dev: InputDevice) -> u64 {
    dev.ring().pushed.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Consumer — one cursor per open file description
// ---------------------------------------------------------------------------

/// What a client's cursor reads next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    /// An event was available.
    Event(InputEvent),
    /// The ring is caught up; nothing more until the ISR pushes.
    Empty,
}

/// One open file description's position in a device's event stream.
///
/// Cloned by `dup`/`fork` only through the refcounted instance in
/// [`crate::proc::linux_fd`]; two *separate* opens get two separate cursors,
/// which is what makes both see every event.
#[derive(Clone, Copy, Debug)]
pub struct EvdevClient {
    device: InputDevice,
    /// Sequence number of the next event this client will read.
    cursor: u64,
    /// A `SYN_DROPPED` is owed to this client before the stream resumes.
    syn_dropped_pending: bool,
    /// Number of times this client has been lapped, for diagnostics.
    drops: u64,
}

impl EvdevClient {
    /// Start a client at the *current* end of the stream.
    ///
    /// Deliberately not at the oldest retained event: a process that opens the
    /// keyboard should not immediately receive whatever was typed before it
    /// existed. Linux behaves the same way — an evdev client's buffer starts
    /// empty.
    #[must_use]
    pub fn new(device: InputDevice) -> Self {
        Self {
            device,
            cursor: device.ring().head.load(Ordering::Acquire),
            syn_dropped_pending: false,
            drops: 0,
        }
    }

    /// Which device this client reads.
    #[must_use]
    pub const fn device(&self) -> InputDevice {
        self.device
    }

    /// How many times this client has been lapped by the producer.
    #[must_use]
    pub const fn drop_count(&self) -> u64 {
        self.drops
    }

    /// Bring `cursor` forward if the producer has lapped it, recording that a
    /// `SYN_DROPPED` is owed. Returns the current head.
    fn resync(&mut self, head: u64) -> u64 {
        let behind = head.saturating_sub(self.cursor);
        if behind > RING_CAP as u64 {
            // Everything between `cursor` and `head - RING_CAP` has been
            // overwritten. Resume at the oldest event that still exists.
            self.cursor = head.saturating_sub(RING_CAP as u64);
            self.syn_dropped_pending = true;
            self.drops = self.drops.saturating_add(1);
        }
        head
    }

    /// Take the next event, or [`Next::Empty`] if the client is caught up.
    fn next_event(&mut self) -> Next {
        // A drop that was detected on a previous call is reported before any
        // further real event, so the client learns of the gap in stream order.
        if self.syn_dropped_pending {
            self.syn_dropped_pending = false;
            let ts = now_ns();
            #[allow(clippy::cast_possible_wrap)]
            let sec = (ts / 1_000_000_000) as i64;
            #[allow(clippy::cast_possible_wrap)]
            let usec = ((ts % 1_000_000_000) / 1_000) as i64;
            return Next::Event(InputEvent {
                sec,
                usec,
                etype: EV_SYN,
                code: SYN_DROPPED,
                value: 0,
            });
        }

        let ring = self.device.ring();

        // Bounded retry. Each iteration either returns or advances past a slot
        // the producer overwrote mid-read; a producer fast enough to lap us
        // twice in a row is one we cannot keep up with anyway, and spinning
        // here in a syscall would be worse than reporting Empty and letting
        // the caller poll again.
        for _ in 0..4 {
            let head = self.resync(ring.head.load(Ordering::Acquire));
            if self.cursor >= head {
                return Next::Empty;
            }

            let Some(ev) = ring.load_slot(self.cursor) else {
                return Next::Empty;
            };

            // Re-read head: if the producer wrapped past our slot while we
            // were reading it, the three fields we just loaded may come from
            // two different events. Discard and resync rather than hand a
            // client a record that never happened.
            let head2 = ring.head.load(Ordering::Acquire);
            if head2.saturating_sub(self.cursor) > RING_CAP as u64 {
                self.resync(head2);
                continue;
            }

            self.cursor = self.cursor.saturating_add(1);
            return Next::Event(ev);
        }
        Next::Empty
    }

    /// True if a [`read`](Self::read_into) would return at least one record
    /// without blocking. Backs `poll`/`epoll` readiness.
    #[must_use]
    pub fn readable(&self) -> bool {
        if self.syn_dropped_pending {
            return true;
        }
        self.cursor < self.device.ring().head.load(Ordering::Acquire)
    }

    /// Fill `buf` with as many whole records as are available and fit.
    ///
    /// Returns the number of **bytes** written, always a multiple of
    /// [`INPUT_EVENT_SIZE`]. Zero means nothing was available; the caller
    /// decides whether that is `EAGAIN` or a reason to block, because only it
    /// knows the fd's `O_NONBLOCK`.
    ///
    /// # Errors
    ///
    /// `InvalidArgument` if `buf` cannot hold one whole record. Linux returns
    /// `EINVAL` for the same case rather than a short read, because a client
    /// that passes a 16-byte buffer has misunderstood the protocol and a short
    /// read would let it keep misunderstanding it.
    pub fn read_into(&mut self, buf: &mut [u8]) -> KernelResult<usize> {
        if buf.len() < INPUT_EVENT_SIZE {
            return Err(KernelError::InvalidArgument);
        }
        let mut written = 0usize;
        while written.saturating_add(INPUT_EVENT_SIZE) <= buf.len() {
            match self.next_event() {
                Next::Event(ev) => {
                    let bytes = event_bytes(&ev);
                    let Some(dst) = buf.get_mut(written..written.saturating_add(INPUT_EVENT_SIZE))
                    else {
                        break;
                    };
                    dst.copy_from_slice(&bytes);
                    written = written.saturating_add(INPUT_EVENT_SIZE);
                }
                Next::Empty => break,
            }
        }
        Ok(written)
    }
}

/// Serialise one event to its 24 wire bytes.
///
/// Written field by field rather than by transmuting the struct: the layout is
/// asserted above, but a byte-wise encoding is what makes the wire format
/// independent of the compiler's opinion, and it costs nothing at this rate.
#[must_use]
pub fn event_bytes(ev: &InputEvent) -> [u8; INPUT_EVENT_SIZE] {
    let mut out = [0u8; INPUT_EVENT_SIZE];
    // Every range is a compile-time constant inside a 24-byte array, so no
    // `get_mut` can fail; the `if let`s are how that is expressed without
    // indexing, per the crate's `indexing_slicing` lint.
    if let Some(d) = out.get_mut(0..8) {
        d.copy_from_slice(&ev.sec.to_le_bytes());
    }
    if let Some(d) = out.get_mut(8..16) {
        d.copy_from_slice(&ev.usec.to_le_bytes());
    }
    if let Some(d) = out.get_mut(16..18) {
        d.copy_from_slice(&ev.etype.to_le_bytes());
    }
    if let Some(d) = out.get_mut(18..20) {
        d.copy_from_slice(&ev.code.to_le_bytes());
    }
    if let Some(d) = out.get_mut(20..24) {
        d.copy_from_slice(&ev.value.to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Scan code set 1 → Linux keycode
// ---------------------------------------------------------------------------

/// Translate a non-extended scan code set 1 code to a Linux keycode.
///
/// **The mapping is the identity across the whole useful range**, and that is
/// not a coincidence to be distrusted: Linux's keycode numbering was derived
/// from AT scan code set 1 in the first place. `KEY_ESC` is 1 and `Esc` is
/// 0x01; `KEY_KPDOT` is 83 and keypad-`.` is 0x53; `KEY_102ND` is 86 and the
/// extra ISO key is 0x56; `KEY_F11`/`KEY_F12` are 87/88 and F11/F12 are
/// 0x57/0x58. The self-test pins those four anchors plus the letter block so a
/// future edit cannot quietly break the correspondence.
///
/// Returns `None` for 0 (not a key) and for codes above 0x58, which set 1 does
/// not assign and which therefore have no keycode to give.
#[must_use]
pub const fn set1_to_keycode(code: u8) -> Option<u16> {
    if code == 0 || code > 0x58 {
        None
    } else {
        Some(code as u16)
    }
}

/// Translate an `0xE0`-prefixed scan code set 1 code to a Linux keycode.
///
/// Here the identity does *not* hold — the extended set was bolted on later
/// and Linux assigns it keycodes from 96 upwards — so this is a real table.
/// It covers the navigation block, both right-hand modifiers, the keypad keys
/// that moved, the Windows/Menu keys, and the common multimedia keys; anything
/// else returns `None` and is reported as `MSC_SCAN` only, which is honest
/// rather than a guessed keycode a client would act on.
#[must_use]
pub const fn set1_extended_to_keycode(code: u8) -> Option<u16> {
    Some(match code {
        0x10 => 165, // KEY_PREVIOUSSONG
        0x19 => 163, // KEY_NEXTSONG
        0x1C => 96,  // KEY_KPENTER
        0x1D => 97,  // KEY_RIGHTCTRL
        0x20 => 113, // KEY_MUTE
        0x21 => 140, // KEY_CALC
        0x22 => 164, // KEY_PLAYPAUSE
        0x24 => 166, // KEY_STOPCD
        0x2E => 114, // KEY_VOLUMEDOWN
        0x30 => 115, // KEY_VOLUMEUP
        0x32 => 150, // KEY_WWW
        0x35 => 98,  // KEY_KPSLASH
        0x37 => 99,  // KEY_SYSRQ  (Print Screen)
        0x38 => 100, // KEY_RIGHTALT
        0x46 => 119, // KEY_PAUSE  (Ctrl+Break form)
        0x47 => 102, // KEY_HOME
        0x48 => 103, // KEY_UP
        0x49 => 104, // KEY_PAGEUP
        0x4B => 105, // KEY_LEFT
        0x4D => 106, // KEY_RIGHT
        0x4F => 107, // KEY_END
        0x50 => 108, // KEY_DOWN
        0x51 => 109, // KEY_PAGEDOWN
        0x52 => 110, // KEY_INSERT
        0x53 => 111, // KEY_DELETE
        0x5B => 125, // KEY_LEFTMETA
        0x5C => 126, // KEY_RIGHTMETA
        0x5D => 127, // KEY_COMPOSE (Menu)
        0x5E => 116, // KEY_POWER
        0x5F => 142, // KEY_SLEEP
        0x63 => 143, // KEY_WAKEUP
        0x65 => 217, // KEY_SEARCH
        0x66 => 156, // KEY_BOOKMARKS
        0x67 => 173, // KEY_REFRESH
        0x68 => 128, // KEY_STOP
        0x69 => 159, // KEY_FORWARD
        0x6A => 158, // KEY_BACK
        0x6B => 157, // KEY_COMPUTER
        0x6C => 155, // KEY_MAIL
        0x6D => 161, // KEY_MEDIA
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise the wire format, the keycode tables, the per-client cursor, and —
/// the part that cannot be checked by inspection — what a client sees when the
/// producer laps it.
///
/// Pushes into the real rings rather than a fixture. That is safe because the
/// test runs during boot before any fd exists, and because a client created
/// afterwards starts at the *current* head, so nothing written here can leak
/// into a real reader's stream.
///
/// # Errors
///
/// [`KernelError::InternalError`] on the first failed invariant.
pub fn self_test() -> KernelResult<()> {
    macro_rules! check {
        ($cond:expr, $msg:expr) => {
            if !($cond) {
                crate::serial_println!("[evdev] SELF-TEST FAILED: {}", $msg);
                return Err(KernelError::InternalError);
            }
        };
    }

    // --- wire format -------------------------------------------------------
    let ev = InputEvent {
        sec: 0x0102_0304_0506_0708,
        usec: 1_000,
        etype: EV_KEY,
        code: 0x1234,
        value: -7,
    };
    let bytes = event_bytes(&ev);
    check!(bytes.len() == 24, "record is 24 bytes");
    check!(
        bytes.get(16..18) == Some(&EV_KEY.to_le_bytes()[..]),
        "type at offset 16"
    );
    check!(
        bytes.get(18..20) == Some(&0x1234_u16.to_le_bytes()[..]),
        "code at offset 18"
    );
    check!(
        bytes.get(20..24) == Some(&(-7_i32).to_le_bytes()[..]),
        "value at offset 20, and it is signed"
    );

    // --- keycode tables ----------------------------------------------------
    // The anchors that pin the set-1/keycode identity. If any of these ever
    // stops holding, `set1_to_keycode` must become a real table, and this is
    // where that will be noticed rather than in a client's keymap.
    check!(set1_to_keycode(0x01) == Some(1), "Esc -> KEY_ESC(1)");
    check!(set1_to_keycode(0x1E) == Some(30), "A -> KEY_A(30)");
    check!(set1_to_keycode(0x39) == Some(57), "Space -> KEY_SPACE(57)");
    check!(set1_to_keycode(0x53) == Some(83), "KP. -> KEY_KPDOT(83)");
    check!(set1_to_keycode(0x56) == Some(86), "ISO -> KEY_102ND(86)");
    check!(set1_to_keycode(0x57) == Some(87), "F11 -> KEY_F11(87)");
    check!(set1_to_keycode(0x58) == Some(88), "F12 -> KEY_F12(88)");
    check!(set1_to_keycode(0x00).is_none(), "0 is not a key");
    check!(set1_to_keycode(0x59).is_none(), "0x59 is unassigned in set 1");

    check!(
        set1_extended_to_keycode(0x48) == Some(103),
        "E0 48 -> KEY_UP(103)"
    );
    check!(
        set1_extended_to_keycode(0x1D) == Some(97),
        "E0 1D -> KEY_RIGHTCTRL(97)"
    );
    check!(
        set1_extended_to_keycode(0x5B) == Some(125),
        "E0 5B -> KEY_LEFTMETA(125)"
    );
    check!(
        set1_extended_to_keycode(0x01).is_none(),
        "an unmapped extended code has no keycode"
    );
    // The two tables must disagree: E0 48 is Up (103), not KP8 (72). That
    // divergence is the entire reason the extended table exists.
    check!(
        set1_extended_to_keycode(0x48) != set1_to_keycode(0x48),
        "extended 0x48 differs from normal 0x48"
    );

    // --- a client starts empty, then sees what it was opened for -----------
    let mut c = EvdevClient::new(InputDevice::Keyboard);
    check!(!c.readable(), "a fresh client has nothing buffered");
    let mut buf = [0u8; INPUT_EVENT_SIZE * 8];
    check!(c.read_into(&mut buf)? == 0, "a fresh client reads zero bytes");

    push_key(InputDevice::Keyboard, 30, 0x1E, true);
    check!(c.readable(), "a keypress makes the client readable");
    check!(
        c.read_into(&mut buf)? == 72,
        "one keypress is MSC_SCAN + EV_KEY + SYN_REPORT"
    );
    check!(
        buf.get(16..18) == Some(&EV_MSC.to_le_bytes()[..]),
        "first record is EV_MSC"
    );
    check!(
        buf.get(18..20) == Some(&MSC_SCAN.to_le_bytes()[..]),
        "...specifically MSC_SCAN"
    );
    check!(
        buf.get(20..24) == Some(&0x1E_i32.to_le_bytes()[..]),
        "MSC_SCAN carries the raw scancode, not the keycode"
    );
    check!(
        buf.get(40..42) == Some(&EV_KEY.to_le_bytes()[..]),
        "second record is EV_KEY"
    );
    check!(
        buf.get(42..44) == Some(&30_u16.to_le_bytes()[..]),
        "EV_KEY carries the keycode"
    );
    check!(
        buf.get(44..48) == Some(&1_i32.to_le_bytes()[..]),
        "EV_KEY value is 1 for a press"
    );
    check!(
        buf.get(64..66) == Some(&EV_SYN.to_le_bytes()[..]),
        "the group is terminated by EV_SYN"
    );
    check!(!c.readable(), "and the client is caught up again");

    // A release is the same group with value 0 — the thing the ASCII path
    // could never express, and the reason this device exists.
    push_key(InputDevice::Keyboard, 30, 0x1E, false);
    check!(c.read_into(&mut buf)? == 72, "a release is a group too");
    check!(
        buf.get(44..48) == Some(&0_i32.to_le_bytes()[..]),
        "EV_KEY value is 0 for a release"
    );

    // A buffer too small for one record is EINVAL, not a partial record.
    let mut tiny = [0u8; INPUT_EVENT_SIZE - 1];
    check!(
        c.read_into(&mut tiny).is_err(),
        "a sub-record buffer is refused"
    );

    // A buffer holding exactly two records returns two, never a third split
    // across the end.
    push_key(InputDevice::Keyboard, 31, 0x1F, true);
    let mut two = [0u8; INPUT_EVENT_SIZE * 2];
    check!(c.read_into(&mut two)? == 48, "exactly two whole records");
    check!(c.readable(), "the third record is still pending");
    check!(c.read_into(&mut buf)? == 24, "and arrives on the next read");

    // --- two clients each see everything -----------------------------------
    let mut a = EvdevClient::new(InputDevice::Keyboard);
    let mut b = EvdevClient::new(InputDevice::Keyboard);
    push_key(InputDevice::Keyboard, 32, 0x20, true);
    check!(a.read_into(&mut buf)? == 72, "client A sees the group");
    check!(
        b.read_into(&mut buf)? == 72,
        "client B sees it too, not a consumed-once queue"
    );

    // --- being lapped yields SYN_DROPPED, exactly once ---------------------
    let mut slow = EvdevClient::new(InputDevice::Keyboard);
    check!(slow.drop_count() == 0, "no drop before being lapped");
    // Three events per press, so this overruns the ring with room to spare.
    for _ in 0..(RING_CAP / 3 + 4) {
        push_key(InputDevice::Keyboard, 30, 0x1E, true);
    }
    let n = slow.read_into(&mut buf)?;
    check!(n >= 48, "a lapped client still reads events");
    check!(
        buf.get(16..18) == Some(&EV_SYN.to_le_bytes()[..]),
        "the first record after a lap is EV_SYN"
    );
    check!(
        buf.get(18..20) == Some(&SYN_DROPPED.to_le_bytes()[..]),
        "...specifically SYN_DROPPED"
    );
    check!(slow.drop_count() == 1, "the lap was counted once");
    // The next record is a real event: a client is told once per gap, not once
    // per lost event, or a burst would bury the stream in notifications.
    check!(
        buf.get(42..44) != Some(&SYN_DROPPED.to_le_bytes()[..]),
        "SYN_DROPPED is not repeated"
    );
    // Draining must terminate rather than resync forever.
    let mut guard = 0_u32;
    while slow.readable() && guard < 10_000 {
        if slow.read_into(&mut buf)? == 0 {
            break;
        }
        guard = guard.saturating_add(1);
    }
    check!(guard < 10_000, "a lapped client drains in bounded time");
    check!(!slow.readable(), "and ends caught up");

    // --- mouse packet grouping ---------------------------------------------
    let mut m = EvdevClient::new(InputDevice::Mouse);
    push_mouse_packet(0b001, 0b000, 5, 3, 0);
    check!(
        m.read_into(&mut buf)? == 96,
        "packet is REL_X + REL_Y + BTN_LEFT + SYN"
    );
    check!(
        buf.get(18..20) == Some(&REL_X.to_le_bytes()[..]),
        "REL_X comes first"
    );
    check!(
        buf.get(20..24) == Some(&5_i32.to_le_bytes()[..]),
        "REL_X carries dx unchanged"
    );
    check!(
        buf.get(44..48) == Some(&(-3_i32).to_le_bytes()[..]),
        "REL_Y is negated: PS/2 +up becomes evdev +down"
    );
    check!(
        buf.get(66..68) == Some(&BTN_LEFT.to_le_bytes()[..]),
        "the button is BTN_LEFT(0x110)"
    );
    check!(
        buf.get(88..90) == Some(&SYN_REPORT.to_le_bytes()[..]),
        "the packet is terminated by SYN_REPORT"
    );

    // A packet in which nothing moved and nothing changed is still one SYN —
    // its absence is what would leave the previous group looking unterminated.
    push_mouse_packet(0b001, 0b001, 0, 0, 0);
    check!(m.read_into(&mut buf)? == 24, "an idle packet is a bare SYN");
    check!(
        buf.get(16..18) == Some(&EV_SYN.to_le_bytes()[..]),
        "...and it really is a SYN"
    );

    // Only the buttons that actually changed are reported.
    push_mouse_packet(0b100, 0b001, 0, 0, 0);
    check!(
        m.read_into(&mut buf)? == 72,
        "left released + middle pressed + SYN, and nothing for right"
    );

    // Scroll maps to REL_WHEEL with its sign intact.
    push_mouse_packet(0b100, 0b100, 0, 0, -1);
    check!(m.read_into(&mut buf)? == 48, "wheel + SYN");
    check!(
        buf.get(18..20) == Some(&REL_WHEEL.to_le_bytes()[..]),
        "the axis is REL_WHEEL"
    );
    check!(
        buf.get(20..24) == Some(&(-1_i32).to_le_bytes()[..]),
        "the wheel sign is preserved"
    );

    // The keyboard and the mouse are separate streams: a mouse packet must not
    // make a keyboard client readable.
    let k = EvdevClient::new(InputDevice::Keyboard);
    push_mouse_packet(0b000, 0b100, 1, 1, 0);
    check!(!k.readable(), "a mouse packet does not wake a keyboard client");

    // --- device naming ------------------------------------------------------
    check!(
        InputDevice::from_minor(0) == Some(InputDevice::Keyboard),
        "event0 is the keyboard"
    );
    check!(
        InputDevice::from_minor(1) == Some(InputDevice::Mouse),
        "event1 is the mouse"
    );
    check!(
        InputDevice::from_minor(2).is_none(),
        "there is no event2 yet"
    );

    crate::serial_println!(
        "[evdev] Self-test PASSED (24-byte records, keycode tables, \
         per-client cursors, SYN_DROPPED on lap, packet grouping)"
    );
    Ok(())
}
