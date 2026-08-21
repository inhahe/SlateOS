//! A DRM card that exists only in memory, and the tests it makes possible.
//!
//! The point of [`FakeCard`] is not that it is convenient — it is that the
//! decisions in [`super`] are the only place a scanout bug can live, and none
//! of them need silicon. So the fake is deliberately *strict* rather than
//! permissive: it re-checks every rule the real kernel enforces (payload sizes
//! derived from the request number, `ADDFB2`'s load-bearing zeroes, a page flip
//! naming a CRTC and a framebuffer that both exist) and fails the same way the
//! kernel would. A fake that accepted anything would let the protocol drift
//! and report green.
//!
//! It models a machine chosen to catch the mistakes that hide on simpler
//! hardware: **two** CRTCs, so a `possible_crtcs` bitmask read as an id instead
//! of an index gives a different answer; a disconnected head listed before the
//! connected one, so a caller that takes `connectors[0]` fails; and a
//! 64-byte-aligned pitch on a width that is not a multiple of 16, so a row copy
//! that recomputes `width * 4` lands in the wrong place from the second row on.

// A test that indexes out of range should fail loudly and point at the line
// that did it — that is the diagnosis. The defensive lints exist to keep panics
// out of code that runs on a user's data, which this is not.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::cell::RefCell;
use std::rc::Rc;

use super::sys::{
    CardPath, CardSource, EBUSY, ENODEV, ENOENT, Errno, KmsSys, MAX_CARDS, Mapped, OutArray,
};
use super::uapi::{
    self, ModeCardRes, ModeCreateDumb, ModeCrtcPageFlip, ModeDestroyDumb, ModeFbCmd2,
    ModeGetConnector, ModeGetEncoder, ModeMapDumb, ModeModeinfo,
};
use super::{DrmScanout, Present, ScanoutError, blit, open_display};

/// Invalid argument, which is what the kernel says to a malformed request.
const EINVAL: Errno = 22;

// ---------------------------------------------------------------- the fake --

/// One connector of a [`FakeCard`].
#[derive(Clone, Debug)]
struct FakeConnector {
    /// Its object id.
    id: u32,
    /// [`uapi::CONNECTED`], or anything else for unplugged.
    connection: u32,
    /// The encoder it is currently routed through, or 0 for none.
    current_encoder: u32,
    /// The encoders that could drive it.
    encoders: Vec<u32>,
    /// The modes it offers, best first as DRM orders them.
    modes: Vec<ModeModeinfo>,
}

/// One encoder of a [`FakeCard`].
#[derive(Clone, Copy, Debug)]
struct FakeEncoder {
    /// Its object id.
    id: u32,
    /// The CRTC it is currently driving, or 0.
    crtc_id: u32,
    /// Bitmask over *indices* into the CRTC id array.
    possible_crtcs: u32,
}

/// A dumb buffer the fake handed out.
#[derive(Clone, Copy, Debug)]
struct FakeDumb {
    /// Its GEM handle.
    handle: u32,
    /// The fake mmap offset that maps it.
    offset: u64,
    /// Its total size in bytes.
    size: u64,
    /// Its pitch.
    pitch: u32,
}

/// Everything the fake card knows, shared between the test and the scanout.
#[derive(Debug, Default)]
struct CardState {
    /// CRTC ids, in the order `GETRESOURCES` reports them.
    crtcs: Vec<u32>,
    /// Connectors, in the order `GETRESOURCES` reports them.
    connectors: Vec<FakeConnector>,
    /// Encoders.
    encoders: Vec<FakeEncoder>,
    /// Pitch alignment in bytes. Real dumb buffers use 64.
    pitch_align: u32,
    /// Next GEM handle to hand out.
    next_handle: u32,
    /// Next framebuffer id to hand out.
    next_fb: u32,
    /// Live dumb buffers.
    dumb: Vec<FakeDumb>,
    /// Live framebuffers, as `(fb_id, handle, pitch, width, height)`.
    fbs: Vec<(u32, u32, u32, u32, u32)>,
    /// Every page flip, as `(crtc_id, fb_id)`.
    flips: Vec<(u32, u32)>,
    /// Handles passed to `DESTROY_DUMB`.
    destroyed: Vec<u32>,
    /// Ids passed to `RMFB`.
    removed: Vec<u32>,
    /// Every ioctl request number, in order.
    log: Vec<u32>,
    /// Failures to inject, consumed in order: the next call whose request
    /// matches the front entry fails with its errno.
    fail: Vec<(u32, Errno)>,
    /// If set, every `count_*` the card reports is this instead of the truth —
    /// a driver that lies about how much there is to enumerate.
    lie_counts: Option<u32>,
    /// Subtract this from the `size` `CREATE_DUMB` reports, to model a driver
    /// that returns a buffer too small for its own pitch.
    shrink_size: u64,
}

/// A DRM card that exists only in memory.
///
/// Cloning shares the state, so a test can hold one handle while the scanout
/// owns another and read what happened without an accessor on the type under
/// test.
#[derive(Clone, Debug, Default)]
struct FakeCard {
    /// The shared state.
    state: Rc<RefCell<CardState>>,
}

/// A mapping backed by a plain `Vec`.
#[derive(Debug)]
struct FakeMapping {
    /// The bytes.
    data: Vec<u8>,
}

impl Mapped for FakeMapping {
    fn bytes(&mut self) -> &mut [u8] {
        &mut self.data
    }

    fn bytes_len(&self) -> usize {
        self.data.len()
    }
}

/// The size an ioctl request number declares its payload to be.
fn declared_size(request: u32) -> usize {
    ((request >> 16) & 0x3FFF) as usize
}

/// Copy a fixed-size payload out of a slice.
fn fixed<const N: usize>(p: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    for (d, s) in out.iter_mut().zip(p.iter()) {
        *d = *s;
    }
    out
}

/// Write an encoded payload back over the caller's buffer.
fn put(p: &mut [u8], bytes: &[u8]) {
    for (d, s) in p.iter_mut().zip(bytes.iter()) {
        *d = *s;
    }
}

/// The out-of-line buffer whose pointer belongs at `ptr_at`, if the caller
/// offered one.
fn array_at<'a>(arrays: &'a mut [OutArray<'_>], ptr_at: usize) -> Option<&'a mut [u8]> {
    arrays
        .iter_mut()
        .find(|a| a.ptr_at == ptr_at)
        .map(|a| &mut *a.buf)
}

/// Fill an out-of-line `u32` array, copying only what fits.
fn fill_u32s(buf: &mut [u8], values: &[u32]) {
    for (slot, &v) in buf.chunks_exact_mut(4).zip(values.iter()) {
        slot.copy_from_slice(&v.to_le_bytes());
    }
}

impl FakeCard {
    /// A one-monitor machine: two CRTCs, a dark head listed first, and a
    /// 1366x768 panel whose native mode is the second one it offers.
    fn desktop() -> Self {
        Self::with(CardState {
            crtcs: vec![1, 2],
            connectors: vec![
                FakeConnector {
                    id: 30,
                    connection: 2, // disconnected
                    current_encoder: 0,
                    encoders: vec![50],
                    modes: Vec::new(),
                },
                FakeConnector {
                    id: 31,
                    connection: uapi::CONNECTED,
                    current_encoder: 0,
                    encoders: vec![51],
                    // Not preferred first, so "take modes[0]" gives a
                    // different answer from "take the preferred one".
                    modes: vec![mode(1024, 768, false), mode(1366, 768, true)],
                },
            ],
            encoders: vec![
                FakeEncoder {
                    id: 50,
                    crtc_id: 0,
                    possible_crtcs: 0b01,
                },
                FakeEncoder {
                    id: 51,
                    crtc_id: 0,
                    // Only the *second* CRTC — index 1, id 2.
                    possible_crtcs: 0b10,
                },
            ],
            pitch_align: 64,
            ..CardState::default()
        })
    }

    /// A card built from an explicit state, with the id counters started.
    fn with(mut state: CardState) -> Self {
        state.next_handle = 1;
        state.next_fb = 1;
        if state.pitch_align == 0 {
            state.pitch_align = 64;
        }
        Self {
            state: Rc::new(RefCell::new(state)),
        }
    }

    /// The same machine with the monitor unplugged: a card that opens fine and
    /// has nothing to show. This is the ordinary state of the *other* card in
    /// a laptop, which is the whole reason the search exists.
    fn dark() -> Self {
        let card = Self::desktop();
        card.edit(|s| {
            for c in &mut s.connectors {
                c.connection = 2;
            }
        });
        card
    }

    /// Mutate the card mid-test — to inject a failure, for instance.
    fn edit(&self, f: impl FnOnce(&mut CardState)) {
        f(&mut self.state.borrow_mut());
    }

    /// Read the card's state.
    fn read<T>(&self, f: impl FnOnce(&CardState) -> T) -> T {
        f(&self.state.borrow())
    }
}

/// A mode, preferred or not.
fn mode(w: u16, h: u16, preferred: bool) -> ModeModeinfo {
    ModeModeinfo {
        clock: 85_500,
        hdisplay: w,
        vdisplay: h,
        vrefresh: 60,
        type_: if preferred {
            uapi::MODE_TYPE_PREFERRED
        } else {
            0
        },
        ..ModeModeinfo::default()
    }
}

impl KmsSys for FakeCard {
    fn ioctl(
        &mut self,
        request: u32,
        payload: &mut [u8],
        arrays: &mut [OutArray<'_>],
    ) -> Result<(), Errno> {
        // The kernel decodes the payload size from the request number and
        // copies exactly that many bytes from userspace. A caller whose buffer
        // disagrees is reading or writing memory it did not mean to, so this
        // is checked on every single call rather than in one test.
        assert_eq!(
            payload.len(),
            declared_size(request),
            "payload for ioctl {request:#010x} is not the size the request number declares"
        );

        let mut state = self.state.borrow_mut();
        state.log.push(request);
        if state.fail.first().is_some_and(|&(r, _)| r == request) {
            let (_, errno) = state.fail.remove(0);
            return Err(errno);
        }
        let lie = state.lie_counts;
        let count = |actual: usize| lie.unwrap_or(actual as u32);

        match request {
            uapi::GETRESOURCES => {
                let mut res = ModeCardRes::from_bytes(&fixed(payload));
                let crtcs = state.crtcs.clone();
                let conns: Vec<u32> = state.connectors.iter().map(|c| c.id).collect();
                if let Some(buf) = array_at(arrays, ModeCardRes::CRTC_ID_PTR_AT) {
                    fill_u32s(buf, &crtcs);
                }
                if let Some(buf) = array_at(arrays, ModeCardRes::CONNECTOR_ID_PTR_AT) {
                    fill_u32s(buf, &conns);
                }
                res.count_crtcs = count(crtcs.len());
                res.count_connectors = count(conns.len());
                res.count_encoders = count(state.encoders.len());
                res.count_fbs = 0;
                res.max_width = 8192;
                res.max_height = 8192;
                put(payload, &res.to_bytes());
                Ok(())
            }
            uapi::GETCONNECTOR => {
                let mut out = ModeGetConnector::from_bytes(&fixed(payload));
                let Some(conn) = state
                    .connectors
                    .iter()
                    .find(|c| c.id == out.connector_id)
                    .cloned()
                else {
                    return Err(ENODEV);
                };
                if let Some(buf) = array_at(arrays, ModeGetConnector::ENCODERS_PTR_AT) {
                    fill_u32s(buf, &conn.encoders);
                }
                if let Some(buf) = array_at(arrays, ModeGetConnector::MODES_PTR_AT) {
                    for (slot, m) in buf
                        .chunks_exact_mut(ModeModeinfo::SIZE)
                        .zip(conn.modes.iter())
                    {
                        slot.copy_from_slice(&m.to_bytes());
                    }
                }
                out.connection = conn.connection;
                out.encoder_id = conn.current_encoder;
                out.count_modes = count(conn.modes.len());
                out.count_encoders = count(conn.encoders.len());
                out.count_props = 0;
                out.connector_type = 11; // HDMI-A
                out.connector_type_id = 1;
                out.mm_width = 340;
                out.mm_height = 190;
                put(payload, &out.to_bytes());
                Ok(())
            }
            uapi::GETENCODER => {
                let mut out = ModeGetEncoder::from_bytes(&fixed(payload));
                let Some(enc) = state.encoders.iter().find(|e| e.id == out.encoder_id) else {
                    return Err(ENODEV);
                };
                out.encoder_type = 2; // TMDS
                out.crtc_id = enc.crtc_id;
                out.possible_crtcs = enc.possible_crtcs;
                out.possible_clones = 0;
                put(payload, &out.to_bytes());
                Ok(())
            }
            uapi::CREATE_DUMB => {
                let mut out = ModeCreateDumb::from_bytes(&fixed(payload));
                if out.width == 0 || out.height == 0 || out.bpp != 32 || out.flags != 0 {
                    return Err(EINVAL);
                }
                let align = u64::from(state.pitch_align);
                let row = u64::from(out.width) * 4;
                let pitch = row.div_ceil(align) * align;
                out.pitch = pitch as u32;
                out.size = (pitch * u64::from(out.height)).saturating_sub(state.shrink_size);
                out.handle = state.next_handle;
                state.next_handle += 1;
                state.dumb.push(FakeDumb {
                    handle: out.handle,
                    offset: u64::from(out.handle) * 0x1000_0000,
                    size: out.size,
                    pitch: out.pitch,
                });
                put(payload, &out.to_bytes());
                Ok(())
            }
            uapi::MAP_DUMB => {
                let mut out = ModeMapDumb::from_bytes(&fixed(payload));
                let Some(d) = state.dumb.iter().find(|d| d.handle == out.handle) else {
                    return Err(EINVAL);
                };
                out.offset = d.offset;
                put(payload, &out.to_bytes());
                Ok(())
            }
            uapi::DESTROY_DUMB => {
                let ask = ModeDestroyDumb::from_bytes(&fixed(payload));
                if !state.dumb.iter().any(|d| d.handle == ask.handle) {
                    return Err(EINVAL);
                }
                state.dumb.retain(|d| d.handle != ask.handle);
                state.destroyed.push(ask.handle);
                Ok(())
            }
            uapi::ADDFB2 => {
                let mut out = ModeFbCmd2::from_bytes(&fixed(payload));
                // The load-bearing zeroes. Linux rejects the request outright
                // if a packed format names more than one plane, and so does
                // the SlateOS kernel — so if this ever stops holding, scanout
                // fails on the target and nowhere else.
                if out.handles[1..] != [0; 3]
                    || out.pitches[1..] != [0; 3]
                    || out.offsets != [0; 4]
                    || out.modifier != [0; 4]
                    || out.flags != 0
                {
                    return Err(EINVAL);
                }
                if out.pixel_format != uapi::FORMAT_XRGB8888 {
                    return Err(EINVAL);
                }
                let Some(d) = state.dumb.iter().find(|d| d.handle == out.handles[0]) else {
                    return Err(EINVAL);
                };
                if out.pitches[0] != d.pitch || out.width == 0 || out.height == 0 {
                    return Err(EINVAL);
                }
                if u64::from(out.pitches[0]) * u64::from(out.height) > d.size {
                    return Err(EINVAL);
                }
                out.fb_id = state.next_fb;
                state.next_fb += 1;
                let record = (
                    out.fb_id,
                    out.handles[0],
                    out.pitches[0],
                    out.width,
                    out.height,
                );
                state.fbs.push(record);
                put(payload, &out.to_bytes());
                Ok(())
            }
            uapi::RMFB => {
                let id = u32::from_le_bytes(fixed(payload));
                if !state.fbs.iter().any(|f| f.0 == id) {
                    return Err(EINVAL);
                }
                state.fbs.retain(|f| f.0 != id);
                state.removed.push(id);
                Ok(())
            }
            uapi::PAGE_FLIP => {
                let flip = ModeCrtcPageFlip::from_bytes(&fixed(payload));
                if flip.reserved != 0 || flip.flags != 0 {
                    return Err(EINVAL);
                }
                if !state.crtcs.contains(&flip.crtc_id) {
                    return Err(EINVAL);
                }
                if !state.fbs.iter().any(|f| f.0 == flip.fb_id) {
                    return Err(EINVAL);
                }
                state.flips.push((flip.crtc_id, flip.fb_id));
                Ok(())
            }
            other => {
                panic!("the compositor issued an ioctl the kernel does not have: {other:#010x}")
            }
        }
    }

    fn map(&mut self, offset: u64, len: usize) -> Result<Box<dyn Mapped>, Errno> {
        let state = self.state.borrow();
        let Some(d) = state.dumb.iter().find(|d| d.offset == offset) else {
            return Err(EINVAL);
        };
        if len as u64 > d.size {
            return Err(EINVAL);
        }
        Ok(Box::new(FakeMapping { data: vec![0; len] }))
    }
}

// ------------------------------------------------------------ setup tests --

#[test]
fn a_desktop_card_is_set_up_end_to_end_and_shows_a_frame_before_anything_is_composed() {
    let card = FakeCard::desktop();
    let scanout = DrmScanout::new(card.clone()).expect("this card can be driven");

    assert_eq!(scanout.size(), (1366, 768), "the panel's native mode");
    assert_eq!(scanout.connector_id(), 31, "not the dark head listed first");
    assert_eq!(scanout.crtc_id(), 2, "the only CRTC its encoder can reach");

    card.read(|s| {
        assert_eq!(s.dumb.len(), 2, "double buffered");
        assert_eq!(s.fbs.len(), 2, "both registered as framebuffers");
        assert_eq!(
            s.flips.len(),
            1,
            "and one is already on screen, so the display is not showing boot leftovers"
        );
    });
}

#[test]
fn the_mode_the_display_calls_native_wins_over_the_first_one_it_offered() {
    // The fake lists 1024x768 first and flags 1366x768 preferred. Taking
    // modes[0] is the obvious implementation and gives a letterboxed picture on
    // every panel whose EDID is ordered this way.
    let scanout = DrmScanout::new(FakeCard::desktop()).unwrap();
    assert_eq!(scanout.size(), (1366, 768));
}

#[test]
fn with_no_preferred_mode_the_first_one_offered_is_taken() {
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors[1].modes = vec![mode(800, 600, false), mode(1920, 1080, false)];
    });
    let scanout = DrmScanout::new(card).unwrap();
    assert_eq!(
        scanout.size(),
        (800, 600),
        "DRM orders a connector's modes best-first, so the first is the answer"
    );
}

#[test]
fn a_zero_sized_mode_is_never_selected() {
    // A connector can report a placeholder mode with no extent; building a
    // framebuffer at 0x0 would fail at CREATE_DUMB with a confusing errno.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors[1].modes = vec![mode(0, 0, true), mode(1280, 1024, false)];
    });
    let scanout = DrmScanout::new(card).unwrap();
    assert_eq!(scanout.size(), (1280, 1024));
}

#[test]
fn a_card_with_nothing_plugged_in_says_so_rather_than_failing_obscurely() {
    let card = FakeCard::desktop();
    card.edit(|s| {
        for c in &mut s.connectors {
            c.connection = 2;
        }
    });
    assert_eq!(
        DrmScanout::new(card).map(|_| ()).unwrap_err(),
        ScanoutError::NoConnectedDisplay
    );
}

#[test]
fn a_connected_display_that_offers_no_mode_is_a_different_error_from_no_display() {
    // Worth distinguishing: "nothing is plugged in" is normal on a server and
    // "there is a monitor and we cannot drive it" is a bug report.
    let card = FakeCard::desktop();
    card.edit(|s| s.connectors[1].modes.clear());
    assert_eq!(
        DrmScanout::new(card).map(|_| ()).unwrap_err(),
        ScanoutError::NoUsableMode
    );
}

#[test]
fn a_connected_display_no_crtc_can_reach_is_also_no_usable_mode() {
    let card = FakeCard::desktop();
    card.edit(|s| s.encoders[1].possible_crtcs = 0);
    assert_eq!(
        DrmScanout::new(card).map(|_| ()).unwrap_err(),
        ScanoutError::NoUsableMode
    );
}

#[test]
fn a_connector_whose_probe_fails_does_not_hide_the_working_one_behind_it() {
    // A card can report a connector it then refuses to describe. Propagating
    // that failure would leave a working monitor dark because of a broken one.
    let card = FakeCard::desktop();
    card.edit(|s| s.fail.push((uapi::GETCONNECTOR, ENODEV)));
    let scanout = DrmScanout::new(card).expect("the second connector still works");
    assert_eq!(scanout.connector_id(), 31);
}

// -------------------------------------------------------- CRTC resolution --

#[test]
fn the_crtc_bitmask_is_read_as_an_index_into_the_crtc_list_not_as_a_crtc_id() {
    // THE bug this file exists to prevent. The card has CRTCs [1, 2] and the
    // connector's encoder reports `possible_crtcs = 0b10`. Read correctly, bit
    // 1 means "the CRTC at index 1", which is id 2. Read as ids — `mask >>
    // crtc_id & 1`, which is what the field's name invites — bit 1 means "CRTC
    // id 1", and the compositor drives the wrong head.
    //
    // On a single-CRTC machine both readings agree, which is exactly why this
    // needs a two-CRTC fake to catch.
    let card = FakeCard::desktop();
    card.edit(|s| s.connectors[1].current_encoder = 0);
    let scanout = DrmScanout::new(card).unwrap();
    assert_eq!(scanout.crtc_id(), 2);
}

#[test]
fn the_crtc_already_driving_the_connector_is_preferred_over_a_merely_possible_one() {
    // The firmware lit a head at boot; that CRTC is already scanning out at
    // this mode, and taking it means the first flip does not have to modeset.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors[1].current_encoder = 51;
        s.encoders[1].crtc_id = 1;
        // …even though the bitmask says only index 1 (id 2) is reachable.
        s.encoders[1].possible_crtcs = 0b10;
    });
    let scanout = DrmScanout::new(card).unwrap();
    assert_eq!(scanout.crtc_id(), 1, "the one already bound");
}

#[test]
fn a_bound_encoder_naming_a_crtc_the_card_does_not_list_is_not_believed() {
    // A stale routing must not become a flip against an id that does not
    // exist, which the kernel answers with EINVAL on every frame.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors[1].current_encoder = 51;
        s.encoders[1].crtc_id = 99;
        s.encoders[1].possible_crtcs = 0b10;
    });
    let scanout = DrmScanout::new(card).unwrap();
    assert_eq!(scanout.crtc_id(), 2, "fell back to the bitmask");
}

// ------------------------------------------------------------ enumeration --

#[test]
fn a_card_that_claims_four_billion_connectors_is_not_believed() {
    // The counts come back from the kernel and size our allocations. Without a
    // clamp this asks for a 16 GiB `Vec` and takes the process out; with one it
    // enumerates the 64 it is willing to look at and drives the real monitor.
    let card = FakeCard::desktop();
    card.edit(|s| s.lie_counts = Some(u32::MAX));
    let scanout = DrmScanout::new(card).expect("the real connector is still found");
    assert_eq!(scanout.size(), (1366, 768));
}

#[test]
fn enumeration_asks_how_many_before_asking_for_the_list() {
    // Two `GETRESOURCES` in a row, then connector probes. Getting this
    // backwards — one call with a guessed capacity — silently truncates on any
    // machine with more heads than the guess.
    let card = FakeCard::desktop();
    let _scanout = DrmScanout::new(card.clone()).unwrap();
    card.read(|s| {
        assert_eq!(
            &s.log[..2],
            &[uapi::GETRESOURCES, uapi::GETRESOURCES],
            "probe then fetch"
        );
    });
}

// ---------------------------------------------------------------- buffers --

#[test]
fn the_scanout_buffers_are_created_mapped_and_registered_in_that_order() {
    let card = FakeCard::desktop();
    let _scanout = DrmScanout::new(card.clone()).unwrap();
    card.read(|s| {
        let tail: Vec<u32> = s
            .log
            .iter()
            .copied()
            .filter(|r| {
                matches!(
                    *r,
                    uapi::CREATE_DUMB | uapi::MAP_DUMB | uapi::ADDFB2 | uapi::PAGE_FLIP
                )
            })
            .collect();
        assert_eq!(
            tail,
            vec![
                uapi::CREATE_DUMB,
                uapi::MAP_DUMB,
                uapi::ADDFB2,
                uapi::CREATE_DUMB,
                uapi::MAP_DUMB,
                uapi::ADDFB2,
                uapi::PAGE_FLIP,
            ]
        );
    });
}

#[test]
fn a_driver_that_returns_a_buffer_smaller_than_its_own_pitch_times_height_is_refused() {
    // This is the check that stands between a driver bug and a wild write
    // across whatever follows the mapping: everything downstream indexes with
    // `pitch`, so `size >= pitch * height` is the precondition that makes those
    // indices safe.
    let card = FakeCard::desktop();
    card.edit(|s| s.shrink_size = 1);
    assert_eq!(
        DrmScanout::new(card).map(|_| ()).unwrap_err(),
        ScanoutError::BadBuffer
    );
}

#[test]
fn a_driver_that_returns_a_zero_handle_is_refused() {
    let card = FakeCard::desktop();
    card.edit(|s| s.next_handle = 0);
    assert_eq!(
        DrmScanout::new(card).map(|_| ()).unwrap_err(),
        ScanoutError::BadBuffer
    );
}

#[test]
fn addfb2_is_sent_with_the_zeroes_the_kernel_insists_on() {
    // The fake rejects a request whose unused planes, offsets or modifiers are
    // non-zero, exactly as the kernel does. That this test reaches a working
    // scanout at all is the assertion.
    let card = FakeCard::desktop();
    let _scanout = DrmScanout::new(card.clone()).unwrap();
    card.read(|s| assert_eq!(s.fbs.len(), 2));
}

#[test]
fn the_framebuffer_is_registered_at_the_pitch_the_driver_chose_not_at_width_times_four() {
    // 1366 * 4 = 5464, which 64-byte alignment rounds to 5504. Registering the
    // framebuffer at 5464 makes the display skew every row after the first.
    let card = FakeCard::desktop();
    let scanout = DrmScanout::new(card.clone()).unwrap();
    assert_eq!(scanout.pitch(), 5504);
    card.read(|s| {
        for &(_, _, pitch, w, h) in &s.fbs {
            assert_eq!(pitch, 5504);
            assert_eq!((w, h), (1366, 768));
        }
    });
}

// ------------------------------------------------------------------ frames --

#[test]
fn a_composed_frame_reaches_the_display_in_the_byte_order_xr24_means() {
    // The compositor's `0xAARRGGBB` and DRM's XR24 are the same four bytes in
    // the same order on a little-endian machine — B, G, R, X. A byte swap here
    // turns the desktop's blue into red on real hardware and nowhere else.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();
    let frame = vec![0xFF35_79BD_u32; (w * h) as usize];
    scanout.show(&frame, w, h);
    let bytes = scanout.scanned_out();
    assert_eq!(&bytes[..4], &[0xBD, 0x79, 0x35, 0xFF]);
}

#[test]
fn each_row_is_written_at_the_drivers_pitch_and_not_at_width_times_four() {
    // The single most likely arithmetic bug in this module, and one that looks
    // *nearly* right on screen: a 40-byte-per-row skew that shears the whole
    // desktop diagonally. Pinned by writing a frame whose every row is a
    // different colour and reading back where row 1 landed.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();
    let pitch = scanout.pitch() as usize;
    assert_ne!(pitch, (w * 4) as usize, "the fake pads, as real drivers do");

    let mut frame = vec![0u32; (w * h) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            frame[y * w as usize + x] = 0xFF00_0000 | y as u32;
        }
    }
    scanout.show(&frame, w, h);
    let bytes = scanout.scanned_out();
    for y in [0usize, 1, 2, 767] {
        let at = y * pitch;
        assert_eq!(
            u32::from_le_bytes(fixed(&bytes[at..at + 4])),
            0xFF00_0000 | y as u32,
            "row {y} is not at row * pitch"
        );
    }
}

#[test]
fn the_padding_between_rows_is_left_alone() {
    // Writing `width * height * 4` bytes contiguously would overwrite it, and
    // would also run off the end of the last row.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();
    let pitch = scanout.pitch() as usize;
    scanout.show(&vec![0xFFFF_FFFF_u32; (w * h) as usize], w, h);
    let bytes = scanout.scanned_out();
    let row_bytes = (w * 4) as usize;
    assert_eq!(
        &bytes[row_bytes..pitch],
        &vec![0u8; pitch - row_bytes][..],
        "the pad after row 0 was written through"
    );
}

#[test]
fn successive_frames_alternate_between_the_two_buffers() {
    // The whole point of allocating two: a frame drawn into the buffer that is
    // currently being scanned out is a frame the user watches being drawn.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    let frame = vec![0u32; (w * h) as usize];
    scanout.show(&frame, w, h);
    scanout.show(&frame, w, h);
    scanout.show(&frame, w, h);
    card.read(|s| {
        let ids: Vec<u32> = s.flips.iter().map(|&(_, fb)| fb).collect();
        assert_eq!(ids, vec![1, 2, 1, 2], "setup, then three frames");
        assert!(s.flips.iter().all(|&(crtc, _)| crtc == 2));
    });
}

#[test]
fn a_frame_smaller_than_the_screen_is_drawn_in_the_corner_and_touches_nothing_else() {
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card).unwrap();
    let pitch = scanout.pitch() as usize;
    scanout.show(&[0xFFAA_BBCC; 4], 2, 2);
    let bytes = scanout.scanned_out();
    assert_eq!(u32::from_le_bytes(fixed(&bytes[..4])), 0xFFAA_BBCC);
    assert_eq!(
        u32::from_le_bytes(fixed(&bytes[pitch..pitch + 4])),
        0xFFAA_BBCC,
        "row 1 too"
    );
    assert_eq!(
        u32::from_le_bytes(fixed(&bytes[8..12])),
        0,
        "and nothing past the third column"
    );
    assert_eq!(
        u32::from_le_bytes(fixed(&bytes[2 * pitch..2 * pitch + 4])),
        0,
        "nor past the second row"
    );
}

#[test]
fn a_frame_larger_than_the_screen_is_clipped_rather_than_overrunning_the_buffer() {
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();
    let big = vec![0xFF11_2233_u32; ((w + 100) * (h + 100)) as usize];
    scanout.show(&big, w + 100, h + 100);
    let pitch = scanout.pitch() as usize;
    let bytes = scanout.scanned_out();
    assert_eq!(u32::from_le_bytes(fixed(&bytes[..4])), 0xFF11_2233);
    // The last row of the screen, first pixel — reached only if the clip kept
    // the row stride of the *source*, which is wider than the screen.
    let last = (h as usize - 1) * pitch;
    assert_eq!(
        u32::from_le_bytes(fixed(&bytes[last..last + 4])),
        0xFF11_2233
    );
}

#[test]
fn a_frame_shorter_than_it_claims_to_be_does_not_take_the_display_server_down() {
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();
    scanout.show(&[0xFFFF_FFFF; 3], w, h);
    assert!(scanout.is_open(), "a bad frame is not a broken display");
}

// -------------------------------------------------------------- liveness --

#[test]
fn a_flip_that_is_merely_early_drops_the_frame_and_keeps_the_display() {
    // EBUSY means the previous flip has not retired. Every compositor drops the
    // frame; tearing down the display because a monitor was slow would take the
    // desktop out under load.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, EBUSY)));
    scanout.show(&vec![0xFF00_FF00_u32; (w * h) as usize], w, h);
    assert!(scanout.is_open());
    card.read(|s| assert_eq!(s.flips.len(), 1, "only the setup flip landed"));

    // And the dropped frame's buffer is reused, rather than the pair getting
    // out of step and every subsequent frame going to the visible buffer.
    scanout.show(&vec![0xFF00_00FF_u32; (w * h) as usize], w, h);
    card.read(|s| {
        assert_eq!(
            s.flips.iter().map(|&(_, fb)| fb).collect::<Vec<_>>(),
            vec![1, 2]
        );
    });
    assert_eq!(
        u32::from_le_bytes(fixed(&scanout.scanned_out()[..4])),
        0xFF00_00FF,
        "the frame on screen is the one that flipped, not the one that did not"
    );
}

#[test]
fn a_flip_that_fails_for_a_real_reason_closes_the_display() {
    // ENODEV is the card going away — an unplugged eGPU, a driver unbind.
    // Continuing to composite into a mapping the kernel has torn down is not a
    // recoverable situation, and `is_open` is how `Server::run_with` learns.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, ENODEV)));
    scanout.show(&vec![0u32; (w * h) as usize], w, h);
    assert!(!scanout.is_open());
}

#[test]
fn a_closed_display_stops_being_written_to() {
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, ENODEV)));
    scanout.show(&vec![0u32; (w * h) as usize], w, h);
    let before = card.read(|s| s.log.len());
    scanout.show(&vec![0u32; (w * h) as usize], w, h);
    assert_eq!(
        card.read(|s| s.log.len()),
        before,
        "no further ioctls after the display went away"
    );
}

// ---------------------------------------------------------------- cleanup --

#[test]
fn every_framebuffer_and_buffer_is_given_back_when_scanout_is_dropped() {
    // The card outlives the scanout here — two scanouts sharing one card is a
    // real thing on a two-headed machine — so relying on the fd close to clean
    // up would leak both framebuffers for the life of the process.
    let card = FakeCard::desktop();
    {
        let _scanout = DrmScanout::new(card.clone()).unwrap();
        card.read(|s| assert_eq!((s.fbs.len(), s.dumb.len()), (2, 2)));
    }
    card.read(|s| {
        assert_eq!(s.removed, vec![1, 2], "both framebuffer ids");
        assert_eq!(s.destroyed, vec![1, 2], "both GEM handles");
        assert!(s.fbs.is_empty() && s.dumb.is_empty());
    });
}

#[test]
fn a_framebuffer_is_removed_before_its_buffer_is_destroyed() {
    // The other order leaves the kernel holding a framebuffer whose backing
    // object the client asked to free, which is refused on some drivers and
    // merely confusing on the rest.
    let card = FakeCard::desktop();
    drop(DrmScanout::new(card.clone()).unwrap());
    card.read(|s| {
        let tail: Vec<u32> = s
            .log
            .iter()
            .copied()
            .filter(|r| matches!(*r, uapi::RMFB | uapi::DESTROY_DUMB))
            .collect();
        assert_eq!(
            tail,
            vec![
                uapi::RMFB,
                uapi::DESTROY_DUMB,
                uapi::RMFB,
                uapi::DESTROY_DUMB
            ]
        );
    });
}

// -------------------------------------------------------- card selection --

/// Permission denied, which is what `/dev/dri/card0` says to a process that is
/// not in the `video` group — the most likely *real* failure of an `open`.
const EACCES: Errno = 13;

/// A `/dev/dri` that exists only in memory.
///
/// The slots are what each index does when opened, in index order; an index
/// past the end is `ENOENT`, exactly as a machine with fewer cards behaves.
/// Every index asked for is recorded, because *which cards were touched* is
/// half of what these tests are checking — a search that quietly opens card 0
/// after being told `--card 1` passes every assertion about its return value.
#[derive(Debug, Default)]
struct FakeCards {
    /// What each card index does when opened.
    slots: Vec<Result<FakeCard, Errno>>,
    /// Every index [`CardSource::open`] was called with, in order.
    opened: Vec<u32>,
}

impl FakeCards {
    /// A `/dev/dri` holding exactly these cards, numbered from zero.
    fn holding(slots: Vec<Result<FakeCard, Errno>>) -> Self {
        Self {
            slots,
            opened: Vec::new(),
        }
    }
}

impl CardSource for FakeCards {
    type Sys = FakeCard;

    fn open(&mut self, index: u32) -> Result<FakeCard, Errno> {
        self.opened.push(index);
        match self.slots.get(index as usize) {
            Some(Ok(card)) => Ok(card.clone()),
            Some(&Err(errno)) => Err(errno),
            None => Err(ENOENT),
        }
    }
}

/// Every index the search is expected to try on a machine with no cards.
fn all_indices() -> Vec<u32> {
    (0..MAX_CARDS).collect()
}

#[test]
fn the_first_card_with_a_display_attached_is_the_one_used() {
    // The case the whole search exists for: a laptop whose panel is on card 1
    // this boot. Opening card 0 unconditionally gives a black screen.
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::dark()), Ok(FakeCard::desktop())]);
    let scanout = open_display(&mut dri, None).expect("card 1 has the panel");
    assert_eq!(scanout.size(), (1366, 768));
    assert_eq!(dri.opened, vec![0, 1], "and it did not keep looking after");
}

#[test]
fn the_search_stops_at_the_first_card_that_works() {
    // Opening a card is not free — it takes a DRM master lease on real
    // hardware — so a search that carries on past its answer would be taking
    // out the discrete GPU on every boot for nothing.
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::desktop()), Ok(FakeCard::desktop())]);
    open_display(&mut dri, None).unwrap();
    assert_eq!(dri.opened, vec![0]);
}

#[test]
fn a_machine_with_no_graphics_at_all_says_no_display_rather_than_no_such_file() {
    // Every index is ENOENT. `Open(ENOENT)` on /dev/dri/card15 would be a true
    // statement and a useless one; the honest report is that there is nothing
    // to scan out on, which is what makes the caller fall back to headless.
    let mut dri = FakeCards::default();
    assert_eq!(
        open_display(&mut dri, None).map(|_| ()).unwrap_err(),
        ScanoutError::NoConnectedDisplay
    );
    assert_eq!(dri.opened, all_indices(), "and it did look everywhere");
}

#[test]
fn a_card_that_is_present_but_unplugged_also_reports_no_display() {
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::dark())]);
    assert_eq!(
        open_display(&mut dri, None).map(|_| ()).unwrap_err(),
        ScanoutError::NoConnectedDisplay
    );
}

#[test]
fn a_broken_first_card_does_not_keep_a_working_second_one_dark() {
    // A card we cannot open is a reason to look at the next one, not a reason
    // to give up: on a machine where the discrete GPU is claimed by something
    // else, the panel is still on the integrated one and still wants lighting.
    let mut dri = FakeCards::holding(vec![Err(EACCES), Ok(FakeCard::desktop())]);
    let scanout = open_display(&mut dri, None).expect("card 1 still works");
    assert_eq!(scanout.size(), (1366, 768));
}

#[test]
fn a_real_failure_is_reported_when_nothing_works_rather_than_being_swallowed() {
    // Being unable to open the only card is a bug report — a missing group
    // membership, usually. Reporting `NoConnectedDisplay` for it would send
    // whoever read it looking behind the monitor.
    let mut dri = FakeCards::holding(vec![Err(EACCES)]);
    assert_eq!(
        open_display(&mut dri, None).map(|_| ()).unwrap_err(),
        ScanoutError::Open(EACCES)
    );
}

#[test]
fn the_first_real_failure_is_reported_and_not_the_last() {
    // Two cards fail differently. The first is the one on the machine's own
    // primary adapter and is far more likely to be the real story; keeping the
    // last would also mean any later ENOENT overwrote it.
    let mut dri = FakeCards::holding(vec![Err(EACCES), Err(ENODEV)]);
    assert_eq!(
        open_display(&mut dri, None).map(|_| ()).unwrap_err(),
        ScanoutError::Open(EACCES)
    );
}

#[test]
fn an_unplugged_card_does_not_displace_a_real_failure_from_the_report() {
    // Order matters here: the unplugged card comes *first*, so an
    // implementation that simply keeps the most recent non-success would still
    // pass. One that treats "nothing plugged in" as newsworthy would not.
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::dark()), Err(EACCES)]);
    assert_eq!(
        open_display(&mut dri, None).map(|_| ()).unwrap_err(),
        ScanoutError::Open(EACCES),
        "the card we could not open is the one worth talking about"
    );
}

#[test]
fn a_named_card_is_opened_and_nothing_else_is() {
    // `--card 1` on a machine where card 0 would also have worked. Falling
    // back would light the wrong monitor and look like the flag was ignored.
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::desktop()), Ok(FakeCard::desktop())]);
    open_display(&mut dri, Some(1)).unwrap();
    assert_eq!(dri.opened, vec![1]);
}

#[test]
fn a_named_card_that_cannot_be_opened_is_an_error_and_not_a_search() {
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::desktop()), Err(EACCES)]);
    assert_eq!(
        open_display(&mut dri, Some(1)).map(|_| ()).unwrap_err(),
        ScanoutError::Open(EACCES)
    );
    assert_eq!(dri.opened, vec![1], "card 0 was never touched");
}

#[test]
fn a_named_card_with_nothing_plugged_in_is_an_error_and_not_a_search() {
    let mut dri = FakeCards::holding(vec![Ok(FakeCard::desktop()), Ok(FakeCard::dark())]);
    assert_eq!(
        open_display(&mut dri, Some(1)).map(|_| ()).unwrap_err(),
        ScanoutError::NoConnectedDisplay
    );
    assert_eq!(dri.opened, vec![1]);
}

#[test]
fn a_card_path_is_the_nul_terminated_name_the_kernel_expects() {
    // `open` takes a C string: without the terminator it reads past the array.
    assert_eq!(CardPath::card(0).as_c_bytes(), b"/dev/dri/card0\0");
    assert_eq!(CardPath::card(0).as_display_bytes(), b"/dev/dri/card0");
}

#[test]
fn a_two_digit_card_path_is_built_correctly() {
    // The last index the search reaches, and the one where a single-digit
    // formatter would produce `/dev/dri/card1` and silently reopen card 1.
    let last = MAX_CARDS - 1;
    assert_eq!(CardPath::card(last).as_display_bytes(), b"/dev/dri/card15");
    assert_eq!(CardPath::card(last).as_c_bytes().len(), 16);
    assert_eq!(CardPath::card(9).as_display_bytes(), b"/dev/dri/card9");
}

#[test]
fn every_index_the_search_covers_has_a_distinct_path() {
    // A path buffer one byte short would truncate, and two indices would name
    // the same file — a search that appeared to work and always used one card.
    let paths: Vec<Vec<u8>> = (0..MAX_CARDS)
        .map(|i| CardPath::card(i).as_display_bytes().to_vec())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), paths.len(), "{paths:?}");
}

// ------------------------------------------------------------------- blit --

#[test]
fn blit_writes_nothing_when_the_pitch_is_zero() {
    let mut dst = vec![0xEEu8; 64];
    blit(&mut dst, 0, 4, 4, &[0xFFFF_FFFF; 16], 4, 4);
    assert!(dst.iter().all(|&b| b == 0xEE));
}

#[test]
fn blit_stops_at_the_end_of_the_destination_rather_than_wrapping() {
    // A destination one row shorter than the source claims: the last row must
    // be dropped, not folded back onto the first.
    let mut dst = vec![0u8; 16]; // one row of 4 pixels
    blit(&mut dst, 16, 4, 2, &[1, 2, 3, 4, 9, 9, 9, 9], 4, 2);
    assert_eq!(u32::from_le_bytes(fixed(&dst[..4])), 1);
    assert_eq!(dst.len(), 16, "and nothing grew");
}

#[test]
fn blit_uses_the_source_width_to_step_rows_not_the_copy_width() {
    // When the source is wider than the screen, row `y` starts at
    // `y * src_width`, not `y * copy_width`. Confusing the two produces a
    // picture that drifts sideways as it goes down — the classic stride bug.
    let mut dst = vec![0u8; 2 * 8];
    // 3-wide source, 2-wide destination.
    let src: Vec<u32> = vec![10, 11, 12, 20, 21, 22];
    blit(&mut dst, 8, 2, 2, &src, 3, 2);
    assert_eq!(u32::from_le_bytes(fixed(&dst[..4])), 10);
    assert_eq!(u32::from_le_bytes(fixed(&dst[4..8])), 11);
    assert_eq!(u32::from_le_bytes(fixed(&dst[8..12])), 20, "not 12");
    assert_eq!(u32::from_le_bytes(fixed(&dst[12..16])), 21);
}
