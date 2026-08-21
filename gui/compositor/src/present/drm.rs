//! Scanout on SlateOS: putting the composited frame on a real screen.
//!
//! This is the [`Present`](super::Present) implementation the rest of this
//! crate was built for. Everything else that implements the trait is either a
//! deliberate no-op ([`Headless`](super::Headless)), a test recorder, or a
//! development harness on the machine the tree happens to be compiled on
//! ([`host`](super::host)). This module is the one that draws on the machine
//! SlateOS is *running* on.
//!
//! ## The pipeline
//!
//! SlateOS's kernel exposes a Linux-compatible DRM/KMS character device at
//! `/dev/dri/card0`. Getting a pixel from that device to a monitor is a fixed
//! sequence, and every step of it exists to answer a question the next step
//! asks:
//!
//! | Step | Question it answers |
//! |---|---|
//! | `GETRESOURCES` | which CRTCs and connectors does this card have? |
//! | `GETCONNECTOR` | is anything plugged into this one, and what modes does it do? |
//! | `GETENCODER` | which of those CRTCs can drive this connector? |
//! | `CREATE_DUMB` | give me a chunk of scanout-capable memory |
//! | `MAP_DUMB` + `mmap` | …and let me write to it from userspace |
//! | `ADDFB2` | treat that memory as a framebuffer of this size and format |
//! | `PAGE_FLIP` | scan it out |
//!
//! `SETCRTC` — the ioctl that would let us pick a mode other than the one the
//! card came up in — is deliberately absent, because the SlateOS kernel does
//! not implement it. That is not a gap in *this* module: the kernel's
//! `DrmDevice::page_flip` validates only that the CRTC id, the framebuffer id
//! and the framebuffer's backing object exist, and the ATI backend performs its
//! own modeset inside the flip when the framebuffer's dimensions differ from
//! the current mode. Driving the boot/native resolution — which is what a
//! compositor wants, and what [`DrmScanout::size`] reports — needs no `SETCRTC`
//! at all. See `known-issues.md` → `TD-COMPOSITOR-CANNOT-CHANGE-MODE`.
//!
//! ## Why this is split in three
//!
//! Every bug that will ever be in this module is a *protocol* bug: a field at
//! the wrong byte offset, a connector walked to the wrong encoder, a row copied
//! at `y * width` when the driver said `y * pitch`. None of those need a
//! graphics card to find — but all of them are invisible if the whole module is
//! behind `#[cfg(target_os = "linux")]`, because the machine this tree is
//! compiled and tested on is not Linux.
//!
//! So: [`uapi`] is the wire format and nothing else — no file descriptors, no
//! `unsafe`, compiled and tested everywhere. [`sys`] is the ~50 lines of
//! syscall mechanism that genuinely cannot run off the target, behind a trait.
//! And this file, which holds all of the decisions, is generic over that trait
//! and is driven in tests by a fake card that models a two-CRTC machine with a
//! disconnected head, a padded pitch and an encoder that can only drive the
//! second CRTC.

pub mod sys;
pub mod uapi;

use super::Present;
use sys::{EAGAIN, EBUSY, EINTR, Errno, KmsSys, Mapped, OutArray};
use uapi::{
    ModeCardRes, ModeCreateDumb, ModeCrtcPageFlip, ModeDestroyDumb, ModeFbCmd2, ModeGetConnector,
    ModeGetEncoder, ModeMapDumb, ModeModeinfo,
};

/// Most CRTCs, connectors or encoders we will enumerate from one card.
///
/// The counts come back from the kernel and are used to size allocations, so
/// they are attacker-adjacent in exactly the way a length field always is: a
/// driver bug that reports four billion connectors must not turn into a
/// four-billion-element `Vec`. No real card has more than a handful.
const MAX_OBJECTS: usize = 64;

/// Most modes we will read from one connector. A monitor's EDID lists tens.
const MAX_MODES: usize = 256;

/// Bytes per pixel in [`uapi::FORMAT_XRGB8888`].
const BYTES_PER_PIXEL: usize = 4;

/// Why scanout could not be set up, or why it stopped working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanoutError {
    /// An ioctl failed. The request number identifies which step.
    Ioctl {
        /// The `DRM_IOCTL_MODE_*` request number, as in [`uapi`].
        request: u32,
        /// The kernel's `errno`.
        errno: Errno,
    },
    /// The card has no connector with a display attached — every head is
    /// unplugged, or the card is a render node with no outputs at all.
    NoConnectedDisplay,
    /// A display is attached but reported no modes, or none of its connectors
    /// could be routed to a CRTC. Both mean "there is a screen and we cannot
    /// drive it", which is one situation from the caller's point of view.
    NoUsableMode,
    /// The driver returned a dumb buffer that does not describe a real
    /// allocation — a zero handle, a zero pitch, or a size smaller than
    /// `pitch * height`. Refusing to map it is the point: this is the check
    /// that stands between a driver bug and a wild write across scanout memory.
    BadBuffer,
    /// The buffer could not be mapped into this process.
    Map(Errno),
    /// The card's device node could not be opened at all — `ENOENT` on a
    /// machine with no graphics driver loaded, `EACCES` when the compositor is
    /// not permitted to open it.
    Open(Errno),
}

impl core::fmt::Display for ScanoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Ioctl { request, errno } => {
                write!(f, "DRM ioctl {request:#010x} failed with errno {errno}")
            }
            Self::NoConnectedDisplay => f.write_str("no display is connected to this card"),
            Self::NoUsableMode => {
                f.write_str("a display is connected but offers no mode this card can drive")
            }
            Self::BadBuffer => f.write_str("the driver returned an unusable dumb buffer"),
            Self::Map(errno) => write!(f, "mapping the scanout buffer failed with errno {errno}"),
            Self::Open(errno) => write!(f, "opening the graphics device failed with errno {errno}"),
        }
    }
}

impl std::error::Error for ScanoutError {}

#[cfg(target_os = "linux")]
impl DrmScanout<sys::Card> {
    /// Open the machine's first graphics card and set up scanout on it.
    ///
    /// The one entry point a caller outside this module needs, and the reason
    /// [`sys::Card`] does not have to be nameable from there.
    ///
    /// # Errors
    ///
    /// [`ScanoutError`]. Every variant means "fall back to headless", not
    /// "abort": a display server with no display still serves remote clients.
    pub fn card0() -> Result<Self, ScanoutError> {
        let card = sys::Card::open(sys::CARD0).map_err(ScanoutError::Open)?;
        Self::new(card)
    }
}

/// A dumb buffer, mapped, and registered as a framebuffer.
struct Framebuffer {
    /// The GEM handle `CREATE_DUMB` returned. Freed by `DESTROY_DUMB`.
    handle: u32,
    /// The framebuffer id `ADDFB2` returned. Freed by `RMFB`.
    fb_id: u32,
    /// Bytes per row, as the *driver* computed it. Not `width * 4`: dumb
    /// buffers are pitch-aligned (64 bytes on this kernel), so a 1366-pixel
    /// row is 5472 bytes wide and occupies 5504.
    pitch: u32,
    /// The mapping. Dropping it unmaps.
    map: Box<dyn Mapped>,
}

/// A display, driven directly.
///
/// The `S` is owned rather than borrowed because the framebuffer ids and GEM
/// handles this holds are per-file-descriptor kernel state: they die with the
/// card, so the thing that must give them back has to outlive nothing else.
///
/// Generic over [`KmsSys`] so that the whole of the protocol above — which is
/// all of the interesting code — is exercised on the build machine against a
/// fake, and only the four system calls underneath it are target-only.
pub struct DrmScanout<S: KmsSys> {
    /// The card.
    sys: S,
    /// The CRTC being driven.
    crtc_id: u32,
    /// The connector being driven. Kept for diagnostics and for the reconnect
    /// logic a later change will want; nothing reads it per frame.
    connector_id: u32,
    /// The two buffers, flipped between.
    buffers: [Framebuffer; 2],
    /// Index into [`Self::buffers`] of the one currently being scanned out.
    /// The *other* one is the one [`Present::show`] draws into.
    front: usize,
    /// The mode's width in pixels.
    width: u32,
    /// The mode's height in pixels.
    height: u32,
    /// Cleared when a flip fails for a reason that is not "try again", which
    /// is what the display going away looks like from here.
    alive: bool,
}

impl<S: KmsSys> DrmScanout<S> {
    /// Set up scanout on an already-open card.
    ///
    /// Picks the first connected connector that has a mode and can be routed
    /// to a CRTC, prefers that connector's `DRM_MODE_TYPE_PREFERRED` (native)
    /// mode, allocates two buffers at that size and shows the first.
    ///
    /// # Errors
    ///
    /// [`ScanoutError`] — see its variants; every one of them means the
    /// caller should fall back to [`Headless`](super::Headless) rather than
    /// abort, because a display server with no display is a working display
    /// server for remote clients.
    pub fn new(mut sys: S) -> Result<Self, ScanoutError> {
        let (crtcs, connectors) = resources(&mut sys)?;
        let chosen = choose_display(&mut sys, &crtcs, &connectors)?;
        let (width, height) = chosen.mode.size();
        let first = make_buffer(&mut sys, width, height)?;
        let second = make_buffer(&mut sys, width, height)?;
        let mut out = Self {
            sys,
            crtc_id: chosen.crtc_id,
            connector_id: chosen.connector_id,
            buffers: [first, second],
            // `show` draws into `1 - front`, so starting at 1 means the first
            // frame lands in buffer 0 and is flipped to. Starting at 0 would
            // draw the first frame into the buffer that is *already* on screen.
            front: 1,
            width,
            height,
            alive: true,
        };
        // Put something defined on the screen straight away. Without this the
        // display scans out whatever the boot framebuffer left, until the
        // compositor has a reason to compose a frame — which on an idle
        // desktop can be a while.
        out.flip()?;
        Ok(out)
    }

    /// The display's size in pixels: the mode that was selected.
    ///
    /// The compositor is built at this size rather than the other way round —
    /// there is no `SETCRTC` to make the display match a size we picked, and
    /// even if there were, the native mode is the one that looks right.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The CRTC being driven, for diagnostics.
    #[must_use]
    pub const fn crtc_id(&self) -> u32 {
        self.crtc_id
    }

    /// The connector being driven, for diagnostics.
    #[must_use]
    pub const fn connector_id(&self) -> u32 {
        self.connector_id
    }

    /// Bytes per row of the scanout buffers.
    #[must_use]
    pub fn pitch(&self) -> u32 {
        self.buffers.first().map_or(0, |b| b.pitch)
    }

    /// The bytes currently on screen — the buffer the last successful flip
    /// selected.
    ///
    /// Exposed because "what reached the display" is a different claim from
    /// "what the compositor drew", and it is the first one that is worth
    /// asserting about. Returns an empty slice only if the buffer array is
    /// somehow malformed, which it cannot be.
    pub fn scanned_out(&mut self) -> &[u8] {
        self.buffers
            .get_mut(self.front)
            .map_or(&[], |b| b.map.bytes())
    }

    /// Show the buffer that is not currently on screen, and make it current.
    fn flip(&mut self) -> Result<(), ScanoutError> {
        let back = 1usize.saturating_sub(self.front);
        let Some(fb_id) = self.buffers.get(back).map(|b| b.fb_id) else {
            return Ok(());
        };
        let flip = ModeCrtcPageFlip {
            crtc_id: self.crtc_id,
            fb_id,
            // No `DRM_MODE_PAGE_FLIP_EVENT`: the compositor has no event loop
            // reading the card fd, and this kernel's backends retire a flip
            // before the ioctl returns, so an event would be a message to
            // nobody about something that already happened.
            flags: 0,
            reserved: 0,
            user_data: 0,
        };
        call(&mut self.sys, uapi::PAGE_FLIP, flip.to_bytes(), &mut [])?;
        self.front = back;
        Ok(())
    }
}

impl<S: KmsSys> Present for DrmScanout<S> {
    fn show(&mut self, pixels: &[u32], width: u32, height: u32) {
        if !self.alive {
            return;
        }
        let back = 1usize.saturating_sub(self.front);
        let (dst_pitch, screen_w, screen_h) = (self.pitch(), self.width, self.height);
        if let Some(buffer) = self.buffers.get_mut(back) {
            blit(
                buffer.map.bytes(),
                dst_pitch,
                screen_w,
                screen_h,
                pixels,
                width,
                height,
            );
        }
        match self.flip() {
            Ok(()) => {}
            // A flip that is merely early is not a failure: `EBUSY` means the
            // previous flip has not retired, `EAGAIN` that the driver is not
            // ready, `EINTR` that a signal arrived. The frame is dropped and
            // the next one goes to the same back buffer — which is why the
            // front index is only advanced on success. Dropping a frame under
            // load is what every compositor does; tearing down the display
            // because a monitor was slow is not.
            Err(ScanoutError::Ioctl { errno, .. })
                if errno == EBUSY || errno == EAGAIN || errno == EINTR => {}
            Err(_) => self.alive = false,
        }
    }

    fn is_open(&self) -> bool {
        self.alive
    }
}

impl<S: KmsSys> Drop for DrmScanout<S> {
    fn drop(&mut self) {
        // Give the ids back explicitly rather than relying on the fd close.
        // Closing the card does release everything, but this type does not own
        // the card — a caller could keep the `S` alive, or share one card
        // between two scanouts on a two-headed machine — so leaking a
        // framebuffer id here would be a leak for as long as the process runs.
        //
        // The mappings are still live at this point (they are fields, and
        // fields drop after `Drop::drop`). That is correct and not an
        // oversight: a GEM object is reference-counted against its mappings,
        // so `DESTROY_DUMB` on a mapped buffer detaches the handle and frees
        // the memory when the last mapping goes, which is a few instructions
        // from now.
        for i in 0..self.buffers.len() {
            let Some(&Framebuffer { fb_id, handle, .. }) = self.buffers.get(i) else {
                continue;
            };
            let rm = fb_id.to_le_bytes();
            let _ = call(&mut self.sys, uapi::RMFB, rm, &mut []);
            let destroy = ModeDestroyDumb { handle };
            let _ = call(
                &mut self.sys,
                uapi::DESTROY_DUMB,
                destroy.to_bytes(),
                &mut [],
            );
        }
    }
}

/// Issue one ioctl, mapping its errno onto [`ScanoutError`].
fn call<const N: usize>(
    sys: &mut dyn KmsSys,
    request: u32,
    payload: [u8; N],
    arrays: &mut [OutArray<'_>],
) -> Result<[u8; N], ScanoutError> {
    let mut buf = payload;
    sys.ioctl(request, &mut buf, arrays)
        .map_err(|errno| ScanoutError::Ioctl { request, errno })?;
    Ok(buf)
}

/// A kernel-reported count, clamped to something we are willing to allocate.
fn capped(count: u32, max: usize) -> usize {
    usize::try_from(count).unwrap_or(max).min(max)
}

/// Decode the first `n` `u32`s of a buffer the kernel filled in.
fn u32s(buf: &[u8], n: usize) -> Vec<u32> {
    buf.chunks_exact(4)
        .take(n)
        .map(|c| u32::from_le_bytes(<[u8; 4]>::try_from(c).unwrap_or([0; 4])))
        .collect()
}

/// The CRTC ids and connector ids a card has.
///
/// Two passes, per Linux's enumeration contract: ask with null arrays to learn
/// the counts, then ask again with arrays that size. The second pass's returned
/// count is *not* trusted to bound the buffer — a display hotplugged between
/// the two calls makes it larger than what we allocated, and the kernel will
/// have copied only what fit. Taking the smaller of the two is the whole guard.
fn resources(sys: &mut dyn KmsSys) -> Result<(Vec<u32>, Vec<u32>), ScanoutError> {
    let probe = call(
        sys,
        uapi::GETRESOURCES,
        ModeCardRes::default().to_bytes(),
        &mut [],
    )?;
    let counts = ModeCardRes::from_bytes(&probe);

    let n_crtcs = capped(counts.count_crtcs, MAX_OBJECTS);
    let n_conns = capped(counts.count_connectors, MAX_OBJECTS);
    let mut crtc_buf = vec![0u8; n_crtcs.saturating_mul(4)];
    let mut conn_buf = vec![0u8; n_conns.saturating_mul(4)];

    let req = ModeCardRes {
        count_crtcs: u32::try_from(n_crtcs).unwrap_or(0),
        count_connectors: u32::try_from(n_conns).unwrap_or(0),
        ..ModeCardRes::default()
    };
    let mut arrays = [
        OutArray::new(ModeCardRes::CRTC_ID_PTR_AT, &mut crtc_buf),
        OutArray::new(ModeCardRes::CONNECTOR_ID_PTR_AT, &mut conn_buf),
    ];
    let got = ModeCardRes::from_bytes(&call(sys, uapi::GETRESOURCES, req.to_bytes(), &mut arrays)?);
    let crtc_n = capped(got.count_crtcs, n_crtcs);
    let conn_n = capped(got.count_connectors, n_conns);
    Ok((u32s(&crtc_buf, crtc_n), u32s(&conn_buf, conn_n)))
}

/// A connector, a mode on it, and a CRTC that can drive it.
struct Chosen {
    /// The connector.
    connector_id: u32,
    /// The mode to run.
    mode: ModeModeinfo,
    /// The CRTC to run it on.
    crtc_id: u32,
}

/// Everything `GETCONNECTOR` says about one connector.
struct Connector {
    /// The fixed-size part of the reply.
    info: ModeGetConnector,
    /// Its mode list.
    modes: Vec<ModeModeinfo>,
    /// The ids of encoders that can drive it.
    encoders: Vec<u32>,
}

/// Ask about one connector, in two passes as [`resources`] does.
fn get_connector(sys: &mut dyn KmsSys, connector_id: u32) -> Result<Connector, ScanoutError> {
    let ask = ModeGetConnector {
        connector_id,
        ..ModeGetConnector::default()
    };
    let probe =
        ModeGetConnector::from_bytes(&call(sys, uapi::GETCONNECTOR, ask.to_bytes(), &mut [])?);

    let n_modes = capped(probe.count_modes, MAX_MODES);
    let n_encs = capped(probe.count_encoders, MAX_OBJECTS);
    let mut mode_buf = vec![0u8; n_modes.saturating_mul(ModeModeinfo::SIZE)];
    let mut enc_buf = vec![0u8; n_encs.saturating_mul(4)];

    let req = ModeGetConnector {
        connector_id,
        count_modes: u32::try_from(n_modes).unwrap_or(0),
        count_encoders: u32::try_from(n_encs).unwrap_or(0),
        ..ModeGetConnector::default()
    };
    let mut arrays = [
        OutArray::new(ModeGetConnector::MODES_PTR_AT, &mut mode_buf),
        OutArray::new(ModeGetConnector::ENCODERS_PTR_AT, &mut enc_buf),
    ];
    let info =
        ModeGetConnector::from_bytes(&call(sys, uapi::GETCONNECTOR, req.to_bytes(), &mut arrays)?);

    // Same hotplug guard as in `resources`, and it bites harder here: the
    // kernel *re-probes* a connector on every `GETCONNECTOR`, so its mode
    // count genuinely changes between the two calls when a monitor is plugged
    // in mid-enumeration.
    let modes = mode_buf
        .chunks_exact(ModeModeinfo::SIZE)
        .take(capped(info.count_modes, n_modes))
        .map(|c| {
            ModeModeinfo::from_bytes(
                &<[u8; ModeModeinfo::SIZE]>::try_from(c).unwrap_or([0; ModeModeinfo::SIZE]),
            )
        })
        .collect();
    let encoders = u32s(&enc_buf, capped(info.count_encoders, n_encs));
    Ok(Connector {
        info,
        modes,
        encoders,
    })
}

/// The mode to run on a connector: the one the display calls native, else the
/// first it offered.
///
/// Not "the largest": a mode list routinely contains a resolution the panel
/// will letterbox or a refresh rate it will not hold, and the display's own
/// `PREFERRED` flag is the answer to exactly this question. Falling back to
/// the first is what the kernel's own list ordering intends — DRM sorts a
/// connector's modes best-first.
fn best_mode(modes: &[ModeModeinfo]) -> Option<ModeModeinfo> {
    modes
        .iter()
        .find(|m| m.is_preferred() && m.size() != (0, 0))
        .or_else(|| modes.iter().find(|m| m.size() != (0, 0)))
        .copied()
}

/// Find a CRTC that can drive this connector.
///
/// Prefers the CRTC the connector is already routed to, which is the one the
/// firmware lit at boot and therefore the one already scanning out at this
/// mode. Otherwise walks the connector's encoders and takes the first CRTC any
/// of them can reach.
fn resolve_crtc(sys: &mut dyn KmsSys, crtcs: &[u32], conn: &Connector) -> Option<u32> {
    if conn.info.encoder_id != 0 {
        if let Some(bound) = get_encoder(sys, conn.info.encoder_id) {
            if bound.crtc_id != 0 && crtcs.contains(&bound.crtc_id) {
                return Some(bound.crtc_id);
            }
        }
    }
    for &encoder_id in &conn.encoders {
        let Some(encoder) = get_encoder(sys, encoder_id) else {
            continue;
        };
        // `possible_crtcs` is a bitmask over the *index* into the CRTC id
        // array from `GETRESOURCES` — not over CRTC ids. Treating it as ids
        // is the classic mistake here, and it hides completely on a
        // single-CRTC machine, where index 0 and the only id both make bit 0
        // look right.
        for (index, &crtc_id) in crtcs.iter().enumerate() {
            let reachable = u32::try_from(index)
                .ok()
                .and_then(|shift| encoder.possible_crtcs.checked_shr(shift))
                .is_some_and(|bits| bits & 1 == 1);
            if reachable {
                return Some(crtc_id);
            }
        }
    }
    None
}

/// One encoder, or `None` if the card would not describe it.
///
/// A failure here is not fatal — the caller is walking a list looking for
/// anything that works — so this swallows the errno rather than propagating a
/// per-encoder failure into the whole setup.
fn get_encoder(sys: &mut dyn KmsSys, encoder_id: u32) -> Option<ModeGetEncoder> {
    let ask = ModeGetEncoder {
        encoder_id,
        ..ModeGetEncoder::default()
    };
    let bytes = call(sys, uapi::GETENCODER, ask.to_bytes(), &mut []).ok()?;
    Some(ModeGetEncoder::from_bytes(&bytes))
}

/// Pick the display to drive.
///
/// The first connected connector that has a mode and a route to a CRTC wins.
/// A machine with two monitors gets one of them driven and the other left
/// dark; extending this to all of them is `known-issues.md` →
/// `TD-COMPOSITOR-DRIVES-ONE-HEAD` and needs a compositor that can compose
/// more than one frame, which is a larger change than this module.
fn choose_display(
    sys: &mut dyn KmsSys,
    crtcs: &[u32],
    connectors: &[u32],
) -> Result<Chosen, ScanoutError> {
    let mut saw_connected = false;
    for &connector_id in connectors {
        // One bad connector must not hide a good one behind it: a card can
        // report a connector whose probe fails while another head works fine.
        let Ok(conn) = get_connector(sys, connector_id) else {
            continue;
        };
        if conn.info.connection != uapi::CONNECTED {
            continue;
        }
        saw_connected = true;
        let Some(mode) = best_mode(&conn.modes) else {
            continue;
        };
        let Some(crtc_id) = resolve_crtc(sys, crtcs, &conn) else {
            continue;
        };
        return Ok(Chosen {
            connector_id,
            mode,
            crtc_id,
        });
    }
    Err(if saw_connected {
        ScanoutError::NoUsableMode
    } else {
        ScanoutError::NoConnectedDisplay
    })
}

/// Allocate, map and register one scanout buffer.
fn make_buffer(sys: &mut dyn KmsSys, width: u32, height: u32) -> Result<Framebuffer, ScanoutError> {
    let ask = ModeCreateDumb {
        height,
        width,
        bpp: uapi::BPP_XRGB8888,
        ..ModeCreateDumb::default()
    };
    let dumb = ModeCreateDumb::from_bytes(&call(sys, uapi::CREATE_DUMB, ask.to_bytes(), &mut [])?);

    // Everything below writes into this buffer using `pitch` and `size`, so
    // this is the boundary at which a driver's numbers stop being data and
    // start being a bound on a memory write. Check them here, once, rather
    // than defensively at every use.
    let need = u64::from(dumb.pitch).saturating_mul(u64::from(height));
    if dumb.handle == 0 || dumb.pitch == 0 || need == 0 || dumb.size < need {
        return Err(ScanoutError::BadBuffer);
    }
    let len = usize::try_from(dumb.size).unwrap_or(usize::MAX);

    let map_ask = ModeMapDumb {
        handle: dumb.handle,
        offset: 0,
    };
    let mapped = ModeMapDumb::from_bytes(&call(sys, uapi::MAP_DUMB, map_ask.to_bytes(), &mut [])?);
    let map = sys.map(mapped.offset, len).map_err(ScanoutError::Map)?;
    if map.bytes_len() < len {
        return Err(ScanoutError::BadBuffer);
    }

    let fb_ask = ModeFbCmd2 {
        width,
        height,
        pixel_format: uapi::FORMAT_XRGB8888,
        handles: [dumb.handle, 0, 0, 0],
        pitches: [dumb.pitch, 0, 0, 0],
        ..ModeFbCmd2::default()
    };
    let fb = ModeFbCmd2::from_bytes(&call(sys, uapi::ADDFB2, fb_ask.to_bytes(), &mut [])?);
    Ok(Framebuffer {
        handle: dumb.handle,
        fb_id: fb.fb_id,
        pitch: dumb.pitch,
        map,
    })
}

/// Copy a composited frame into a scanout buffer.
///
/// Free function, and generic over nothing, because this is where the
/// arithmetic that can be wrong lives and a free function can be tested with
/// no card, fake or otherwise.
///
/// The source is `0xAARRGGBB` in native `u32`s and the destination is `XR24`,
/// which on a little-endian machine is the same four bytes in the same order —
/// the alpha byte lands in `X` and is ignored by the display. So this is a copy
/// and not a conversion, and the only thing it has to get right is *where* each
/// row goes: `row * pitch`, never `row * width * 4`.
///
/// A source smaller than the screen is drawn in the top-left and the rest of
/// the buffer is left alone; a source larger is clipped. Neither is expected —
/// the compositor is built at [`DrmScanout::size`] — but a mismatched frame
/// must not be able to write outside the buffer, and "must not" is a stronger
/// claim when it is arithmetic rather than an assertion.
fn blit(
    dst: &mut [u8],
    dst_pitch: u32,
    dst_width: u32,
    dst_height: u32,
    src: &[u32],
    src_width: u32,
    src_height: u32,
) {
    let pitch = usize::try_from(dst_pitch).unwrap_or(0);
    let src_w = usize::try_from(src_width).unwrap_or(0);
    let copy_w = src_w.min(usize::try_from(dst_width).unwrap_or(0));
    let copy_h = usize::try_from(src_height)
        .unwrap_or(0)
        .min(usize::try_from(dst_height).unwrap_or(0));
    if pitch == 0 || copy_w == 0 || src_w == 0 {
        return;
    }
    let copy_bytes = copy_w.saturating_mul(BYTES_PER_PIXEL);
    for row in 0..copy_h {
        let src_start = row.saturating_mul(src_w);
        let Some(src_row) = src.get(src_start..src_start.saturating_add(copy_w)) else {
            // The caller gave us fewer pixels than it claimed. Documented as
            // the caller's bug and explicitly not a panic: a short frame must
            // not take the display server down with it.
            return;
        };
        let dst_start = row.saturating_mul(pitch);
        let Some(dst_row) = dst.get_mut(dst_start..dst_start.saturating_add(copy_bytes)) else {
            return;
        };
        for (out, &pixel) in dst_row.chunks_exact_mut(BYTES_PER_PIXEL).zip(src_row) {
            out.copy_from_slice(&pixel.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests;
