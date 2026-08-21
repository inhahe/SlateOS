//! The Linux DRM/KMS userspace ABI, as bytes.
//!
//! Everything the compositor needs to drive a display is an `ioctl` on
//! `/dev/dri/card0` whose argument is a `#[repr(C)]` struct fixed by Linux's
//! `include/uapi/drm/drm_mode.h`. This module is those structs and nothing
//! else: no file descriptors, no `unsafe`, no policy about *which* display to
//! use. That lives in the parent module, and keeping the split means the layer
//! with all the byte offsets in it can be tested exhaustively on a machine with
//! no graphics device at all.
//!
//! ## Why bytes rather than `#[repr(C)]` mirrors
//!
//! The obvious implementation is a `#[repr(C)] struct` per payload and a
//! `transmute` to `&mut [u8]` at the ioctl boundary. It is shorter, and it is
//! how the kernel side of this same ABI is written — reasonably, because the
//! kernel is `no_std` and already full of `unsafe`.
//!
//! Here it would be the wrong trade. `#[repr(C)]` gets the layout right by
//! *asking the compiler nicely*: the padding before `drm_mode_fb_cmd2`'s
//! `modifier[4]` is inserted because `u64` wants 8-byte alignment, which is
//! true today on every target this will ever run on and is nowhere stated in
//! the source. Explicit encoders state it. More importantly, a wrong
//! `#[repr(C)]` layout is invisible — the struct still compiles, the ioctl
//! still runs, and the kernel reads a field from the wrong offset — whereas a
//! wrong explicit layout is caught three separate ways below.
//!
//! ## Three independent checks on the layout
//!
//! A field at the wrong offset is the defect this module exists to prevent, so
//! it is checked from three directions that fail for different reasons:
//!
//! 1. **The round trip.** [`tests`] encodes every payload with a distinct
//!    value in every field and decodes it back. A field written to the wrong
//!    offset lands on top of another and one of the two comes back wrong.
//! 2. **The declared size.** Each payload's `SIZE` is asserted against the
//!    authoritative Linux value. A missing field or a forgotten pad changes it.
//! 3. **The ioctl number.** Linux encodes `sizeof(struct)` into the request
//!    number itself, so [`GETCONNECTOR`] *is* a statement that
//!    `drm_mode_get_connector` is 80 bytes. The numbers here are derived from
//!    `SIZE` and then asserted against their known hex values — which means
//!    check 2 and check 3 have to agree with each other *and* with Linux, and
//!    they are the same numbers the kernel side of this tree asserts in
//!    `kernel/src/drm/uapi.rs`.
//!
//! Together they mean a layout mistake cannot reach a real device: it fails on
//! the build machine, in a test that needs no display.

// ---------------------------------------------------------------------------
// ioctl request-number encoding (Linux `include/uapi/asm-generic/ioctl.h`)
// ---------------------------------------------------------------------------

/// Number-field shift — the per-driver command index sits in the low 8 bits.
const IOC_NRSHIFT: u32 = 0;
/// Type-field shift — the driver "magic" letter.
const IOC_TYPESHIFT: u32 = 8;
/// Size-field shift — `sizeof` the argument struct.
const IOC_SIZESHIFT: u32 = 16;
/// Direction-field shift.
const IOC_DIRSHIFT: u32 = 30;
/// Width of the size field, in bits. A payload larger than this cannot be
/// encoded; every DRM struct is far below the 16383-byte limit.
const IOC_SIZEBITS: u32 = 14;

/// Direction: userspace writes to the kernel.
const IOC_WRITE: u32 = 1;
/// Direction: the kernel writes back to userspace.
const IOC_READ: u32 = 2;

/// The DRM ioctl "magic" letter (`'d'`). Every `DRM_IOCTL_*` carries it, and
/// it is what makes an ioctl number mean *this* driver rather than a terminal
/// or a socket that happens to use the same command index.
const DRM_IOCTL_BASE: u32 = 0x64;

/// Encode `_IOWR('d', nr, size)` — a bidirectional struct payload.
///
/// Every KMS ioctl the compositor issues is bidirectional: even the ones that
/// read nothing back are declared `DRM_IOWR` by Linux, and the direction bits
/// are part of the number, so getting them "more accurate" than Linux would
/// simply produce a number no kernel recognises.
const fn iowr(nr: u32, size: usize) -> u32 {
    // `size` is masked to the 14-bit field. Every payload here is checked
    // against that bound in `tests::no_payload_overflows_the_ioctl_size_field`,
    // so the mask never actually discards anything — it is present because the
    // shift below would otherwise corrupt the direction bits rather than
    // truncating quietly, which is a worse failure.
    let size_bits = (size as u32) & ((1 << IOC_SIZEBITS) - 1);
    ((IOC_READ | IOC_WRITE) << IOC_DIRSHIFT)
        | (DRM_IOCTL_BASE << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | (size_bits << IOC_SIZESHIFT)
}

/// `DRM_IOCTL_MODE_GETRESOURCES` — enumerate CRTC / connector / encoder ids.
pub const GETRESOURCES: u32 = iowr(0xA0, ModeCardRes::SIZE);
/// `DRM_IOCTL_MODE_GETENCODER` — an encoder's bound CRTC and the set it can
/// drive.
pub const GETENCODER: u32 = iowr(0xA6, ModeGetEncoder::SIZE);
/// `DRM_IOCTL_MODE_GETCONNECTOR` — a connector's status, modes and encoders.
pub const GETCONNECTOR: u32 = iowr(0xA7, ModeGetConnector::SIZE);
/// `DRM_IOCTL_MODE_RMFB` — destroy a framebuffer. The payload is a bare
/// `unsigned int`, not a struct, which is why its size is spelled out.
pub const RMFB: u32 = iowr(0xAF, 4);
/// `DRM_IOCTL_MODE_PAGE_FLIP` — put a different framebuffer on a CRTC.
pub const PAGE_FLIP: u32 = iowr(0xB0, ModeCrtcPageFlip::SIZE);
/// `DRM_IOCTL_MODE_CREATE_DUMB` — allocate a CPU-writable scanout buffer.
pub const CREATE_DUMB: u32 = iowr(0xB2, ModeCreateDumb::SIZE);
/// `DRM_IOCTL_MODE_MAP_DUMB` — get the `mmap` offset for a dumb buffer.
pub const MAP_DUMB: u32 = iowr(0xB3, ModeMapDumb::SIZE);
/// `DRM_IOCTL_MODE_DESTROY_DUMB` — free a dumb buffer.
pub const DESTROY_DUMB: u32 = iowr(0xB4, ModeDestroyDumb::SIZE);
/// `DRM_IOCTL_MODE_ADDFB2` — make a framebuffer object out of a dumb buffer.
pub const ADDFB2: u32 = iowr(0xB8, ModeFbCmd2::SIZE);

// ---------------------------------------------------------------------------
// Constants a caller has to compare against
// ---------------------------------------------------------------------------

/// `DRM_MODE_CONNECTED` — something is plugged into this connector.
pub const CONNECTED: u32 = 1;

/// `DRM_MODE_TYPE_PREFERRED` — the display told us this is its native mode.
///
/// Worth honouring rather than taking the first mode in the list: on a fixed
/// panel (a laptop screen, a virtual display) every other mode is a scaled
/// approximation of this one, and picking one of those makes text blurry for
/// no gain.
pub const MODE_TYPE_PREFERRED: u32 = 1 << 3;

/// FourCC `XR24` — 32 bits per pixel, `0xXXRRGGBB` as a little-endian `u32`.
///
/// The same byte order the compositor's front buffer already uses, so a frame
/// reaches the scanout buffer as a copy and never a conversion. The `X` says
/// the top byte is ignored rather than blended, which is what a screen wants:
/// there is nothing behind it to blend with.
pub const FORMAT_XRGB8888: u32 = 0x3432_3258;

/// Bits per pixel to ask [`ModeCreateDumb`] for, paired with
/// [`FORMAT_XRGB8888`].
pub const BPP_XRGB8888: u32 = 32;

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

/// Appends little-endian fields to a fixed-size buffer.
///
/// Sequential rather than offset-addressed on purpose: an offset-addressed
/// writer lets two fields be given the same offset, and that is precisely the
/// mistake this module is guarding against. Writing in order makes each
/// field's offset the sum of the ones before it, so it cannot be stated wrongly
/// — only the *order* can be wrong, and the round-trip test catches that.
struct Writer<const N: usize> {
    /// The buffer being filled. Zeroed, so a field that is deliberately not
    /// written (reserved, padding, an output-only field) is already correct.
    buf: [u8; N],
    /// How many bytes have been written.
    at: usize,
}

impl<const N: usize> Writer<N> {
    /// A zeroed buffer with nothing written yet.
    const fn new() -> Self {
        Self { buf: [0; N], at: 0 }
    }

    /// Append raw bytes.
    ///
    /// A write that would not fit is dropped rather than panicking. It cannot
    /// happen — every caller writes exactly `N` bytes and
    /// [`Self::finish`] is checked — and a display server must not be brought
    /// down by an arithmetic slip in a header encoder.
    fn put(mut self, bytes: &[u8]) -> Self {
        let end = self.at.saturating_add(bytes.len());
        if let Some(dst) = self.buf.get_mut(self.at..end) {
            dst.copy_from_slice(bytes);
            self.at = end;
        }
        self
    }

    /// Append a `u16`.
    fn u16(self, v: u16) -> Self {
        self.put(&v.to_le_bytes())
    }

    /// Append a `u32`.
    fn u32(self, v: u32) -> Self {
        self.put(&v.to_le_bytes())
    }

    /// Append a `u64`.
    fn u64(self, v: u64) -> Self {
        self.put(&v.to_le_bytes())
    }

    /// Append `n` zero bytes — explicit `#[repr(C)]` padding.
    ///
    /// Named rather than folded into the next field so that the padding Linux
    /// inserts between `drm_mode_fb_cmd2`'s `offsets` and `modifier` is a line
    /// of source that says so.
    fn pad(self, n: usize) -> Self {
        self.put([0; 8].get(..n).unwrap_or(&[]))
    }

    /// The finished buffer, if every byte of it was written.
    ///
    /// `None` means an encoder does not agree with its own `SIZE`, which is a
    /// bug in this file rather than anything a caller did.
    fn finish(self) -> Option<[u8; N]> {
        (self.at == N).then_some(self.buf)
    }
}

/// Reads little-endian fields back out of a fixed-size buffer.
///
/// Takes `&[u8; N]` rather than `&[u8]`: with the length in the type there is
/// no short-buffer case to handle, so no reader here can return a zero that
/// means "ran out" and be mistaken for a zero the kernel wrote.
struct Reader<'a, const N: usize> {
    /// The buffer being read.
    buf: &'a [u8; N],
    /// How many bytes have been consumed.
    at: usize,
}

impl<'a, const N: usize> Reader<'a, N> {
    /// A reader positioned at the start of `buf`.
    const fn new(buf: &'a [u8; N]) -> Self {
        Self { buf, at: 0 }
    }

    /// Consume `n` bytes.
    fn take(&mut self, n: usize) -> &[u8] {
        let end = self.at.saturating_add(n);
        let out = self.buf.get(self.at..end).unwrap_or(&[]);
        self.at = end;
        out
    }

    /// Read a `u16`.
    fn u16(&mut self) -> u16 {
        let b = self.take(2);
        u16::from_le_bytes([first(b), nth(b, 1)])
    }

    /// Read a `u32`.
    fn u32(&mut self) -> u32 {
        let b = self.take(4);
        u32::from_le_bytes([first(b), nth(b, 1), nth(b, 2), nth(b, 3)])
    }

    /// Read a `u64`.
    fn u64(&mut self) -> u64 {
        let b = self.take(8);
        u64::from_le_bytes([
            first(b),
            nth(b, 1),
            nth(b, 2),
            nth(b, 3),
            nth(b, 4),
            nth(b, 5),
            nth(b, 6),
            nth(b, 7),
        ])
    }

    /// Skip `n` bytes of padding.
    fn skip(&mut self, n: usize) {
        let _ = self.take(n);
    }
}

/// The first byte of a slice, or zero. See [`nth`].
fn first(b: &[u8]) -> u8 {
    nth(b, 0)
}

/// The `i`th byte of a slice, or zero.
///
/// Only reachable with an out-of-range `i` if a [`Reader`] over-reads, which
/// its fixed-size buffer makes impossible; it exists so that the byte-assembly
/// above can be written without indexing.
fn nth(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

/// `struct drm_mode_card_res` — the device's KMS resource ids.
///
/// Follows Linux's two-pass enumeration contract: the caller sets each
/// `count_*` to the capacity of the array it is offering and the kernel writes
/// `min(capacity, actual)` entries, then reports the **actual** count back. A
/// first call with null pointers and zero counts therefore just asks "how
/// many?", and a second with real arrays fetches them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeCardRes {
    /// Array to receive framebuffer ids. Unused here — the compositor knows
    /// the framebuffers it created and has no business enumerating anyone
    /// else's.
    pub fb_id_ptr: u64,
    /// Array to receive CRTC ids.
    pub crtc_id_ptr: u64,
    /// Array to receive connector ids.
    pub connector_id_ptr: u64,
    /// Array to receive encoder ids.
    pub encoder_id_ptr: u64,
    /// In: capacity of `fb_id_ptr`. Out: how many framebuffers exist.
    pub count_fbs: u32,
    /// In: capacity of `crtc_id_ptr`. Out: how many CRTCs exist.
    pub count_crtcs: u32,
    /// In: capacity of `connector_id_ptr`. Out: how many connectors exist.
    pub count_connectors: u32,
    /// In: capacity of `encoder_id_ptr`. Out: how many encoders exist.
    pub count_encoders: u32,
    /// Smallest framebuffer width the device will accept.
    pub min_width: u32,
    /// Largest framebuffer width the device will accept.
    pub max_width: u32,
    /// Smallest framebuffer height the device will accept.
    pub min_height: u32,
    /// Largest framebuffer height the device will accept.
    pub max_height: u32,
}

impl ModeCardRes {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 64;

    /// Byte offset of [`Self::crtc_id_ptr`], for
    /// [`OutArray`](super::sys::OutArray).
    ///
    /// The out-of-line arrays are addressed by offset rather than by writing
    /// the field, because the address is formed in `sys` and the struct is
    /// built here — see that module's header for why the two are separated.
    pub const CRTC_ID_PTR_AT: usize = 8;

    /// Byte offset of [`Self::connector_id_ptr`].
    pub const CONNECTOR_ID_PTR_AT: usize = 16;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u64(self.fb_id_ptr)
            .u64(self.crtc_id_ptr)
            .u64(self.connector_id_ptr)
            .u64(self.encoder_id_ptr)
            .u32(self.count_fbs)
            .u32(self.count_crtcs)
            .u32(self.count_connectors)
            .u32(self.count_encoders)
            .u32(self.min_width)
            .u32(self.max_width)
            .u32(self.min_height)
            .u32(self.max_height)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        Self {
            fb_id_ptr: r.u64(),
            crtc_id_ptr: r.u64(),
            connector_id_ptr: r.u64(),
            encoder_id_ptr: r.u64(),
            count_fbs: r.u32(),
            count_crtcs: r.u32(),
            count_connectors: r.u32(),
            count_encoders: r.u32(),
            min_width: r.u32(),
            max_width: r.u32(),
            min_height: r.u32(),
            max_height: r.u32(),
        }
    }
}

/// `struct drm_mode_get_connector` — one connector's status and modes.
///
/// Same two-pass contract as [`ModeCardRes`], with the extra wrinkle that a
/// connector's mode list is re-probed by the kernel on each call, so the count
/// really can change between the two passes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeGetConnector {
    /// Array to receive the ids of encoders that can drive this connector.
    pub encoders_ptr: u64,
    /// Array to receive [`ModeModeinfo`] entries.
    pub modes_ptr: u64,
    /// Array to receive property ids. Unused: legacy modesetting needs none.
    pub props_ptr: u64,
    /// Array to receive property values. Unused, as `props_ptr`.
    pub prop_values_ptr: u64,
    /// In: capacity of `modes_ptr`. Out: how many modes this connector has.
    pub count_modes: u32,
    /// In: capacity of `props_ptr`. Out: how many properties it has.
    pub count_props: u32,
    /// In: capacity of `encoders_ptr`. Out: how many encoders can drive it.
    pub count_encoders: u32,
    /// The encoder currently bound to it, or 0 for none.
    pub encoder_id: u32,
    /// In: which connector to ask about.
    pub connector_id: u32,
    /// `DRM_MODE_CONNECTOR_*` — HDMI, DisplayPort, Virtual, and so on.
    pub connector_type: u32,
    /// Which one of that type this is, counting from 1.
    pub connector_type_id: u32,
    /// [`CONNECTED`], disconnected, or unknown.
    pub connection: u32,
    /// Physical width in millimetres, or 0 if the display did not say.
    pub mm_width: u32,
    /// Physical height in millimetres, or 0 if the display did not say.
    pub mm_height: u32,
    /// `DRM_MODE_SUBPIXEL_*` — the physical order of the subpixels, which a
    /// text renderer needs and this compositor does not use yet.
    pub subpixel: u32,
}

impl ModeGetConnector {
    /// Wire size, fixed by Linux. The last four bytes are explicit padding
    /// that Linux names `pad`; it has no field here because it carries nothing.
    pub const SIZE: usize = 80;

    /// Byte offset of [`Self::encoders_ptr`], for
    /// [`OutArray`](super::sys::OutArray).
    pub const ENCODERS_PTR_AT: usize = 0;

    /// Byte offset of [`Self::modes_ptr`].
    pub const MODES_PTR_AT: usize = 8;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u64(self.encoders_ptr)
            .u64(self.modes_ptr)
            .u64(self.props_ptr)
            .u64(self.prop_values_ptr)
            .u32(self.count_modes)
            .u32(self.count_props)
            .u32(self.count_encoders)
            .u32(self.encoder_id)
            .u32(self.connector_id)
            .u32(self.connector_type)
            .u32(self.connector_type_id)
            .u32(self.connection)
            .u32(self.mm_width)
            .u32(self.mm_height)
            .u32(self.subpixel)
            .pad(4)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        let out = Self {
            encoders_ptr: r.u64(),
            modes_ptr: r.u64(),
            props_ptr: r.u64(),
            prop_values_ptr: r.u64(),
            count_modes: r.u32(),
            count_props: r.u32(),
            count_encoders: r.u32(),
            encoder_id: r.u32(),
            connector_id: r.u32(),
            connector_type: r.u32(),
            connector_type_id: r.u32(),
            connection: r.u32(),
            mm_width: r.u32(),
            mm_height: r.u32(),
            subpixel: r.u32(),
        };
        r.skip(4);
        out
    }
}

/// `struct drm_mode_get_encoder` — the link between a connector and a CRTC.
///
/// An encoder is the piece of hardware that turns a CRTC's pixel stream into
/// the signal a particular connector carries. It matters here for one field:
/// [`Self::possible_crtcs`], which is how a connector's set of usable CRTCs is
/// discovered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeGetEncoder {
    /// In: which encoder to ask about.
    pub encoder_id: u32,
    /// `DRM_MODE_ENCODER_*` — TMDS, DAC, Virtual, and so on.
    pub encoder_type: u32,
    /// The CRTC currently driving it, or 0 for none.
    pub crtc_id: u32,
    /// Bitmask over the CRTC id array from [`ModeCardRes`]: bit *n* set means
    /// the *n*th CRTC in that array can drive this encoder. An index into an
    /// array, not a CRTC id — mixing the two up is the classic bug here, and
    /// on a single-CRTC device the two coincide and it goes unnoticed.
    pub possible_crtcs: u32,
    /// Bitmask of encoders that can be cloned onto the same CRTC.
    pub possible_clones: u32,
}

impl ModeGetEncoder {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 20;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u32(self.encoder_id)
            .u32(self.encoder_type)
            .u32(self.crtc_id)
            .u32(self.possible_crtcs)
            .u32(self.possible_clones)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        Self {
            encoder_id: r.u32(),
            encoder_type: r.u32(),
            crtc_id: r.u32(),
            possible_crtcs: r.u32(),
            possible_clones: r.u32(),
        }
    }
}

/// `struct drm_mode_modeinfo` — one resolution and its timings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeModeinfo {
    /// Pixel clock in kHz.
    pub clock: u32,
    /// Horizontal active pixels — the width a person would name.
    pub hdisplay: u16,
    /// Where horizontal sync starts.
    pub hsync_start: u16,
    /// Where horizontal sync ends.
    pub hsync_end: u16,
    /// Total horizontal pixels including blanking.
    pub htotal: u16,
    /// Horizontal skew.
    pub hskew: u16,
    /// Vertical active lines — the height a person would name.
    pub vdisplay: u16,
    /// Where vertical sync starts.
    pub vsync_start: u16,
    /// Where vertical sync ends.
    pub vsync_end: u16,
    /// Total vertical lines including blanking.
    pub vtotal: u16,
    /// Vertical scan.
    pub vscan: u16,
    /// Refresh rate in Hz.
    pub vrefresh: u32,
    /// `DRM_MODE_FLAG_*` — sync polarity, interlacing, and so on.
    pub flags: u32,
    /// `DRM_MODE_TYPE_*`; see [`MODE_TYPE_PREFERRED`].
    pub type_: u32,
    /// NUL-padded name, conventionally `"1920x1080"`.
    pub name: [u8; 32],
}

impl ModeModeinfo {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 68;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u32(self.clock)
            .u16(self.hdisplay)
            .u16(self.hsync_start)
            .u16(self.hsync_end)
            .u16(self.htotal)
            .u16(self.hskew)
            .u16(self.vdisplay)
            .u16(self.vsync_start)
            .u16(self.vsync_end)
            .u16(self.vtotal)
            .u16(self.vscan)
            .u32(self.vrefresh)
            .u32(self.flags)
            .u32(self.type_)
            .put(&self.name)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode one mode out of the array the kernel filled.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        let mut out = Self {
            clock: r.u32(),
            hdisplay: r.u16(),
            hsync_start: r.u16(),
            hsync_end: r.u16(),
            htotal: r.u16(),
            hskew: r.u16(),
            vdisplay: r.u16(),
            vsync_start: r.u16(),
            vsync_end: r.u16(),
            vtotal: r.u16(),
            vscan: r.u16(),
            vrefresh: r.u32(),
            flags: r.u32(),
            type_: r.u32(),
            name: [0; 32],
        };
        let tail = r.take(32);
        for (dst, src) in out.name.iter_mut().zip(tail.iter()) {
            *dst = *src;
        }
        out
    }

    /// Whether the display named this its native mode.
    #[must_use]
    pub const fn is_preferred(&self) -> bool {
        self.type_ & MODE_TYPE_PREFERRED != 0
    }

    /// The mode's size in pixels.
    ///
    /// Widening to `u32` here rather than at every call site: a mode's extent
    /// is a `u16` on the wire and a `u32` everywhere else in the compositor,
    /// and a conversion repeated at eight call sites is a conversion that will
    /// eventually be written the other way round at one of them.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.hdisplay as u32, self.vdisplay as u32)
    }
}

/// `struct drm_mode_create_dumb` — allocate a buffer the CPU can write and the
/// display can scan out.
///
/// "Dumb" as opposed to a GPU-allocated buffer with a tiled or compressed
/// layout: linear memory, no acceleration, and the only kind of buffer that
/// can be filled by a `memcpy`. Exactly what a software compositor wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeCreateDumb {
    /// In: height in pixels. Note the order — Linux puts height first here,
    /// and nowhere else.
    pub height: u32,
    /// In: width in pixels.
    pub width: u32,
    /// In: bits per pixel; see [`BPP_XRGB8888`].
    pub bpp: u32,
    /// In: allocation flags. Must be 0; none are defined.
    pub flags: u32,
    /// Out: the buffer's handle.
    pub handle: u32,
    /// Out: bytes per row, which is **not** `width * 4` — the driver pads it
    /// for alignment, so every row copy has to use this and not recompute it.
    pub pitch: u32,
    /// Out: total bytes allocated, which is what to `mmap`.
    pub size: u64,
}

impl ModeCreateDumb {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 32;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u32(self.height)
            .u32(self.width)
            .u32(self.bpp)
            .u32(self.flags)
            .u32(self.handle)
            .u32(self.pitch)
            .u64(self.size)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        Self {
            height: r.u32(),
            width: r.u32(),
            bpp: r.u32(),
            flags: r.u32(),
            handle: r.u32(),
            pitch: r.u32(),
            size: r.u64(),
        }
    }
}

/// `struct drm_mode_map_dumb` — ask where to `mmap` a dumb buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeMapDumb {
    /// In: the buffer's handle.
    pub handle: u32,
    /// Out: an opaque token to pass as `mmap`'s offset. Not a byte offset into
    /// anything — it is a key into the driver's table, and arithmetic on it is
    /// meaningless.
    pub offset: u64,
}

impl ModeMapDumb {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 16;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u32(self.handle)
            .pad(4)
            .u64(self.offset)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        let handle = r.u32();
        r.skip(4);
        Self {
            handle,
            offset: r.u64(),
        }
    }
}

/// `struct drm_mode_destroy_dumb` — free a dumb buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeDestroyDumb {
    /// The buffer's handle.
    pub handle: u32,
}

impl ModeDestroyDumb {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 4;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u32(self.handle)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        Self { handle: r.u32() }
    }
}

/// `struct drm_mode_fb_cmd2` — wrap a buffer up as something a CRTC can scan.
///
/// The arrays are per-plane, for formats that keep luma and chroma in separate
/// buffers. A packed RGB format uses index 0 and leaves the rest zero, and the
/// kernel rejects the request outright if they are not — so they are not
/// merely unused, they are load-bearing zeroes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeFbCmd2 {
    /// Out: the new framebuffer's id.
    pub fb_id: u32,
    /// In: width in pixels.
    pub width: u32,
    /// In: height in pixels.
    pub height: u32,
    /// In: FourCC format; see [`FORMAT_XRGB8888`].
    pub pixel_format: u32,
    /// In: `DRM_MODE_FB_*` flags.
    pub flags: u32,
    /// In: per-plane buffer handles.
    pub handles: [u32; 4],
    /// In: per-plane bytes per row.
    pub pitches: [u32; 4],
    /// In: per-plane byte offsets within their buffers.
    pub offsets: [u32; 4],
    /// In: per-plane format modifiers (tiling/compression descriptors), only
    /// read when `flags` has `DRM_MODE_FB_MODIFIERS`.
    pub modifier: [u64; 4],
}

impl ModeFbCmd2 {
    /// Wire size, fixed by Linux. 100 bytes of fields and four of padding: the
    /// `u64` `modifier` array has to start 8-byte aligned and `offsets` ends
    /// at 68.
    pub const SIZE: usize = 104;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut w = Writer::<{ Self::SIZE }>::new()
            .u32(self.fb_id)
            .u32(self.width)
            .u32(self.height)
            .u32(self.pixel_format)
            .u32(self.flags);
        for v in self.handles {
            w = w.u32(v);
        }
        for v in self.pitches {
            w = w.u32(v);
        }
        for v in self.offsets {
            w = w.u32(v);
        }
        w = w.pad(4);
        for v in self.modifier {
            w = w.u64(v);
        }
        w.finish().unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        let mut out = Self {
            fb_id: r.u32(),
            width: r.u32(),
            height: r.u32(),
            pixel_format: r.u32(),
            flags: r.u32(),
            handles: [0; 4],
            pitches: [0; 4],
            offsets: [0; 4],
            modifier: [0; 4],
        };
        for v in &mut out.handles {
            *v = r.u32();
        }
        for v in &mut out.pitches {
            *v = r.u32();
        }
        for v in &mut out.offsets {
            *v = r.u32();
        }
        r.skip(4);
        for v in &mut out.modifier {
            *v = r.u64();
        }
        out
    }
}

/// `struct drm_mode_crtc_page_flip` — put a different framebuffer on a CRTC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModeCrtcPageFlip {
    /// Which CRTC to flip.
    pub crtc_id: u32,
    /// The framebuffer to start scanning out.
    pub fb_id: u32,
    /// `DRM_MODE_PAGE_FLIP_*` flags.
    pub flags: u32,
    /// Reserved; must be 0.
    pub reserved: u32,
    /// A cookie handed back in the flip-complete event, so a client with
    /// several flips in the air can tell which one retired.
    pub user_data: u64,
}

impl ModeCrtcPageFlip {
    /// Wire size, fixed by Linux.
    pub const SIZE: usize = 24;

    /// Encode for the ioctl.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        Writer::<{ Self::SIZE }>::new()
            .u32(self.crtc_id)
            .u32(self.fb_id)
            .u32(self.flags)
            .u32(self.reserved)
            .u64(self.user_data)
            .finish()
            .unwrap_or([0; Self::SIZE])
    }

    /// Decode what the kernel wrote back.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        let mut r = Reader::new(bytes);
        Self {
            crtc_id: r.u32(),
            fb_id: r.u32(),
            flags: r.u32(),
            reserved: r.u32(),
            user_data: r.u64(),
        }
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::{
        ADDFB2, CREATE_DUMB, DESTROY_DUMB, GETCONNECTOR, GETENCODER, GETRESOURCES, MAP_DUMB,
        MODE_TYPE_PREFERRED, ModeCardRes, ModeCreateDumb, ModeCrtcPageFlip, ModeDestroyDumb,
        ModeFbCmd2, ModeGetConnector, ModeGetEncoder, ModeMapDumb, ModeModeinfo, PAGE_FLIP, RMFB,
    };

    // -- The ioctl numbers ------------------------------------------------
    //
    // These are the same constants `kernel/src/drm/uapi.rs` asserts, and that
    // is the point of duplicating them: the two sides of this ABI derive their
    // numbers from independently-written struct layouts, so agreeing on the
    // hex means agreeing on every `sizeof` that went into it. A single shared
    // constant would agree with itself no matter how wrong it was.

    #[test]
    fn every_ioctl_number_matches_the_one_linux_defines() {
        assert_eq!(GETRESOURCES, 0xC040_64A0, "MODE_GETRESOURCES");
        assert_eq!(GETENCODER, 0xC014_64A6, "MODE_GETENCODER");
        assert_eq!(GETCONNECTOR, 0xC050_64A7, "MODE_GETCONNECTOR");
        assert_eq!(RMFB, 0xC004_64AF, "MODE_RMFB");
        assert_eq!(PAGE_FLIP, 0xC018_64B0, "MODE_PAGE_FLIP");
        assert_eq!(CREATE_DUMB, 0xC020_64B2, "MODE_CREATE_DUMB");
        assert_eq!(MAP_DUMB, 0xC010_64B3, "MODE_MAP_DUMB");
        assert_eq!(DESTROY_DUMB, 0xC004_64B4, "MODE_DESTROY_DUMB");
        assert_eq!(ADDFB2, 0xC068_64B8, "MODE_ADDFB2");
    }

    #[test]
    fn the_size_a_number_encodes_is_the_size_the_payload_declares() {
        // The link between the two checks. If a `SIZE` were changed to make a
        // round-trip test pass, this fails unless the hex above was changed to
        // match — and that one is pinned to Linux.
        let size_of = |ioctl: u32| (ioctl >> 16) & ((1 << 14) - 1);
        assert_eq!(size_of(GETRESOURCES) as usize, ModeCardRes::SIZE);
        assert_eq!(size_of(GETENCODER) as usize, ModeGetEncoder::SIZE);
        assert_eq!(size_of(GETCONNECTOR) as usize, ModeGetConnector::SIZE);
        assert_eq!(size_of(PAGE_FLIP) as usize, ModeCrtcPageFlip::SIZE);
        assert_eq!(size_of(CREATE_DUMB) as usize, ModeCreateDumb::SIZE);
        assert_eq!(size_of(MAP_DUMB) as usize, ModeMapDumb::SIZE);
        assert_eq!(size_of(DESTROY_DUMB) as usize, ModeDestroyDumb::SIZE);
        assert_eq!(size_of(ADDFB2) as usize, ModeFbCmd2::SIZE);
    }

    #[test]
    fn every_ioctl_carries_the_drm_magic_and_asks_for_both_directions() {
        // A wrong magic letter reaches a *different driver's* handler with a
        // command index that means something else there, which is how an
        // ioctl-number typo turns into a mystery rather than an ENOTTY.
        for (name, ioctl) in [
            ("GETRESOURCES", GETRESOURCES),
            ("GETENCODER", GETENCODER),
            ("GETCONNECTOR", GETCONNECTOR),
            ("RMFB", RMFB),
            ("PAGE_FLIP", PAGE_FLIP),
            ("CREATE_DUMB", CREATE_DUMB),
            ("MAP_DUMB", MAP_DUMB),
            ("DESTROY_DUMB", DESTROY_DUMB),
            ("ADDFB2", ADDFB2),
        ] {
            assert_eq!((ioctl >> 8) & 0xFF, 0x64, "{name} magic is not 'd'");
            assert_eq!((ioctl >> 30) & 0x3, 0x3, "{name} is not read-write");
        }
    }

    #[test]
    fn no_payload_overflows_the_ioctl_size_field() {
        // The mask in `iowr` would silently truncate rather than refuse. It
        // cannot, because of this — but the bound is worth stating, since a
        // future payload with a large inline array is exactly the shape of
        // thing that would breach it.
        for size in [
            ModeCardRes::SIZE,
            ModeGetConnector::SIZE,
            ModeGetEncoder::SIZE,
            ModeModeinfo::SIZE,
            ModeCreateDumb::SIZE,
            ModeMapDumb::SIZE,
            ModeDestroyDumb::SIZE,
            ModeFbCmd2::SIZE,
            ModeCrtcPageFlip::SIZE,
        ] {
            assert!(size < (1 << 14), "{size} does not fit the _IOC size field");
        }
    }

    // -- Round trips -------------------------------------------------------
    //
    // Every field gets a distinct value, so a field encoded at another's
    // offset shows up as a mismatch rather than being masked by a shared zero.

    #[test]
    fn card_res_survives_the_round_trip_with_every_field_distinct() {
        let sent = ModeCardRes {
            fb_id_ptr: 0x1111_1111_1111_1111,
            crtc_id_ptr: 0x2222_2222_2222_2222,
            connector_id_ptr: 0x3333_3333_3333_3333,
            encoder_id_ptr: 0x4444_4444_4444_4444,
            count_fbs: 5,
            count_crtcs: 6,
            count_connectors: 7,
            count_encoders: 8,
            min_width: 9,
            max_width: 10,
            min_height: 11,
            max_height: 12,
        };
        assert_eq!(ModeCardRes::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn the_crtc_and_connector_pointers_are_not_transposed() {
        // The single most consequential ordering mistake in this file: swap
        // these two and enumeration still "works", the counts are still right,
        // and the compositor drives a connector id as though it were a CRTC.
        // Pinned by absolute offset rather than by round trip, because a
        // consistent swap round-trips perfectly.
        let res = ModeCardRes {
            crtc_id_ptr: 0xAAAA_AAAA_AAAA_AAAA,
            connector_id_ptr: 0xBBBB_BBBB_BBBB_BBBB,
            ..ModeCardRes::default()
        };
        let b = res.to_bytes();
        assert_eq!(
            &b[8..16],
            &0xAAAA_AAAA_AAAA_AAAA_u64.to_le_bytes(),
            "crtcs at 8"
        );
        assert_eq!(
            &b[16..24],
            &0xBBBB_BBBB_BBBB_BBBB_u64.to_le_bytes(),
            "connectors at 16"
        );
    }

    #[test]
    fn every_out_of_line_pointer_offset_names_the_field_it_claims_to() {
        // These constants are how `sys` finds the pointer fields, and `sys`
        // cannot be tested on this machine — so the constants are checked
        // here, against the encoder that defines the layout. A wrong one would
        // put a buffer address on top of a count and the kernel would copy
        // into whatever the count happened to look like as an address.
        let res = ModeCardRes {
            crtc_id_ptr: 0x1111_1111_1111_1111,
            connector_id_ptr: 0x2222_2222_2222_2222,
            ..ModeCardRes::default()
        };
        let b = res.to_bytes();
        let at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        assert_eq!(at(ModeCardRes::CRTC_ID_PTR_AT), res.crtc_id_ptr);
        assert_eq!(at(ModeCardRes::CONNECTOR_ID_PTR_AT), res.connector_id_ptr);

        let conn = ModeGetConnector {
            encoders_ptr: 0x3333_3333_3333_3333,
            modes_ptr: 0x4444_4444_4444_4444,
            ..ModeGetConnector::default()
        };
        let c = conn.to_bytes();
        let at = |o: usize| u64::from_le_bytes(c[o..o + 8].try_into().unwrap());
        assert_eq!(at(ModeGetConnector::ENCODERS_PTR_AT), conn.encoders_ptr);
        assert_eq!(at(ModeGetConnector::MODES_PTR_AT), conn.modes_ptr);
    }

    #[test]
    fn get_connector_survives_the_round_trip_with_every_field_distinct() {
        let sent = ModeGetConnector {
            encoders_ptr: 0x1111_1111_1111_1111,
            modes_ptr: 0x2222_2222_2222_2222,
            props_ptr: 0x3333_3333_3333_3333,
            prop_values_ptr: 0x4444_4444_4444_4444,
            count_modes: 5,
            count_props: 6,
            count_encoders: 7,
            encoder_id: 8,
            connector_id: 9,
            connector_type: 10,
            connector_type_id: 11,
            connection: 12,
            mm_width: 13,
            mm_height: 14,
            subpixel: 15,
        };
        assert_eq!(ModeGetConnector::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn the_connector_id_a_caller_asks_about_lands_where_linux_reads_it() {
        // GETCONNECTOR is the one ioctl here with a meaningful *input* field
        // buried in the middle of the struct. At the wrong offset the kernel
        // reads a zero and answers ENOENT for a connector that exists, which
        // presents as "no display" rather than as a layout bug.
        let ask = ModeGetConnector {
            connector_id: 0xDEAD_BEEF,
            ..ModeGetConnector::default()
        };
        assert_eq!(&ask.to_bytes()[48..52], &0xDEAD_BEEF_u32.to_le_bytes());
    }

    #[test]
    fn get_encoder_survives_the_round_trip_with_every_field_distinct() {
        let sent = ModeGetEncoder {
            encoder_id: 1,
            encoder_type: 2,
            crtc_id: 3,
            possible_crtcs: 4,
            possible_clones: 5,
        };
        assert_eq!(ModeGetEncoder::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn modeinfo_survives_the_round_trip_including_its_name() {
        let mut name = [0_u8; 32];
        name[..9].copy_from_slice(b"1920x1080");
        let sent = ModeModeinfo {
            clock: 148_500,
            hdisplay: 1920,
            hsync_start: 2008,
            hsync_end: 2052,
            htotal: 2200,
            hskew: 1,
            vdisplay: 1080,
            vsync_start: 1084,
            vsync_end: 1089,
            vtotal: 1125,
            vscan: 2,
            vrefresh: 60,
            flags: 5,
            type_: MODE_TYPE_PREFERRED,
            name,
        };
        assert_eq!(ModeModeinfo::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn a_modes_width_and_height_are_not_transposed() {
        // hdisplay and vdisplay are both u16 and eight bytes apart with four
        // other u16 timings between them. Swap them and a 1920x1080 screen is
        // driven as 1080x1920, which allocates a buffer of the right *size* —
        // so nothing fails until pixels appear diagonally sheared.
        let mode = ModeModeinfo {
            hdisplay: 1920,
            vdisplay: 1080,
            ..ModeModeinfo::default()
        };
        let b = mode.to_bytes();
        assert_eq!(&b[4..6], &1920_u16.to_le_bytes(), "hdisplay at 4");
        assert_eq!(&b[14..16], &1080_u16.to_le_bytes(), "vdisplay at 14");
        assert_eq!(mode.size(), (1920, 1080));
    }

    #[test]
    fn a_preferred_mode_is_recognised_and_an_ordinary_one_is_not() {
        let preferred = ModeModeinfo {
            type_: MODE_TYPE_PREFERRED | (1 << 6),
            ..ModeModeinfo::default()
        };
        let driver_only = ModeModeinfo {
            type_: 1 << 6,
            ..ModeModeinfo::default()
        };
        assert!(preferred.is_preferred());
        assert!(!driver_only.is_preferred(), "driver-supplied is not native");
    }

    #[test]
    fn create_dumb_survives_the_round_trip_with_every_field_distinct() {
        let sent = ModeCreateDumb {
            height: 1080,
            width: 1920,
            bpp: 32,
            flags: 0,
            handle: 7,
            pitch: 7808,
            size: 8_432_640,
        };
        assert_eq!(ModeCreateDumb::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn create_dumb_puts_height_first_because_linux_does() {
        // The one struct in the whole ABI where height precedes width. Get it
        // backwards and a 1920x1080 request allocates 1080x1920 — which
        // succeeds, returns a plausible pitch, and shows a quarter of the
        // desktop stretched across the screen.
        let ask = ModeCreateDumb {
            height: 1080,
            width: 1920,
            ..ModeCreateDumb::default()
        };
        let b = ask.to_bytes();
        assert_eq!(&b[0..4], &1080_u32.to_le_bytes(), "height first");
        assert_eq!(&b[4..8], &1920_u32.to_le_bytes(), "then width");
    }

    #[test]
    fn map_dumb_survives_the_round_trip_and_keeps_its_offset_eight_aligned() {
        let sent = ModeMapDumb {
            handle: 7,
            offset: 0x0000_0001_0000_4000,
        };
        assert_eq!(ModeMapDumb::from_bytes(&sent.to_bytes()), sent);
        // The four bytes of padding after `handle` exist so `offset` starts at
        // 8. Without them the u64 lands at 4 and every bit of it is wrong.
        assert_eq!(
            &sent.to_bytes()[8..16],
            &0x0000_0001_0000_4000_u64.to_le_bytes()
        );
    }

    #[test]
    fn destroy_dumb_survives_the_round_trip() {
        let sent = ModeDestroyDumb {
            handle: 0x0BAD_F00D,
        };
        assert_eq!(ModeDestroyDumb::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn fb_cmd2_survives_the_round_trip_with_every_field_distinct() {
        let sent = ModeFbCmd2 {
            fb_id: 1,
            width: 1920,
            height: 1080,
            pixel_format: super::FORMAT_XRGB8888,
            flags: 0,
            handles: [11, 12, 13, 14],
            pitches: [21, 22, 23, 24],
            offsets: [31, 32, 33, 34],
            modifier: [41, 42, 43, 44],
        };
        assert_eq!(ModeFbCmd2::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn fb_cmd2_pads_before_its_modifiers_so_they_start_eight_aligned() {
        // The padding Linux's compiler inserts and this encoder has to write
        // by hand. Omit it and every modifier is four bytes early, which for a
        // request with `flags = 0` the kernel does not even read — so the bug
        // lies dormant until the day a modifier matters.
        let fb = ModeFbCmd2 {
            offsets: [0xAAAA_AAAA; 4],
            modifier: [0x1122_3344_5566_7788; 4],
            ..ModeFbCmd2::default()
        };
        let b = fb.to_bytes();
        assert_eq!(
            &b[64..68],
            &0xAAAA_AAAA_u32.to_le_bytes(),
            "last offset at 64"
        );
        assert_eq!(&b[68..72], &[0, 0, 0, 0], "four bytes of padding at 68");
        assert_eq!(
            &b[72..80],
            &0x1122_3344_5566_7788_u64.to_le_bytes(),
            "first modifier at 72"
        );
    }

    #[test]
    fn fb_cmd2_keeps_the_first_plane_first() {
        // A packed format uses index 0 and the kernel rejects the request if
        // any other index is non-zero, so an array written in reverse turns
        // into a flat EINVAL with nothing to point at.
        let fb = ModeFbCmd2 {
            handles: [9, 0, 0, 0],
            pitches: [7808, 0, 0, 0],
            ..ModeFbCmd2::default()
        };
        let b = fb.to_bytes();
        assert_eq!(&b[20..24], &9_u32.to_le_bytes(), "handles[0] at 20");
        assert_eq!(&b[24..36], &[0_u8; 12], "handles[1..] are zero");
        assert_eq!(&b[36..40], &7808_u32.to_le_bytes(), "pitches[0] at 36");
        assert_eq!(&b[40..52], &[0_u8; 12], "pitches[1..] are zero");
    }

    #[test]
    fn page_flip_survives_the_round_trip_with_every_field_distinct() {
        let sent = ModeCrtcPageFlip {
            crtc_id: 1,
            fb_id: 2,
            flags: 3,
            reserved: 0,
            user_data: 0x0102_0304_0506_0708,
        };
        assert_eq!(ModeCrtcPageFlip::from_bytes(&sent.to_bytes()), sent);
    }

    #[test]
    fn page_flip_names_the_crtc_before_the_framebuffer() {
        // Two u32s side by side, both plausible small ids. Transposed, the
        // flip asks to put CRTC 1 onto framebuffer 2 — and since both ids
        // exist, the kernel's answer is ENOENT or a flip of the wrong thing
        // rather than anything that names the mistake.
        let flip = ModeCrtcPageFlip {
            crtc_id: 0xC0C0_C0C0,
            fb_id: 0x0FB0_0FB0,
            ..ModeCrtcPageFlip::default()
        };
        let b = flip.to_bytes();
        assert_eq!(&b[0..4], &0xC0C0_C0C0_u32.to_le_bytes(), "crtc first");
        assert_eq!(&b[4..8], &0x0FB0_0FB0_u32.to_le_bytes(), "then fb");
    }

    #[test]
    fn an_all_zero_payload_encodes_to_all_zeroes() {
        // The probe call in the parent module sends exactly this, and relies
        // on every count and pointer being zero so the kernel reports the true
        // counts without writing anywhere. A stray non-zero byte from padding
        // handled wrongly would be read as a capacity and a pointer.
        assert_eq!(ModeCardRes::default().to_bytes(), [0_u8; ModeCardRes::SIZE]);
        assert_eq!(
            ModeGetConnector::default().to_bytes(),
            [0_u8; ModeGetConnector::SIZE]
        );
    }
}
