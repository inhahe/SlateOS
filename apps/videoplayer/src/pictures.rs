//! The film's pictures: decoded on a thread of their own, and taken by the
//! window as its clock reaches them.
//!
//! `videocodec::Video` (lane F's) turns a file into pictures: it reads the
//! container, decodes the video track and converts each picture to pixels.
//! A picture of a full-HD film is eight megabytes and can take most of a
//! frame's time to decode, so the window's thread -- which draws, and answers
//! the keyboard and the pointer -- never does it. A [`Pictures`] runs the
//! decoder on a thread of its own and hands the frames over through a queue
//! [`QUEUE`] deep, so that the thread is never more than a few frames ahead
//! and a paused film holds no more than that.
//!
//! # The clock is the window's
//!
//! The player keeps its own clock (`VideoPlayerApp::tick`), as it did before
//! anything decoded, and asks for the frame showing at a time
//! ([`Pictures::show_at`]): the latest at or before it. The thread reads the
//! clock too, through an atomic the window writes, so that a picture already
//! behind it -- a decoder slower than the film, a film played fast -- is
//! passed over without its conversion (`Video::next_picture`), the costlier
//! half of the work, rather than converted to be thrown away. Only the
//! picture the clock is still on is converted -- and, while a decoder slower
//! than the film keeps every picture behind the clock, one every
//! [`LONGEST_UNSHOWN`], so that the picture still moves.
//!
//! # Seeking
//!
//! [`Pictures::seek`] numbers each seek, and the thread stamps every frame
//! with the number of the seek it follows. A frame stamped with an older
//! number -- decoded before the seek, and still in the queue -- is dropped
//! when it is taken, so it can never be shown after the seek. The first frame
//! after a seek is shown whatever its time: it is the picture at the time
//! sought, or the film's first where the time is before it.
//!
//! # Ending
//!
//! Dropping a [`Pictures`] ends its thread: the queue's other end is gone,
//! and the thread's next hand-over or wait says so.

use std::io;
use std::sync::atomic::{AtomicI64, Ordering};
#[cfg(test)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::Waker;
use std::thread;

use guitk::canvas::WireBytes;

use crate::grade::Grade;

pub use videocodec::{Frame, SeekMode};

/// How many converted frames the thread may have waiting. Two ride out a
/// picture that takes longer than its time to decode; few enough that a 4K
/// film's queue holds 66 MB rather than a second of pictures.
pub const QUEUE: usize = 2;

/// The window's waker, shared with every film's thread: filled once, when
/// the window attaches it (`App::attach_waker`) -- which may be after a film
/// named on the command line has started decoding.
pub type WakerSlot = Arc<OnceLock<Waker>>;

/// The Adjustments tab's grade, shared with every film's thread, which puts
/// each picture through it as it is converted: `None` for none.
pub type GradeSlot = Arc<Mutex<Option<Arc<Grade>>>>;

/// A picture ready to go on screen: graded, and its pixels in the
/// compositor's byte order already -- eight megabytes a full-HD picture,
/// copied on the thread rather than on the window's.
pub struct Ready {
    /// When it is shown, in nanoseconds on the film's clock.
    pub time: i64,
    /// How long, in nanoseconds; 0 where the file does not say.
    pub duration: u64,
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, for `ImageChange::Upload` as `Argb8888`.
    pub bytes: WireBytes,
}

impl Ready {
    /// `frame` through `grade`, if there is one, and into the wire's order.
    fn of(mut frame: Frame, grade: Option<&Grade>) -> Self {
        if let Some(grade) = grade {
            grade.apply(&mut frame);
        }
        Self {
            time: frame.time,
            duration: frame.duration,
            width: frame.width,
            height: frame.height,
            bytes: WireBytes::from_le_argb(&frame.pixels),
        }
    }
}

/// The longest the thread passes pictures over before converting one anyway.
///
/// A decoder slower than the film finds every picture behind the clock by
/// the time it is decoded; passing them all over would leave the picture on
/// screen frozen until the film ended. With this, such a film still moves,
/// four pictures a second at the least.
pub const LONGEST_UNSHOWN: std::time::Duration = std::time::Duration::from_millis(250);

/// What the thread decodes from: a [`videocodec::Video`] in the player, a
/// list of pictures in tests -- so that what is passed over, converted and
/// handed back can be counted.
pub trait Source: Send + 'static {
    /// A decoded picture, not yet converted.
    type Picture: Send;

    /// The next picture, `None` at the end.
    ///
    /// # Errors
    ///
    /// Why the film cannot be read further: the file failed.
    fn next(&mut self) -> Result<Option<Self::Picture>, String>;

    /// When `picture` is shown, in nanoseconds on the film's clock.
    fn time(picture: &Self::Picture) -> i64;

    /// `picture`, as pixels.
    ///
    /// # Errors
    ///
    /// Why it cannot be shown: a colour description that cannot be
    /// converted, which no picture of the film would pass either.
    fn convert(&mut self, picture: &Self::Picture) -> Result<Frame, String>;

    /// Go to `time`: the next picture is the one `mode` says.
    ///
    /// # Errors
    ///
    /// Why the film cannot be read from there: the file failed.
    fn seek(&mut self, time: i64, mode: SeekMode) -> Result<(), String>;
}

impl<R: io::Read + io::Seek + Send + 'static> Source for videocodec::Video<R> {
    type Picture = videocodec::Picture;

    fn next(&mut self) -> Result<Option<Self::Picture>, String> {
        self.next_picture().map_err(|e| e.to_string())
    }

    fn time(picture: &Self::Picture) -> i64 {
        picture.time
    }

    fn convert(&mut self, picture: &Self::Picture) -> Result<Frame, String> {
        videocodec::Video::convert(self, picture).map_err(|e| e.to_string())
    }

    fn seek(&mut self, time: i64, mode: SeekMode) -> Result<(), String> {
        videocodec::Video::seek(self, time, mode).map_err(|e| e.to_string())
    }
}

/// What the window asks of the thread.
enum Ask {
    /// Go to `time`, as `mode` says; stamp what follows `seek`.
    Seek {
        time: i64,
        mode: SeekMode,
        seek: u64,
    },
}

/// What the thread hands back, each stamped with the seek it follows.
enum Delivery {
    Frame {
        seek: u64,
        frame: Ready,
    },
    /// No pictures after the last handed over.
    End {
        seek: u64,
    },
    /// The film cannot be read further, or its pictures shown: why.
    Failed {
        seek: u64,
        why: String,
    },
}

/// What waits after the picture being handed over.
enum Ahead<P> {
    /// Nothing decoded yet.
    Nothing,
    /// The next picture, decoded and not converted.
    Picture(P),
    /// The film's end.
    End,
    /// The film failed there: why.
    Failed(String),
}

/// A film's pictures, decoded on a thread of their own; see the module docs.
pub struct Pictures {
    asks: Sender<Ask>,
    delivered: Receiver<Delivery>,
    /// The window's clock, in nanoseconds on the film's: written by the
    /// window, read by the thread to pass over pictures already behind it.
    clock: Arc<AtomicI64>,
    /// The newest seek's number. Frames stamped with an older one are dropped.
    seek: u64,
    /// The next frame, taken from the queue and not yet due.
    next: Option<Ready>,
    /// Nothing shown since the last seek, or since the film was opened: the
    /// first frame to come is shown whatever its time.
    fresh: bool,
    /// The thread has handed over the last picture of the film.
    end_taken: bool,
    /// Why the film stopped, if it failed.
    failed: Option<String>,
}

impl Pictures {
    /// Decode `source` on a thread of its own, from its start. The waker in
    /// `waker`, once there is one, is woken whenever a frame, the end or a
    /// failure is handed over.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started.
    pub fn start<S: Source>(source: S, waker: WakerSlot, grade: GradeSlot) -> io::Result<Self> {
        let (asks, asked) = mpsc::channel();
        let (out, delivered) = mpsc::sync_channel(QUEUE);
        let clock = Arc::new(AtomicI64::new(0));
        let theirs = Arc::clone(&clock);
        thread::Builder::new()
            .name(String::from("videoplayer-pictures"))
            .spawn(move || decode(source, &asked, &out, &theirs, &waker, &grade))?;
        Ok(Self {
            asks,
            delivered,
            clock,
            seek: 0,
            next: None,
            fresh: true,
            end_taken: false,
            failed: None,
        })
    }

    /// Go to `time` (nanoseconds on the film's clock): the frame
    /// [`Self::show_at`] gives next is the one `mode` lands on, shown
    /// whatever its time. Until it comes, the frame showing stays.
    pub fn seek(&mut self, time: i64, mode: SeekMode) {
        self.seek = self.seek.wrapping_add(1);
        self.clock.store(time, Ordering::Relaxed);
        self.next = None;
        self.fresh = true;
        self.end_taken = false;
        self.failed = None;
        // The thread may be waiting to hand over a frame of the last seek,
        // with the queue full: taking what is queued frees it to read this.
        while self.delivered.try_recv().is_ok() {}
        // The thread only ends when this end is dropped, so a send cannot
        // fail while `self` lives; if it somehow had, the film is over and
        // `show_at` will say so.
        if self
            .asks
            .send(Ask::Seek {
                time,
                mode,
                seek: self.seek,
            })
            .is_err()
        {
            self.failed = Some(String::from("the decoder stopped"));
        }
    }

    /// The frame to show at `now`, if it is not the one shown already: the
    /// latest of those come that are due by then, or the first after a seek.
    /// Frames passed over in between are dropped.
    pub fn show_at(&mut self, now: i64) -> Option<Ready> {
        self.clock.store(now, Ordering::Relaxed);
        let mut shown = None;
        loop {
            if self.next.is_none() {
                self.next = self.take();
            }
            match &self.next {
                Some(frame) if self.fresh || frame.time <= now => {
                    shown = self.next.take();
                    self.fresh = false;
                }
                _ => break,
            }
        }
        shown
    }

    /// When the next frame is due, if it has come.
    pub fn due(&self) -> Option<i64> {
        self.next.as_ref().map(|f| f.time)
    }

    /// Whether every picture of the film has been shown: the last handed over
    /// and taken.
    pub fn ended(&self) -> bool {
        self.end_taken && self.next.is_none()
    }

    /// Why the film stopped being read, if it failed.
    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// Wait up to `timeout` for the thread to hand something over, for a
    /// caller with nothing else to do: a test, which has no waker. Returns
    /// whether anything came.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: std::time::Duration) -> bool {
        if self.next.is_some() {
            return true;
        }
        match self.delivered.recv_timeout(timeout) {
            Ok(delivery) => {
                self.next = self.accept(delivery);
                true
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => false,
        }
    }

    /// The next frame of the newest seek, from the queue; the end or a
    /// failure, noted, along the way.
    fn take(&mut self) -> Option<Ready> {
        loop {
            match self.delivered.try_recv() {
                Ok(delivery) => {
                    if let Some(frame) = self.accept(delivery) {
                        return Some(frame);
                    }
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    // The thread only ends on its own when this end is gone;
                    // reaching here means it died, which says the same as a
                    // failure.
                    if !self.end_taken && self.failed.is_none() {
                        self.failed = Some(String::from("the decoder stopped"));
                    }
                    return None;
                }
            }
        }
    }

    /// One delivery: a frame of the newest seek, or nothing -- the end or a
    /// failure noted, anything of an older seek dropped.
    fn accept(&mut self, delivery: Delivery) -> Option<Ready> {
        match delivery {
            Delivery::Frame { seek, frame } if seek == self.seek => Some(frame),
            Delivery::End { seek } if seek == self.seek => {
                self.end_taken = true;
                None
            }
            Delivery::Failed { seek, why } if seek == self.seek => {
                self.failed = Some(why);
                None
            }
            Delivery::Frame { .. } | Delivery::End { .. } | Delivery::Failed { .. } => None,
        }
    }
}

/// The thread: decode `source` from its start, handing each picture the
/// clock has not left behind over to `out`, until the window is gone.
fn decode<S: Source>(
    mut source: S,
    asks: &Receiver<Ask>,
    out: &SyncSender<Delivery>,
    clock: &AtomicI64,
    waker: &OnceLock<Waker>,
    grade: &Mutex<Option<Arc<Grade>>>,
) {
    let mut seek = 0_u64;
    let mut ahead: Ahead<S::Picture> = Ahead::Nothing;
    let mut asked: Option<Ask> = None;
    // The first picture after a seek is the one the seek asked for, and the
    // window shows it whatever the clock says (`Pictures::fresh`). Passing
    // it over against a clock the window has not moved yet -- a drag's first
    // key frame, measured against where the film was before the drag --
    // showed the picture after it instead, whenever this thread won the race.
    let mut first_since_seek = true;
    loop {
        // The newest seek asked for: an older one waiting behind it is moot.
        loop {
            match asks.try_recv() {
                Ok(ask) => asked = Some(ask),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if let Some(Ask::Seek {
            time,
            mode,
            seek: number,
        }) = asked.take()
        {
            seek = number;
            first_since_seek = true;
            ahead = match source.seek(time, mode) {
                Ok(()) => Ahead::Nothing,
                Err(why) => Ahead::Failed(why),
            };
        }

        // The picture to hand over next.
        let picture = match std::mem::replace(&mut ahead, Ahead::Nothing) {
            Ahead::Picture(picture) => picture,
            Ahead::Nothing => match source.next() {
                Ok(Some(picture)) => picture,
                Ok(None) => {
                    ahead = Ahead::End;
                    continue;
                }
                Err(why) => {
                    ahead = Ahead::Failed(why);
                    continue;
                }
            },
            last @ (Ahead::End | Ahead::Failed(_)) => {
                let delivery = match last {
                    Ahead::Failed(why) => Delivery::Failed { seek, why },
                    _ => Delivery::End { seek },
                };
                if !hand(out, waker, delivery) {
                    return;
                }
                // Nothing more until the window seeks, or goes.
                match asks.recv() {
                    Ok(ask) => {
                        asked = Some(ask);
                        continue;
                    }
                    Err(_) => return,
                }
            }
        };

        // Passed over, unconverted, while the picture after it is already
        // due: the clock has left it behind before it could be shown -- for
        // no longer than `LONGEST_UNSHOWN`, so that a decoder slower than
        // the film still moves the picture.
        let mut picture = picture;
        let mut interrupted = false;
        let passing_since = std::time::Instant::now();
        while !first_since_seek {
            match asks.try_recv() {
                Ok(ask) => {
                    // A seek makes this picture moot as well.
                    asked = Some(ask);
                    interrupted = true;
                    break;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return,
            }
            match source.next() {
                Ok(Some(after))
                    if S::time(&after) <= clock.load(Ordering::Relaxed)
                        && passing_since.elapsed() < LONGEST_UNSHOWN =>
                {
                    picture = after;
                }
                Ok(Some(after)) => {
                    ahead = Ahead::Picture(after);
                    break;
                }
                Ok(None) => {
                    ahead = Ahead::End;
                    break;
                }
                Err(why) => {
                    ahead = Ahead::Failed(why);
                    break;
                }
            }
        }
        if interrupted {
            ahead = Ahead::Nothing;
            continue;
        }
        first_since_seek = false;

        let delivery = match source.convert(&picture) {
            Ok(frame) => Delivery::Frame {
                seek,
                frame: Ready::of(frame, current(grade).as_deref()),
            },
            Err(why) => {
                // No picture of the film would convert either: it is said,
                // and the thread waits for a seek -- which will meet the same.
                ahead = Ahead::Nothing;
                Delivery::Failed { seek, why }
            }
        };
        let failed = matches!(delivery, Delivery::Failed { .. });
        if !hand(out, waker, delivery) {
            return;
        }
        if failed {
            match asks.recv() {
                Ok(ask) => asked = Some(ask),
                Err(_) => return,
            }
        }
    }
}

/// The grade in `slot` now. A slot whose lock was poisoned -- a panic while
/// it was held, which a build that aborts on panic never sees -- still holds
/// the last grade put in it, and that is used.
fn current(slot: &Mutex<Option<Arc<Grade>>>) -> Option<Arc<Grade>> {
    match slot.lock() {
        Ok(grade) => grade.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Hand `delivery` to the window, waiting while its queue is full, and wake
/// it, if it has given a waker yet. False when the window is gone.
fn hand(out: &SyncSender<Delivery>, waker: &OnceLock<Waker>, delivery: Delivery) -> bool {
    if out.send(delivery).is_err() {
        return false;
    }
    if let Some(waker) = waker.get() {
        waker.wake_by_ref();
    }
    true
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    /// A film of pictures at the given times, counting what is converted.
    struct Film {
        times: Vec<i64>,
        at: usize,
        converted: Arc<Mutex<Vec<i64>>>,
        /// A picture to fail at, as a damaged file does.
        fail_at: Option<usize>,
        /// How long each picture takes to decode.
        delay: Duration,
    }

    impl Film {
        fn new(times: &[i64]) -> (Self, Arc<Mutex<Vec<i64>>>) {
            let converted = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    times: times.to_vec(),
                    at: 0,
                    converted: Arc::clone(&converted),
                    fail_at: None,
                    delay: Duration::ZERO,
                },
                converted,
            )
        }
    }

    impl Source for Film {
        type Picture = i64;

        fn next(&mut self) -> Result<Option<i64>, String> {
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            if self.fail_at == Some(self.at) {
                return Err(String::from("the disk failed"));
            }
            let t = self.times.get(self.at).copied();
            self.at += 1;
            Ok(t)
        }

        fn time(picture: &i64) -> i64 {
            *picture
        }

        fn convert(&mut self, picture: &i64) -> Result<Frame, String> {
            self.converted.lock().unwrap().push(*picture);
            Ok(Frame {
                time: *picture,
                duration: 0,
                keyframe: true,
                width: 1,
                height: 1,
                pixels: vec![0xFF00_0000],
            })
        }

        fn seek(&mut self, time: i64, mode: SeekMode) -> Result<(), String> {
            // The latest picture at or before the time, for both modes: every
            // picture of this film is a key frame.
            let _ = mode;
            self.at = self.times.iter().rposition(|&t| t <= time).unwrap_or(0);
            Ok(())
        }
    }

    const WAIT: Duration = Duration::from_secs(10);

    /// The picture `show_at(now)` settles on once the thread has caught up --
    /// handed over a picture after `now`, or the end -- or `None` if the one
    /// showing stays.
    fn shown_at(pictures: &mut Pictures, now: i64) -> Option<i64> {
        let mut shown = None;
        loop {
            if let Some(frame) = pictures.show_at(now) {
                shown = Some(frame.time);
            }
            let caught_up = pictures.due().is_some_and(|due| due > now)
                || pictures.ended()
                || pictures.failed().is_some();
            if caught_up || !pictures.wait(WAIT) {
                return shown;
            }
        }
    }

    #[test]
    fn the_first_picture_shows_at_once_and_the_rest_when_due() {
        let (film, _) = Film::new(&[0, 40, 80, 120]);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
        assert_eq!(shown_at(&mut pictures, 39), None, "not due yet");
        assert_eq!(pictures.due(), Some(40));
        assert_eq!(shown_at(&mut pictures, 40), Some(40));
        assert_eq!(shown_at(&mut pictures, 200), Some(120), "the latest due");
        assert!(pictures.ended(), "the film's end");
    }

    #[test]
    fn a_film_that_starts_late_shows_its_first_picture_at_once() {
        let (film, _) = Film::new(&[1_000, 1_040]);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(1_000));
        assert_eq!(shown_at(&mut pictures, 500), None);
    }

    #[test]
    fn a_picture_the_clock_has_left_behind_is_not_converted() {
        let times: Vec<i64> = (0..10).map(|i| i * 40).collect();
        let (film, converted) = Film::new(&times);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
        // The clock jumps on while the thread waits on a full queue: once
        // freed -- by the window taking from the queue, which it does only
        // after setting the clock -- it passes over everything already
        // behind the clock.
        assert_eq!(
            shown_at(&mut pictures, 330),
            Some(320),
            "the picture showing at 330"
        );
        let converted = converted.lock().unwrap().clone();
        assert!(
            ![200, 240, 280].iter().any(|t| converted.contains(t)),
            "pictures behind the clock were converted: {converted:?}"
        );
        assert!(converted.contains(&320), "{converted:?}");
    }

    #[test]
    fn a_seek_shows_the_picture_at_its_time_and_nothing_from_before_it() {
        let times: Vec<i64> = (0..100).map(|i| i * 40).collect();
        let (film, _) = Film::new(&times);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
        // Let the thread fill the queue with the start of the film.
        std::thread::sleep(Duration::from_millis(50));
        pictures.seek(2_010, SeekMode::Exact);
        assert_eq!(
            shown_at(&mut pictures, 2_010),
            Some(2_000),
            "the picture at the time sought, not one queued before the seek"
        );
        assert_eq!(shown_at(&mut pictures, 2_040), Some(2_040));
        // A seek back.
        pictures.seek(100, SeekMode::KeyFrame);
        assert_eq!(shown_at(&mut pictures, 100), Some(80));
    }

    #[test]
    fn the_picture_a_seek_asks_for_is_not_passed_over_for_a_clock_left_behind() {
        // The window moves the clock after it asks for a seek -- a drag
        // holds it -- so the thread can decode the seek's picture while the
        // clock still says where the film was. That picture is shown
        // whatever the clock says, so it must not be passed over against it.
        let times: Vec<i64> = (0..100).map(|i| i * 40).collect();
        let (film, _) = Film::new(&times);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        // A picture handed over first, so that the seek below is not the
        // film's start, where the thread begins as if just sought.
        assert!(pictures.wait(Duration::from_secs(10)), "nothing came");
        // Only the clock matters here: it is left at 3 s, whatever shows.
        let _whatever_shows = pictures.show_at(3_000);
        pictures.seek(400, SeekMode::KeyFrame);
        while pictures.due().is_none() {
            assert!(
                pictures.wait(Duration::from_secs(10)),
                "nothing came after the seek"
            );
        }
        let due = pictures.due().unwrap_or(i64::MAX);
        assert!(
            due <= 400,
            "the seek's picture was passed over for the one at {due}"
        );
    }

    #[test]
    fn a_seek_after_the_end_plays_again() {
        let (film, _) = Film::new(&[0, 40]);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 100), Some(40));
        assert!(pictures.ended());
        pictures.seek(0, SeekMode::Exact);
        assert!(!pictures.ended(), "a seek starts it again");
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
    }

    #[test]
    fn a_failure_is_said_after_the_pictures_before_it() {
        let (mut film, _) = Film::new(&[0, 40, 80]);
        film.fail_at = Some(2);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
        assert_eq!(pictures.failed(), None, "said before its pictures");
        assert_eq!(shown_at(&mut pictures, 100), Some(40));
        assert_eq!(pictures.failed(), Some("the disk failed"));
        assert!(!pictures.ended(), "a failure is not the end");
    }

    #[test]
    fn the_window_is_woken_for_each_frame() {
        use std::sync::atomic::AtomicUsize;
        use std::task::Wake;
        struct Count(AtomicUsize);
        impl Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let count = Arc::new(Count(AtomicUsize::new(0)));
        // More to hand over than the queue holds, so that some of it waits
        // for the window to take what is queued -- after the waker is in.
        let (film, _) = Film::new(&[0, 40, 80, 120]);
        let slot = WakerSlot::default();
        let mut pictures = Pictures::start(film, Arc::clone(&slot), GradeSlot::default()).unwrap();
        // Attached after the film started, as a window attaches it after a
        // film named on its command line has started.
        assert!(slot.set(Waker::from(Arc::clone(&count))).is_ok());
        assert_eq!(shown_at(&mut pictures, 1_000), Some(120));
        assert!(pictures.ended());
        // The thread wakes the window just after each hand-over.
        let deadline = std::time::Instant::now() + WAIT;
        while count.0.load(Ordering::Relaxed) == 0 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            count.0.load(Ordering::Relaxed) >= 1,
            "nothing handed over after the waker came woke the window"
        );
    }

    /// A waker that counts its wakes.
    struct Wakes(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// **A seek reaches a thread waiting on a full queue** -- a paused film's
    /// -- by itself: the window, paused, asks for no picture until one comes,
    /// so nothing else would ever free the thread to read the seek.
    #[test]
    fn a_seek_reaches_a_thread_waiting_on_a_full_queue() {
        let times: Vec<i64> = (0..100).map(|i| i * 40).collect();
        let (film, _) = Film::new(&times);
        let wakes = Arc::new(Wakes(std::sync::atomic::AtomicUsize::new(0)));
        let slot = WakerSlot::default();
        assert!(slot.set(Waker::from(Arc::clone(&wakes))).is_ok());
        let mut pictures = Pictures::start(film, slot, GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
        // The queue full behind the picture held: the thread is waiting to
        // hand over the next.
        let deadline = std::time::Instant::now() + WAIT;
        let mut before = wakes.0.load(Ordering::Relaxed);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            let now = wakes.0.load(Ordering::Relaxed);
            if now == before {
                break;
            }
            before = now;
        }
        pictures.seek(2_010, SeekMode::Exact);
        // Nothing is taken from the queue now: the thread must free itself.
        while wakes.0.load(Ordering::Relaxed) < before + 2 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            wakes.0.load(Ordering::Relaxed) >= before + 2,
            "the seek's picture never came to a full queue"
        );
        assert_eq!(shown_at(&mut pictures, 2_010), Some(2_000));
    }

    /// **A decoder slower than the film still moves the picture**: every
    /// picture is behind the clock by the time it is decoded, and one is
    /// converted at least every `LONGEST_UNSHOWN` -- not none until the end.
    #[test]
    fn a_decoder_slower_than_the_film_still_moves_the_picture() {
        let times: Vec<i64> = (0..10_000).map(|i| i * 40).collect();
        let (mut film, converted) = Film::new(&times);
        film.delay = Duration::from_millis(2);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        // The clock far past the film: every picture is behind it.
        let far = i64::MAX / 2;
        let deadline = std::time::Instant::now() + WAIT;
        let mut shown = 0;
        while shown < 4 && std::time::Instant::now() < deadline {
            if pictures.show_at(far).is_some() {
                shown += 1;
            }
            pictures.wait(Duration::from_millis(100));
        }
        assert!(
            shown >= 4,
            "the picture froze: {} shown, {} converted",
            shown,
            converted.lock().unwrap().len()
        );
        assert!(
            converted.lock().unwrap().len() < 100,
            "it converted the pictures it should have passed over"
        );
    }

    /// **The thread puts each picture through the grade in the slot**, as
    /// it stands when the picture is converted: black lifted to white with
    /// brightness at its top, and back to black when the grade is taken out.
    #[test]
    fn each_picture_goes_through_the_grade_in_the_slot() {
        let (film, _) = Film::new(&[0, 40, 80, 120]);
        let grade = GradeSlot::default();
        let lift = crate::VideoAdjustments {
            brightness: 1.0,
            ..crate::VideoAdjustments::default()
        };
        *grade.lock().unwrap() = Grade::new(&lift).map(Arc::new);
        let mut pictures = Pictures::start(film, WakerSlot::default(), Arc::clone(&grade)).unwrap();
        let colour = |ready: &Ready| ready.bytes.as_slice().get(..3).map(<[u8]>::to_vec);
        let first = loop {
            if let Some(ready) = pictures.show_at(0) {
                break ready;
            }
            assert!(pictures.wait(WAIT));
        };
        assert_eq!(colour(&first), Some(vec![255, 255, 255]), "not lifted");
        *grade.lock().unwrap() = None;
        // The queue already holds pictures graded before; a seek starts the
        // film again under the grade as it is now.
        pictures.seek(0, SeekMode::Exact);
        let again = loop {
            if let Some(ready) = pictures.show_at(0) {
                break ready;
            }
            assert!(pictures.wait(WAIT));
        };
        assert_eq!(colour(&again), Some(vec![0, 0, 0]), "still lifted");
    }

    /// **Two seeks in a row land on the second**, the first passed over.
    #[test]
    fn two_seeks_in_a_row_land_on_the_second() {
        let times: Vec<i64> = (0..100).map(|i| i * 40).collect();
        let (film, _) = Film::new(&times);
        let mut pictures =
            Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        assert_eq!(shown_at(&mut pictures, 0), Some(0));
        pictures.seek(1_000, SeekMode::Exact);
        pictures.seek(3_000, SeekMode::Exact);
        assert_eq!(shown_at(&mut pictures, 3_000), Some(3_000));
        assert_eq!(shown_at(&mut pictures, 3_050), Some(3_040));
    }

    #[test]
    fn dropping_the_pictures_ends_the_thread() {
        let times: Vec<i64> = (0..1_000).map(|i| i * 40).collect();
        let (film, converted) = Film::new(&times);
        let pictures = Pictures::start(film, WakerSlot::default(), GradeSlot::default()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        drop(pictures);
        std::thread::sleep(Duration::from_millis(50));
        let after_drop = converted.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            converted.lock().unwrap().len(),
            after_drop,
            "the thread went on decoding with nobody to show it to"
        );
        assert!(
            after_drop <= QUEUE + 2,
            "it ran ahead of a queue {QUEUE} deep"
        );
    }
}
