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
//! ## One frame, several monitors
//!
//! The sequence above runs once per *connected* monitor, and each one gets its
//! own CRTC, its own pair of buffers and its own flip. What they share is the
//! picture: [`Present::show`] is handed one frame the size of the whole desktop
//! and each head copies out the rectangle it occupies, at the offset
//! [`DrmScanout::heads`] reports. A window straddling two monitors is one
//! rectangle in that one frame, and nothing above this module — not the
//! compositing pipeline, not the damage tracker, not the window rectangles —
//! ever learns that a head exists. See `design-decisions.md` §514 for why that
//! is the arrangement rather than one composited frame per head.
//!
//! Two rules make the layout agree with the compositor's without either side
//! telling the other:
//!
//! * **Heads are laid out left to right in enumeration order**, which is
//!   precisely what `DisplayManager::add_display` does with the displays the
//!   caller attaches in the order `heads()` returns them.
//! * **A CRTC drives at most one connector.** It scans out one framebuffer at a
//!   time, so handing the same one to two monitors would not light the second —
//!   it would take the first monitor's picture away. A connector we cannot give
//!   a free CRTC to is declined, leaving one working monitor rather than two
//!   broken ones.
//!
//! A head that fails is dropped rather than fatal: one whose first flip fails
//! never enters the layout, and one whose flips start failing mid-session stops
//! being drawn on while the others carry on. Only when no head is left does
//! [`Present::is_open`] go false and the display server exit. Their buffers stay
//! owned either way, so `Drop` still gives every id back.
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
use sys::{EAGAIN, EBUSY, EINTR, ENOENT, Errno, KmsSys, Mapped, OutArray};
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
    /// Find the machine's display and set up scanout on it.
    ///
    /// The one entry point a caller outside this module needs, and the reason
    /// [`sys::Card`] does not have to be nameable from there. `wanted` names a
    /// specific `/dev/dri/cardN` when the user asked for one; `None` searches.
    ///
    /// # Errors
    ///
    /// [`ScanoutError`]. Every variant means "fall back to headless", not
    /// "abort": a display server with no display still serves remote clients.
    pub fn open(wanted: Option<u32>) -> Result<Self, ScanoutError> {
        open_display(&mut sys::Cards, wanted)
    }
}

/// Set up scanout on the first card that has a display, or on a named one.
///
/// **Why this is not simply `card0`.** A laptop with integrated graphics and a
/// discrete GPU has `card0` and `card1`, and which one is which is not stable
/// across boots — it depends on driver probe order. Opening `card0`
/// unconditionally gives a black screen half the time on exactly the hardware
/// this is most likely to run on. Searching costs at most
/// [`MAX_CARDS`](sys::MAX_CARDS) failed `open`s on a machine with no graphics
/// at all, which is a machine that is about to fall back to headless anyway.
///
/// **What "has a display" means here.** [`DrmScanout::new`] already
/// distinguishes "nothing is plugged into this card" ([`NoConnectedDisplay`])
/// from every other failure, so the search is exactly "try each, keep the first
/// that is not that error". A card that *has* a display and fails for some
/// other reason does not stop the search — a broken first card must not keep a
/// working second one dark — but the failure is remembered, because that is a
/// card we were supposed to be able to drive and losing the reason would turn a
/// diagnosable driver bug into an unexplained black screen.
///
/// **What is reported when nothing works.** The first non-`NoConnectedDisplay`
/// error seen, because that is the one that says something; only if every card
/// was merely unplugged (or absent) is [`NoConnectedDisplay`] the answer.
/// Reporting the *last* error instead would mean reporting `ENOENT` on
/// `/dev/dri/card15` — true, useless, and actively misleading about which card
/// the real problem was on.
///
/// [`NoConnectedDisplay`]: ScanoutError::NoConnectedDisplay
///
/// # Errors
///
/// [`ScanoutError`], as above.
pub fn open_display<C: sys::CardSource>(
    source: &mut C,
    wanted: Option<u32>,
) -> Result<DrmScanout<C::Sys>, ScanoutError> {
    if let Some(index) = wanted {
        // An explicit request is obeyed exactly, including its failure: a user
        // who passed `--card 1` wants to know that card 1 did not work, not to
        // be quietly given card 0 and left wondering why the wrong monitor lit.
        let card = source.open(index).map_err(ScanoutError::Open)?;
        return DrmScanout::new(card);
    }
    let mut first_real_error = None;
    for index in 0..sys::MAX_CARDS {
        let outcome = match source.open(index) {
            Ok(card) => DrmScanout::new(card),
            Err(errno) => Err(ScanoutError::Open(errno)),
        };
        match outcome {
            Ok(scanout) => return Ok(scanout),
            Err(ScanoutError::NoConnectedDisplay) => {}
            // `ENOENT` past the last card is the ordinary end of the list, not
            // a fault, so it must not become the reported error — otherwise a
            // machine whose only card is unplugged would be described as having
            // no `/dev/dri/card15`.
            Err(ScanoutError::Open(errno)) if errno == ENOENT => {}
            Err(other) => first_real_error = first_real_error.or(Some(other)),
        }
    }
    Err(first_real_error.unwrap_or(ScanoutError::NoConnectedDisplay))
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

/// One monitor being driven: a CRTC, the connector it feeds, its own pair of
/// buffers, and the rectangle of the composited frame it scans out.
///
/// The rectangle is the whole point. The compositor draws **one** frame covering
/// the whole virtual desktop (`design-decisions.md` §514), and a head is a
/// viewport onto it — so a window straddling the seam between two monitors is
/// one rectangle in one buffer, and the only thing that has to know about heads
/// is this struct and the copy that reads from `(x, y)` instead of from the
/// origin.
struct Head {
    /// The CRTC being driven. No two live heads share one: a CRTC drives a
    /// single scanout at a time, so handing the same one to two connectors
    /// would mean the second silently stole the first's picture.
    crtc_id: u32,
    /// The connector being driven. Kept for diagnostics and for the reconnect
    /// logic a later change will want; nothing reads it per frame.
    connector_id: u32,
    /// The two buffers, flipped between.
    buffers: [Framebuffer; 2],
    /// Index into [`Self::buffers`] of the one currently being scanned out.
    /// The *other* one is the one [`Present::show`] draws into.
    front: usize,
    /// This head's left edge within the composited frame.
    x: u32,
    /// This head's top edge within the composited frame. Always 0 today —
    /// [`DrmScanout::new`] lays heads out in a row, matching
    /// `DisplayManager::add_display`, which is what makes the two agree about
    /// where each monitor is without either telling the other.
    y: u32,
    /// The mode's width in pixels.
    width: u32,
    /// The mode's height in pixels.
    height: u32,
    /// Cleared when a flip fails for a reason that is not "try again", which
    /// is what *this* display going away looks like from here. The other heads
    /// carry on: one monitor unplugged is not a reason to stop drawing on the
    /// rest.
    alive: bool,
}

impl Head {
    /// Index of the buffer *not* currently on screen — the one [`Present::show`]
    /// draws into and the next flip selects.
    ///
    /// One definition, because "the back buffer" is worked out in three places
    /// and two of them being right is not enough.
    const fn back(&self) -> usize {
        1usize.saturating_sub(self.front)
    }
}

/// One driven monitor, as seen from outside: where it is on the desktop and
/// what it is plugged into.
///
/// Returned by [`DrmScanout::heads`] so the caller can declare the same
/// arrangement to the compositor's `DisplayManager`. The two compute the layout
/// by the same rule — left to right, in enumeration order — so they agree by
/// construction rather than by one being told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadInfo {
    /// The connector this monitor is plugged into.
    pub connector_id: u32,
    /// The CRTC driving it.
    pub crtc_id: u32,
    /// Its left edge within the composited frame.
    pub x: u32,
    /// Its top edge within the composited frame.
    pub y: u32,
    /// Its width in pixels — the mode it is running.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
}

/// Every display on one card, driven directly.
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
    /// The monitors, in enumeration order, laid out left to right.
    heads: Vec<Head>,
    /// The composited frame's width: the bounding box of every live head.
    width: u32,
    /// The composited frame's height.
    height: u32,
}

impl<S: KmsSys> DrmScanout<S> {
    /// Set up scanout on an already-open card, driving **every** monitor
    /// plugged into it.
    ///
    /// Takes each connected connector that has a mode and can be routed to a
    /// CRTC no earlier connector has already claimed, prefers that connector's
    /// `DRM_MODE_TYPE_PREFERRED` (native) mode, allocates two buffers per head
    /// at that head's size and shows the first frame on each.
    ///
    /// Heads are laid out **left to right in enumeration order**, which is the
    /// same rule `DisplayManager::add_display` uses, so the compositor's idea of
    /// where each monitor is and this module's idea of where each monitor is
    /// agree without either being told. See `design-decisions.md` §514 for why
    /// the arrangement is one frame with a viewport per head rather than one
    /// frame per head.
    ///
    /// # Errors
    ///
    /// [`ScanoutError`] — see its variants; every one of them means the
    /// caller should fall back to [`Headless`](super::Headless) rather than
    /// abort, because a display server with no display is a working display
    /// server for remote clients.
    pub fn new(mut sys: S) -> Result<Self, ScanoutError> {
        let (crtcs, connectors) = resources(&mut sys)?;
        let chosen = choose_displays(&mut sys, &crtcs, &connectors)?;
        // A head we cannot build is declined rather than fatal, for the same
        // reason a head we cannot route is: one monitor whose buffers will not
        // allocate must not blank the monitor next to it that would have
        // worked. The first failure is kept so that a card on which *nothing*
        // works reports why rather than reporting "no display".
        let mut first_error = None;
        let mut heads = Vec::with_capacity(chosen.len());
        for pick in &chosen {
            match make_head(&mut sys, pick) {
                Ok(head) => heads.push(head),
                Err(e) => first_error = first_error.or(Some(e)),
            }
        }
        if heads.is_empty() {
            return Err(first_error.unwrap_or(ScanoutError::NoConnectedDisplay));
        }
        let mut out = Self {
            sys,
            heads,
            width: 0,
            height: 0,
        };
        // Put something defined on every screen straight away. Without this a
        // display scans out whatever the boot framebuffer left, until the
        // compositor has a reason to compose a frame — which on an idle
        // desktop can be a while.
        for index in 0..out.heads.len() {
            if let Err(e) = out.flip_head(index) {
                first_error = first_error.or(Some(e));
                if let Some(head) = out.heads.get_mut(index) {
                    head.alive = false;
                }
            }
        }
        // A head that could not show its first frame is not part of the
        // desktop: laying it out anyway would reserve a strip of the frame that
        // nothing scans out and put windows on a monitor that is not there. Its
        // buffers stay in the vector so `Drop` still gives their ids back.
        if !out.heads.iter().any(|h| h.alive) {
            return Err(first_error.unwrap_or(ScanoutError::NoConnectedDisplay));
        }
        out.lay_out_heads();
        Ok(out)
    }

    /// Place the live heads in a row and take the composited frame's size from
    /// their bounding box.
    fn lay_out_heads(&mut self) {
        let mut x = 0u32;
        for head in &mut self.heads {
            if !head.alive {
                continue;
            }
            head.x = x;
            head.y = 0;
            x = x.saturating_add(head.width);
        }
        self.width = x;
        self.height = self
            .heads
            .iter()
            .filter(|h| h.alive)
            .map(|h| h.y.saturating_add(h.height))
            .max()
            .unwrap_or(0);
    }

    /// The composited frame's size in pixels: the bounding box of every monitor.
    ///
    /// The compositor is built at this size rather than the other way round —
    /// there is no `SETCRTC` to make a display match a size we picked, and even
    /// if there were, each monitor's native mode is the one that looks right.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Every monitor being driven, in layout order.
    ///
    /// The caller declares these to the compositor's `DisplayManager` so that
    /// window placement, maximise, fullscreen and the snap zones all resolve
    /// against the right screen.
    #[must_use]
    pub fn heads(&self) -> Vec<HeadInfo> {
        self.heads
            .iter()
            .filter(|h| h.alive)
            .map(|h| HeadInfo {
                connector_id: h.connector_id,
                crtc_id: h.crtc_id,
                x: h.x,
                y: h.y,
                width: h.width,
                height: h.height,
            })
            .collect()
    }

    /// The connector id of the leftmost live monitor, or `None` on a scanout
    /// whose every head has died.
    ///
    /// The per-head accessors below are keyed on the **connector id** rather
    /// than on a position, because a position means two different things here:
    /// [`Self::heads`] reports only the live heads, while `self.heads` also
    /// holds the dead ones so `Drop` can still give their buffers back. A
    /// caller enumerating `heads()` and passing the loop index back in would
    /// silently read the wrong monitor the moment one died. A connector id is
    /// the same in both worlds and is what `HeadInfo` already carries.
    fn first_live(&self) -> Option<&Head> {
        self.heads.iter().find(|h| h.alive)
    }

    /// Index of the live head plugged into `connector_id`.
    fn live_head(&self, connector_id: u32) -> Option<usize> {
        self.heads
            .iter()
            .position(|h| h.alive && h.connector_id == connector_id)
    }

    /// The CRTC driving the leftmost live monitor, for diagnostics.
    #[must_use]
    pub fn crtc_id(&self) -> u32 {
        self.first_live().map_or(0, |h| h.crtc_id)
    }

    /// The connector the leftmost live monitor is plugged into, for
    /// diagnostics.
    #[must_use]
    pub fn connector_id(&self) -> u32 {
        self.first_live().map_or(0, |h| h.connector_id)
    }

    /// Bytes per row of the leftmost live monitor's scanout buffers.
    #[must_use]
    pub fn pitch(&self) -> u32 {
        self.first_live()
            .and_then(|h| h.buffers.first())
            .map_or(0, |b| b.pitch)
    }

    /// Bytes per row of the buffer [`Present::show`] is about to write on head
    /// `index` — its *back* buffer.
    ///
    /// Taken from the buffer rather than computed from the head's width because
    /// the pitch is the driver's answer, not ours: it pads rows to whatever
    /// alignment the card wants, and two heads of different widths generally
    /// pad differently.
    fn back_pitch(&self, index: usize) -> u32 {
        let Some(head) = self.heads.get(index) else {
            return 0;
        };
        head.buffers.get(head.back()).map_or(0, |b| b.pitch)
    }

    /// Bytes per row of the scanout buffers of the monitor on `connector_id`.
    ///
    /// Zero if no live head is plugged into that connector.
    #[must_use]
    pub fn pitch_for(&self, connector_id: u32) -> u32 {
        self.live_head(connector_id)
            .and_then(|i| self.heads.get(i))
            .and_then(|h| h.buffers.first())
            .map_or(0, |b| b.pitch)
    }

    /// The bytes currently on the leftmost live monitor — the buffer the last
    /// successful flip selected.
    ///
    /// Exposed because "what reached the display" is a different claim from
    /// "what the compositor drew", and it is the first one that is worth
    /// asserting about. Returns an empty slice only if the buffer array is
    /// somehow malformed, which it cannot be.
    pub fn scanned_out(&mut self) -> &[u8] {
        match self.first_live().map(|h| h.connector_id) {
            Some(connector_id) => self.scanned_out_for(connector_id),
            None => &[],
        }
    }

    /// The bytes currently on the monitor plugged into `connector_id`, or an
    /// empty slice if no live head is.
    pub fn scanned_out_for(&mut self, connector_id: u32) -> &[u8] {
        let Some(index) = self.live_head(connector_id) else {
            return &[];
        };
        let Some(h) = self.heads.get_mut(index) else {
            return &[];
        };
        let front = h.front;
        h.buffers.get_mut(front).map_or(&[], |b| b.map.bytes())
    }

    /// Show the buffer that is not currently on screen for one head, and make
    /// it current.
    fn flip_head(&mut self, index: usize) -> Result<(), ScanoutError> {
        let Some(head) = self.heads.get(index) else {
            return Ok(());
        };
        let back = head.back();
        let (crtc_id, Some(fb_id)) = (head.crtc_id, head.buffers.get(back).map(|b| b.fb_id)) else {
            return Ok(());
        };
        let flip = ModeCrtcPageFlip {
            crtc_id,
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
        if let Some(head) = self.heads.get_mut(index) {
            head.front = back;
        }
        Ok(())
    }
}

impl<S: KmsSys> Present for DrmScanout<S> {
    fn show(&mut self, pixels: &[u32], width: u32, height: u32) {
        for index in 0..self.heads.len() {
            // Read before the mutable borrow below, and per head: a head whose
            // width pads differently from its neighbour's has a different row
            // stride, so writing one at another's skews it from its second row.
            let dst_pitch = self.back_pitch(index);
            let Some(head) = self.heads.get_mut(index) else {
                continue;
            };
            if !head.alive {
                continue;
            }
            let back = head.back();
            // Each head copies out *its own* rectangle of the one composited
            // frame. This is the whole of the multi-head arithmetic: everything
            // above this line composes a single desktop-sized picture and knows
            // nothing about monitors.
            let view = Viewport {
                pitch: dst_pitch,
                width: head.width,
                height: head.height,
                src_x: head.x,
                src_y: head.y,
            };
            if let Some(buffer) = head.buffers.get_mut(back) {
                blit(buffer.map.bytes(), &view, pixels, width, height);
            }
            match self.flip_head(index) {
                Ok(()) => {}
                // A flip that is merely early is not a failure: `EBUSY` means
                // the previous flip has not retired, `EAGAIN` that the driver
                // is not ready, `EINTR` that a signal arrived. The frame is
                // dropped and the next one goes to the same back buffer — which
                // is why the front index is only advanced on success. Dropping
                // a frame under load is what every compositor does; tearing
                // down the display because a monitor was slow is not.
                Err(ScanoutError::Ioctl { errno, .. })
                    if errno == EBUSY || errno == EAGAIN || errno == EINTR => {}
                // Only *this* head. A monitor unplugged mid-session must not
                // stop the others being drawn on, which is the difference
                // between one screen going dark and the desktop going dark.
                Err(_) => {
                    if let Some(head) = self.heads.get_mut(index) {
                        head.alive = false;
                    }
                }
            }
        }
    }

    fn is_open(&self) -> bool {
        self.heads.iter().any(|h| h.alive)
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
        // Every buffer of every head, including heads that died: a head marked
        // not-alive still holds the ids it was given, and dropping the vector
        // would only unmap the memory.
        let ids: Vec<(u32, u32)> = self
            .heads
            .iter()
            .flat_map(|h| h.buffers.iter())
            .map(|b| (b.fb_id, b.handle))
            .collect();
        for (fb_id, handle) in ids {
            release_buffer(&mut self.sys, fb_id, handle);
        }
    }
}

/// Give one scanout buffer's framebuffer id and GEM handle back to the card.
///
/// The framebuffer goes first: the other order leaves the kernel holding a
/// framebuffer whose backing object the client asked to free, which is refused
/// on some drivers and merely confusing on the rest. Both failures are ignored
/// because there is nothing a caller could do about them — this runs on
/// teardown paths, including one inside a constructor that is already failing.
fn release_buffer(sys: &mut dyn KmsSys, fb_id: u32, handle: u32) {
    let _ = call(sys, uapi::RMFB, fb_id.to_le_bytes(), &mut []);
    let destroy = ModeDestroyDumb { handle };
    let _ = call(sys, uapi::DESTROY_DUMB, destroy.to_bytes(), &mut []);
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

/// Find a CRTC that can drive this connector and that no earlier head has
/// already taken.
///
/// Prefers the CRTC the connector is already routed to, which is the one the
/// firmware lit at boot and therefore the one already scanning out at this
/// mode. Otherwise walks the connector's encoders and takes the first CRTC any
/// of them can reach.
///
/// `taken` is what makes two monitors possible rather than a way of driving one
/// monitor twice: a CRTC scans out one framebuffer at a time, so giving the same
/// one to a second connector does not light a second screen — it replaces the
/// first screen's picture with the second's. On the fake card's two-CRTC machine
/// and on most real ones the boot-bound preference alone would hand every
/// connector the same CRTC, so this exclusion is load-bearing and not defensive.
fn resolve_crtc(
    sys: &mut dyn KmsSys,
    crtcs: &[u32],
    conn: &Connector,
    taken: &[u32],
) -> Option<u32> {
    if conn.info.encoder_id != 0 {
        if let Some(bound) = get_encoder(sys, conn.info.encoder_id) {
            if bound.crtc_id != 0
                && crtcs.contains(&bound.crtc_id)
                && !taken.contains(&bound.crtc_id)
            {
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
            if taken.contains(&crtc_id) {
                continue;
            }
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

/// Pick every display to drive.
///
/// Each connected connector that has a mode and a route to a CRTC no earlier
/// connector claimed becomes a head, in enumeration order. A machine with two
/// monitors gets both.
///
/// The number of heads is bounded by the number of CRTCs the card has, because
/// each takes one exclusively — which is a tighter and more meaningful bound
/// than any constant this module could pick, and it is the hardware's own limit
/// on how many pictures it can scan out at once.
fn choose_displays(
    sys: &mut dyn KmsSys,
    crtcs: &[u32],
    connectors: &[u32],
) -> Result<Vec<Chosen>, ScanoutError> {
    let mut saw_connected = false;
    let mut taken: Vec<u32> = Vec::new();
    let mut chosen: Vec<Chosen> = Vec::new();
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
        let Some(crtc_id) = resolve_crtc(sys, crtcs, &conn, &taken) else {
            continue;
        };
        taken.push(crtc_id);
        chosen.push(Chosen {
            connector_id,
            mode,
            crtc_id,
        });
    }
    if chosen.is_empty() {
        return Err(if saw_connected {
            ScanoutError::NoUsableMode
        } else {
            ScanoutError::NoConnectedDisplay
        });
    }
    Ok(chosen)
}

/// Build one head: two scanout buffers at its mode's size.
///
/// The pair is all-or-nothing, because a head with one buffer cannot flip. That
/// makes the second allocation's failure path load-bearing: nothing owns the
/// first buffer yet — `Drop` walks `DrmScanout::heads` and this head never
/// reaches it — so its framebuffer id and GEM handle have to be given back here
/// or they leak for the life of the process. Which matters precisely because
/// this type does not own the card.
fn make_head(sys: &mut dyn KmsSys, pick: &Chosen) -> Result<Head, ScanoutError> {
    let (width, height) = pick.mode.size();
    let first = make_buffer(sys, width, height)?;
    let second = match make_buffer(sys, width, height) {
        Ok(buffer) => buffer,
        Err(e) => {
            release_buffer(sys, first.fb_id, first.handle);
            return Err(e);
        }
    };
    Ok(Head {
        crtc_id: pick.crtc_id,
        connector_id: pick.connector_id,
        buffers: [first, second],
        // `show` draws into `1 - front`, so starting at 1 means the first frame
        // lands in buffer 0 and is flipped to. Starting at 0 would draw the
        // first frame into the buffer that is *already* on screen.
        front: 1,
        x: 0,
        y: 0,
        width,
        height,
        alive: true,
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

/// One monitor's window onto the composited frame, and the shape of the buffer
/// it is copied into.
///
/// A struct rather than five more parameters because [`blit`] would otherwise
/// take nine, and because these five are one idea: *this* rectangle of the
/// desktop goes into *this* buffer.
struct Viewport {
    /// Bytes per row of the destination, as the driver computed it.
    pitch: u32,
    /// The destination's width in pixels.
    width: u32,
    /// The destination's height in pixels.
    height: u32,
    /// The left edge, within the composited frame, of the part to copy.
    src_x: u32,
    /// The top edge, within the composited frame, of the part to copy.
    src_y: u32,
}

/// Copy one monitor's rectangle of a composited frame into its scanout buffer.
///
/// Free function, and generic over nothing, because this is where the
/// arithmetic that can be wrong lives and a free function can be tested with
/// no card, fake or otherwise.
///
/// The source is `0xAARRGGBB` in native `u32`s and the destination is `XR24`,
/// which on a little-endian machine is the same four bytes in the same order —
/// the alpha byte lands in `X` and is ignored by the display. So this is a copy
/// and not a conversion, and the two things it has to get right are *where each
/// row goes* — `row * pitch`, never `row * width * 4` — and *where each row
/// comes from*, which is `(src_y + row) * src_width + src_x` and is the whole of
/// what makes a second monitor show the right half of the desktop rather than
/// the left half again.
///
/// A source that does not reach the far side of the viewport is drawn in the
/// top-left of the buffer and the rest is left alone; one that overruns it is
/// clipped. Neither is expected — the compositor is built at
/// [`DrmScanout::size`] — but a mismatched frame must not be able to write
/// outside the buffer, and "must not" is a stronger claim when it is arithmetic
/// rather than an assertion. A viewport whose origin is past the end of the
/// frame copies nothing, which is the honest answer for a monitor the composited
/// desktop does not reach.
fn blit(dst: &mut [u8], view: &Viewport, src: &[u32], src_width: u32, src_height: u32) {
    let pitch = usize::try_from(view.pitch).unwrap_or(0);
    let src_w = usize::try_from(src_width).unwrap_or(0);
    let src_h = usize::try_from(src_height).unwrap_or(0);
    let off_x = usize::try_from(view.src_x).unwrap_or(usize::MAX);
    let off_y = usize::try_from(view.src_y).unwrap_or(usize::MAX);
    // What the frame actually has to the right of and below this head's origin.
    // Saturating, so an origin past the end of the frame yields zero rather
    // than an enormous count.
    let copy_w = src_w
        .saturating_sub(off_x)
        .min(usize::try_from(view.width).unwrap_or(0));
    let copy_h = src_h
        .saturating_sub(off_y)
        .min(usize::try_from(view.height).unwrap_or(0));
    if pitch == 0 || copy_w == 0 || src_w == 0 {
        return;
    }
    let copy_bytes = copy_w.saturating_mul(BYTES_PER_PIXEL);
    for row in 0..copy_h {
        let src_start = off_y
            .saturating_add(row)
            .saturating_mul(src_w)
            .saturating_add(off_x);
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
