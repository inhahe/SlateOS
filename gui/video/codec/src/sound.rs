//! A file's sound, block by block: [`Sound`].
//!
//! It reads the file's packets in order -- from a Matroska or WebM file
//! (`container.rs`) -- keeps those of its sound track, decodes them, and
//! gives back each packet's samples with their time: 16-bit, interleaved, in
//! the stream's channel order -- for more than two channels, Vorbis's (5.1:
//! front left, centre, front right, rear left, rear right, LFE), which RFC
//! 7845 takes for Opus too. Opus is decoded by `gui/video/opus`, libopus's
//! decoder ported and held to it sample for sample, at 48 kHz (Opus's own
//! rate, whatever the encoder was given); Vorbis by `gui/video/vorbis`,
//! Tremor ported and held to it the same way, at the stream's own rate.
//!
//! **Which track.** Of the file's sound tracks, the first that is
//! decodable, enabled and marked default, else the first decodable one;
//! [`Sound::open_track`] names another.
//!
//! **What is dropped, and when a block is.** As FFmpeg drops and times it
//! (libavcodec's `decode.c`, given the side data libavformat and its
//! Matroska demuxer attach): the codec delay's samples from the stream's
//! start -- Opus's pre-skip, the encoder's look-ahead, not sound --, as many
//! whole packets of it as it covers and the start of the next; and each
//! packet's `DiscardPadding`, from its end (the stream's last samples, past
//! the sound the encoder was given) or, negative, its start. A block's time
//! is its packet's, plus the samples dropped from its start, rounded to the
//! file's tick, in nanoseconds on the file's own clock -- so the first sample
//! is at 0 in a file that starts there, the codec delay having been taken
//! off every packet's time already. A packet that decodes to nothing gives
//! no block, and FFmpeg never reads its side data: a Vorbis stream's first
//! packet, the first half of the overlap the second completes, so that the
//! codec delay a newer muxer gives a Vorbis track is played, as FFmpeg plays
//! it, and only its discard padding is dropped.
//!
//! **Damage.** A packet that does not decode is concealed, for as long as
//! the last block lasted: as libopus conceals a lost packet, for an Opus
//! stream (for as long as the packet says it lasts, when it can say); with
//! silence, for a Vorbis stream, which has no concealment of its own, the
//! decoder carrying on from the last block it decoded. FFmpeg drops such a
//! packet's sound, which leaves a hole in the timeline a player's clock runs
//! on. [`Sound::damaged`] counts them.
//!
//! **Seeking** ([`Sound::seek`]) goes to the packet at or before the time
//! less the track's pre-roll (`SeekPreRoll`: Opus's output is not right
//! until it has decoded 80 ms; a Vorbis packet decodes only after the one
//! before it, so a long block at least), starts the decoder afresh, and
//! drops what decodes before the time: the first block after a seek starts
//! at the first sample at or after it. "At or after" by the times blocks
//! carry, which are as exact as the file's tick: a muxer rounds each
//! packet's time to it (a millisecond in WebM), so no sample's time is
//! truer than that -- FFmpeg's and Chrome's seeks trim by the same times.
//! The stream's first packet is dropped from as on opening, so a seek to its
//! start plays what opening it plays, to the sample.
//!
//! What plays: Opus and Vorbis. A file whose sound is AAC is refused by the
//! codec's name ([`crate::Error::SoundCodec`]); MP4's sound tracks are not
//! read yet.

use std::io::{Read, Seek};

use crate::container::{Container, Sample, SoundTrack};
use crate::{Error, SoundCodec, time};

/// Opus's rate: every Opus block is decoded at it.
const OPUS_RATE: u64 = 48_000;

/// The most samples a channel one Opus packet holds: 120 ms.
const OPUS_MAX_PACKET: usize = 5760;

/// A file's sound, as [`Sound::info`] describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundInfo {
    /// The track's number in the file (Matroska's `TrackNumber`): what
    /// [`Sound::open_track`] takes.
    pub track: u64,
    pub codec: SoundCodec,
    /// Samples a second: 48 000 for Opus; a Vorbis stream's own.
    pub sample_rate: u32,
    /// Channels a sample, interleaved in that order in a block.
    pub channels: usize,
    /// How long the file plays, in nanoseconds, if it says.
    pub duration: Option<u64>,
}

/// One packet's sound, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// When its first sample plays, in nanoseconds on the file's clock.
    pub time: i64,
    /// Its samples: [`SoundInfo::channels`] to a sample, interleaved.
    pub samples: Vec<i16>,
}

/// What decodes the track's packets.
enum Decoder {
    Opus(opus::AnyDecoder),
    /// Boxed: it is 376 bytes where it stands, an Opus decoder (which
    /// boxes its own state) 16.
    Vorbis(Box<vorbis::Decoder>),
}

/// A file's sound track, decoded block by block.
pub struct Sound<R> {
    demuxer: Container<R>,
    /// What the container's packets name the track by.
    key: u64,
    info: SoundInfo,
    /// The decoded samples' rate, a second.
    rate: u64,
    /// The track's tick: `num / den` seconds.
    time_base: (u64, u64),
    decoder: Decoder,
    /// What the stream's first packet drops from its start: the codec delay
    /// in samples, as FFmpeg's demuxer gives it (`initial_padding`) -- or,
    /// in a file without one, the `OpusHead`'s pre-skip, as the decoder
    /// drops it (none for Vorbis).
    start_skip: StartSkip,
    /// Where the track's first packet is in the file: the packet the start's
    /// skip comes off, whenever it is decoded.
    first_position: Option<u64>,
    /// That packet, read on opening to find it, and not yet decoded.
    pending: Option<Sample>,
    /// The track's pre-roll, in nanoseconds.
    pre_roll: u64,
    /// Samples a channel still to drop from what decodes next.
    skip: u64,
    /// When the next packet without a time of its own plays, in ticks.
    next_ticks: i64,
    /// A seek's time, while what decodes before it is passed by.
    target: Option<i64>,
    /// Where the decoder writes: the most samples one packet holds.
    pcm: Vec<i16>,
    /// Samples a channel the last block decoded had: how long a damaged
    /// Vorbis packet is concealed for.
    last_samples: usize,
    /// Packets that did not decode, and why the last did not.
    damaged: u64,
    last_damage: Option<Error>,
}

/// How the stream's start is dropped: what its first packet loses.
#[derive(Clone, Copy, Debug)]
enum StartSkip {
    /// FFmpeg's side data on the first packet: it replaces what is left to
    /// drop.
    CodecDelay(u64),
    /// The decoder's own pre-skip.
    PreSkip(u64),
}

/// A Vorbis track's three headers -- identification, comments, setup --
/// from its codec private data (FFmpeg's `avpriv_split_xiph_headers`):
/// Xiph lacing (a count of 2, the first two headers' lengths in 255s and a
/// remainder, then the three, the last taking what is left), or three
/// headers each after its length in two big-endian bytes, the first's 30.
fn xiph_headers(config: &[u8]) -> Option<[&[u8]; 3]> {
    if config.len() >= 6 && config.get(..2) == Some(&[0, 30][..]) {
        let mut rest = config;
        let mut take = || -> Option<&[u8]> {
            let (len, after) = rest.split_first_chunk::<2>()?;
            let (header, after) = after.split_at_checked(usize::from(u16::from_be_bytes(*len)))?;
            rest = after;
            Some(header)
        };
        return Some([take()?, take()?, take()?]);
    }
    let (&2, mut rest) = config.split_first()? else {
        return None;
    };
    let mut lengths = [0usize; 2];
    for len in &mut lengths {
        loop {
            let (&byte, after) = rest.split_first()?;
            rest = after;
            *len = len.checked_add(usize::from(byte))?;
            if byte != 0xff {
                break;
            }
        }
    }
    let (id, rest) = rest.split_at_checked(lengths[0])?;
    let (comments, setup) = rest.split_at_checked(lengths[1])?;
    Some([id, comments, setup])
}

impl<R: Read + Seek> Sound<R> {
    /// The sound of `source` -- a Matroska or WebM file -- from its best
    /// track (see the module documentation).
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the file cannot be read, or is not one of
    /// those; [`Error::NoSound`] when it has no sound track;
    /// [`Error::SoundCodec`] when none of its sound is in a codec decoded
    /// here (the codec of the one it would have played); [`Error::Opus`]
    /// when the track's `OpusHead` is not one, [`Error::Vorbis`] when its
    /// Vorbis headers are not.
    pub fn open(source: R) -> Result<Self, Error> {
        Self::open_with(source, None)
    }

    /// The sound track numbered `track` of `source` (as [`SoundInfo::track`]
    /// numbers it).
    ///
    /// # Errors
    ///
    /// As [`Self::open`]; [`Error::NoSound`] when `track` is not a sound
    /// track of the file.
    pub fn open_track(source: R, track: u64) -> Result<Self, Error> {
        Self::open_with(source, Some(track))
    }

    fn open_with(source: R, track: Option<u64>) -> Result<Self, Error> {
        let mut demuxer = Container::open(source)?;
        let sounds = demuxer.sounds();
        let decodable = |c: SoundCodec| matches!(c, SoundCodec::Opus | SoundCodec::Vorbis);
        let chosen: &SoundTrack = match track {
            Some(n) => sounds.iter().find(|t| t.number == n),
            // Decodable first, then enabled, then marked default; the file's
            // order among equals.
            None => sounds
                .iter()
                .min_by_key(|t| (!decodable(t.codec), !t.enabled, !t.default)),
        }
        .ok_or(Error::NoSound)?;
        let (decoder, rate, channels, pre_skip, max_packet) = match chosen.codec {
            SoundCodec::Opus => {
                let head = opus::Head::parse(&chosen.config)
                    .ok_or(Error::Opus(opus::Error::BadArgument))?;
                let decoder = head.decoder(OPUS_RATE as u32).map_err(Error::Opus)?;
                let channels = decoder.channels();
                (
                    Decoder::Opus(decoder),
                    OPUS_RATE,
                    channels,
                    u64::from(head.pre_skip),
                    OPUS_MAX_PACKET,
                )
            }
            SoundCodec::Vorbis => {
                let [id, _, setup] =
                    xiph_headers(&chosen.config).ok_or(Error::Vorbis(vorbis::Error::BadHeader))?;
                let decoder = vorbis::Decoder::new(id, setup).map_err(Error::Vorbis)?;
                let (rate, channels) = (decoder.info().rate, decoder.info().channels);
                let max = decoder.max_samples();
                (
                    Decoder::Vorbis(Box::new(decoder)),
                    u64::from(rate),
                    channels,
                    0,
                    max,
                )
            }
            other => return Err(Error::SoundCodec(other)),
        };
        let start_skip = if chosen.codec_delay > 0 {
            // FFmpeg's `initial_padding`: the delay in samples, rounded.
            StartSkip::CodecDelay(rescale(chosen.codec_delay, (1, 1_000_000_000), (1, rate)))
        } else {
            StartSkip::PreSkip(pre_skip)
        };
        let info = SoundInfo {
            track: chosen.number,
            codec: chosen.codec,
            sample_rate: u32::try_from(rate).unwrap_or(u32::MAX),
            channels,
            duration: demuxer.duration(),
        };
        let (key, time_base) = (chosen.key, chosen.time_base);
        // A Vorbis packet decodes only after the one before it (the first
        // after a reset only primes the overlap), which starts at most a long
        // block before the time: at least that much pre-roll, whatever the
        // track says (Matroska's for Vorbis is 0).
        let pre_roll = match &decoder {
            Decoder::Vorbis(d) => chosen.seek_pre_roll.max(rescale(
                d.info().blocksizes[1] as u64,
                (1, rate),
                (1, 1_000_000_000),
            )),
            Decoder::Opus(_) => chosen.seek_pre_roll,
        };
        // The track's first packet: where the start's skip comes off.
        let mut pending = None;
        while let Some(sample) = demuxer.next_packet()? {
            if sample.track == key {
                pending = Some(sample);
                break;
            }
        }
        Ok(Self {
            demuxer,
            key,
            info,
            rate,
            time_base,
            decoder,
            start_skip,
            first_position: pending.as_ref().map(|s| s.position),
            pending,
            pre_roll,
            skip: 0,
            next_ticks: 0,
            target: None,
            pcm: vec![0; max_packet.saturating_mul(channels)],
            last_samples: 0,
            damaged: 0,
            last_damage: None,
        })
    }

    /// What the file says of its sound.
    pub fn info(&self) -> &SoundInfo {
        &self.info
    }

    /// The next block of sound; `None` at the end of the file.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails. A packet that does not
    /// decode is not an error: it is concealed (see the module
    /// documentation).
    pub fn next_block(&mut self) -> Result<Option<Block>, Error> {
        loop {
            let sample = match self.pending.take() {
                Some(s) => s,
                None => match self.demuxer.next_packet()? {
                    Some(s) => s,
                    None => return Ok(None),
                },
            };
            if sample.track != self.key {
                continue;
            }
            if let Some(block) = self.decode(&sample) {
                return Ok(Some(block));
            }
        }
    }

    /// Go to `time` (nanoseconds on the file's clock): the next block starts
    /// at the first sample at or after it.
    ///
    /// # Errors
    ///
    /// [`Error::Container`] when the source fails, or the track cannot be
    /// sought in.
    pub fn seek(&mut self, time: i64) -> Result<(), Error> {
        let from = time.saturating_sub(i64::try_from(self.pre_roll).unwrap_or(i64::MAX));
        self.demuxer
            .seek(self.key, time::to_ticks(from, self.time_base))?;
        self.pending = None;
        match &mut self.decoder {
            Decoder::Opus(d) => d.reset(),
            Decoder::Vorbis(d) => d.reset(),
        }
        self.last_samples = 0;
        self.skip = 0;
        self.target = Some(time);
        Ok(())
    }

    /// How many packets have not decoded (each concealed).
    pub fn damaged(&self) -> u64 {
        self.damaged
    }

    /// Why the last packet that did not decode did not.
    pub fn last_damage(&self) -> Option<&Error> {
        self.last_damage.as_ref()
    }

    /// One packet's samples a channel, into `self.pcm`: decoded, or
    /// concealed when it does not decode.
    fn decode_packet(&mut self, data: &[u8]) -> usize {
        let channels = self.info.channels;
        let decoded = match &mut self.decoder {
            Decoder::Opus(d) => match d.decode(Some(data), &mut self.pcm, false) {
                Ok(n) => Ok(n),
                Err(e) => {
                    // Concealed, for as long as the packet says it lasts;
                    // else as long as the last.
                    let n = opus::packet_samples(data, OPUS_RATE as u32)
                        .ok()
                        .filter(|&n| n > 0)
                        .unwrap_or_else(|| d.last_packet_duration())
                        .min(OPUS_MAX_PACKET);
                    let room = self.pcm.get_mut(..n.saturating_mul(channels));
                    Err((
                        Error::Opus(e),
                        d.decode(None, room.unwrap_or_default(), false).unwrap_or(0),
                    ))
                }
            },
            Decoder::Vorbis(d) => match d.decode(data, &mut self.pcm) {
                Ok(n) => Ok(n),
                Err(e) => {
                    // Silence, as long as the last block: the decoder, which
                    // a packet it refuses leaves as it was, overlaps the next
                    // with the last it decoded.
                    let n = self.last_samples;
                    if let Some(room) = self.pcm.get_mut(..n.saturating_mul(channels)) {
                        room.fill(0);
                    }
                    Err((Error::Vorbis(e), n))
                }
            },
        };
        match decoded {
            Ok(n) => {
                self.last_samples = n;
                n
            }
            Err((e, n)) => {
                self.damaged = self.damaged.saturating_add(1);
                self.last_damage = Some(e);
                n
            }
        }
    }

    /// One packet decoded and trimmed: its block, or `None` where nothing of
    /// it is left.
    fn decode(&mut self, sample: &Sample) -> Option<Block> {
        let channels = self.info.channels;
        let rate = self.rate;
        let decoded = self.decode_packet(&sample.data);
        let packet_ticks = sample.time.unwrap_or(self.next_ticks);
        self.next_ticks =
            packet_ticks.saturating_add(rescale_i(decoded as u64, (1, rate), self.time_base));
        // A packet that decodes to nothing makes no frame in FFmpeg, and the
        // side data it carries is never read: a Vorbis stream's first, whose
        // codec delay FFmpeg therefore does not drop.
        if decoded == 0 {
            return None;
        }
        // The side data FFmpeg would give the packet: the stream's codec
        // delay to drop from its start; its `DiscardPadding`, in samples.
        let mut side_skip = None;
        if Some(sample.position) == self.first_position {
            match self.start_skip {
                StartSkip::CodecDelay(n) => side_skip = Some(n),
                StartSkip::PreSkip(n) => self.skip = n,
            }
        }
        let padding = sample.discard_padding;
        let mut discard = 0u64;
        if padding > 0 {
            discard = rescale(padding.unsigned_abs(), (1, 1_000_000_000), (1, rate));
            side_skip.get_or_insert(0);
        } else if padding < 0 {
            side_skip = Some(rescale(
                padding.unsigned_abs(),
                (1, 1_000_000_000),
                (1, rate),
            ));
        }
        if let Some(s) = side_skip {
            self.skip = s;
        }
        let mut start = 0u64;
        let mut n = decoded as u64;
        if self.skip > 0 {
            // A packet the skip covers whole is dropped, and the skip goes on
            // into the next.
            let Some(rest) = n.checked_sub(self.skip).filter(|&r| r > 0) else {
                self.skip = self.skip.saturating_sub(n);
                return None;
            };
            start = self.skip;
            n = rest;
            self.skip = 0;
        }
        // Padding longer than what is left is ignored, as FFmpeg ignores it.
        if discard > 0 {
            match n.checked_sub(discard) {
                Some(0) => return None,
                Some(rest) => n = rest,
                None => {}
            }
        }
        // After a seek: what plays before its time is passed by, by the times
        // the samples carry -- the block's, and a sample's length on from it.
        if let Some(target) = self.target {
            let block_ns = time::to_ns(
                packet_ticks.saturating_add(rescale_i(start, (1, rate), self.time_base)),
                self.time_base,
            );
            let before = samples_before(block_ns, target, rate);
            let rest = n.checked_sub(before).filter(|&r| r > 0)?;
            start = start.saturating_add(before);
            n = rest;
            self.target = None;
        }
        let from = usize::try_from(start).ok()?.checked_mul(channels)?;
        let to = usize::try_from(n)
            .ok()?
            .checked_mul(channels)?
            .checked_add(from)?;
        Some(Block {
            time: time::to_ns(
                packet_ticks.saturating_add(rescale_i(start, (1, rate), self.time_base)),
                self.time_base,
            ),
            samples: self.pcm.get(from..to)?.to_vec(),
        })
    }
}

/// `av_rescale_q(a, from, to)`: `a` of `from` units in `to` units, rounded to
/// the nearest, halves away from zero.
fn rescale(a: u64, from: (u64, u64), to: (u64, u64)) -> u64 {
    let num = u128::from(a)
        .saturating_mul(u128::from(from.0))
        .saturating_mul(u128::from(to.1));
    let den = u128::from(from.1).saturating_mul(u128::from(to.0)).max(1);
    num.saturating_add(den >> 1)
        .checked_div(den)
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(u64::MAX)
}

/// [`rescale`], as a signed count of ticks.
fn rescale_i(a: u64, from: (u64, u64), to: (u64, u64)) -> i64 {
    i64::try_from(rescale(a, from, to)).unwrap_or(i64::MAX)
}

/// How many of a block's samples, the first at `first_ns`, play before
/// `target_ns` at `rate` a second: the index of the first at or after it.
fn samples_before(first_ns: i64, target_ns: i64, rate: u64) -> u64 {
    if target_ns <= first_ns {
        return 0;
    }
    // ceil((target - first) * rate / 10^9)
    let diff = u128::from(target_ns.abs_diff(first_ns));
    u64::try_from(
        diff.saturating_mul(u128::from(rate))
            .div_ceil(1_000_000_000),
    )
    .unwrap_or(u64::MAX)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn rescaling_rounds_halves_away_from_zero() {
        // FFmpeg's start of an Opus stream: 312 samples, 6.5 ms, 7 ticks.
        assert_eq!(rescale(312, (1, OPUS_RATE), (1, 1000)), 7);
        assert_eq!(rescale(6_500_000, (1, 1_000_000_000), (1, OPUS_RATE)), 312);
        assert_eq!(rescale(311, (1, OPUS_RATE), (1, 1000)), 6);
    }

    #[test]
    fn the_first_sample_at_or_after_a_time() {
        assert_eq!(samples_before(0, 0, OPUS_RATE), 0);
        assert_eq!(samples_before(100, 0, OPUS_RATE), 0);
        // One sample is 20 833.3 ns.
        assert_eq!(samples_before(0, 1, OPUS_RATE), 1);
        assert_eq!(samples_before(0, 20_833, OPUS_RATE), 1);
        assert_eq!(samples_before(0, 20_834, OPUS_RATE), 2);
        assert_eq!(samples_before(0, 1_000_000_000, OPUS_RATE), 48_000);
        // At 44.1 kHz, 22 675.7 ns.
        assert_eq!(samples_before(0, 22_675, 44_100), 1);
        assert_eq!(samples_before(0, 22_676, 44_100), 2);
    }

    #[test]
    fn vorbis_headers_come_laced_either_way() {
        let (id, comments, setup) = (&[1u8; 30][..], &[3u8; 300][..], &[5u8; 7][..]);
        // Xiph lacing: 2, then 30, then 300 as 255 + 45.
        let mut laced = vec![2, 30, 255, 45];
        laced.extend_from_slice(id);
        laced.extend_from_slice(comments);
        laced.extend_from_slice(setup);
        assert_eq!(xiph_headers(&laced), Some([id, comments, setup]));
        // Each after its length in two big-endian bytes, the first's 30.
        let mut sized = Vec::new();
        for h in [id, comments, setup] {
            sized.extend_from_slice(&u16::try_from(h.len()).unwrap().to_be_bytes());
            sized.extend_from_slice(h);
        }
        assert_eq!(xiph_headers(&sized), Some([id, comments, setup]));
        // Lengths past the end, a count other than 2, nothing at all.
        assert_eq!(xiph_headers(&laced[..40]), None);
        assert_eq!(xiph_headers(&[3, 1, 1, 0, 0, 0]), None);
        assert_eq!(xiph_headers(&[]), None);
        assert_eq!(xiph_headers(&[2, 255]), None);
        assert_eq!(xiph_headers(&sized[..sized.len() - 1]), None);
    }
}
