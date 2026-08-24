//! Every decision this module makes, driven by a scripted byte stream.
//!
//! The point of the three-way split is that this file needs no keyboard, no
//! kernel and no capability: [`FakeDevice`] hands back the exact bytes an evdev
//! device would, so a scroll direction inverted or a stuck Shift after a drop
//! fails here, on the machine the tree is written on, rather than on a machine
//! nobody can run a debugger on.

// A test that indexes out of range should fail loudly and point at the line
// that did it — that is the diagnosis. The defensive lints exist to keep panics
// out of code that runs on a user's data, which this is not.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::*;
use crate::keymap::key_for_scancode;
use guitk::event::Key;
use sys::{ENODEV, ENOENT, Errno};
use uapi::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, EV_KEY, EV_MSC, EV_REL, EV_SYN, MSC_SCAN};

// ---------------------------------------------------------------------------
// Linux keycodes used below, named so the assertions read as keys
// ---------------------------------------------------------------------------

/// `KEY_A`, whose set-1 scan code is `0x1E`.
const KEY_A: u16 = 30;
/// `KEY_B`, whose set-1 scan code is `0x30`.
const KEY_B: u16 = 48;
/// `KEY_LEFTSHIFT`, scan code `0x2A` — a modifier, so it must not repeat.
const KEY_LEFTSHIFT: u16 = 42;
/// `KEY_LEFTCTRL`, scan code `0x1D`.
const KEY_LEFTCTRL: u16 = 29;
/// `KEY_LEFT`, an extended key: scan code `0xE04B`, distinct from keypad 4.
const KEY_LEFT: u16 = 105;
/// A keycode above the PS/2 range, with no set-1 equivalent at all.
const KEY_UNMAPPABLE: u16 = 200;

/// Scan code of [`KEY_A`].
const SCAN_A: u32 = 0x1E;
/// Scan code of [`KEY_B`].
const SCAN_B: u32 = 0x30;
/// Scan code of [`KEY_LEFTSHIFT`].
const SCAN_LEFTSHIFT: u32 = 0x2A;
/// Scan code of [`KEY_LEFTCTRL`].
const SCAN_LEFTCTRL: u32 = 0x1D;
/// Scan code of [`KEY_LEFT`], prefix included.
const SCAN_LEFT: u32 = 0xE04B;

// ---------------------------------------------------------------------------
// The fake device
// ---------------------------------------------------------------------------

/// Everything a scripted device knows, shared with the test that scripted it.
#[derive(Debug)]
struct DeviceState {
    /// One entry per `read`, oldest first. An exhausted script answers `EAGAIN`,
    /// which is what an idle keyboard answers.
    reads: VecDeque<Vec<u8>>,
    /// What `EVIOCGKEY` reports.
    bitmap: [u8; KEY_BITMAP_BYTES],
    /// Whether `EVIOCGKEY` answers at all.
    bitmap_readable: bool,
    /// What `EVIOCGNAME` reports, or `None` to fail it.
    name: Option<&'static str>,
    /// Once set, every `read` fails with this errno for ever.
    fail_reads_with: Option<Errno>,
    /// How many `read`s have been issued, so a test can prove a dead device is
    /// left alone rather than retried once a frame.
    reads_issued: usize,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            reads: VecDeque::new(),
            bitmap: [0u8; KEY_BITMAP_BYTES],
            bitmap_readable: true,
            name: Some("scripted device"),
            fail_reads_with: None,
            reads_issued: 0,
        }
    }
}

/// A scripted `/dev/input/eventN`.
///
/// Cloning shares the state, so a test keeps a handle to the device it handed
/// to [`EvdevInput`] and can both feed it more bytes and see what was asked of
/// it.
#[derive(Clone, Debug)]
struct FakeDevice(Rc<RefCell<DeviceState>>);

impl FakeDevice {
    /// An idle device that answers `EAGAIN` to everything.
    fn new() -> Self {
        Self(Rc::new(RefCell::new(DeviceState::default())))
    }

    /// Queue a batch of records to be returned by one `read`.
    fn feed(&self, records: &[Record]) -> &Self {
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend_from_slice(&record.encode());
        }
        self.0.borrow_mut().reads.push_back(bytes);
        self
    }

    /// Queue raw bytes, so a read can be cut in the middle of a record.
    fn feed_bytes(&self, bytes: &[u8]) -> &Self {
        self.0.borrow_mut().reads.push_back(bytes.to_vec());
        self
    }

    /// Mark a keycode as held in the `EVIOCGKEY` bitmap.
    fn hold(&self, keycode: u16) -> &Self {
        let mut state = self.0.borrow_mut();
        let byte = usize::from(keycode) / 8;
        state.bitmap[byte] |= 1u8 << (keycode % 8);
        self
    }

    /// Make `EVIOCGKEY` fail, as a revoked or unplugged device would.
    fn without_key_state(&self) -> &Self {
        self.0.borrow_mut().bitmap_readable = false;
        self
    }

    /// Give the device a name, or `None` to make `EVIOCGNAME` fail.
    fn named(&self, name: Option<&'static str>) -> &Self {
        self.0.borrow_mut().name = name;
        self
    }

    /// Make every `read` from now on fail permanently.
    fn breaks(&self) -> &Self {
        self.0.borrow_mut().fail_reads_with = Some(ENODEV);
        self
    }

    /// How many `read`s have been issued to this device.
    fn reads_issued(&self) -> usize {
        self.0.borrow().reads_issued
    }
}

impl EventSys for FakeDevice {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        let mut state = self.0.borrow_mut();
        state.reads_issued += 1;
        if let Some(errno) = state.fail_reads_with {
            return Err(errno);
        }
        let Some(batch) = state.reads.pop_front() else {
            return Err(EAGAIN);
        };
        let len = batch.len().min(buf.len());
        buf[..len].copy_from_slice(&batch[..len]);
        Ok(len)
    }

    fn ioctl_read(&mut self, request: u32, buf: &mut [u8]) -> Result<usize, Errno> {
        let state = self.0.borrow();
        match request & 0xFF {
            uapi::EVIOC_NR_GKEY => {
                if !state.bitmap_readable {
                    return Err(ENODEV);
                }
                let len = state.bitmap.len().min(buf.len());
                buf[..len].copy_from_slice(&state.bitmap[..len]);
                Ok(len)
            }
            uapi::EVIOC_NR_GNAME => {
                let Some(name) = state.name else {
                    return Err(ENODEV);
                };
                let bytes = name.as_bytes();
                let len = bytes.len().min(buf.len().saturating_sub(1));
                buf[..len].copy_from_slice(&bytes[..len]);
                buf[len] = 0;
                Ok(len + 1)
            }
            _ => Err(ENODEV),
        }
    }
}

/// A set of `/dev/input/eventN` nodes, some of which may refuse to open.
#[derive(Debug, Default)]
struct FakeDevices {
    /// What each index answers, in the order [`EvdevInput::from_source`] asks.
    slots: Vec<(u32, Result<FakeDevice, Errno>)>,
}

impl FakeDevices {
    /// A set with one working device at index 0.
    fn one() -> (Self, FakeDevice) {
        let device = FakeDevice::new();
        let mut source = Self::default();
        source.slots.push((0, Ok(device.clone())));
        (source, device)
    }

    /// A set with working devices at indices 0 and 1.
    fn two() -> (Self, FakeDevice, FakeDevice) {
        let first = FakeDevice::new();
        let second = FakeDevice::new();
        let mut source = Self::default();
        source.slots.push((0, Ok(first.clone())));
        source.slots.push((1, Ok(second.clone())));
        (source, first, second)
    }

    /// Make index `index` answer `errno`.
    fn refusing(mut self, index: u32, errno: Errno) -> Self {
        self.slots.push((index, Err(errno)));
        self
    }

    /// Add a working device at `index`.
    ///
    /// [`Self::one`] and [`Self::two`] put their devices at 0 and 1, which is
    /// where the search starts — so a test built from them cannot tell
    /// "scans every index" from "looks at the first two", nor "skips a
    /// refusal" from "stops at the first refusal". Placing a device
    /// deliberately out of the way is what makes those distinctions visible.
    fn also(mut self, index: u32) -> (Self, FakeDevice) {
        let device = FakeDevice::new();
        self.slots.push((index, Ok(device.clone())));
        (self, device)
    }
}

impl DeviceSource for FakeDevices {
    type Sys = FakeDevice;

    fn open(&mut self, index: u32) -> Result<Self::Sys, Errno> {
        match self.slots.iter().find(|(i, _)| *i == index) {
            Some((_, Ok(device))) => Ok(device.clone()),
            Some((_, Err(errno))) => Err(*errno),
            None => Err(ENOENT),
        }
    }
}

// ---------------------------------------------------------------------------
// Record constructors, so a stream reads like the hardware describing itself
// ---------------------------------------------------------------------------

/// A key or button transition.
fn key(code: u16, value: i32) -> Record {
    Record::new(EV_KEY, code, value)
}

/// A relative axis movement.
fn rel(code: u16, value: i32) -> Record {
    Record::new(EV_REL, code, value)
}

/// The raw scan code that precedes a key event on a PS/2 device.
fn scan(code: i32) -> Record {
    Record::new(EV_MSC, MSC_SCAN, code)
}

/// End of packet.
fn syn() -> Record {
    Record::new(EV_SYN, SYN_REPORT, 0)
}

/// The kernel telling a lagging reader it was lapped.
fn dropped() -> Record {
    Record::new(EV_SYN, SYN_DROPPED, 0)
}

// ---------------------------------------------------------------------------
// Settings and assertions
// ---------------------------------------------------------------------------

/// Settings with acceleration off, so a delta reaches the pointer unchanged.
///
/// The default profile is `Adaptive`, which multiplies anything above four
/// counts — correct for a person and useless for an assertion about a number.
fn plain() -> InputSettings {
    let mut settings = InputSettings::default();
    settings.mouse.accel_profile = AccelProfile::Flat;
    settings
}

/// Build an input source over one scripted device on an 800x600 desktop.
fn one_device() -> (EvdevInput<FakeDevice>, FakeDevice) {
    let (mut source, device) = FakeDevices::one();
    let input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();
    (input, device)
}

/// The scan codes of every `KeyDown` in `events`, in order.
fn key_downs(events: &[InputEvent]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::KeyDown { scancode, .. } => Some(*scancode),
            _ => None,
        })
        .collect()
}

/// The scan codes of every `KeyUp` in `events`, in order.
fn key_ups(events: &[InputEvent]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::KeyUp { scancode } => Some(*scancode),
            _ => None,
        })
        .collect()
}

/// Every `MouseMove` in `events`, in order.
fn moves(events: &[InputEvent]) -> Vec<(i32, i32)> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::MouseMove { x, y } => Some((*x, *y)),
            _ => None,
        })
        .collect()
}

/// Every `MouseButton` in `events`, in order.
fn buttons(events: &[InputEvent]) -> Vec<(MouseButton, bool, i32, i32)> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::MouseButton {
                button,
                pressed,
                x,
                y,
            } => Some((*button, *pressed, *x, *y)),
            _ => None,
        })
        .collect()
}

/// Every `MouseScroll` in `events`, in order.
fn scrolls(events: &[InputEvent]) -> Vec<(f32, f32)> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::MouseScroll { dx, dy, .. } => Some((*dx, *dy)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

#[test]
fn a_keystroke_arrives_as_the_scancode_the_compositors_keymap_knows() {
    let (mut input, device) = one_device();
    device.feed(&[scan(0x1E), key(KEY_A, 1), syn()]);

    let events = input.poll_at(Instant::now());

    assert_eq!(key_downs(&events), vec![SCAN_A]);
    // Through the real table, not a number this test made up: the whole
    // translation exists so that this ends at the letter the user pressed.
    assert_eq!(key_for_scancode(SCAN_A), Key::A);
}

#[test]
fn an_extended_key_keeps_the_prefix_that_separates_it_from_the_keypad() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_LEFT, 1), syn()]);

    let events = input.poll_at(Instant::now());

    assert_eq!(key_downs(&events), vec![SCAN_LEFT]);
    assert_eq!(key_for_scancode(SCAN_LEFT), Key::Left);
    // The whole reason the prefix is carried: without it this is a different
    // key entirely — keypad 4, which the keymap does not name and which would
    // therefore reach a client as an unknown code instead of as an arrow.
    assert_eq!(key_for_scancode(0x4B), Key::Unknown(0x4B));
}

#[test]
fn a_release_is_reported_as_one_rather_than_swallowed() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn(), key(KEY_A, 0), syn()]);

    let events = input.poll_at(Instant::now());

    assert_eq!(key_downs(&events), vec![SCAN_A]);
    assert_eq!(key_ups(&events), vec![SCAN_A]);
}

#[test]
fn nothing_is_emitted_until_the_packet_is_terminated() {
    let (mut input, device) = one_device();
    // A key event with no SYN_REPORT after it: the packet is not finished, and
    // acting on half a packet is what makes a diagonal drag draw a staircase.
    device.feed(&[key(KEY_A, 1)]);
    let now = Instant::now();

    assert!(input.poll_at(now).is_empty());

    device.feed(&[syn()]);
    assert_eq!(key_downs(&input.poll_at(now)), vec![SCAN_A]);
}

#[test]
fn a_keycode_with_no_scan_code_falls_back_to_the_raw_one_the_device_sent() {
    let (mut input, device) = one_device();
    // A media key on a USB device: no set-1 equivalent, but the device said
    // what it sent, so the event reaches a client as an unknown code it could
    // be bound to rather than as silence.
    device.feed(&[scan(0x1234), key(KEY_UNMAPPABLE, 1), syn()]);

    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![0x1234]);
}

#[test]
fn a_keycode_with_no_scan_code_and_no_raw_one_is_dropped_rather_than_guessed() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_UNMAPPABLE, 1), syn()]);

    // A fabricated scan code is a keypress the user did not make.
    assert!(input.poll_at(Instant::now()).is_empty());
}

#[test]
fn the_keycode_table_wins_over_a_raw_code_that_disagrees_with_it() {
    let (mut input, device) = one_device();
    // What a USB keyboard does: MSC_SCAN carries the HID usage, not a set-1
    // code. Reading it would name a completely different key.
    device.feed(&[scan(0x0007_0004), key(KEY_A, 1), syn()]);

    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![SCAN_A]);
}

#[test]
fn a_raw_code_belongs_to_the_key_event_that_follows_it_and_not_the_next_one() {
    let (mut input, device) = one_device();
    device.feed(&[
        scan(0x1234),
        key(KEY_UNMAPPABLE, 1),
        key(KEY_UNMAPPABLE, 0),
        syn(),
    ]);

    let events = input.poll_at(Instant::now());

    // The press consumed the scan code; the release had none of its own, so it
    // is dropped rather than inheriting a stale one.
    assert_eq!(key_downs(&events), vec![0x1234]);
    assert!(key_ups(&events).is_empty());
}

#[test]
fn hardware_autorepeat_is_passed_through_as_a_press() {
    let (mut input, device) = one_device();
    // SlateOS never sends value 2, but a Linux host does, and a repeat
    // delivered as a second down without an up must not leave two keys held.
    device.feed(&[
        key(KEY_A, 1),
        syn(),
        key(KEY_A, 2),
        syn(),
        key(KEY_A, 0),
        syn(),
    ]);

    let events = input.poll_at(Instant::now());

    assert_eq!(key_downs(&events), vec![SCAN_A, SCAN_A]);
    assert_eq!(key_ups(&events), vec![SCAN_A]);
}

// ---------------------------------------------------------------------------
// Key repeat
// ---------------------------------------------------------------------------

#[test]
fn a_held_key_repeats_after_the_delay_and_then_at_the_interval() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();

    assert_eq!(key_downs(&input.poll_at(start)), vec![SCAN_A]);
    // Default delay is 500 ms: nothing before it.
    assert!(input.poll_at(start + Duration::from_millis(400)).is_empty());
    assert_eq!(
        key_downs(&input.poll_at(start + Duration::from_millis(500))),
        vec![SCAN_A]
    );
    // Default interval is 30 ms.
    assert!(input.poll_at(start + Duration::from_millis(520)).is_empty());
    assert_eq!(
        key_downs(&input.poll_at(start + Duration::from_millis(530))),
        vec![SCAN_A]
    );
}

#[test]
fn releasing_a_held_key_stops_it_repeating() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();
    input.poll_at(start);

    device.feed(&[key(KEY_A, 0), syn()]);
    let events = input.poll_at(start + Duration::from_millis(100));
    assert_eq!(key_ups(&events), vec![SCAN_A]);

    // Long past when the repeat would have been due.
    assert!(input.poll_at(start + Duration::from_secs(2)).is_empty());
}

#[test]
fn a_held_modifier_never_repeats() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_LEFTSHIFT, 1), syn()]);
    let start = Instant::now();

    assert_eq!(key_downs(&input.poll_at(start)), vec![SCAN_LEFTSHIFT]);
    // A Shift that repeated would deliver a stream of Shift presses to every
    // client for as long as it was held.
    assert!(input.poll_at(start + Duration::from_secs(5)).is_empty());
}

#[test]
fn the_key_that_repeats_is_the_one_pressed_last() {
    let (mut input, device) = one_device();
    let start = Instant::now();
    device.feed(&[key(KEY_A, 1), syn()]);
    input.poll_at(start);
    device.feed(&[key(KEY_B, 1), syn()]);
    input.poll_at(start + Duration::from_millis(100));

    // A real keyboard has one repeat timer, and it belongs to B now.
    assert_eq!(
        key_downs(&input.poll_at(start + Duration::from_millis(600))),
        vec![SCAN_B]
    );
}

#[test]
fn turning_repeat_off_in_the_settings_turns_it_off() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.keyboard.enabled = false;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();
    input.poll_at(start);

    assert!(input.poll_at(start + Duration::from_secs(5)).is_empty());
}

#[test]
fn the_users_own_delay_and_interval_are_the_ones_used() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.keyboard.set_delay(150);
    settings.keyboard.set_interval(10);
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();
    input.poll_at(start);

    // These settings have been written by the Settings panel and read by
    // nothing since they were added; this is the assertion that they mean
    // something.
    assert!(input.poll_at(start + Duration::from_millis(140)).is_empty());
    assert_eq!(
        key_downs(&input.poll_at(start + Duration::from_millis(150))),
        vec![SCAN_A]
    );
    assert_eq!(
        key_downs(&input.poll_at(start + Duration::from_millis(160))),
        vec![SCAN_A]
    );
}

#[test]
fn a_stall_does_not_pay_out_the_repeats_it_missed() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();
    input.poll_at(start);

    // Ten seconds at a 30 ms interval is over three hundred repeats' worth of
    // "should have happened". A user who held a key across a suspend should
    // not get a screenful of one letter when the machine wakes up.
    let events = input.poll_at(start + Duration::from_secs(10));
    assert_eq!(key_downs(&events).len(), MAX_REPEATS_PER_TICK);

    // And the backlog is discarded rather than carried: the next tick is
    // ordinary again.
    let next = input.poll_at(start + Duration::from_secs(10) + Duration::from_millis(30));
    assert_eq!(key_downs(&next).len(), 1);
}

#[test]
fn changing_the_settings_while_running_takes_effect_without_a_restart() {
    let (mut input, device) = one_device();
    let mut settings = plain();
    settings.keyboard.enabled = false;
    input.set_settings(settings);

    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();
    input.poll_at(start);
    assert!(input.poll_at(start + Duration::from_secs(2)).is_empty());
}

// ---------------------------------------------------------------------------
// The pointer
// ---------------------------------------------------------------------------

#[test]
fn the_pointer_starts_at_the_centre_rather_than_under_a_control() {
    let pointer = Pointer::new(800, 600);
    assert_eq!(pointer.position(), (400, 300));
}

#[test]
fn a_mouse_packet_moves_the_pointer_once_and_not_once_per_axis() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_X, 10), rel(REL_Y, -5), syn()]);

    let events = input.poll_at(Instant::now());

    // One movement of the hand is one movement of the pointer. Two would draw
    // a staircase and make every diagonal drag jitter.
    assert_eq!(moves(&events), vec![(410, 295)]);
}

#[test]
fn vertical_motion_is_positive_downwards_as_the_screen_counts() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_Y, 20), syn()]);

    assert_eq!(moves(&input.poll_at(Instant::now())), vec![(400, 320)]);
}

#[test]
fn the_pointer_cannot_be_pushed_off_the_desktop() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_X, 100_000), rel(REL_Y, 100_000), syn()]);

    // The far edge is width - 1: a pointer at 800 is on no pixel of any
    // monitor, so the last column could never be clicked.
    assert_eq!(moves(&input.poll_at(Instant::now())), vec![(799, 599)]);
}

#[test]
fn the_pointer_cannot_be_pushed_past_the_origin_either() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_X, -100_000), rel(REL_Y, -100_000), syn()]);

    assert_eq!(moves(&input.poll_at(Instant::now())), vec![(0, 0)]);
}

#[test]
fn a_slow_movement_at_the_lowest_speed_setting_still_moves_the_pointer() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.mouse.speed = -10;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    let now = Instant::now();

    // A quarter of a pixel per count. An integer position would round this to
    // zero every packet and the pointer would be immovable at the setting a
    // person with a very sensitive mouse would choose.
    for _ in 0..3 {
        device.feed(&[rel(REL_X, 1), syn()]);
        assert_eq!(moves(&input.poll_at(now)), vec![(400, 300)]);
    }
    device.feed(&[rel(REL_X, 1), syn()]);
    assert_eq!(moves(&input.poll_at(now)), vec![(401, 300)]);
}

#[test]
fn the_highest_speed_setting_multiplies_movement_by_four() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.mouse.speed = 10;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[rel(REL_X, 10), syn()]);

    assert_eq!(moves(&input.poll_at(Instant::now())), vec![(440, 300)]);
}

#[test]
fn speed_is_geometric_so_the_slider_feels_even_end_to_end() {
    // Each of the twenty steps is the same proportional change, which is what
    // stops all the slider's effect crowding into one end.
    assert_eq!(speed_multiplier(0), 1.0);
    assert!((speed_multiplier(-10) - 0.25).abs() < 1e-6);
    assert!((speed_multiplier(10) - 4.0).abs() < 1e-6);
    assert!((speed_multiplier(5) - 2.0).abs() < 1e-6);
    assert!((speed_multiplier(-5) - 0.5).abs() < 1e-6);
    // Out of range is clamped rather than producing an absurd multiplier.
    assert_eq!(speed_multiplier(1000), speed_multiplier(10));
    assert_eq!(speed_multiplier(-1000), speed_multiplier(-10));
}

#[test]
fn a_flat_profile_delivers_the_movement_the_hand_made() {
    let config = MouseConfig {
        accel_profile: AccelProfile::Flat,
        ..MouseConfig::default()
    };
    let (dx, dy) = accelerate(30.0, 40.0, &config);
    assert_eq!((dx, dy), (30.0, 40.0));
}

#[test]
fn careful_movement_below_the_threshold_is_never_accelerated() {
    let config = MouseConfig {
        accel_profile: AccelProfile::Adaptive,
        accel_threshold: 10,
        ..MouseConfig::default()
    };
    // The kind of movement used to hit a small target. Multiplying it would
    // make small targets unhittable, which is the whole point of a threshold.
    let (dx, dy) = accelerate(3.0, 0.0, &config);
    assert_eq!((dx, dy), (3.0, 0.0));
}

#[test]
fn fast_movement_above_the_threshold_is_accelerated() {
    let config = MouseConfig {
        accel_profile: AccelProfile::Adaptive,
        accel_threshold: 4,
        ..MouseConfig::default()
    };
    let (dx, _) = accelerate(20.0, 0.0, &config);
    // over = 20/4 - 1 = 4; factor = 1*4 + 1 = 5, clamped to MAX_ACCELERATION.
    assert_eq!(dx, 20.0 * MAX_ACCELERATION);
}

#[test]
fn an_absurd_acceleration_gain_is_clamped_rather_than_obeyed() {
    let config = MouseConfig {
        accel_profile: AccelProfile::Custom,
        accel_gain: 1000.0,
        accel_threshold: 1,
        ..MouseConfig::default()
    };
    let (dx, _) = accelerate(10.0, 0.0, &config);
    // `accel_gain` is user-editable and nothing stops someone typing 1000. The
    // clamp keeps a badly-chosen setting fast rather than unusable.
    assert_eq!(dx, 10.0 * MAX_ACCELERATION);
}

#[test]
fn a_zero_acceleration_threshold_does_not_divide_by_zero() {
    let config = MouseConfig {
        accel_profile: AccelProfile::Custom,
        accel_gain: 1.0,
        accel_threshold: 0,
        ..MouseConfig::default()
    };
    // Small enough that the *guarded* curve does not itself reach
    // `MAX_ACCELERATION`. At a magnitude of 5 it does, and then the guarded and
    // the divide-by-zero answers are the same number and neither assertion
    // below can tell them apart.
    let (dx, dy) = accelerate(2.0, 0.0, &config);
    assert!(dx.is_finite() && dy.is_finite(), "got {dx}, {dy}");

    // Finiteness alone does not prove the guard, which is how this test used to
    // pass with the guard deleted: a threshold of zero divides to an *infinite*
    // factor, which the `MAX_ACCELERATION` clamp two lines later turns back
    // into a finite — but maximal — one. Every movement, however careful, would
    // be multiplied by four and the pointer would be unusable, while the
    // assertion above stayed green.
    //
    // What the guard actually promises is that a threshold of zero behaves as a
    // threshold of one, so that is what is asserted.
    let as_one = MouseConfig {
        accel_threshold: 1,
        ..config.clone()
    };
    assert_eq!(
        (dx, dy),
        accelerate(2.0, 0.0, &as_one),
        "a zero threshold must behave as a threshold of one, not saturate"
    );
}

#[test]
fn a_click_after_the_desktop_shrinks_lands_inside_it_without_moving_first() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_X, 100_000), rel(REL_Y, 100_000), syn()]);
    assert_eq!(moves(&input.poll_at(Instant::now())), vec![(799, 599)]);

    // A monitor unplugged, and then a click with *no movement at all*.
    //
    // Every other test here observes the pointer only after feeding an
    // `EV_REL`, and `Pointer::nudge` clamps on its own — so `set_bounds`'s own
    // clamp is invisible to all of them, and deleting it leaves the suite
    // green. A click is the one thing that reads the position without first
    // moving it, which makes this the only test that can see the difference.
    input.set_bounds(640, 480);
    device.feed(&[key(BTN_LEFT, 1), syn()]);

    assert_eq!(
        buttons(&input.poll_at(Instant::now())),
        vec![(MouseButton::Left, true, 639, 479)],
        "the click was delivered at a coordinate the desktop no longer has"
    );
}

#[test]
fn shrinking_the_desktop_brings_the_pointer_back_inside_it() {
    // Asserted on the `Pointer` itself, and *immediately*, because that is what
    // the name claims. Driving this through `EvdevInput` would mean feeding an
    // `EV_REL` to observe the result, and `nudge` clamps on its own — so the
    // pointer would end up inside the new bounds whether `set_bounds` clamped
    // or not, and the test would pass with the clamp deleted. This version
    // reads the position with nothing in between.
    let mut pointer = Pointer::new(800, 600);
    pointer.nudge(100_000, 100_000, &MouseConfig::default());
    assert_eq!(pointer.position(), (799, 599));

    // A monitor being unplugged. A pointer left at its old position would be
    // off-screen and unreachable, because every subsequent movement clamps back
    // to a coordinate that is not displayed.
    pointer.set_bounds(640, 480);
    assert_eq!(
        pointer.position(),
        (639, 479),
        "the resize itself must move the pointer, not the next thing that does"
    );
}

#[test]
fn a_zero_sized_desktop_pins_the_pointer_at_the_origin() {
    let mut pointer = Pointer::new(0, 0);
    assert_eq!(pointer.position(), (0, 0));
    pointer.nudge(50, 50, &MouseConfig::default());
    assert_eq!(pointer.position(), (0, 0));
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

#[test]
fn a_click_is_delivered_at_the_position_the_movement_in_its_packet_reached() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_X, 50), key(BTN_LEFT, 1), syn()]);

    let events = input.poll_at(Instant::now());

    // A fast click on a small target arrives in the same packet as the movement
    // that got there. Emitting the button first would click whatever used to be
    // under the pointer.
    assert_eq!(moves(&events), vec![(450, 300)]);
    assert_eq!(buttons(&events), vec![(MouseButton::Left, true, 450, 300)]);
}

#[test]
fn every_button_the_compositor_knows_is_recognised() {
    let (mut input, device) = one_device();
    device.feed(&[
        key(BTN_LEFT, 1),
        key(BTN_RIGHT, 1),
        key(BTN_MIDDLE, 1),
        key(uapi::BTN_SIDE, 1),
        key(uapi::BTN_EXTRA, 1),
        syn(),
    ]);

    let pressed: Vec<MouseButton> = buttons(&input.poll_at(Instant::now()))
        .into_iter()
        .map(|(b, _, _, _)| b)
        .collect();
    assert_eq!(
        pressed,
        vec![
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
        ]
    );
}

#[test]
fn a_left_handed_mapping_swaps_the_primary_and_secondary_buttons() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.mouse.button_mapping = ButtonMapping::LeftHanded;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[key(BTN_LEFT, 1), key(BTN_RIGHT, 1), syn()]);

    let pressed: Vec<MouseButton> = buttons(&input.poll_at(Instant::now()))
        .into_iter()
        .map(|(b, _, _, _)| b)
        .collect();
    assert_eq!(pressed, vec![MouseButton::Right, MouseButton::Left]);
}

#[test]
fn a_left_handed_mapping_leaves_the_thumb_buttons_where_they_are() {
    // "Back" and "forward" are named by what they do, not by where they are, so
    // swapping them would make the setting mean something it does not say.
    assert_eq!(
        button_for(uapi::BTN_SIDE, ButtonMapping::LeftHanded),
        Some(MouseButton::Back)
    );
    assert_eq!(
        button_for(uapi::BTN_EXTRA, ButtonMapping::LeftHanded),
        Some(MouseButton::Forward)
    );
    assert_eq!(
        button_for(BTN_MIDDLE, ButtonMapping::LeftHanded),
        Some(MouseButton::Middle)
    );
}

#[test]
fn a_button_the_compositor_has_no_name_for_is_dropped_rather_than_guessed() {
    assert_eq!(button_for(0x11F, ButtonMapping::RightHanded), None);
}

#[test]
fn a_button_release_is_reported_at_the_pointers_position() {
    let (mut input, device) = one_device();
    device.feed(&[key(BTN_LEFT, 1), syn(), key(BTN_LEFT, 0), syn()]);

    assert_eq!(
        buttons(&input.poll_at(Instant::now())),
        vec![
            (MouseButton::Left, true, 400, 300),
            (MouseButton::Left, false, 400, 300),
        ]
    );
}

// ---------------------------------------------------------------------------
// Scroll
// ---------------------------------------------------------------------------

#[test]
fn a_wheel_notch_becomes_one_scroll_event() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_WHEEL, 1), syn()]);

    assert_eq!(scrolls(&input.poll_at(Instant::now())), vec![(0.0, 1.0)]);
}

#[test]
fn horizontal_and_vertical_scroll_in_one_packet_are_one_event() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_WHEEL, 2), rel(REL_HWHEEL, -1), syn()]);

    assert_eq!(scrolls(&input.poll_at(Instant::now())), vec![(-1.0, 2.0)]);
}

#[test]
fn natural_scroll_reverses_the_direction() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.mouse.natural_scroll = true;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[rel(REL_WHEEL, 1), syn()]);

    assert_eq!(scrolls(&input.poll_at(Instant::now())), vec![(-0.0, -1.0)]);
}

#[test]
fn the_users_scroll_speed_scales_the_notch() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.mouse.scroll_speed = 2.5;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[rel(REL_WHEEL, 2), syn()]);

    assert_eq!(scrolls(&input.poll_at(Instant::now())), vec![(0.0, 5.0)]);
}

#[test]
fn a_nonsense_scroll_speed_does_not_produce_a_nonsense_scroll() {
    let (mut source, device) = FakeDevices::one();
    let mut settings = plain();
    settings.mouse.scroll_speed = f32::NAN;
    let mut input = EvdevInput::from_source(&mut source, settings, 800, 600).unwrap();
    device.feed(&[rel(REL_WHEEL, 1), syn()]);

    // The file is user-editable and a NaN reaching a client would poison every
    // scroll offset it was added to.
    assert_eq!(scrolls(&input.poll_at(Instant::now())), vec![(0.0, 1.0)]);
}

#[test]
fn scrolling_does_not_move_the_pointer() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_WHEEL, 3), syn()]);

    let events = input.poll_at(Instant::now());
    assert!(moves(&events).is_empty());
}

// ---------------------------------------------------------------------------
// SYN_DROPPED and re-synchronisation
// ---------------------------------------------------------------------------

#[test]
fn a_drop_releases_a_key_the_device_is_no_longer_holding() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_LEFTSHIFT, 1), syn()]);
    let now = Instant::now();
    input.poll_at(now);

    // The bitmap is empty: the Shift went up during the events that were lost.
    device.feed(&[dropped()]);
    let events = input.poll_at(now);

    // Without this the Shift is stuck down for ever and every subsequent letter
    // is a capital.
    assert_eq!(key_ups(&events), vec![SCAN_LEFTSHIFT]);
}

#[test]
fn a_drop_presses_a_key_the_device_is_holding_that_we_never_saw_go_down() {
    let (mut input, device) = one_device();
    device.hold(KEY_LEFTCTRL);
    device.feed(&[dropped()]);

    let events = input.poll_at(Instant::now());

    // A Ctrl held across a drop would otherwise be missing from the modifier
    // state and every shortcut would stop working.
    assert_eq!(key_downs(&events), vec![SCAN_LEFTCTRL]);
}

#[test]
fn a_drop_leaves_a_key_that_is_still_held_alone() {
    let (mut input, device) = one_device();
    device.hold(KEY_LEFTSHIFT);
    device.feed(&[key(KEY_LEFTSHIFT, 1), syn()]);
    let now = Instant::now();
    input.poll_at(now);

    device.feed(&[dropped()]);
    let events = input.poll_at(now);

    // Neither released nor pressed again: a Shift that was re-pressed would
    // deliver a spurious keystroke to whatever had focus.
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn a_drop_whose_key_state_cannot_be_read_releases_what_that_device_held() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_LEFTSHIFT, 1), syn()]);
    let now = Instant::now();
    input.poll_at(now);

    device.without_key_state();
    device.feed(&[dropped()]);

    // A key reported up that is really down recovers the moment it is pressed
    // again; one reported down that is really up never recovers at all.
    assert_eq!(key_ups(&input.poll_at(now)), vec![SCAN_LEFTSHIFT]);
}

#[test]
fn a_drop_on_one_device_does_not_release_keys_held_on_another() {
    let (mut source, keyboard, mouse) = FakeDevices::two();
    let mut input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();
    keyboard.hold(KEY_LEFTSHIFT);
    keyboard.feed(&[key(KEY_LEFTSHIFT, 1), syn()]);
    let now = Instant::now();
    input.poll_at(now);

    // The mouse was lapped. It knows nothing about the keyboard's keys.
    mouse.feed(&[dropped()]);
    assert!(input.poll_at(now).is_empty());
}

#[test]
fn a_drop_discards_the_half_built_packet_and_nothing_after_it() {
    let (mut input, device) = one_device();
    device.feed(&[rel(REL_X, 50), dropped(), rel(REL_X, 10), syn()]);

    let events = input.poll_at(Instant::now());

    // The 50 was part of a packet whose siblings were never delivered; the 10
    // arrived after the drop and is coherent.
    assert_eq!(moves(&events), vec![(410, 300)]);
}

#[test]
fn a_drop_does_not_synthesise_a_button_press_from_the_key_state() {
    let (mut input, device) = one_device();
    device.hold(BTN_LEFT);
    device.feed(&[dropped()]);

    // `EVIOCGKEY` reports buttons too, and pressing one here would deliver a
    // click nobody made. Buttons are re-derived from the next real packet.
    assert!(buttons(&input.poll_at(Instant::now())).is_empty());
}

#[test]
fn a_drop_stops_the_repeat_of_a_key_it_released() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn()]);
    let start = Instant::now();
    input.poll_at(start);

    device.feed(&[dropped()]);
    input.poll_at(start + Duration::from_millis(100));

    assert!(input.poll_at(start + Duration::from_secs(2)).is_empty());
}

#[test]
fn the_key_state_is_re_read_only_after_the_events_in_the_same_read() {
    let (mut input, device) = one_device();
    // A drop followed by the real press of the very key the bitmap reports. If
    // resync ran before the press was folded in, the key would be pressed twice.
    device.hold(KEY_A);
    device.feed(&[dropped(), key(KEY_A, 1), syn()]);

    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![SCAN_A]);
}

#[test]
fn a_correction_lands_in_the_same_poll_as_the_events_it_corrects() {
    let (mut input, device) = one_device();
    // The other direction of the same rule, and the one that shows *when* the
    // re-read happens rather than merely that it happens once. The bitmap holds
    // nothing; the stream after the drop says KEY_A went down. Both facts
    // arrive in a single read, so a single poll owes the caller both the press
    // and the release the bitmap implies. Resyncing before the drain would emit
    // the press alone and leave the key stuck down until the next tick — which
    // `the_key_state_is_re_read_only_after_the_events_in_the_same_read` cannot
    // see, because there the bitmap and the stream agree.
    device.feed(&[dropped(), key(KEY_A, 1), syn()]);

    let events = input.poll_at(Instant::now());
    assert_eq!(key_downs(&events), vec![SCAN_A]);
    assert_eq!(
        key_ups(&events),
        vec![SCAN_A],
        "the re-read must reconcile the events from the same read, not the next one"
    );
}

// ---------------------------------------------------------------------------
// Reading, buffering and failure
// ---------------------------------------------------------------------------

#[test]
fn a_read_cut_in_the_middle_of_a_record_is_reassembled() {
    let (mut input, device) = one_device();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&key(KEY_A, 1).encode());
    bytes.extend_from_slice(&syn().encode());
    // The SlateOS kernel never does this, but this module is not entitled to
    // assume it of every kernel it might run on.
    device.feed_bytes(&bytes[..12]);
    device.feed_bytes(&bytes[12..]);
    let now = Instant::now();

    assert!(input.poll_at(now).is_empty());
    assert_eq!(key_downs(&input.poll_at(now)), vec![SCAN_A]);
}

#[test]
fn an_idle_device_produces_nothing_and_is_not_a_fault() {
    let (mut input, device) = one_device();
    assert!(input.poll_at(Instant::now()).is_empty());
    assert!(input.poll_at(Instant::now()).is_empty());

    // And it still works afterwards. Producing nothing is what a quiet device
    // and a dead one have in common, so the two assertions above pass whether
    // `EAGAIN` is treated as "no news" or as a fault that retires the device —
    // which is the whole difference between a keyboard that is idle between
    // keystrokes and one that stops working after the first pause in typing.
    device.feed(&[key(KEY_A, 1), syn()]);
    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![SCAN_A]);
}

#[test]
fn a_device_that_fails_permanently_goes_quiet_rather_than_erroring_every_frame() {
    let (mut input, device) = one_device();
    device.breaks();
    let now = Instant::now();

    input.poll_at(now);
    let after_first = device.reads_issued();
    for _ in 0..10 {
        input.poll_at(now);
    }

    // Unplugged, or the fd revoked. Issuing a failing syscall once a frame for
    // the rest of the session is how a lost device becomes a lost desktop.
    assert_eq!(device.reads_issued(), after_first);
}

#[test]
fn one_device_failing_does_not_silence_another() {
    let (mut source, broken, working) = FakeDevices::two();
    let mut input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();
    broken.breaks();
    working.feed(&[key(KEY_A, 1), syn()]);

    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![SCAN_A]);
}

#[test]
fn a_short_read_ends_the_tick_rather_than_asking_again_for_nothing() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn()]);
    let now = Instant::now();

    input.poll_at(now);

    // A read that came back shorter than the buffer means the device had
    // nothing more to give, which is every ordinary keystroke. Asking again
    // would be a syscall whose answer is known.
    assert_eq!(device.reads_issued(), 1);
}

#[test]
fn a_burst_larger_than_one_tick_can_take_is_left_for_the_next_one() {
    let (mut input, device) = one_device();
    // Full buffers, which is the only thing that means "there may be more":
    // thirty-two records is exactly READ_CHUNK, so the drain loop keeps going
    // until it hits its own bound rather than until the device runs dry.
    let mut chunk = Vec::new();
    for _ in 0..8 {
        chunk.extend_from_slice(&[key(KEY_A, 1), syn(), key(KEY_A, 0), syn()]);
    }
    assert_eq!(chunk.len() * EVENT_SIZE, READ_CHUNK);
    for _ in 0..(MAX_READS_PER_TICK + 2) {
        device.feed(&chunk);
    }
    let now = Instant::now();

    // Bounded, so a device producing events faster than they are consumed
    // cannot hold the compositing loop: the desktop would stop drawing while
    // remaining perfectly responsive to input, which is the worst of both.
    let first = input.poll_at(now);
    assert_eq!(key_downs(&first).len(), MAX_READS_PER_TICK * 8);
    // And nothing was dropped — the rest arrives on the next tick.
    let second = input.poll_at(now);
    assert_eq!(key_downs(&second).len(), 2 * 8);
}

// ---------------------------------------------------------------------------
// Opening devices
// ---------------------------------------------------------------------------

#[test]
fn every_device_that_opens_is_read_not_just_the_first_two() {
    // Three devices, and the third is deliberately at index 7 with a gap in
    // front of it. Two devices at 0 and 1 cannot test the claim this test's
    // name makes: `0..2` finds both of them, so the search bound could be two
    // rather than `MAX_DEVICES` and nothing here would notice. A device out
    // past the gap is what distinguishes scanning from assuming.
    let (source, first, second) = FakeDevices::two();
    let (mut source, third) = source.also(7);
    let mut input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();
    // No index is hardcoded and no device is assumed to be one thing or the
    // other: a keyboard with a trackpoint reports both from the same node.
    first.feed(&[key(KEY_A, 1), syn()]);
    second.feed(&[rel(REL_X, 10), syn()]);
    third.feed(&[key(KEY_B, 1), syn()]);

    let events = input.poll_at(Instant::now());
    assert_eq!(key_downs(&events), vec![SCAN_A, SCAN_B]);
    assert_eq!(moves(&events), vec![(410, 300)]);
}

#[test]
fn a_keyboard_and_a_mouse_on_the_same_node_are_told_apart_by_the_record() {
    let (mut input, device) = one_device();
    // `EV_KEY` below BTN_MISC is a key; at or above it, a button. That is what
    // the ABI guarantees, and it needs no per-device classification at all.
    device.feed(&[key(KEY_A, 1), key(BTN_LEFT, 1), rel(REL_X, 5), syn()]);

    let events = input.poll_at(Instant::now());
    assert_eq!(key_downs(&events), vec![SCAN_A]);
    assert_eq!(buttons(&events).len(), 1);
    assert_eq!(moves(&events), vec![(405, 300)]);
}

#[test]
fn a_permission_failure_is_reported_as_the_capability_it_needs() {
    let mut source = FakeDevices::default()
        .refusing(0, EACCES)
        .refusing(1, EACCES);

    let error = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap_err();

    // Not "no devices": the fix is a capability grant in the compositor's
    // ancestor, and no amount of retrying or path-guessing here produces one.
    assert_eq!(error, EvdevError::Denied);
    assert!(
        error.to_string().contains("InputDevice capability"),
        "{error}"
    );
}

#[test]
fn a_machine_with_no_input_devices_is_not_reported_as_a_permission_problem() {
    let mut source = FakeDevices::default();

    let error = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap_err();

    assert_eq!(error, EvdevError::NoDevices);
    assert!(!error.to_string().contains("capability"), "{error}");
}

#[test]
fn a_device_that_opens_alongside_a_refused_one_is_still_used() {
    // The refusal comes *first*, at the index the search starts from, and the
    // usable device sits behind it. With the working device at 0 the search
    // has already found what it needs before it meets the refusal, so it
    // cannot tell "skip a refused index" from "stop at the first one" — and
    // stopping is the behaviour that leaves a grantable keyboard unopened.
    let (source, device) = FakeDevices::default().also(1);
    let mut source = source.refusing(0, EACCES);
    let mut input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();
    device.feed(&[key(KEY_A, 1), syn()]);

    // A machine where one node is grantable and another is not still types.
    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![SCAN_A]);
}

#[test]
fn the_devices_that_opened_are_named_for_the_startup_diagnostic() {
    let (mut source, first, second) = FakeDevices::two();
    first.named(Some("AT Translated Set 2 keyboard"));
    second.named(Some("PS/2 Generic Mouse"));
    let input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();

    assert_eq!(
        input.devices(),
        vec![
            (0, "AT Translated Set 2 keyboard"),
            (1, "PS/2 Generic Mouse"),
        ]
    );
}

#[test]
fn a_device_that_will_not_name_itself_is_still_used() {
    let (mut source, device) = FakeDevices::one();
    device.named(None);
    let mut input = EvdevInput::from_source(&mut source, plain(), 800, 600).unwrap();
    device.feed(&[key(KEY_A, 1), syn()]);

    // The name is for a person reading a log. A device that will not say is not
    // a device that will not work.
    assert_eq!(input.devices(), vec![(0, "unnamed device")]);
    assert_eq!(key_downs(&input.poll_at(Instant::now())), vec![SCAN_A]);
}

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

#[test]
fn polling_through_the_trait_reads_the_same_devices() {
    let (mut input, device) = one_device();
    device.feed(&[key(KEY_A, 1), syn()]);

    let events = InputSource::poll(&mut input);
    assert_eq!(key_downs(&events), vec![SCAN_A]);
}

#[test]
fn setting_bounds_through_the_trait_reaches_the_pointer() {
    let (mut input, device) = one_device();
    InputSource::set_bounds(&mut input, 100, 100);
    device.feed(&[rel(REL_X, 10_000), syn()]);

    // The pointer was at the centre of 800x600 and is now inside 100x100, at
    // its far corner: shrinking the desktop moved it, and it cannot be pushed
    // back out.
    assert_eq!(moves(&input.poll_at(Instant::now())), vec![(99, 99)]);
}

// ---------------------------------------------------------------------------
// Which keys repeat
// ---------------------------------------------------------------------------

#[test]
fn no_modifier_or_latch_is_classified_as_repeating() {
    // Derived from the key's name rather than a second list of scan codes, so a
    // key added to the keymap is classified by the table that already knows
    // what it is.
    for scancode in [
        0x2A, 0x36, 0x1D, 0xE01D, 0x38, 0xE038, 0xE05B, 0xE05C, 0x3A, 0x45, 0x46,
    ] {
        assert!(
            !repeats(scancode),
            "{:?} ({scancode:#06X}) must not repeat",
            key_for_scancode(scancode)
        );
    }
}

#[test]
fn an_ordinary_key_is_classified_as_repeating() {
    for scancode in [SCAN_A, SCAN_B, 0x39, 0x0E, 0x1C, SCAN_LEFT] {
        assert!(
            repeats(scancode),
            "{:?} ({scancode:#06X}) must repeat",
            key_for_scancode(scancode)
        );
    }
}
