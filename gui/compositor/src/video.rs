//! The video-encoded capture fallback (`roadmap-detailed.md` §3.3,
//! design-decisions §1343): a window that presents its own pixels -- a
//! buffer it renders into, as a game or a video player does -- has no draw
//! commands a remote viewer could replay, so each stream session codes those
//! pixels as VP9, one stream per such window, and sends its frames in the
//! scene stream (`guiremote::scene::VideoUpdate`).
//!
//! A window's stream codes its buffer when a new one has been attached since
//! its last frame ([`SharedBuffer::serial`]), each picture timed by the
//! session's clock as having shown since the last one did, so that the rate
//! control shares the bitrate out by how long each lasted. It starts again,
//! with a key frame, when the buffer changes size; it stops when the window
//! goes back to drawing commands; and it is forgotten when the window leaves
//! the stream (minimised, on another workspace, gone), to start again with a
//! key frame if the window comes back.
//!
//! The coding is done by whoever asks for the capture, as the capture
//! itself is: see `known-issues/F-the-capture-stream-codes-video-on-the-compositors-thread.md`.

use std::collections::{BTreeMap, BTreeSet};

use guiremote::scene::{SceneVideo, VIDEO_VP9, VideoUpdate};
use vp9::rgb::argb_to_yuv420;
use vp9::{Encoder, EncoderConfig};

use crate::buffer::SharedBuffer;

/// The bitrate a window's video is coded at, in kilobits a second: a tenth
/// of a bit per pixel at thirty pictures a second -- 2.8 Mbit/s for
/// 1280x720, 6.2 for 1920x1080 -- and at least 300.
pub(crate) fn video_kbps(width: u32, height: u32) -> u32 {
    let kbps = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(30)
        / 10_000;
    u32::try_from(kbps.max(300)).unwrap_or(u32::MAX)
}

/// How long a stream's first picture is taken to have shown, in
/// milliseconds: a thirtieth of a second.
const FIRST_PICTURE_MS: u64 = 33;

/// One window's video in one stream session.
pub(crate) struct VideoStream {
    encoder: Encoder,
    /// The pictures' size: the buffer's when the stream started.
    width: u32,
    height: u32,
    /// The serial of the buffer last coded.
    serial: u64,
    /// When the last picture's showing ended, in the session's
    /// milliseconds: the next one's start.
    last_end: Option<u64>,
}

impl VideoStream {
    /// A stream of `width` x `height` pictures, `None` if the encoder
    /// refuses the size.
    fn new(width: u32, height: u32) -> Option<Self> {
        let config = EncoderConfig {
            timebase: (1, 1000),
            ..EncoderConfig::realtime(width, height, video_kbps(width, height))
        };
        Some(Self {
            encoder: Encoder::new(config).ok()?,
            width,
            height,
            serial: 0,
            last_end: None,
        })
    }

    /// Code `buffer`, captured at `now_ms`: shown from the last picture's end
    /// until now. `None` if the encoder refuses it, which leaves the buffer to
    /// be tried again at the next capture.
    fn code(&mut self, buffer: &SharedBuffer, now_ms: u64) -> Option<SceneVideo> {
        let (w, h) = (
            usize::try_from(buffer.width()).ok()?,
            usize::try_from(buffer.height()).ok()?,
        );
        let yuv = argb_to_yuv420(buffer.pixels(), w, h, w).ok()?;
        let start = self
            .last_end
            .unwrap_or_else(|| now_ms.saturating_sub(FIRST_PICTURE_MS));
        // Every picture lasts, and ends after the last one: the encoder's
        // rate control divides by the durations.
        let end = now_ms.max(start.saturating_add(1));
        let frame = self.encoder.encode_timed(yuv.planes(), start, end).ok()?;
        self.last_end = Some(end);
        self.serial = buffer.serial();
        Some(SceneVideo {
            codec: VIDEO_VP9,
            width: self.width,
            height: self.height,
            frame,
        })
    }
}

/// The video updates of one capture for a session holding `streams`, at
/// `now_ms` into it: for each of `windows` -- every window the frame shows,
/// with the buffer it presents, if any -- its next frame where its buffer is
/// new, and a stop where it had a stream and presents no buffer now.
pub(crate) fn capture(
    streams: &mut BTreeMap<u64, VideoStream>,
    windows: &[(u64, Option<&SharedBuffer>)],
    now_ms: u64,
) -> BTreeMap<u64, VideoUpdate> {
    let mut updates = BTreeMap::new();
    let mut shown = BTreeSet::new();
    for &(id, buffer) in windows {
        shown.insert(id);
        let Some(buffer) = buffer else {
            if streams.remove(&id).is_some() {
                updates.insert(id, VideoUpdate::Stop);
            }
            continue;
        };
        let size = (buffer.width(), buffer.height());
        if streams.get(&id).is_none_or(|s| (s.width, s.height) != size) {
            // A new stream, or a new size: start again, with a key frame.
            match VideoStream::new(size.0, size.1) {
                Some(stream) => {
                    streams.insert(id, stream);
                }
                None => {
                    streams.remove(&id);
                    continue;
                }
            }
        }
        if let Some(stream) = streams.get_mut(&id)
            && stream.serial != buffer.serial()
            && let Some(video) = stream.code(buffer, now_ms)
        {
            updates.insert(id, VideoUpdate::Frame(video));
        }
    }
    // A window the frame does not show leaves the viewer, video and all.
    streams.retain(|id, _| shown.contains(id));
    updates
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::buffer::BufferFormat;

    #[test]
    fn the_bitrate_follows_the_size() {
        assert_eq!(video_kbps(1280, 720), 2764);
        assert_eq!(video_kbps(1920, 1080), 6220);
        assert_eq!(video_kbps(64, 64), 300, "the floor");
    }

    /// A buffer of `w` x `h` opaque pixels of `colour`, attached as `serial`.
    fn buffer(w: u32, h: u32, colour: u32, serial: u64) -> SharedBuffer {
        let bytes: Vec<u8> = (0..w * h).flat_map(|_| colour.to_le_bytes()).collect();
        let mut b = SharedBuffer::import(1, w, h, w * 4, BufferFormat::Xrgb8888, &bytes).unwrap();
        b.set_serial(serial);
        b
    }

    /// A window's stream codes each new buffer once, starts again on a new
    /// size, stops when the buffer goes, and is forgotten when the window
    /// is not shown.
    #[test]
    fn a_stream_follows_its_windows_buffers() {
        let mut streams = BTreeMap::new();
        let first = buffer(96, 64, 0xff20_4080, 1);
        let at = |updates: &BTreeMap<u64, VideoUpdate>| updates.get(&7).cloned();
        let key = at(&capture(&mut streams, &[(7, Some(&first))], 0));
        let Some(VideoUpdate::Frame(v)) = key else {
            panic!("a first frame")
        };
        assert_eq!((v.codec, v.width, v.height), (VIDEO_VP9, 96, 64));
        let mut d = vp9::Decoder::new();
        assert!(
            d.decode(&v.frame).unwrap().is_some(),
            "a key frame decodes alone"
        );

        // The same buffer again: nothing new.
        assert_eq!(at(&capture(&mut streams, &[(7, Some(&first))], 20)), None);
        // A new one: the next frame, which decodes after the first.
        let second = buffer(96, 64, 0xff80_4020, 2);
        let Some(VideoUpdate::Frame(v)) = at(&capture(&mut streams, &[(7, Some(&second))], 40))
        else {
            panic!("a second frame")
        };
        assert!(d.decode(&v.frame).unwrap().is_some());

        // Another size: a stream started again, with a key frame.
        let bigger = buffer(128, 64, 0xff80_4020, 3);
        let Some(VideoUpdate::Frame(v)) = at(&capture(&mut streams, &[(7, Some(&bigger))], 60))
        else {
            panic!("a new stream")
        };
        assert_eq!((v.width, v.height), (128, 64));
        assert!(vp9::Decoder::new().decode(&v.frame).unwrap().is_some());

        // The buffer gone: the video stops, once.
        assert_eq!(
            at(&capture(&mut streams, &[(7, None)], 80)),
            Some(VideoUpdate::Stop)
        );
        assert_eq!(at(&capture(&mut streams, &[(7, None)], 100)), None);

        // A window not shown is forgotten: back, it starts with a key frame.
        capture(&mut streams, &[(7, Some(&bigger))], 120);
        assert!(capture(&mut streams, &[], 140).is_empty());
        assert!(streams.is_empty());
        let Some(VideoUpdate::Frame(v)) = at(&capture(&mut streams, &[(7, Some(&bigger))], 160))
        else {
            panic!("a fresh stream")
        };
        assert!(vp9::Decoder::new().decode(&v.frame).unwrap().is_some());
    }
}
