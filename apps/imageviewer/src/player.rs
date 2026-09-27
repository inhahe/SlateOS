//! Playing an animated GIF or WebP, frame by frame, off the window's thread.
//!
//! `imagecodec` decodes an animation one composited frame at a time
//! (`gif::Animation`, `webp::Animation`; lane F's
//! `requests/f-ce-gif-decodes-and-animates.md`), lending each frame from one
//! canvas it keeps. A [`Player`] runs that on a thread of its own and hands
//! the frames over through a channel one deep: the thread is never more than
//! a frame ahead, so a 500-frame animation costs two frames of memory, not
//! five hundred -- and the window's thread never decodes one.
//!
//! The window takes a frame when the one showing has been up its time
//! ([`Frame::delay_ms`], the browsers' timing: a GIF's 0 or 1 hundredths are
//! a tenth of a second, a WebP's 10 ms or less is 100 ms). Frames are turned
//! as the picture is (`View`), on the player's thread.
//!
//! An animation plays as its file says: forever, or a number of times -- a
//! GIF's loop count is the number of plays *after* the first, as browsers
//! read it; a WebP's is the number of plays in all. When it ends, the last
//! frame stays up. A frame that will not decode ends it there too (libwebp's
//! behaviour; a GIF's broken frame draws what it has, inside `imagecodec`).

use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread;

use imagecodec::orientation::Orientation;

/// One frame of an animation, turned, and how long it stays up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The whole picture at this frame, composited.
    pub image: imagecodec::Image,
    /// How long it is shown, in milliseconds.
    pub delay_ms: u32,
    /// The first frame of the first play: the one already on screen when the
    /// player starts, decoded with the picture.
    pub first: bool,
}

/// What asking for the next frame came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Here it is.
    Frame(Frame),
    /// Not decoded yet: ask again shortly.
    NotYet,
    /// The animation has ended; the frame on screen is its last.
    Ended,
}

/// An animation being played; see the module docs.
///
/// Dropping it stops the player's thread at its next frame.
pub struct Player {
    frames: Receiver<Frame>,
}

/// Which animation format a file is, if it is one this plays.
fn kind(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else if imagecodec::webp::is_webp(bytes) {
        Some(Kind::WebP)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Gif,
    WebP,
}

/// Whether `bytes` are an animation -- a GIF or a WebP of more than one
/// frame -- read from its structure alone.
#[must_use]
pub fn is_animation(bytes: &[u8]) -> bool {
    let limits = imagecodec::Limits::default();
    match kind(bytes) {
        Some(Kind::Gif) => {
            imagecodec::gif::Animation::new(bytes, limits).is_ok_and(|a| a.frame_count() > 1)
        }
        Some(Kind::WebP) => {
            imagecodec::webp::Animation::new(bytes, limits).is_ok_and(|a| a.frame_count() > 1)
        }
        None => false,
    }
}

impl Player {
    /// Start playing the animation in `bytes`, each frame turned by `turn`.
    ///
    /// # Errors
    ///
    /// When the player's thread cannot be started; the picture's first frame,
    /// already on screen, stays.
    pub fn start(bytes: Vec<u8>, turn: Orientation) -> std::io::Result<Self> {
        let (send, frames) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name(String::from("imageviewer-player"))
            .spawn(move || match kind(&bytes) {
                Some(Kind::Gif) => play_gif(&bytes, turn, &send),
                Some(Kind::WebP) => play_webp(&bytes, turn, &send),
                None => {}
            })?;
        Ok(Self { frames })
    }

    /// The next frame, if the player has it.
    pub fn next(&mut self) -> Next {
        match self.frames.try_recv() {
            Ok(frame) => Next::Frame(frame),
            Err(TryRecvError::Empty) => Next::NotYet,
            Err(TryRecvError::Disconnected) => Next::Ended,
        }
    }
}

/// Send one frame; `false` when the viewer has stopped listening.
fn send(
    to: &SyncSender<Frame>,
    image: &imagecodec::Image,
    delay_ms: u32,
    first: bool,
    turn: Orientation,
) -> bool {
    to.send(Frame {
        image: turn.apply(image.clone()),
        delay_ms,
        first,
    })
    .is_ok()
}

fn play_gif(bytes: &[u8], turn: Orientation, to: &SyncSender<Frame>) {
    use imagecodec::gif::{Animation, Repeat};
    let Ok(mut animation) = Animation::new(bytes, imagecodec::Limits::default()) else {
        return;
    };
    let mut plays: u32 = 0;
    loop {
        let mut index: usize = 0;
        // To the end of a play -- or of a file that stops mid-frame.
        while let Ok(Some(frame)) = animation.next_frame() {
            let first = plays == 0 && index == 0;
            if !send(to, frame.image, frame.display_delay_ms(), first, turn) {
                return;
            }
            index = index.saturating_add(1);
        }
        if index == 0 {
            // A play with no frame in it would loop forever doing nothing.
            return;
        }
        plays = plays.saturating_add(1);
        match animation.repeat() {
            Repeat::Forever => {}
            // The loop count is the plays after the first.
            Repeat::Count(more) if plays <= u32::from(more) => {}
            Repeat::Count(_) | Repeat::Once => return,
        }
        animation.rewind();
    }
}

fn play_webp(bytes: &[u8], turn: Orientation, to: &SyncSender<Frame>) {
    use imagecodec::webp::{Animation, Repeat};
    let Ok(mut animation) = Animation::new(bytes, imagecodec::Limits::default()) else {
        return;
    };
    let mut plays: u32 = 0;
    loop {
        let mut index: usize = 0;
        loop {
            let frame = match animation.next_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                // A broken frame ends the animation where it is.
                Err(_) => return,
            };
            let first = plays == 0 && index == 0;
            if !send(to, frame.image, frame.display_duration_ms(), first, turn) {
                return;
            }
            index = index.saturating_add(1);
        }
        if index == 0 {
            return;
        }
        plays = plays.saturating_add(1);
        match animation.repeat() {
            Repeat::Forever => {}
            // The count is the plays in all, the first included.
            Repeat::Times(all) if plays < u32::from(all) => {}
            Repeat::Times(_) => return,
        }
        animation.rewind();
    }
}
