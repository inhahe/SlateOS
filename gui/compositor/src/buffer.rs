//! buffer.rs — DMA-BUF-style shared pixel-buffer surfaces.
//!
//! # Why this exists
//!
//! The compositor's primary client path is *draw-command forwarding*: a
//! client submits a [`RenderTree`](guitk::render::RenderTree) and the
//! compositor rasterizes it (see `Compositor::submit_render`). That is ideal
//! for lightweight, mostly-static UI, but it forces the compositor to re-run
//! the client's drawing every frame and cannot represent content the client
//! produced with its own renderer (a software/GPU rasterizer, a video decoder,
//! a 3D engine).
//!
//! This module adds the second, zero-copy path modeled on Wayland's
//! `wl_buffer` / Linux `dma-buf`: the client allocates a pixel buffer in
//! shared memory, renders into it directly, and hands the compositor a *handle*
//! to that buffer. The compositor *imports* the buffer once, then blits its
//! pixels straight into the framebuffer each frame — no per-frame command
//! replay, no pixel copy across an IPC message body.
//!
//! # Trust boundary
//!
//! A buffer handle and its geometry come from an untrusted client. Every field
//! is validated on import ([`SharedBuffer::import`]): dimensions are capped,
//! the stride must be large enough for the declared width and must not overflow,
//! and the backing bytes must actually cover the declared rows. Nothing here
//! ever indexes without bounds checks or performs unchecked arithmetic, so a
//! hostile or buggy client cannot panic the compositor or read out of bounds.
//!
//! # Buffer-release protocol
//!
//! Like `wl_buffer.release`, the compositor must tell the client when it has
//! finished reading a buffer so the client may safely reuse/overwrite it. After
//! a buffer is composited it is marked *released*
//! ([`SharedBuffer::mark_released`]); `Compositor::take_released_buffer_handles`
//! drains those handles so the IPC layer can notify each client. Until a client
//! attaches a fresh buffer, the same imported pixels keep being shown, so a
//! purely static surface costs one import and then cheap blits.
//!
//! # IPC status
//!
//! Real SlateOS shared-memory handles are not yet wired into the compositor's
//! (currently stubbed) IPC layer, so [`SharedBuffer::import`] takes the mapped
//! bytes directly and normalizes them into an internal ARGB8888 pixel vector.
//! When channel IPC lands, the only change is where those bytes come from (a
//! mapping of the shared pages named by `handle`); the validation, format
//! conversion, blit, and release logic stay exactly the same.

use crate::{CompositorError, CompositorResult, MAX_FB_HEIGHT, MAX_FB_WIDTH};

/// Pixel layout of a client-supplied shared buffer.
///
/// Both supported formats are 32-bit little-endian. Internally the compositor
/// works exclusively in `0xAARRGGBB` (the same layout
/// [`Framebuffer`](crate::Framebuffer) blends), so import converts to that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferFormat {
    /// 32 bits per pixel with an alpha channel. Memory bytes (low→high) are
    /// `[B, G, R, A]`, i.e. a little-endian `u32` of `0xAARRGGBB`.
    Argb8888,
    /// Same byte order as [`Argb8888`](BufferFormat::Argb8888) but the
    /// high byte is ignored and the pixel is treated as fully opaque.
    Xrgb8888,
}

impl BufferFormat {
    /// Bytes occupied by a single pixel in this format.
    #[must_use]
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Argb8888 | Self::Xrgb8888 => 4,
        }
    }

    /// Whether this format carries per-pixel alpha. `Xrgb8888` does not.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Argb8888)
    }
}

/// Upper bound on a shared buffer's pixel count, mirroring the framebuffer cap.
///
/// `u64` arithmetic keeps the product well clear of overflow; the result fits
/// comfortably in `usize` on the 64-bit targets the compositor runs on.
const MAX_BUFFER_PIXELS: u64 = MAX_FB_WIDTH as u64 * MAX_FB_HEIGHT as u64;

/// An imported, validated, client-shared pixel buffer.
///
/// Pixels are stored normalized: exactly `width * height` entries in
/// `0xAARRGGBB` order with the stride padding and source format already
/// resolved away, so blitting is a tight bounds-checked loop.
#[derive(Clone, Debug)]
pub struct SharedBuffer {
    /// Opaque kernel handle naming the shared pages (for real-IPC wiring and
    /// for the release notification). Not interpreted here.
    handle: u64,
    /// Width in pixels.
    width: u32,
    /// Height in pixels.
    height: u32,
    /// Source byte stride (bytes per row) as declared by the client. Retained
    /// for diagnostics / re-import; the normalized `pixels` are densely packed.
    src_stride: u32,
    /// Source pixel format the client rendered in.
    src_format: BufferFormat,
    /// Normalized ARGB8888 pixels, row-major, length `width * height`.
    pixels: Vec<u32>,
    /// Set once the compositor has finished reading this buffer for a frame.
    released: bool,
}

impl SharedBuffer {
    /// Import and validate a client buffer from its mapped bytes.
    ///
    /// `handle` is the kernel shared-memory handle (echoed back on release).
    /// `stride` is the client's bytes-per-row, which may exceed
    /// `width * bytes_per_pixel` for alignment. `bytes` is the mapped region.
    ///
    /// # Errors
    ///
    /// Returns [`CompositorError::InvalidBuffer`] if the geometry is degenerate,
    /// the stride is too small or overflows, or `bytes` is shorter than the
    /// declared rows require; [`CompositorError::BufferTooLarge`] if the buffer
    /// exceeds the supported pixel cap.
    pub fn import(
        handle: u64,
        width: u32,
        height: u32,
        stride: u32,
        format: BufferFormat,
        bytes: &[u8],
    ) -> CompositorResult<Self> {
        if width == 0 || height == 0 {
            return Err(CompositorError::InvalidBuffer(format!(
                "degenerate dimensions {width}x{height}"
            )));
        }

        let bpp = format.bytes_per_pixel();
        // Minimum bytes a single row must contain for the declared width.
        let min_row_bytes = (width as u64)
            .checked_mul(bpp as u64)
            .ok_or_else(|| CompositorError::InvalidBuffer("row size overflow".into()))?;
        if (stride as u64) < min_row_bytes {
            return Err(CompositorError::InvalidBuffer(format!(
                "stride {stride} < required {min_row_bytes} bytes/row"
            )));
        }

        let pixel_count = (width as u64)
            .checked_mul(height as u64)
            .ok_or_else(|| CompositorError::InvalidBuffer("pixel count overflow".into()))?;
        if pixel_count > MAX_BUFFER_PIXELS {
            return Err(CompositorError::BufferTooLarge { width, height });
        }

        // The mapped region must cover every full row. The last row only needs
        // `min_row_bytes`; preceding rows need a full stride to step past.
        let required_bytes = (stride as u64)
            .checked_mul((height as u64).saturating_sub(1))
            .and_then(|v| v.checked_add(min_row_bytes))
            .ok_or_else(|| CompositorError::InvalidBuffer("buffer size overflow".into()))?;
        if (bytes.len() as u64) < required_bytes {
            return Err(CompositorError::InvalidBuffer(format!(
                "mapped {} bytes < required {required_bytes}",
                bytes.len()
            )));
        }

        // Normalize into densely-packed ARGB8888, honoring stride and format.
        let mut pixels = Vec::with_capacity(pixel_count as usize);
        let stride_us = stride as usize;
        let force_opaque = !format.has_alpha();
        for row in 0..height as usize {
            // `row * stride` cannot overflow usize here: required_bytes (which
            // bounds the same product plus a row) already fit in u64 and was
            // checked against bytes.len(), itself a usize.
            let row_off = row * stride_us;
            for col in 0..width as usize {
                let off = row_off + col * 4;
                // Bounds-checked 4-byte little-endian read; never indexes blind.
                let b0 = *bytes.get(off).unwrap_or(&0);
                let b1 = *bytes.get(off + 1).unwrap_or(&0);
                let b2 = *bytes.get(off + 2).unwrap_or(&0);
                let b3 = *bytes.get(off + 3).unwrap_or(&0);
                let mut px = u32::from_le_bytes([b0, b1, b2, b3]);
                if force_opaque {
                    px |= 0xFF00_0000;
                }
                pixels.push(px);
            }
        }

        Ok(Self {
            handle,
            width,
            height,
            src_stride: stride,
            src_format: format,
            pixels,
            released: false,
        })
    }

    /// The kernel handle naming this buffer's shared pages.
    #[must_use]
    pub const fn handle(&self) -> u64 {
        self.handle
    }

    /// Width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Source stride (bytes per row) the client declared on import.
    #[must_use]
    pub const fn src_stride(&self) -> u32 {
        self.src_stride
    }

    /// The pixel format the client rendered in.
    #[must_use]
    pub const fn format(&self) -> BufferFormat {
        self.src_format
    }

    /// Normalized ARGB8888 pixels (row-major, `width * height` entries).
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Bounds-checked single-pixel read in normalized ARGB8888.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y as usize)
            .checked_mul(self.width as usize)
            .and_then(|v| v.checked_add(x as usize))?;
        self.pixels.get(idx).copied()
    }

    /// Normalized size in bytes (`width * height * 4`).
    #[must_use]
    pub fn size_bytes(&self) -> usize {
        self.pixels.len().saturating_mul(4)
    }

    /// Mark this buffer as fully read for the current frame. Idempotent.
    pub fn mark_released(&mut self) {
        self.released = true;
    }

    /// Whether the compositor has finished reading this buffer.
    #[must_use]
    pub const fn is_released(&self) -> bool {
        self.released
    }

    /// If released, clear the flag and return the handle to notify the client;
    /// otherwise `None`. Used to drain pending release notifications exactly once.
    pub fn take_release(&mut self) -> Option<u64> {
        if self.released {
            self.released = false;
            Some(self.handle)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build `height` rows of `stride` bytes, with `width` ARGB pixels per row
    /// written little-endian and the rest of each row left as padding.
    fn make_bytes(width: u32, height: u32, stride: u32, pixels: &[u32]) -> Vec<u8> {
        let mut bytes = vec![0u8; (stride as usize) * (height as usize)];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let p = pixels[y * width as usize + x];
                let off = y * stride as usize + x * 4;
                bytes[off..off + 4].copy_from_slice(&p.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn format_props() {
        assert_eq!(BufferFormat::Argb8888.bytes_per_pixel(), 4);
        assert_eq!(BufferFormat::Xrgb8888.bytes_per_pixel(), 4);
        assert!(BufferFormat::Argb8888.has_alpha());
        assert!(!BufferFormat::Xrgb8888.has_alpha());
    }

    #[test]
    fn import_tightly_packed_argb() {
        let px = [0xFF11_2233, 0x8044_5566, 0x00AA_BBCC, 0xFFFF_FFFF];
        let bytes = make_bytes(2, 2, 8, &px);
        let buf =
            SharedBuffer::import(7, 2, 2, 8, BufferFormat::Argb8888, &bytes).expect("import");
        assert_eq!(buf.handle(), 7);
        assert_eq!(buf.width(), 2);
        assert_eq!(buf.height(), 2);
        assert_eq!(buf.size_bytes(), 16);
        assert_eq!(buf.pixels(), &px);
        assert_eq!(buf.pixel(0, 0), Some(0xFF11_2233));
        assert_eq!(buf.pixel(1, 1), Some(0xFFFF_FFFF));
        assert_eq!(buf.pixel(2, 0), None);
        assert_eq!(buf.pixel(0, 2), None);
    }

    #[test]
    fn import_honors_padded_stride() {
        // 1 pixel wide, but a 16-byte stride (12 bytes padding per row).
        let px = [0xFF00_0000, 0xFFFF_0000, 0xFF00_FF00];
        let bytes = make_bytes(1, 3, 16, &px);
        let buf =
            SharedBuffer::import(1, 1, 3, 16, BufferFormat::Argb8888, &bytes).expect("import");
        assert_eq!(buf.pixels(), &px);
        assert_eq!(buf.src_stride(), 16);
    }

    #[test]
    fn xrgb_forces_opaque() {
        // Alpha bytes are 0x00 in the source but Xrgb must report opaque.
        let px = [0x0011_2233, 0x0044_5566];
        let bytes = make_bytes(2, 1, 8, &px);
        let buf =
            SharedBuffer::import(0, 2, 1, 8, BufferFormat::Xrgb8888, &bytes).expect("import");
        assert_eq!(buf.pixel(0, 0), Some(0xFF11_2233));
        assert_eq!(buf.pixel(1, 0), Some(0xFF44_5566));
    }

    #[test]
    fn reject_zero_dimensions() {
        assert!(matches!(
            SharedBuffer::import(0, 0, 4, 16, BufferFormat::Argb8888, &[]),
            Err(CompositorError::InvalidBuffer(_))
        ));
        assert!(matches!(
            SharedBuffer::import(0, 4, 0, 16, BufferFormat::Argb8888, &[]),
            Err(CompositorError::InvalidBuffer(_))
        ));
    }

    #[test]
    fn reject_stride_too_small() {
        // width 4 needs 16 bytes/row but stride says 8.
        let bytes = vec![0u8; 64];
        assert!(matches!(
            SharedBuffer::import(0, 4, 2, 8, BufferFormat::Argb8888, &bytes),
            Err(CompositorError::InvalidBuffer(_))
        ));
    }

    #[test]
    fn reject_truncated_mapping() {
        // 4x4 @ stride 16 needs 16*3 + 16 = 64 bytes; supply 60.
        let bytes = vec![0u8; 60];
        assert!(matches!(
            SharedBuffer::import(0, 4, 4, 16, BufferFormat::Argb8888, &bytes),
            Err(CompositorError::InvalidBuffer(_))
        ));
    }

    #[test]
    fn accept_exact_minimum_mapping() {
        // Last row may omit trailing stride padding: 16*3 + 16 = 64 bytes exact.
        let bytes = vec![0u8; 64];
        let buf = SharedBuffer::import(0, 4, 4, 16, BufferFormat::Argb8888, &bytes)
            .expect("exact-fit import");
        assert_eq!(buf.width(), 4);
        assert_eq!(buf.height(), 4);
    }

    #[test]
    fn reject_oversized_buffer() {
        // Exceed the framebuffer pixel cap; the mapping is never even read.
        let res = SharedBuffer::import(
            0,
            MAX_FB_WIDTH,
            MAX_FB_HEIGHT + 1,
            MAX_FB_WIDTH * 4,
            BufferFormat::Argb8888,
            &[],
        );
        assert!(matches!(res, Err(CompositorError::BufferTooLarge { .. })));
    }

    #[test]
    fn release_protocol_roundtrip() {
        let bytes = make_bytes(1, 1, 4, &[0xFFFF_FFFF]);
        let mut buf =
            SharedBuffer::import(42, 1, 1, 4, BufferFormat::Argb8888, &bytes).expect("import");
        assert!(!buf.is_released());
        assert_eq!(buf.take_release(), None);
        buf.mark_released();
        assert!(buf.is_released());
        // Drains exactly once.
        assert_eq!(buf.take_release(), Some(42));
        assert!(!buf.is_released());
        assert_eq!(buf.take_release(), None);
    }
}
