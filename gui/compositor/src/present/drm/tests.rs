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
use std::time::Duration;

use super::sys::{
    CardPath, CardSource, EBUSY, ENODEV, ENOENT, Errno, KmsSys, MAX_CARDS, Mapped, OutArray,
};
use super::uapi::{
    self, ModeCardRes, ModeCreateDumb, ModeCrtc, ModeCrtcPageFlip, ModeDestroyDumb, ModeFbCmd2,
    ModeGetConnector, ModeGetEncoder, ModeMapDumb, ModeModeinfo,
};
use super::{
    DrmScanout, HeadInfo, MonitorInfo, Present, ScanoutError, Viewport, blit, open_display,
};

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
    /// Every accepted `SETCRTC` that enabled a CRTC, as
    /// `(crtc_id, connector_id, fb_id, hdisplay, vdisplay)`.
    mode_sets: Vec<(u32, u32, u32, u32, u32)>,
    /// The mode currently programmed on each CRTC, as
    /// `(crtc_id, hdisplay, vdisplay)`.
    ///
    /// A CRTC absent from this list has no mode, and [`uapi::PAGE_FLIP`] on it
    /// is refused — which is what makes the mode-set load-bearing here rather
    /// than decorative. Before the kernel became strict, a fake that accepted
    /// any flip would have passed just as happily with no `SETCRTC` at all.
    modes: Vec<(u32, u32, u32)>,
    /// Handles passed to `DESTROY_DUMB`.
    destroyed: Vec<u32>,
    /// Ids passed to `RMFB`.
    removed: Vec<u32>,
    /// Every ioctl request number, in order.
    log: Vec<u32>,
    /// Failures to inject, consumed in order: the next call whose request
    /// matches the front entry fails with its errno.
    fail: Vec<(u32, Errno)>,
    /// Failures to inject at a *counted* occurrence: `(request, nth, errno)`
    /// fails the `nth` call (zero-based) with that request and leaves every
    /// other one alone.
    ///
    /// [`Self::fail`] cannot express this — its front entry waits for a
    /// matching call and blocks the ones behind it — and "the *second*
    /// monitor's buffers will not allocate" needs exactly this: the first
    /// monitor's three ioctls have to succeed first.
    fail_nth: Vec<(u32, u32, Errno)>,
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

/// Read `n` `u32`s the caller placed in the out-of-line array at `ptr_at`.
///
/// The mirror of [`fill_u32s`]: `SETCRTC` is the one ioctl here whose array
/// travels *into* the kernel rather than out of it.
fn read_u32s(arrays: &mut [OutArray<'_>], ptr_at: usize, n: u32) -> Vec<u32> {
    let Some(buf) = array_at(arrays, ptr_at) else {
        return Vec::new();
    };
    buf.chunks_exact(4)
        .take(n as usize)
        .map(|c| u32::from_le_bytes(fixed(c)))
        .collect()
}

/// The kernel's cap on `count_connectors`, past which it refuses rather than
/// allocating. `0xFFFF_FFFF` would otherwise ask for 16 GiB.
const MAX_SET_CONNECTORS: u32 = 32;

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

    /// A two-monitor machine: both connectors plugged in, each reachable by a
    /// *different* CRTC, and different sizes so a head that scans out the wrong
    /// rectangle cannot pass by coincidence.
    ///
    /// The widths are chosen so the second head's pitch is padded (1366 * 4 =
    /// 5464, aligned up to 5504) while the first's is not — a `show` that reuses
    /// head 0's pitch for head 1 skews the picture from its second row on.
    fn two_monitors() -> Self {
        let card = Self::desktop();
        card.edit(|s| {
            s.connectors[0].connection = uapi::CONNECTED;
            s.connectors[0].modes = vec![mode(1024, 768, true)];
        });
        card
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
        // How many calls with this request have been made before this one. The
        // log was pushed to above, so this call is itself in the count.
        let seen = state.log.iter().filter(|&&r| r == request).count() as u32 - 1;
        if let Some(i) = state
            .fail_nth
            .iter()
            .position(|&(r, n, _)| r == request && n == seen)
        {
            let (_, _, errno) = state.fail_nth.remove(i);
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
            uapi::SETCRTC => {
                // Modelled on the kernel's `DrmDevice::set_crtc`, checks in the
                // same order and with the same errnos, so that a compositor
                // that satisfies this fake satisfies the real one.
                let set = ModeCrtc::from_bytes(&fixed(payload));
                if !state.crtcs.contains(&set.crtc_id) {
                    return Err(ENOENT);
                }
                if set.mode_valid == 0 {
                    // A disable that also names a framebuffer or a connector is
                    // self-contradictory and is more likely a caller bug.
                    if set.fb_id != 0 || set.count_connectors != 0 {
                        return Err(EINVAL);
                    }
                    state.modes.retain(|m| m.0 != set.crtc_id);
                    return Ok(());
                }
                // A timed CRTC driving nothing, or fetching from nowhere, is
                // not a state worth entering.
                if set.count_connectors == 0 || set.fb_id == 0 {
                    return Err(EINVAL);
                }
                if set.count_connectors > MAX_SET_CONNECTORS {
                    return Err(EINVAL);
                }
                let ids = read_u32s(
                    arrays,
                    ModeCrtc::SET_CONNECTORS_PTR_AT,
                    set.count_connectors,
                );
                let crtc_index = state
                    .crtcs
                    .iter()
                    .position(|&c| c == set.crtc_id)
                    .unwrap_or(usize::MAX);
                for id in &ids {
                    let Some(conn) = state.connectors.iter().find(|c| c.id == *id).cloned() else {
                        return Err(ENOENT);
                    };
                    // Routable to this CRTC through one of its encoders.
                    let routable = conn.encoders.iter().any(|e| {
                        state
                            .encoders
                            .iter()
                            .find(|enc| enc.id == *e)
                            .is_some_and(|enc| {
                                crtc_index < 32 && enc.possible_crtcs & (1 << crtc_index) != 0
                            })
                    });
                    if !routable {
                        return Err(EINVAL);
                    }
                    // The mode must be one the display actually advertised.
                    // `vrefresh == 0` means "don't care", as Linux's
                    // `drm_mode_equal` effectively does.
                    let advertised = conn.modes.iter().any(|m| {
                        m.hdisplay == set.mode.hdisplay
                            && m.vdisplay == set.mode.vdisplay
                            && (set.mode.vrefresh == 0 || m.vrefresh == set.mode.vrefresh)
                    });
                    if !advertised {
                        return Err(EINVAL);
                    }
                }
                // "Invalid fb size": the buffer must cover the mode's extent
                // from the origin the caller named.
                let Some(&fb) = state.fbs.iter().find(|f| f.0 == set.fb_id) else {
                    return Err(EINVAL);
                };
                let need_w = set.x.saturating_add(u32::from(set.mode.hdisplay));
                let need_h = set.y.saturating_add(u32::from(set.mode.vdisplay));
                if fb.3 < need_w || fb.4 < need_h {
                    return Err(EINVAL);
                }
                let (w, h) = (u32::from(set.mode.hdisplay), u32::from(set.mode.vdisplay));
                state.modes.retain(|m| m.0 != set.crtc_id);
                state.modes.push((set.crtc_id, w, h));
                for id in ids {
                    state.mode_sets.push((set.crtc_id, id, set.fb_id, w, h));
                }
                Ok(())
            }
            uapi::PAGE_FLIP => {
                let flip = ModeCrtcPageFlip::from_bytes(&fixed(payload));
                if flip.reserved != 0 || flip.flags != 0 {
                    return Err(EINVAL);
                }
                if !state.crtcs.contains(&flip.crtc_id) {
                    return Err(ENOENT);
                }
                let Some(&fb) = state.fbs.iter().find(|f| f.0 == flip.fb_id) else {
                    return Err(EINVAL);
                };
                // The two checks the kernel gained, and the reason `SETCRTC` is
                // no longer optional: a CRTC with no mode has nothing to flip
                // *into*, and a framebuffer that is not the mode's size would
                // be cropped or over-read by whichever backend is fitted.
                let Some(&(_, mw, mh)) = state.modes.iter().find(|m| m.0 == flip.crtc_id) else {
                    return Err(EINVAL);
                };
                if fb.3 != mw || fb.4 != mh {
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
    // The firmware lit a head at boot; adopting that CRTC retimes a head that
    // is already running rather than moving the picture to a different one.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors[1].current_encoder = 51;
        // Either CRTC would do, so the bitmask alone would hand out index 0
        // (id 1) — the first reachable one. The binding says 2.
        s.encoders[1].possible_crtcs = 0b11;
        s.encoders[1].crtc_id = 2;
    });
    let scanout = DrmScanout::new(card).unwrap();
    assert_eq!(scanout.crtc_id(), 2, "the one already bound");
}

#[test]
fn a_bound_encoder_naming_a_crtc_its_own_bitmask_forbids_is_not_believed() {
    // A card can contradict itself: the encoder reports a live binding to a
    // CRTC that its `possible_crtcs` says it cannot reach. That used to be
    // harmless — the CRTC really was scanning out, so a flip against it worked.
    // It is not harmless now: `SETCRTC` refuses a connector that is not
    // routable to the named CRTC, so preferring the bound one would program
    // nothing and lose the head. The bitmask is what the kernel checks, so the
    // bitmask is what we believe.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors[1].current_encoder = 51;
        s.encoders[1].crtc_id = 1;
        // …even though the bitmask says only index 1 (id 2) is reachable.
        s.encoders[1].possible_crtcs = 0b10;
    });
    let scanout = DrmScanout::new(card.clone()).unwrap();
    assert_eq!(scanout.crtc_id(), 2, "fell back to the bitmask");
    card.read(|s| {
        assert_eq!(
            s.mode_sets,
            vec![(2, 31, s.fbs[1].0, 1366, 768)],
            "and the mode-set the kernel would have refused was never sent"
        );
    });
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
                    uapi::CREATE_DUMB
                        | uapi::MAP_DUMB
                        | uapi::ADDFB2
                        | uapi::SETCRTC
                        | uapi::PAGE_FLIP
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
                // Both buffers exist before the mode-set, because the mode-set
                // has to name one of them and the kernel checks that it covers
                // the mode. Ordering it the other way round is the mistake the
                // resize path has to avoid too.
                uapi::SETCRTC,
                uapi::PAGE_FLIP,
            ]
        );
    });
}

// --------------------------------------------------------------- mode-set --

#[test]
fn the_mode_set_names_the_connector_the_native_mode_and_the_buffer_not_yet_shown() {
    // Every field of it matters and each one fails differently: a wrong
    // connector lights nothing, a mode the display did not advertise is
    // EINVAL, and a framebuffer smaller than the mode is "Invalid fb size".
    let card = FakeCard::desktop();
    let scanout = DrmScanout::new(card.clone()).unwrap();
    card.read(|s| {
        assert_eq!(s.mode_sets.len(), 1, "one head, one mode-set");
        let (crtc, conn, fb, w, h) = s.mode_sets[0];
        assert_eq!(crtc, scanout.crtc_id());
        assert_eq!(conn, scanout.connector_id());
        assert_eq!((w, h), (1366, 768), "the panel's native mode");
        // The mode-set adopts the *back* buffer of the pair, so the first flip
        // is a real change rather than a flip to what is already scanning out.
        assert_ne!(fb, s.flips[0].1, "the first flip flips away from it");
        assert!(
            s.fbs.iter().any(|f| f.0 == fb),
            "and it is one of ours, not a boot leftover"
        );
    });
}

#[test]
fn a_first_flip_would_be_refused_without_the_mode_set() {
    // The reason the mode-set is issued at all. This asserts the *fake* is
    // strict in the way the kernel became strict — if this ever passes with
    // the CRTC unprogrammed, the fake has stopped modelling the kernel and
    // every other test here has stopped proving anything about mode-setting.
    let card = FakeCard::desktop();
    let _scanout = DrmScanout::new(card.clone()).unwrap();
    let (crtc, fb) = card.read(|s| s.flips[0]);

    card.edit(|s| s.modes.retain(|m| m.0 != crtc));
    let flip = ModeCrtcPageFlip {
        crtc_id: crtc,
        fb_id: fb,
        ..ModeCrtcPageFlip::default()
    };
    let mut payload = flip.to_bytes();
    let mut card = card;
    assert_eq!(
        card.ioctl(uapi::PAGE_FLIP, &mut payload, &mut []),
        Err(EINVAL),
        "a CRTC with no mode has nothing to flip into"
    );
}

#[test]
fn a_card_that_refuses_the_mode_set_still_comes_up_if_its_flips_work() {
    // The `limine-fb` shape: the display is already timed at the only mode it
    // advertises, so the mode-set is redundant and its failure is not a reason
    // to decline the connector. The fake keeps the mode programmed here, which
    // is precisely the case the real backend presents.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.modes.push((2, 1366, 768));
        s.fail.push((uapi::SETCRTC, ENODEV));
    });
    let scanout = DrmScanout::new(card.clone()).expect("a redundant mode-set is not load-bearing");
    assert_eq!(scanout.size(), (1366, 768));
    card.read(|s| {
        assert!(s.mode_sets.is_empty(), "the mode-set really did fail");
        assert_eq!(s.flips.len(), 1, "and the head came up anyway");
    });
}

#[test]
fn a_card_that_refuses_the_mode_set_and_has_no_mode_declines_the_head() {
    // The `ATI` shape, and the failure this whole change exists to prevent
    // reaching a user: nothing is timed, the mode-set fails, and so the flip
    // fails. The head is dropped by the path that already existed for a head
    // whose first flip fails — no new error handling, which is the argument for
    // letting the flip be the arbiter.
    let card = FakeCard::desktop();
    card.edit(|s| s.fail.push((uapi::SETCRTC, ENODEV)));
    assert_eq!(
        DrmScanout::new(card.clone()).map(|_| ()).unwrap_err(),
        ScanoutError::Ioctl {
            request: uapi::PAGE_FLIP,
            errno: EINVAL,
        },
        "the flip is what reports it, and it names the flip rather than the \
         mode-set — which is the cost of letting the flip be the arbiter"
    );
    card.read(|s| assert!(s.flips.is_empty(), "nothing was ever scanned out"));
}

#[test]
fn a_mode_set_the_display_never_advertised_is_refused() {
    // Guards the fake's own strictness in the other direction: if it accepted
    // any mode, the compositor could pass a mode it invented and no test would
    // notice. Issued by hand because the compositor only ever picks an
    // advertised one.
    let card = FakeCard::desktop();
    let scanout = DrmScanout::new(card.clone()).unwrap();
    let fb = card.read(|s| s.fbs[0].0);
    let mut card = card;
    let mut conn = 31_u32.to_le_bytes();
    let set = ModeCrtc {
        count_connectors: 1,
        crtc_id: 2,
        fb_id: fb,
        mode_valid: 1,
        mode: ModeModeinfo {
            hdisplay: 640,
            vdisplay: 480,
            ..ModeModeinfo::default()
        },
        ..ModeCrtc::default()
    };
    let mut payload = set.to_bytes();
    let mut arrays = [OutArray::new(ModeCrtc::SET_CONNECTORS_PTR_AT, &mut conn)];
    assert_eq!(
        card.ioctl(uapi::SETCRTC, &mut payload, &mut arrays),
        Err(EINVAL)
    );
    drop(scanout);
}

#[test]
fn every_head_of_a_multi_monitor_card_is_mode_set_before_it_is_flipped() {
    let card = FakeCard::two_monitors();
    let scanout = DrmScanout::new(card.clone()).unwrap();
    assert_eq!(scanout.heads().len(), 2, "both heads came up");
    card.read(|s| {
        assert_eq!(s.mode_sets.len(), 2, "one mode-set each");
        let last_set = s.log.iter().rposition(|&r| r == uapi::SETCRTC);
        let first_flip = s.log.iter().position(|&r| r == uapi::PAGE_FLIP);
        assert!(
            last_set < first_flip,
            "every head is timed before any head is flipped"
        );
        assert_eq!(
            s.mode_sets.iter().map(|m| m.0).collect::<Vec<_>>(),
            vec![1, 2],
            "each head programmed its own CRTC"
        );
        assert_eq!(
            s.mode_sets.iter().map(|m| (m.3, m.4)).collect::<Vec<_>>(),
            vec![(1024, 768), (1366, 768)],
            "each at its own display's native mode, not one shared size"
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

// ------------------------------------------------------ more than one head --

#[test]
fn every_connected_monitor_is_driven_and_not_just_the_first() {
    // The defect this section exists for: `choose_display` returned one
    // `Chosen` and stopped, so a machine with two monitors lit one and left the
    // other dark — while the compositor above happily composited windows onto
    // it, because `DisplayManager` had been told about both.
    let card = FakeCard::two_monitors();
    let scanout = DrmScanout::new(card.clone()).expect("both monitors can be driven");

    assert_eq!(scanout.heads().len(), 2, "one head per connected monitor");
    card.read(|s| {
        assert_eq!(s.dumb.len(), 4, "two buffers each");
        assert_eq!(s.fbs.len(), 4);
        assert_eq!(s.flips.len(), 2, "and both are showing something already");
    });
}

#[test]
fn the_composited_frame_spans_every_monitor_rather_than_the_first_one() {
    // `size()` is what the compositor is built at. If it were one monitor's
    // mode, every window past that monitor's right edge would be composited
    // outside the frame and thrown away — which is exactly the bug §514 fixed
    // on the model side, arriving from the other direction.
    let scanout = DrmScanout::new(FakeCard::two_monitors()).unwrap();
    assert_eq!(
        scanout.size(),
        (1024 + 1366, 768),
        "the bounding box of both monitors, not either one"
    );
}

#[test]
fn heads_are_reported_left_to_right_so_the_compositor_can_place_them() {
    // The compositor never learns the layout from us directly: it calls
    // `attach_display` per head, and `DisplayManager::add_display` places each
    // new monitor to the right of the ones already there. That is the same rule
    // `lay_out_heads` uses, so the two agree by construction — but only while
    // this order and these offsets hold, which is what is pinned here.
    let scanout = DrmScanout::new(FakeCard::two_monitors()).unwrap();
    assert_eq!(
        scanout.heads(),
        vec![
            HeadInfo {
                connector_id: 30,
                crtc_id: 1,
                x: 0,
                y: 0,
                width: 1024,
                height: 768,
                refresh_hz: 60,
            },
            // Abutting the first, not overlapping it and not on top of it.
            HeadInfo {
                connector_id: 31,
                crtc_id: 2,
                x: 1024,
                y: 0,
                width: 1366,
                height: 768,
                refresh_hz: 60,
            },
        ]
    );
}

// ------------------------------------------------- a cable that moves later --

/// Probe on every call, which is what a test wants and a frame loop does not.
fn eager<S: KmsSys>(scanout: &mut DrmScanout<S>) {
    scanout.set_probe_interval(Duration::ZERO);
}

/// The monitor ids a scanout reports, in order.
fn reported<S: KmsSys>(scanout: &mut DrmScanout<S>) -> Vec<u32> {
    scanout
        .monitors()
        .expect("a card always has an opinion about its own connectors")
        .iter()
        .map(|m| m.id)
        .collect()
}

#[test]
fn a_monitor_plugged_in_after_startup_becomes_a_head() {
    // The bug: `new` enumerated the connectors once, so a screen plugged in
    // afterwards stayed dark until the display server was restarted. Nothing
    // asked the card a second question for the rest of the session.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    eager(&mut scanout);
    assert_eq!(
        reported(&mut scanout),
        vec![31],
        "the one plugged in at boot"
    );

    card.edit(|s| {
        s.connectors[0].connection = uapi::CONNECTED;
        s.connectors[0].modes = vec![mode(1024, 768, true)];
    });

    assert_eq!(
        scanout.monitors().unwrap(),
        vec![
            MonitorInfo {
                id: 31,
                width: 1366,
                height: 768,
                refresh_hz: 60,
            },
            MonitorInfo {
                id: 30,
                width: 1024,
                height: 768,
                refresh_hz: 60,
            },
        ],
        "reported by connector id, which is the key the compositor reconciles \
         on -- an index would name a different monitor after the first unplug"
    );
    assert_eq!(
        scanout.heads()[1].x,
        1366,
        "the arriving monitor goes to the right of the ones already there, \
         which is the rule `DisplayManager::add_display` uses -- any other \
         offset puts every window on it in the wrong place"
    );
    assert_eq!(
        scanout.size(),
        (1366 + 1024, 768),
        "and the frame reaches it"
    );
}

#[test]
fn a_monitor_unplugged_stops_being_a_head_and_gives_its_buffers_back() {
    // Not merely marked dead, as a flip failure does. A probe runs every second
    // for the life of the session, and a cable working loose in its socket
    // would otherwise grow the head list -- and the card's buffer count -- once
    // per unplug for hours.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    eager(&mut scanout);
    let (fbs, dumbs) = card.read(|s| (s.fbs.len(), s.dumb.len()));
    assert_eq!((fbs, dumbs), (4, 4), "two buffers each");

    card.edit(|s| s.connectors[1].connection = 2);
    assert_eq!(
        reported(&mut scanout),
        vec![30],
        "31 is still on the desktop"
    );
    card.read(|s| {
        assert_eq!(s.removed.len(), 2, "its framebuffer ids went back");
        assert_eq!(s.destroyed.len(), 2, "and its GEM handles");
    });
    assert!(scanout.is_open(), "the other monitor is still there");
}

#[test]
fn the_survivor_of_an_unplug_keeps_the_offset_it_had() {
    // The scanout is the authority on where each monitor is: the compositor
    // does not re-flow its displays when one leaves (design-decisions.md §515,
    // §516) and the two layouts have to agree pixel for pixel. Sliding the
    // survivor left here -- the tidier arrangement, and the one `lay_out_heads`
    // would produce if it were called again -- would draw every window on it
    // 1024 pixels from where the compositor thinks it is.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    eager(&mut scanout);
    assert_eq!(scanout.heads()[1].x, 1024);

    card.edit(|s| s.connectors[0].connection = 2);
    assert_eq!(reported(&mut scanout), vec![31]);
    assert_eq!(
        scanout.heads()[0].x,
        1024,
        "the survivor was re-flowed leftwards"
    );
    assert_eq!(
        scanout.size(),
        (1024 + 1366, 768),
        "and the frame still spans the hole the departed monitor left, which is \
         composited and scanned out nowhere"
    );
}

#[test]
fn a_monitor_moved_to_another_port_gets_the_crtc_the_old_one_gave_back() {
    // Why retirements run before adoptions. This machine has two CRTCs and the
    // second is the only one either of these connectors can reach, so a cable
    // moved from port 31 to port 32 needs 31's CRTC released before 32 can be
    // lit. Adopting first leaves the new port dark until the *next* probe --
    // or for ever, on a card where the old head never fully retires.
    let card = FakeCard::desktop();
    card.edit(|s| {
        s.connectors.push(FakeConnector {
            id: 32,
            connection: 2,
            current_encoder: 0,
            encoders: vec![52],
            modes: Vec::new(),
        });
        s.encoders.push(FakeEncoder {
            id: 52,
            crtc_id: 0,
            possible_crtcs: 0b10,
        });
    });
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    eager(&mut scanout);
    assert_eq!(scanout.heads()[0].crtc_id, 2);

    card.edit(|s| {
        s.connectors[1].connection = 2;
        s.connectors[2].connection = uapi::CONNECTED;
        s.connectors[2].modes = vec![mode(1280, 1024, true)];
    });

    assert_eq!(
        reported(&mut scanout),
        vec![32],
        "the cable's new port is lit"
    );
    assert_eq!(
        scanout.heads()[0].crtc_id,
        2,
        "on the CRTC the old port gave back"
    );
}

#[test]
fn a_monitor_whose_first_frame_never_reaches_it_is_not_adopted() {
    // Same rule as `new`: a head that cannot show a frame is not part of the
    // desktop. Leaving it in would reserve a strip of the composited frame that
    // nothing scans out and put windows on a monitor that is not there.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    eager(&mut scanout);
    let destroyed = card.read(|s| s.destroyed.len());
    card.edit(|s| {
        s.connectors[0].connection = uapi::CONNECTED;
        s.connectors[0].modes = vec![mode(1024, 768, true)];
        // The zeroth flip was the surviving head's first frame, in `new`.
        s.fail_nth.push((uapi::PAGE_FLIP, 1, ENODEV));
    });

    assert_eq!(
        reported(&mut scanout),
        vec![31],
        "and not the one that failed"
    );
    assert_eq!(scanout.size(), (1366, 768), "the frame did not grow for it");
    assert_eq!(
        card.read(|s| s.destroyed.len()),
        destroyed + 2,
        "its buffers went straight back rather than waiting for `Drop`"
    );
}

#[test]
fn the_probe_is_rate_limited_so_a_frame_does_not_pay_for_it() {
    // `GETCONNECTOR` with a zero mode count makes the kernel re-probe the
    // connector, which is DDC traffic to the monitor and tens of milliseconds
    // per head on real hardware. Asking once a frame would spend most of a
    // 144Hz budget on a question whose answer changes about once a week.
    let card = FakeCard::desktop();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let calls = card.read(|s| s.log.len());

    card.edit(|s| {
        s.connectors[0].connection = uapi::CONNECTED;
        s.connectors[0].modes = vec![mode(1024, 768, true)];
    });
    assert_eq!(
        reported(&mut scanout),
        vec![31],
        "the default interval has not elapsed, so the answer is the cached one"
    );
    assert_eq!(
        card.read(|s| s.log.len()),
        calls,
        "and it cost the card nothing at all"
    );

    eager(&mut scanout);
    assert_eq!(reported(&mut scanout), vec![31, 30], "once it is due");
}

#[test]
fn a_probe_the_card_refuses_leaves_the_arrangement_exactly_as_it_was() {
    // A poll, not a transaction: the next one is a second away, so a card that
    // will not answer this time is a reason to do nothing rather than a reason
    // to tear the desktop down.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    eager(&mut scanout);
    card.edit(|s| s.fail.push((uapi::GETRESOURCES, ENODEV)));

    assert_eq!(
        reported(&mut scanout),
        vec![30, 31],
        "both monitors survived"
    );
    assert_eq!(scanout.size(), (1024 + 1366, 768));
}

#[test]
fn each_head_flips_its_own_crtc_and_its_own_framebuffer() {
    // Two heads flipping the same CRTC would mean the second monitor's picture
    // landing on the first, alternating every frame. Two heads flipping the
    // same *framebuffer* would mean both monitors showing one monitor's half of
    // the desktop.
    let card = FakeCard::two_monitors();
    let _scanout = DrmScanout::new(card.clone()).unwrap();
    card.read(|s| {
        assert_eq!(
            s.flips,
            vec![(1, 1), (2, 3)],
            "CRTC 1 got head 0's first buffer and CRTC 2 got head 1's"
        );
    });
}

#[test]
fn a_second_monitor_scans_out_its_own_part_of_the_frame() {
    // The whole point of one desktop-sized frame: each head copies out the
    // rectangle it occupies. A head that blits from the frame's origin shows
    // the *first* monitor's picture on the second one — a mirror rather than an
    // extension, and one the compositor has no idea it is producing.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();

    // Left monitor's columns red, right monitor's blue.
    let mut frame = vec![0xFFFF_0000_u32; (w * h) as usize];
    for row in 0..h as usize {
        for col in 1024..w as usize {
            frame[row * w as usize + col] = 0xFF00_00FF;
        }
    }
    scanout.show(&frame, w, h);

    let left = u32::from_le_bytes(fixed(&scanout.scanned_out_for(30)[..4]));
    assert_eq!(
        left, 0xFFFF_0000,
        "the first monitor shows the left columns"
    );

    let right = u32::from_le_bytes(fixed(&scanout.scanned_out_for(31)[..4]));
    assert_eq!(
        right, 0xFF00_00FF,
        "the second monitor shows the columns at its own offset, not the frame's"
    );
}

#[test]
fn a_second_monitors_last_row_is_reached_through_its_own_padded_pitch() {
    // 1366 * 4 is 5464 and the card pads rows to 5504. A `show` that steps the
    // destination by the head's *width* instead of its pitch, or by the first
    // head's pitch, drifts a little further left on every row — the classic
    // skew, and it only appears on the head whose pitch is padded.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card).unwrap();
    let (w, h) = scanout.size();
    let frame = vec![0xFF12_3456_u32; (w * h) as usize];
    scanout.show(&frame, w, h);

    let pitch = scanout.pitch_for(31) as usize;
    assert_eq!(pitch, 5504, "the fake pads to 64 bytes, as real drivers do");
    let bytes = scanout.scanned_out_for(31);
    let last = (h as usize - 1) * pitch;
    assert_eq!(
        u32::from_le_bytes(fixed(&bytes[last..last + 4])),
        0xFF12_3456,
        "the bottom-left pixel of the second monitor"
    );
}

#[test]
fn two_connectors_that_can_only_share_one_crtc_yield_one_head() {
    // A CRTC scans out one framebuffer at a time. Handing the same one to both
    // connectors would not light the second monitor; it would take the first
    // monitor's picture away and replace it with the second's. Declining the
    // head we cannot drive leaves one working monitor instead of two broken
    // ones.
    let card = FakeCard::two_monitors();
    card.edit(|s| s.encoders[1].possible_crtcs = 0b01);
    let scanout = DrmScanout::new(card.clone()).unwrap();

    assert_eq!(scanout.heads().len(), 1);
    assert_eq!(scanout.crtc_id(), 1, "claimed by the first connector");
    assert_eq!(scanout.connector_id(), 30);
    assert_eq!(
        scanout.size(),
        (1024, 768),
        "and the desktop is that monitor"
    );
    card.read(|s| {
        assert_eq!(
            s.fbs.len(),
            2,
            "no buffers built for a head we cannot drive"
        );
    });
}

#[test]
fn the_crtc_a_connector_is_already_bound_to_is_still_not_taken_twice() {
    // The boot-bound preference in `resolve_crtc` returns early, so it is the
    // path most likely to hand out a CRTC that is already spoken for: the
    // firmware can leave two connectors routed through one CRTC.
    let card = FakeCard::two_monitors();
    card.edit(|s| {
        s.connectors[0].current_encoder = 50;
        s.encoders[0].crtc_id = 2;
        s.connectors[1].current_encoder = 51;
        s.encoders[1].crtc_id = 2;
        // Both bindings have to be ones the bitmask agrees with, or they are
        // not believed at all and the double-claim never happens. Both
        // connectors can in fact reach either CRTC, so there is somewhere for
        // the second to go once the first has claimed the bound one.
        s.encoders[0].possible_crtcs = 0b11;
        s.encoders[1].possible_crtcs = 0b11;
    });
    let scanout = DrmScanout::new(card).unwrap();
    let heads = scanout.heads();
    assert_eq!(heads.len(), 2, "the second fell back to its bitmask");
    assert_eq!(heads[0].crtc_id, 2, "the one it was already bound to");
    assert_eq!(
        heads[1].crtc_id, 1,
        "and the second took the other one rather than the same: {heads:?}"
    );
}

#[test]
fn a_monitor_that_dies_mid_session_does_not_take_the_others_with_it() {
    // An unplugged DisplayPort cable makes one CRTC's flips fail forever. The
    // single-head code closed the display on the first real error, which as
    // written would have blanked a working monitor because a different one went
    // away.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, ENODEV)));
    scanout.show(&vec![0xFF00_FF00_u32; (w * h) as usize], w, h);

    assert!(
        scanout.is_open(),
        "one monitor going away is not the display server's cue to exit"
    );
    assert_eq!(
        scanout.heads().len(),
        1,
        "and the dead one is no longer ours"
    );

    // The survivor keeps taking frames, and the dead head is not flipped again.
    let before = card.read(|s| s.flips.len());
    scanout.show(&vec![0xFF00_00FF_u32; (w * h) as usize], w, h);
    card.read(|s| {
        assert_eq!(
            s.flips.len(),
            before + 1,
            "exactly one head flipped: the live one"
        );
        assert_eq!(s.flips.last().unwrap().0, 2, "CRTC 2, the survivor");
    });
}

#[test]
fn the_single_head_accessors_name_a_monitor_that_is_still_there() {
    // `crtc_id`, `connector_id`, `pitch` and `scanned_out` are the "there is
    // one screen" accessors the diagnostics and the single-head tests use. A
    // dead head stays in the vector so `Drop` can give its buffers back, so
    // reading position zero answers with the monitor that just went away —
    // reporting the pitch of a buffer nothing scans out, and naming a CRTC that
    // is no longer being flipped.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    // Kill head 0 — connector 30 on CRTC 1, the one `first()` would find.
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, ENODEV)));
    scanout.show(&vec![0xFF44_5566_u32; (w * h) as usize], w, h);

    assert_eq!(scanout.connector_id(), 31, "the survivor, not the corpse");
    assert_eq!(scanout.crtc_id(), 2, "and the CRTC still being flipped");
    assert_eq!(
        scanout.pitch(),
        5504,
        "the survivor's padded pitch, not the dead head's 4096"
    );
    assert_eq!(
        u32::from_le_bytes(fixed(&scanout.scanned_out()[..4])),
        0xFF44_5566,
        "and the bytes read back are the ones a monitor is showing"
    );
}

#[test]
fn a_dead_heads_buffers_are_not_reachable_through_its_connector() {
    // The other half of the same rule: asking about a monitor that has died
    // gets nothing rather than its stale last frame. A caller that kept a
    // `HeadInfo` from before the failure must not be able to read a buffer no
    // CRTC is scanning out and conclude it is on screen.
    let card = FakeCard::two_monitors();
    let mut scanout = DrmScanout::new(card.clone()).unwrap();
    let (w, h) = scanout.size();
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, ENODEV)));
    scanout.show(&vec![0xFF44_5566_u32; (w * h) as usize], w, h);

    assert_eq!(scanout.pitch_for(30), 0, "connector 30 is gone");
    assert!(scanout.scanned_out_for(30).is_empty());
    assert_eq!(scanout.pitch_for(99), 0, "and so is one that never existed");
    assert!(scanout.scanned_out_for(99).is_empty());
}

#[test]
fn a_head_that_never_showed_its_first_frame_still_gives_its_buffers_back() {
    // A head excluded from the layout is still holding two dumb buffers and two
    // framebuffer ids. Dropping it from `self.heads` on failure would leak them
    // for the life of the process, which on a card shared with a second scanout
    // is a real leak rather than a bookkeeping one.
    let card = FakeCard::two_monitors();
    card.edit(|s| s.fail.push((uapi::PAGE_FLIP, ENODEV)));
    {
        let scanout = DrmScanout::new(card.clone()).expect("the other monitor still works");
        assert_eq!(scanout.heads().len(), 1);
        assert_eq!(
            scanout.size(),
            (1366, 768),
            "the dead head takes no space in the desktop"
        );
        assert_eq!(
            scanout.heads()[0].x,
            0,
            "and the survivor moved left to fill the gap"
        );
    }
    card.read(|s| {
        assert_eq!(
            s.removed.len(),
            4,
            "all four framebuffer ids: {:?}",
            s.removed
        );
        assert_eq!(s.destroyed.len(), 4, "all four GEM handles");
        assert!(s.fbs.is_empty() && s.dumb.is_empty());
    });
}

#[test]
fn every_head_gives_back_its_framebuffers_when_scanout_is_dropped() {
    let card = FakeCard::two_monitors();
    {
        let _scanout = DrmScanout::new(card.clone()).unwrap();
        card.read(|s| assert_eq!((s.fbs.len(), s.dumb.len()), (4, 4)));
    }
    card.read(|s| {
        assert_eq!(s.removed, vec![1, 2, 3, 4]);
        assert_eq!(s.destroyed, vec![1, 2, 3, 4]);
        assert!(s.fbs.is_empty() && s.dumb.is_empty());
    });
}

#[test]
fn a_monitor_whose_buffers_will_not_allocate_does_not_blank_the_one_next_to_it() {
    // A card can run out of scanout-capable memory partway through, and the
    // second monitor is the one that finds out. Failing the whole setup would
    // mean the *first* monitor — whose buffers allocated fine — shows nothing
    // either, which is the same wrong answer that sharing a CRTC gives.
    let card = FakeCard::two_monitors();
    // The third `CREATE_DUMB` is the second head's first buffer: the first
    // head's pair has to succeed before this bites, which is the whole point.
    card.edit(|s| s.fail_nth.push((uapi::CREATE_DUMB, 2, ENODEV)));

    let scanout = DrmScanout::new(card.clone()).expect("the first monitor is still usable");
    assert_eq!(scanout.heads().len(), 1, "just the one that allocated");
    assert_eq!(scanout.connector_id(), 30, "the first connector");
    assert_eq!(
        scanout.size(),
        (1024, 768),
        "and the desktop is sized as if the other were not plugged in"
    );
    assert!(scanout.is_open());
    card.read(|s| assert_eq!((s.fbs.len(), s.dumb.len()), (2, 2)));
}

#[test]
fn a_head_that_got_only_one_of_its_two_buffers_gives_that_one_back() {
    // A head is all-or-nothing: it cannot flip with one buffer. So the second
    // allocation's failure path has to hand the first buffer back *here* —
    // nothing owns it yet, since the head never reaches `DrmScanout::heads`,
    // and this type does not own the card, so a leaked id is a real leak.
    let card = FakeCard::two_monitors();
    // The fourth `CREATE_DUMB`: the second head's *second* buffer, after its
    // first one already succeeded and took fb id 3.
    card.edit(|s| s.fail_nth.push((uapi::CREATE_DUMB, 3, ENODEV)));
    {
        let scanout = DrmScanout::new(card.clone()).expect("the first monitor is still usable");
        assert_eq!(scanout.heads().len(), 1);
        card.read(|s| {
            assert_eq!(
                s.removed,
                vec![3],
                "the orphan was released while the scanout was still alive, \
                 which is the only moment anything knows about it"
            );
            assert_eq!(s.destroyed, vec![3]);
            assert_eq!(
                (s.fbs.len(), s.dumb.len()),
                (2, 2),
                "leaving exactly the surviving head's pair"
            );
        });
    }
    card.read(|s| {
        assert_eq!(s.removed, vec![3, 1, 2], "then the survivor's on drop");
        assert_eq!(s.destroyed, vec![3, 1, 2]);
        assert!(s.fbs.is_empty() && s.dumb.is_empty());
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

/// A viewport onto the top-left of the frame: what every single-head blit is.
fn view(pitch: u32, width: u32, height: u32) -> Viewport {
    Viewport {
        pitch,
        width,
        height,
        src_x: 0,
        src_y: 0,
    }
}

#[test]
fn blit_writes_nothing_when_the_pitch_is_zero() {
    let mut dst = vec![0xEEu8; 64];
    blit(&mut dst, &view(0, 4, 4), &[0xFFFF_FFFF; 16], 4, 4);
    assert!(dst.iter().all(|&b| b == 0xEE));
}

#[test]
fn blit_stops_at_the_end_of_the_destination_rather_than_wrapping() {
    // A destination one row shorter than the source claims: the last row must
    // be dropped, not folded back onto the first.
    let mut dst = vec![0u8; 16]; // one row of 4 pixels
    blit(&mut dst, &view(16, 4, 2), &[1, 2, 3, 4, 9, 9, 9, 9], 4, 2);
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
    blit(&mut dst, &view(8, 2, 2), &src, 3, 2);
    assert_eq!(u32::from_le_bytes(fixed(&dst[..4])), 10);
    assert_eq!(u32::from_le_bytes(fixed(&dst[4..8])), 11);
    assert_eq!(u32::from_le_bytes(fixed(&dst[8..12])), 20, "not 12");
    assert_eq!(u32::from_le_bytes(fixed(&dst[12..16])), 21);
}

#[test]
fn blit_reads_from_the_viewports_origin_and_not_the_frames() {
    // What makes a head a *viewport*. Ignoring `src_x`/`src_y` copies the
    // top-left of the desktop to every monitor — the picture is not corrupt,
    // which is what makes it hard to spot: it is a mirror where an extension
    // was asked for.
    let mut dst = vec![0u8; 2 * 8];
    // A 4x3 frame; the viewport is the 2x2 block at (2, 1).
    let src: Vec<u32> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let port = Viewport {
        pitch: 8,
        width: 2,
        height: 2,
        src_x: 2,
        src_y: 1,
    };
    blit(&mut dst, &port, &src, 4, 3);
    assert_eq!(u32::from_le_bytes(fixed(&dst[..4])), 7);
    assert_eq!(u32::from_le_bytes(fixed(&dst[4..8])), 8);
    assert_eq!(u32::from_le_bytes(fixed(&dst[8..12])), 11);
    assert_eq!(u32::from_le_bytes(fixed(&dst[12..16])), 12);
}

#[test]
fn a_viewport_starting_past_the_end_of_the_frame_copies_nothing() {
    // The frame the compositor hands us can be smaller than the monitors span —
    // during a resize, or if `attach_display` failed and the desktop never grew.
    // Subtracting an offset larger than the width must clamp to zero rather than
    // wrap into a copy width of four billion.
    let mut dst = vec![0xEEu8; 32];
    let port = Viewport {
        pitch: 8,
        width: 2,
        height: 2,
        src_x: 10,
        src_y: 0,
    };
    blit(&mut dst, &port, &[1, 2, 3, 4, 5, 6, 7, 8], 4, 2);
    assert!(dst.iter().all(|&b| b == 0xEE), "nothing was written");

    // Same for a viewport below the bottom of the frame.
    let port = Viewport {
        pitch: 8,
        width: 2,
        height: 2,
        src_x: 0,
        src_y: 9,
    };
    blit(&mut dst, &port, &[1, 2, 3, 4, 5, 6, 7, 8], 4, 2);
    assert!(dst.iter().all(|&b| b == 0xEE));
}

#[test]
fn a_viewport_wider_than_what_is_left_of_the_frame_copies_what_there_is() {
    // A monitor hanging off the right edge of a too-small desktop: the part of
    // it that has pixels gets them, and the rest keeps whatever was there —
    // rather than the row copy running off the end of the frame.
    let mut dst = vec![0u8; 2 * 16];
    let port = Viewport {
        pitch: 16,
        width: 4,
        height: 2,
        src_x: 2,
        src_y: 0,
    };
    // A 3-wide frame, so only one column lies inside the viewport.
    blit(&mut dst, &port, &[1, 2, 3, 4, 5, 6], 3, 2);
    assert_eq!(u32::from_le_bytes(fixed(&dst[..4])), 3);
    assert_eq!(u32::from_le_bytes(fixed(&dst[4..8])), 0, "and no more");
    assert_eq!(u32::from_le_bytes(fixed(&dst[16..20])), 6, "the second row");
}
