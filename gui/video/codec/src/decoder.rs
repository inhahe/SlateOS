//! Packets to pictures: a video codec's decoder, fed one track's packets in
//! the order the file holds them, giving back the pictures they show in the
//! order they are shown.
//!
//! VP9 decodes a packet at a time -- at most one picture each, the last
//! frame of a superframe that is shown, as libvpx gives it -- and WebM's
//! alpha channel, where a packet carries one, is a second VP9 stream decoded
//! beside it (as Chrome and FFmpeg's libvpx wrapper decode it). AV1 goes
//! through dav1d's queue: a packet in, a picture out when one is ready, each
//! picture carrying the time of the packet it came from, since with frame
//! threads it comes out a few packets later.

use std::collections::VecDeque;

use rav1d::safe as av1;

use crate::picture::{Picture, Planes};
use crate::{Codec, ColourHint, Error, Limits};

/// One packet of a track: a frame, or (VP9) a superframe of several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub data: &'a [u8],
    /// For VP9 with WebM's transparency, the alpha channel's packet: the
    /// block's `BlockAdditional` with ID 1, itself a VP9 frame whose luma
    /// is the alpha.
    pub alpha: Option<&'a [u8]>,
    /// When the picture it shows is shown, in nanoseconds.
    pub time: i64,
    /// How long, in nanoseconds; 0 if the file does not say.
    pub duration: u64,
    /// Whether the file marks it a key frame.
    pub keyframe: bool,
}

/// One track's decoder.
pub struct Decoder {
    codec: Inner,
    /// What the file says of the track's colour, which each picture is
    /// converted by where its bitstream is silent.
    hint: ColourHint,
    max_pixels: u64,
    /// Pictures decoded and not yet taken, in order.
    ready: VecDeque<Picture>,
}

enum Inner {
    // Boxed: a VP9 decoder's saved probability contexts make it some
    // kilobytes, where dav1d's lives behind a handle.
    Vp9 {
        picture: Box<vp9::Decoder>,
        /// The alpha channel's decoder, made when the first alpha packet
        /// comes.
        alpha: Option<Box<vp9::Decoder>>,
    },
    Av1(Av1),
}

/// How many times [`Av1::push`] hands dav1d the same packet before giving
/// up: each pass takes the packet or a picture, so two or three suffice; this
/// is a backstop against a decoder that does neither.
const MAX_PASSES: usize = 64;

struct Av1 {
    decoder: av1::Decoder,
    /// The configuration OBUs from the track's `av1C` (its sequence header),
    /// sent before the first packet and again after each reset, so that a
    /// stream that carries its sequence header only there decodes too.
    config: Vec<u8>,
    /// Whether `config` is to be sent before the next packet.
    configure: bool,
}

impl Decoder {
    /// A decoder for `codec`, set up with the track's `config` -- the codec's
    /// configuration as the file stores it: an AV1 track's `av1C` record (its
    /// Matroska `CodecPrivate`); nothing for VP9 -- and `hint`, what the file
    /// says of the track's colour.
    ///
    /// # Errors
    ///
    /// [`Error::Codec`] for a codec not decoded here; [`Error::Av1`] if the
    /// AV1 decoder could not start (its worker threads).
    pub fn new(
        codec: Codec,
        config: &[u8],
        hint: ColourHint,
        limits: Limits,
    ) -> Result<Self, Error> {
        let max_pixels = limits.max_pixels;
        let codec = match codec {
            Codec::Vp9 => Inner::Vp9 {
                picture: Box::new(vp9::Decoder::with_max_pixels(max_pixels)),
                alpha: None,
            },
            Codec::Av1 => Inner::Av1(Av1::new(config, max_pixels)?),
            Codec::Vp8 | Codec::Other => return Err(Error::Codec(codec)),
        };
        Ok(Self {
            codec,
            hint,
            max_pixels,
            ready: VecDeque::new(),
        })
    }

    /// Decode `packet`; its picture, if it shows one, is then (or later, for
    /// AV1) waiting in [`Self::receive`].
    ///
    /// # Errors
    ///
    /// The codec's own, for a packet that does not decode. The decoder stays
    /// usable: decoding resumes at the next key frame.
    pub fn send(&mut self, packet: &Packet<'_>) -> Result<(), Error> {
        match &mut self.codec {
            Inner::Vp9 { picture, alpha } => {
                // The alpha stream is decoded first and whatever becomes of
                // the picture, so that the two stay in step.
                let alpha_picture = match packet.alpha {
                    Some(bytes) => alpha
                        .get_or_insert_with(|| {
                            Box::new(vp9::Decoder::with_max_pixels(self.max_pixels))
                        })
                        .decode(bytes)
                        // An alpha packet that does not decode leaves its
                        // picture opaque rather than costing the picture --
                        // the colour is what is watched -- and the alpha
                        // stream resumes at its own next key frame.
                        .unwrap_or(None),
                    None => None,
                };
                if let Some(shown) = picture.decode(packet.data).map_err(Error::Vp9)? {
                    self.ready.push_back(Picture::new(
                        packet,
                        Planes::Vp9 {
                            picture: shown,
                            alpha: alpha_picture,
                        },
                        self.hint,
                    ));
                }
                Ok(())
            }
            Inner::Av1(av1) => av1.send(packet, &mut self.ready, self.hint),
        }
    }

    /// The next picture, in the order they are shown; `None` until another
    /// packet is sent, or the end is [`Self::finish`]ed.
    pub fn receive(&mut self) -> Option<Picture> {
        self.ready.pop_front()
    }

    /// The end of the stream: every picture still being decoded is made
    /// ready for [`Self::receive`].
    ///
    /// # Errors
    ///
    /// The codec's own, for a frame still being decoded that fails.
    pub fn finish(&mut self) -> Result<(), Error> {
        match &mut self.codec {
            // libvpx's decoder holds nothing back.
            Inner::Vp9 { .. } => Ok(()),
            Inner::Av1(av1) => av1.drain(&mut self.ready, self.hint),
        }
    }

    /// Forget everything sent, for a seek: the next packet must be a key
    /// frame.
    ///
    /// # Errors
    ///
    /// Only for an AV1 decoder that has been closed, which never happens.
    pub fn reset(&mut self) -> Result<(), Error> {
        self.ready.clear();
        match &mut self.codec {
            Inner::Vp9 { picture, alpha } => {
                **picture = vp9::Decoder::with_max_pixels(self.max_pixels);
                *alpha = None;
                Ok(())
            }
            Inner::Av1(av1) => {
                av1.decoder.flush().map_err(Error::Av1)?;
                av1.configure = true;
                Ok(())
            }
        }
    }
}

impl Av1 {
    fn new(config: &[u8], max_pixels: u64) -> Result<Self, Error> {
        let settings = av1::Settings {
            // As many worker threads, and frames in flight, as dav1d chooses
            // for the machine: the pictures are the same on any number.
            threads: 0,
            max_frame_delay: 0,
            apply_grain: true,
            operating_point: 0,
            // A player shows the highest spatial layer alone, as FFmpeg's
            // libdav1d wrapper asks by default.
            all_layers: false,
            frame_size_limit: u32::try_from(max_pixels).unwrap_or(u32::MAX),
        };
        Ok(Self {
            decoder: av1::Decoder::new(&settings).map_err(Error::Av1)?,
            config: config_obus(config).to_vec(),
            configure: true,
        })
    }

    fn send(
        &mut self,
        packet: &Packet<'_>,
        ready: &mut VecDeque<Picture>,
        hint: ColourHint,
    ) -> Result<(), Error> {
        if self.configure {
            self.configure = false;
            if !self.config.is_empty() {
                let mut data = av1::Data::new(&self.config).map_err(Error::Av1)?;
                self.push(&mut data, ready, hint)?;
            }
        }
        if packet.data.is_empty() {
            // dav1d takes no empty data; an empty frame shows nothing.
            return Ok(());
        }
        let duration = i64::try_from(packet.duration).unwrap_or(i64::MAX);
        let mut data =
            av1::Data::with_time(packet.data, packet.time, duration).map_err(Error::Av1)?;
        self.push(&mut data, ready, hint)
    }

    /// Hand dav1d `data` whole, taking a picture whenever it has one ready
    /// -- one per pass, as FFmpeg's wrapper takes them: asking twice in a row
    /// would make dav1d wait for every frame its threads are decoding.
    fn push(
        &mut self,
        data: &mut av1::Data,
        ready: &mut VecDeque<Picture>,
        hint: ColourHint,
    ) -> Result<(), Error> {
        for _ in 0..MAX_PASSES {
            // dav1d takes the data, or -- still holding an earlier packet's
            // rest, which the request below goes on parsing -- refuses it
            // for this pass.
            match self.decoder.send(data) {
                Ok(()) | Err(av1::Error::Again) => {}
                Err(e) => return Err(Error::Av1(e)),
            }
            match self.decoder.picture() {
                Ok(p) => ready.push_back(Picture::av1(p, hint)),
                Err(av1::Error::Again) => {}
                Err(e) => return Err(Error::Av1(e)),
            }
            if data.is_consumed() {
                return Ok(());
            }
        }
        Err(Error::Av1(av1::Error::Failed))
    }

    /// The end: every picture dav1d is still decoding. Its first request
    /// after the last packet already drains (dav1d sets its drain flag on
    /// every request), so a second "again" in a row means nothing is left.
    fn drain(&mut self, ready: &mut VecDeque<Picture>, hint: ColourHint) -> Result<(), Error> {
        let mut empty_once = false;
        loop {
            match self.decoder.picture() {
                Ok(p) => {
                    ready.push_back(Picture::av1(p, hint));
                    empty_once = false;
                }
                Err(av1::Error::Again) if empty_once => return Ok(()),
                Err(av1::Error::Again) => empty_once = true,
                Err(e) => return Err(Error::Av1(e)),
            }
        }
    }
}

/// The OBUs of an `av1C` record (ISO/IEC 14496-15 for AV1, which Matroska's
/// `V_AV1` `CodecPrivate` is): what follows its four bytes, when the first
/// says it is one (its marker bit and version 1). Anything else is not a
/// configuration this knows, and gives nothing.
fn config_obus(config: &[u8]) -> &[u8] {
    match config {
        [0x81, _, _, _, obus @ ..] => obus,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_av1c_gives_its_obus() {
        assert_eq!(config_obus(&[0x81, 0, 0, 0, 0x0a, 1, 2]), [0x0a, 1, 2]);
        assert!(config_obus(&[0x81, 0, 0, 0]).is_empty());
        // Not an av1C (another marker, or too short): nothing.
        assert!(config_obus(&[0x80, 0, 0, 0, 0x0a]).is_empty());
        assert!(config_obus(&[0x81, 0]).is_empty());
        assert!(config_obus(&[]).is_empty());
    }

    #[test]
    fn codecs_not_decoded_here_are_refused() {
        for codec in [Codec::Vp8, Codec::Other] {
            assert!(matches!(
                Decoder::new(codec, &[], ColourHint::default(), Limits::default()),
                Err(Error::Codec(c)) if c == codec
            ));
        }
    }
}
