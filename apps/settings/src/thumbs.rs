//! Pictures drawn small on the Background page: each theme's recommended
//! picture, and the pictures themes bundle (design-decisions §1243).
//!
//! A wallpaper is a full-screen photograph, and decoding one takes long
//! enough that a page doing it on the window's thread would stall on opening.
//! So each is read and decoded on a thread of its own, scaled as it is decoded
//! (`imagecodec::decode_scaled`, which never holds the photograph at its own
//! size), handed to the window's loop as an upload, and drawn by its id once
//! it is there. A picture that cannot be read or decoded is remembered as
//! such, and the page draws its card plain.
//!
//! Every picture is asked for by its path and decoded once for the life of
//! the window: the pages that draw these name a few dozen pictures at most,
//! each a few hundred kilobytes once small.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::task::Waker;

use oswindow::app::ImageChange;

/// The width a picture is scaled to fit while it is decoded, in pixels:
/// twice the card it is drawn on, so that it stays sharp at a scale of two.
pub const MAX_WIDTH: u32 = 352;
/// The height to fit, likewise.
pub const MAX_HEIGHT: u32 = 198;

/// The largest file read to make a thumbnail. A picture larger than this is
/// drawn as a plain card rather than read whole into memory to be made small.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// What is known of one picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thumb {
    /// Being read and decoded.
    Loading,
    /// Ready to draw: the id its pixels were uploaded under, and its size.
    Ready {
        /// The image id to draw it by.
        id: u64,
        /// Its width in pixels, as decoded.
        width: u32,
        /// Its height in pixels.
        height: u32,
    },
    /// Could not be read or decoded.
    Failed,
}

/// A decoded picture, or why there is none, from the worker.
struct Done {
    id: u64,
    result: Result<imagecodec::Image, String>,
}

/// The pictures a window has asked to draw small, and the thread making them.
pub struct Thumbs {
    /// Each picture asked for, by path: its id and what is known of it.
    by_path: HashMap<PathBuf, (u64, Thumb)>,
    /// The id the next picture asked for gets.
    next_id: u64,
    /// The worker's queue; `None` until the first picture is asked for, and
    /// again if the thread could not be started.
    jobs: Option<Sender<(u64, PathBuf)>>,
    /// What the worker has finished.
    done: Option<Receiver<Done>>,
    /// How the worker wakes the window's loop; shared, so a waker attached
    /// after the worker started still reaches it.
    waker: Arc<Mutex<Option<Waker>>>,
    /// Pictures decoded and not yet handed to the loop.
    uploads: Vec<ImageChange>,
}

impl Default for Thumbs {
    fn default() -> Self {
        Self::new()
    }
}

impl Thumbs {
    /// No pictures yet, and no thread: one is started by the first ask.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_path: HashMap::new(),
            // Ids a window's other pictures do not use: none, today, but an
            // id is the window's whole namespace, so these keep to a range.
            next_id: 0x7448_0000_0000_0001,
            jobs: None,
            done: None,
            waker: Arc::new(Mutex::new(None)),
            uploads: Vec::new(),
        }
    }

    /// The handle for waking the window's loop when a picture is ready.
    pub fn attach_waker(&mut self, waker: Waker) {
        // A poisoned lock is a worker that panicked while waking; the waker
        // is still the right one to store.
        let mut slot = self
            .waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(waker);
    }

    /// Ask for the picture at `path`, drawn small; what is known of it now.
    /// The first ask starts it decoding.
    pub fn ask(&mut self, path: &Path) -> Thumb {
        if let Some((_, thumb)) = self.by_path.get(path) {
            return *thumb;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let queued = self
            .worker()
            .is_some_and(|jobs| jobs.send((id, path.to_path_buf())).is_ok());
        let thumb = if queued {
            Thumb::Loading
        } else {
            Thumb::Failed
        };
        self.by_path.insert(path.to_path_buf(), (id, thumb));
        thumb
    }

    /// What is known of the picture at `path`; `None` if it was never asked
    /// for.
    #[must_use]
    pub fn get(&self, path: &Path) -> Option<Thumb> {
        self.by_path.get(path).map(|(_, thumb)| *thumb)
    }

    /// Take what the worker has finished: each decoded picture is queued for
    /// upload and marked ready, each failure marked failed. Whether anything
    /// changed, so the caller knows to draw.
    pub fn collect(&mut self) -> bool {
        let Some(done) = self.done.as_ref() else {
            return false;
        };
        let finished: Vec<Done> = done.try_iter().collect();
        let mut changed = false;
        for Done { id, result } in finished {
            let Some(entry) = self.by_path.values_mut().find(|(at, _)| *at == id) else {
                continue;
            };
            entry.1 = match result {
                Ok(image) => {
                    let (width, height) = (image.width, image.height);
                    self.uploads.push(ImageChange::Upload {
                        id,
                        width,
                        height,
                        // `imagecodec` never pads a row.
                        stride: width.saturating_mul(4),
                        format: oswindow::PixelFormat::Argb8888,
                        bytes: guitk::canvas::WireBytes::from_le_argb(&image.pixels),
                    });
                    Thumb::Ready { id, width, height }
                }
                Err(_why) => Thumb::Failed,
            };
            changed = true;
        }
        changed
    }

    /// The uploads waiting for the window's loop, drained.
    pub fn take_uploads(&mut self) -> Vec<ImageChange> {
        std::mem::take(&mut self.uploads)
    }

    /// The worker's queue, starting the worker if it is not running.
    fn worker(&mut self) -> Option<&Sender<(u64, PathBuf)>> {
        if self.jobs.is_none() {
            let (job_tx, job_rx) = mpsc::channel::<(u64, PathBuf)>();
            let (done_tx, done_rx) = mpsc::channel::<Done>();
            let waker = Arc::clone(&self.waker);
            let spawned = std::thread::Builder::new()
                .name(String::from("settings-thumbs"))
                .spawn(move || {
                    for (id, path) in job_rx {
                        let result = decode(&path);
                        if done_tx.send(Done { id, result }).is_err() {
                            // The window is gone; nothing is waiting.
                            return;
                        }
                        let slot = waker
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(waker) = slot.as_ref() {
                            waker.wake_by_ref();
                        }
                    }
                });
            // A thread that could not be started leaves every picture to be
            // drawn as a plain card: `ask` marks them failed.
            if spawned.is_ok() {
                self.jobs = Some(job_tx);
                self.done = Some(done_rx);
            }
        }
        self.jobs.as_ref()
    }
}

/// Read the picture at `path` and decode it small.
fn decode(path: &Path) -> Result<imagecodec::Image, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    // Read through `take`, so a file growing between a size check and the
    // read cannot be read past the cap.
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(String::from("too large to make small"));
    }
    imagecodec::decode_scaled(&bytes, imagecodec::Limits::default(), MAX_WIDTH, MAX_HEIGHT)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    /// A picture written to `dir` as a PNG, `width` by `height`, one colour.
    pub(crate) fn png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let count = usize::try_from(width.saturating_mul(height)).unwrap();
        let pixels = vec![0xFF33_6699_u32; count];
        let bytes = imagecodec::encode_png(width, height, &pixels).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Wait, for at most ten seconds, until the worker has answered about
    /// the picture at `path`.
    pub(crate) fn settle(thumbs: &mut Thumbs, path: &Path) -> Thumb {
        let until = std::time::Instant::now()
            .checked_add(std::time::Duration::from_secs(10))
            .unwrap();
        loop {
            thumbs.collect();
            let thumb = thumbs.get(path).unwrap();
            if thumb != Thumb::Loading || std::time::Instant::now() > until {
                return thumb;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// A picture is decoded off the thread, made no larger than the box and
    /// its shape kept, and uploaded once under the id it is drawn by.
    #[test]
    fn a_picture_is_made_small_and_uploaded_once() {
        let scratch = scratchdir::ScratchDir::new("settings-thumbs");
        let path = png(scratch.dir(), "wide.png", 1000, 500);
        let mut thumbs = Thumbs::new();
        assert_eq!(thumbs.ask(&path), Thumb::Loading);
        let Thumb::Ready { id, width, height } = settle(&mut thumbs, &path) else {
            panic!("not decoded: {:?}", thumbs.get(&path));
        };
        assert!(
            width <= MAX_WIDTH && height <= MAX_HEIGHT,
            "{width}x{height}"
        );
        assert_eq!(width, 2 * height, "its shape was not kept");
        let uploads = thumbs.take_uploads();
        assert_eq!(uploads.len(), 1);
        assert!(matches!(
            &uploads[0],
            ImageChange::Upload { id: up, width: w, height: h, .. }
                if *up == id && *w == width && *h == height
        ));
        // Asked again: the same, and nothing decoded or uploaded twice.
        assert_eq!(thumbs.ask(&path), Thumb::Ready { id, width, height });
        assert!(!thumbs.collect());
        assert!(thumbs.take_uploads().is_empty());
    }

    /// A file that is not a picture, and one that is not there, are failures
    /// -- drawn plain -- not a card that waits for ever.
    #[test]
    fn a_picture_that_cannot_be_read_fails() {
        let scratch = scratchdir::ScratchDir::new("settings-thumbs-bad");
        let text = scratch.dir().join("not-a-picture.png");
        std::fs::write(&text, "hello").unwrap();
        let gone = scratch.dir().join("gone.png");
        let mut thumbs = Thumbs::new();
        thumbs.ask(&text);
        thumbs.ask(&gone);
        assert_eq!(settle(&mut thumbs, &text), Thumb::Failed);
        assert_eq!(settle(&mut thumbs, &gone), Thumb::Failed);
        assert!(thumbs.take_uploads().is_empty());
    }

    /// A picture decoded wakes the window, through the waker attached --
    /// before any picture was asked for, as the window attaches it.
    #[test]
    fn a_picture_decoded_wakes_the_window() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Count(AtomicUsize);
        impl std::task::Wake for Count {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let scratch = scratchdir::ScratchDir::new("settings-thumbs-wake");
        let path = png(scratch.dir(), "a.png", 8, 8);
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let mut thumbs = Thumbs::new();
        thumbs.attach_waker(Waker::from(Arc::clone(&count)));
        thumbs.ask(&path);
        assert!(matches!(settle(&mut thumbs, &path), Thumb::Ready { .. }));
        // The wake follows the result by a moment.
        let until = std::time::Instant::now()
            .checked_add(std::time::Duration::from_secs(10))
            .unwrap();
        while count.0.load(Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < until,
                "the worker did not wake the window"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Two pictures get two ids.
    #[test]
    fn each_picture_has_an_id_of_its_own() {
        let scratch = scratchdir::ScratchDir::new("settings-thumbs-two");
        let a = png(scratch.dir(), "a.png", 8, 8);
        let b = png(scratch.dir(), "b.png", 8, 8);
        let mut thumbs = Thumbs::new();
        thumbs.ask(&a);
        thumbs.ask(&b);
        let (Thumb::Ready { id: ia, .. }, Thumb::Ready { id: ib, .. }) =
            (settle(&mut thumbs, &a), settle(&mut thumbs, &b))
        else {
            panic!("not decoded");
        };
        assert_ne!(ia, ib);
        assert_eq!(thumbs.take_uploads().len(), 2);
    }
}
