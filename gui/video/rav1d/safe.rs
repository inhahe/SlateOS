//! A safe Rust interface to the decoder, for callers inside SlateOS.
//!
//! **Not part of rav1d.** Added when rav1d was vendored into SlateOS
//! (`VENDORED.md`, change 3): rav1d 1.1.0 exports dav1d's C interface, whose
//! every function is `unsafe extern "C"` over raw pointers, while the Rust
//! functions behind it (`rav1d_open`, `rav1d_send_data`, `rav1d_get_picture`)
//! are private to the crate. This module sits beside them, so a Rust caller
//! gets the decoder without writing `unsafe`: it owns a context, feeds it
//! bytes, and copies each decoded picture's planes out.
//!
//! The copying is deliberate. A decoded picture's planes live in buffers the
//! decoder pools and reuses, reachable only through rav1d's `DisjointMut`
//! guards; handing those out would tie every caller to rav1d's borrowing
//! rules. A picture is copied once, row by row, into plain vectors -- the
//! caller converts it to RGB next anyway, which reads every sample.

use crate::include::common::bitdepth::BitDepth;
#[cfg(feature = "bitdepth_8")]
use crate::include::common::bitdepth::BitDepth8;
#[cfg(feature = "bitdepth_16")]
use crate::include::common::bitdepth::BitDepth16;
use crate::include::dav1d::data::Rav1dData;
use crate::include::dav1d::dav1d::Rav1dSettings;
use crate::include::dav1d::headers::Rav1dFrameType;
use crate::include::dav1d::headers::Rav1dPixelLayout;
use crate::include::dav1d::picture::Rav1dPicture;
use crate::src::c_arc::CArc;
use crate::src::c_box::CBox;
use crate::src::error::Rav1dError;
use crate::src::internal::Rav1dContext;
use crate::src::lib::rav1d_close;
use crate::src::lib::rav1d_flush;
use crate::src::lib::rav1d_get_picture;
use crate::src::lib::rav1d_open;
use crate::src::lib::rav1d_send_data;
use crate::src::pixels::Pixels as _;
use std::fmt;
use std::sync::Arc;

/// Why the decoder refused, or asked to be called again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `EAGAIN`: the decoder wants more data before it has a picture, or has a
    /// picture to hand out before it takes more data.
    Again,
    /// `EINVAL`: an argument, or the bitstream, is not valid.
    Invalid,
    /// `ENOMEM`: an allocation failed, or a worker thread could not start.
    NoMemory,
    /// `ERANGE`: a value out of the range the decoder handles.
    Range,
    /// `ENOPROTOOPT`: the bitstream asks for something the decoder does not
    /// support.
    Unsupported,
    /// Any other failure (`EGeneric`, `EIO`, `ENOENT`).
    Failed,
}

impl From<Rav1dError> for Error {
    fn from(error: Rav1dError) -> Self {
        match error {
            Rav1dError::EAGAIN => Self::Again,
            Rav1dError::EINVAL => Self::Invalid,
            Rav1dError::ENOMEM => Self::NoMemory,
            Rav1dError::ERANGE => Self::Range,
            Rav1dError::ENOPROTOOPT => Self::Unsupported,
            Rav1dError::EGeneric | Rav1dError::EIO | Rav1dError::ENOENT => Self::Failed,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Again => "the AV1 decoder needs more data first",
            Self::Invalid => "invalid AV1 bitstream or setting",
            Self::NoMemory => "out of memory, or of threads, for AV1 decoding",
            Self::Range => "AV1 value out of range",
            Self::Unsupported => "AV1 feature the decoder does not support",
            Self::Failed => "AV1 decoding failed",
        })
    }
}

impl std::error::Error for Error {}

/// How to open a decoder: dav1d's `Dav1dSettings`, as far as a Rust caller
/// sets them. The decoder never logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Worker threads, at most 256; 0 lets the decoder choose from the number
    /// of CPUs.
    pub threads: u32,
    /// Frames decoded ahead of the one returned, at most 256; 0 lets the
    /// decoder choose. A still picture wants 1.
    pub max_frame_delay: u32,
    /// Apply the film grain the bitstream describes, as players do.
    pub apply_grain: bool,
    /// The operating point to decode, 0 to 31.
    pub operating_point: u8,
    /// Output every spatial layer rather than only the highest.
    pub all_layers: bool,
    /// The most pixels a frame may have; 0 for no limit.
    pub frame_size_limit: u32,
}

impl Default for Settings {
    /// dav1d's defaults.
    fn default() -> Self {
        Self {
            threads: 0,
            max_frame_delay: 0,
            apply_grain: true,
            operating_point: 0,
            all_layers: true,
            frame_size_limit: 0,
        }
    }
}

/// Bytes to decode: one sample, which may hold several OBUs.
pub struct Data(Rav1dData);

impl Data {
    /// A copy of `bytes`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for no bytes at all, which the decoder does not
    /// take.
    pub fn new(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() {
            return Err(Error::Invalid);
        }
        let owned: Box<[u8]> = Box::from(bytes);
        Ok(Self(Rav1dData::from(CArc::wrap(CBox::from_box(owned))?)))
    }

    /// A copy of `bytes` that carries `timestamp` and `duration` -- in
    /// whatever unit the caller keeps time in -- through to the picture it
    /// decodes to ([`Picture::timestamp`]): dav1d's `Dav1dDataProps`. A
    /// decoder decoding several frames at once hands pictures back calls
    /// after the data they came from, so this is how a caller knows which is
    /// which.
    ///
    /// # Errors
    ///
    /// As [`Data::new`].
    pub fn with_time(bytes: &[u8], timestamp: i64, duration: i64) -> Result<Self, Error> {
        let mut data = Self::new(bytes)?;
        data.0.m.timestamp = timestamp;
        data.0.m.duration = duration;
        Ok(data)
    }

    /// Whether the decoder has taken it all.
    pub fn is_consumed(&self) -> bool {
        self.0.data.is_none()
    }
}

/// An open decoder: dav1d's `Dav1dContext`, closed when dropped.
pub struct Decoder {
    context: Option<Arc<Rav1dContext>>,
}

impl Decoder {
    /// `dav1d_open`.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for settings out of range; [`Error::NoMemory`] if a
    /// worker thread could not be started.
    pub fn new(settings: &Settings) -> Result<Self, Error> {
        let n_threads = i32::try_from(settings.threads).map_err(|_| Error::Invalid)?;
        let max_frame_delay =
            i32::try_from(settings.max_frame_delay).map_err(|_| Error::Invalid)?;
        let settings = Rav1dSettings {
            n_threads,
            max_frame_delay,
            apply_grain: settings.apply_grain,
            operating_point: settings.operating_point,
            all_layers: settings.all_layers,
            frame_size_limit: settings.frame_size_limit,
            logger: None,
            ..Rav1dSettings::default()
        };
        Ok(Self {
            context: Some(rav1d_open(&settings)?),
        })
    }

    fn context(&self) -> Result<&Rav1dContext, Error> {
        self.context.as_deref().ok_or(Error::Failed)
    }

    /// `dav1d_send_data`: give the decoder `data`, which it takes whole or not
    /// at all.
    ///
    /// # Errors
    ///
    /// [`Error::Again`] when a picture must be taken first (the data is then
    /// left as it was); otherwise the decoder's error.
    pub fn send(&mut self, data: &mut Data) -> Result<(), Error> {
        Ok(rav1d_send_data(self.context()?, &mut data.0)?)
    }

    /// `dav1d_get_picture`: the next decoded picture.
    ///
    /// # Errors
    ///
    /// [`Error::Again`] when the decoder needs more data first; otherwise the
    /// decoder's error.
    pub fn picture(&mut self) -> Result<Picture, Error> {
        let mut out = Rav1dPicture::default();
        rav1d_get_picture(self.context()?, &mut out)?;
        Ok(Picture(out))
    }

    /// `dav1d_flush`: forget everything sent and every picture not yet
    /// taken, as a seek must. The sequence header is forgotten too, so the
    /// next data must carry one -- or follow the stream's configuration OBUs
    /// (a Matroska or MP4 track's `av1C`), sent first.
    ///
    /// # Errors
    ///
    /// Only for a decoder that is already closed, which the safe interface
    /// never leaves one.
    pub fn flush(&mut self) -> Result<(), Error> {
        rav1d_flush(self.context()?);
        Ok(())
    }
}

impl Drop for Decoder {
    /// `dav1d_close`: flush, and stop the worker threads.
    fn drop(&mut self) {
        if let Some(context) = self.context.take() {
            rav1d_close(context);
        }
    }
}

/// How a picture's chroma is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Monochrome: luma only.
    I400,
    I420,
    I422,
    I444,
}

/// A picture's colour, from its sequence header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    /// ITU-T H.273 colour primaries.
    pub primaries: u8,
    /// ITU-T H.273 transfer characteristics.
    pub transfer: u8,
    /// ITU-T H.273 matrix coefficients.
    pub matrix: u8,
    /// Full range (`color_range` 1) rather than limited.
    pub full_range: bool,
    /// The chroma sample position, 0 to 3.
    pub chroma_sample_position: u8,
}

/// One plane of samples copied out of a picture: `height` rows of `width`
/// samples, packed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plane<T> {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<T>,
}

/// A decoded picture.
pub struct Picture(Rav1dPicture);

// SAFETY: a picture the decoder has handed out is finished. dav1d -- and so
// rav1d -- outputs a frame only once every row of it is decoded (with frame
// threads, after waiting on the frame's condition variable under the task
// lock, which also orders the worker's writes before this thread's reads);
// film grain is applied into a copy; and from then on the decoder only reads
// the frame, as a reference for the frames after it, perhaps on its worker
// threads. So for as long as this holds it, the picture's pixels are
// immutable, its buffers live as long as the `Arc` this holds (whose count is
// atomic), and they are freed through rav1d's picture allocator, which any
// thread may call. Moving a picture to another thread, and reading it there
// while decoder threads read it too, is reading immutable memory from two
// threads. What keeps the compiler from seeing this is the raw pointer in
// `Rav1dPictureDataComponentInner` behind the picture's `DisjointMut` -- the
// same pointer upstream's own threads reach through the `unsafe impl Send`s
// of `src/internal.rs`.
unsafe impl Send for Picture {}

impl Picture {
    /// The frame's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        let p = &self.0.p;
        (
            u32::try_from(p.w).unwrap_or(0),
            u32::try_from(p.h).unwrap_or(0),
        )
    }

    /// Bits per sample: 8, 10 or 12.
    pub fn bit_depth(&self) -> u8 {
        self.0.p.bpc
    }

    pub fn layout(&self) -> Layout {
        match self.0.p.layout {
            Rav1dPixelLayout::I400 => Layout::I400,
            Rav1dPixelLayout::I420 => Layout::I420,
            Rav1dPixelLayout::I422 => Layout::I422,
            Rav1dPixelLayout::I444 => Layout::I444,
        }
    }

    /// The sequence header's colour description.
    pub fn colour(&self) -> Option<Colour> {
        let header = &self.0.seq_hdr.as_ref()?.rav1d;
        Some(Colour {
            primaries: header.pri.0,
            transfer: header.trc.0,
            matrix: header.mtrx.0,
            full_range: header.color_range != 0,
            chroma_sample_position: header.chr as u8,
        })
    }

    /// The frame header's spatial layer.
    pub fn spatial_id(&self) -> Option<u8> {
        Some(self.0.frame_hdr.as_ref()?.rav1d.spatial_id)
    }

    /// Whether this is a key frame -- one decoding can start from.
    pub fn is_key_frame(&self) -> bool {
        self.0
            .frame_hdr
            .as_ref()
            .is_some_and(|h| h.rav1d.frame_type == Rav1dFrameType::Key)
    }

    /// The timestamp of the data the picture was decoded from
    /// ([`Data::with_time`]); `None` for data that carried none.
    pub fn timestamp(&self) -> Option<i64> {
        // dav1d's "no timestamp" is INT64_MIN.
        let t = self.0.m.timestamp;
        (t != i64::MIN).then_some(t)
    }

    /// The duration of the data the picture was decoded from
    /// ([`Data::with_time`]); 0 for data that carried none.
    pub fn duration(&self) -> i64 {
        self.0.m.duration
    }

    /// The size of plane `index`: 0 luma, 1 and 2 chroma.
    fn plane_size(&self, index: usize) -> Option<(usize, usize)> {
        let (w, h) = self.size();
        let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
        let half = |n: usize| n.div_ceil(2);
        Some(match (index, self.layout()) {
            (0, _) => (w, h),
            (1 | 2, Layout::I420) => (half(w), half(h)),
            (1 | 2, Layout::I422) => (half(w), h),
            (1 | 2, Layout::I444) => (w, h),
            _ => return None,
        })
    }

    fn plane<BD: BitDepth>(&self, index: usize) -> Option<Plane<BD::Pixel>> {
        let data = self.0.data.as_ref()?;
        let component = data.data.get(index)?;
        let (width, height) = self.plane_size(index)?;
        let stride = self.0.stride[usize::from(index != 0)];
        let stride = BD::pxstride(isize::try_from(stride).ok()?);
        let base = isize::try_from(component.pixel_offset::<BD>()).ok()?;
        let mut samples = Vec::with_capacity(width.checked_mul(height)?);
        for row in 0..height {
            let start = base.checked_add(isize::try_from(row).ok()?.checked_mul(stride)?)?;
            let start = usize::try_from(start).ok()?;
            let end = start.checked_add(width)?;
            if end.checked_mul(std::mem::size_of::<BD::Pixel>())? > component.byte_len() {
                return None;
            }
            samples.extend_from_slice(&component.slice::<BD, _>((start.., ..width)));
        }
        Some(Plane {
            width,
            height,
            samples,
        })
    }

    /// Plane `index` (0 luma, 1 and 2 chroma) of an 8-bit picture; `None`
    /// for a plane the picture does not have, or a picture of another depth.
    #[cfg(feature = "bitdepth_8")]
    pub fn plane_u8(&self, index: usize) -> Option<Plane<u8>> {
        if self.bit_depth() != 8 {
            return None;
        }
        self.plane::<BitDepth8>(index)
    }

    /// Plane `index` of a 10- or 12-bit picture.
    #[cfg(feature = "bitdepth_16")]
    pub fn plane_u16(&self, index: usize) -> Option<Plane<u16>> {
        if self.bit_depth() == 8 {
            return None;
        }
        self.plane::<BitDepth16>(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// libavif's `white_1x1.avif`'s AV1 payload: a sequence header and one
    /// key frame of one white pixel.
    const WHITE_1X1: [u8; 23] = [
        0x12, 0x00, 0x0a, 0x07, 0x38, 0x00, 0x06, 0x10, 0x10, 0xd0, 0x69, 0x32, 0x0a, 0x1f, 0xf0,
        0x3f, 0xff, 0xff, 0xc4, 0x00, 0x00, 0xaf, 0x70,
    ];

    fn still(threads: u32) -> Settings {
        Settings {
            threads,
            max_frame_delay: 1,
            ..Settings::default()
        }
    }

    fn decode(bytes: &[u8], threads: u32) -> Result<Picture, Error> {
        let mut decoder = Decoder::new(&still(threads))?;
        let mut data = Data::new(bytes)?;
        decoder.send(&mut data)?;
        assert!(data.is_consumed());
        decoder.picture()
    }

    #[test]
    fn a_still_picture_decodes_to_its_planes() {
        let picture = decode(&WHITE_1X1, 1).unwrap();
        assert_eq!(picture.size(), (1, 1));
        assert_eq!(picture.bit_depth(), 8);
        assert_eq!(picture.layout(), Layout::I444);
        assert_eq!(
            picture.colour(),
            Some(Colour {
                primaries: 1,
                transfer: 13,
                matrix: 6,
                full_range: true,
                chroma_sample_position: 0,
            })
        );
        assert_eq!(picture.spatial_id(), Some(0));
        let plane = |i| picture.plane_u8(i).unwrap().samples;
        assert_eq!(
            (plane(0), plane(1), plane(2)),
            (vec![253], vec![128], vec![128])
        );
        // An 8-bit picture has no 16-bit planes, and no fourth plane.
        assert_eq!(picture.plane_u16(0), None);
        assert_eq!(picture.plane_u8(3), None);
    }

    #[test]
    fn worker_threads_decode_the_same_picture() {
        let one = decode(&WHITE_1X1, 1).unwrap();
        let four = decode(&WHITE_1X1, 4).unwrap();
        for i in 0..3 {
            assert_eq!(one.plane_u8(i), four.plane_u8(i));
        }
    }

    /// A picture carries the time of the data it was decoded from, and says
    /// it is a key frame; data with no time gives a picture with none.
    #[test]
    fn a_picture_carries_its_datas_time() {
        let mut decoder = Decoder::new(&still(1)).unwrap();
        let mut data = Data::with_time(&WHITE_1X1, 42, 7).unwrap();
        decoder.send(&mut data).unwrap();
        let picture = decoder.picture().unwrap();
        assert_eq!((picture.timestamp(), picture.duration()), (Some(42), 7));
        assert!(picture.is_key_frame());
        let untimed = decode(&WHITE_1X1, 1).unwrap();
        assert_eq!((untimed.timestamp(), untimed.duration()), (None, 0));
    }

    /// A flush drops a picture not yet taken, and the sequence header: the
    /// frame alone no longer decodes until a sequence header comes again.
    #[test]
    fn a_flush_forgets_pictures_and_the_sequence_header() {
        let mut decoder = Decoder::new(&still(1)).unwrap();
        let mut data = Data::new(&WHITE_1X1).unwrap();
        decoder.send(&mut data).unwrap();
        decoder.flush().unwrap();
        assert_eq!(decoder.picture().err(), Some(Error::Again));

        // The temporal delimiter (two bytes) and the frame, without the
        // sequence header between them (nine bytes from the third).
        let frame: Vec<u8> = WHITE_1X1[..2]
            .iter()
            .chain(&WHITE_1X1[11..])
            .copied()
            .collect();
        let mut alone = Data::new(&frame).unwrap();
        let sent = decoder.send(&mut alone);
        assert!(
            sent.is_err() || decoder.picture().is_err(),
            "a frame decoded with no sequence header"
        );

        let mut again = Data::new(&WHITE_1X1).unwrap();
        decoder.send(&mut again).unwrap();
        let picture = decoder.picture().unwrap();
        assert_eq!(picture.plane_u8(0).unwrap().samples, [253]);
    }

    #[test]
    fn no_data_and_a_frame_cut_short_are_refused() {
        assert_eq!(Data::new(&[]).err(), Some(Error::Invalid));
        // A temporal delimiter and the sequence header alone (two bytes, then
        // two and seven): the decoder waits for a frame.
        assert_eq!(decode(&WHITE_1X1[..11], 1).err(), Some(Error::Again));
        // The frame cut short.
        assert!(decode(&WHITE_1X1[..20], 1).is_err());
    }
}
