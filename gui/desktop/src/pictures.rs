//! Decoding the shell's pictures -- the wallpaper and the greeter's background
//! -- on a thread of their own.
//!
//! # Why
//!
//! A photograph from a camera takes about a second to decode in a release
//! build and several in a debug one (`known-issues.md`
//! `TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT` measures
//! 4000x5333). The session decoded on the thread that draws, so the desktop
//! drew nothing and took no input for that long -- at login, whenever a
//! wallpaper was chosen, and at every step of a slideshow.
//!
//! # How
//!
//! One worker thread per session. The session asks for a picture by the slot
//! it is for, the id it will be uploaded under and the file; the worker reads
//! and decodes it and sends the result back, then wakes the loop through the
//! event loop's waker, and the session collects what is ready at the start of
//! its next pump. Nothing here touches the connection: uploading stays on the
//! loop's thread, where the upload's ordering against the frame that names the
//! picture is kept (design-decisions §557, §861).
//!
//! Only the newest request for a slot is worth decoding. A slideshow that
//! steps, or a user trying one wallpaper after another, can queue several
//! before the first is done; the worker takes everything queued and decodes
//! the last for each slot, so a burst of changes costs one decode per slot.
//! The others are answered at once with [`SUPERSEDED`] -- every request gets
//! exactly one answer, which is what lets a caller count what it is owed.
//!
//! One file asked for twice in a row is decoded once. A greeter that follows
//! the desktop (`LoginBackground::SameAsDesktop`) asks for the file the
//! wallpaper has just asked for -- at login, both at once -- and the two
//! slots are two uploads to two surfaces but need not be two decodes. See
//! [`Recent`].
//!
//! The thread ends when the session drops its end of the request channel --
//! after the decode in hand, if one is -- and is not joined: waiting for a
//! photograph to finish decoding would make dropping a session take a second.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::task::Waker;
use std::time::SystemTime;

/// The reason given for a request that was not decoded because a newer one
/// for the same slot came first. The session ignores it with every other
/// answer for a picture it no longer wants.
pub(crate) const SUPERSEDED: &str = "superseded by a newer picture";

/// Which of the shell's pictures a decode is for. Each has one picture at a
/// time, so a newer request for a slot makes an older one moot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// The desktop's wallpaper, on the background surface.
    Wallpaper,
    /// The login screen's background, on the greeter's surface.
    Greeter,
    /// The picture a photo frame on the desktop shows, by the frame's widget
    /// id: on the background surface, one per frame.
    Frame(u64),
}

/// A picture to decode: for which slot, under which image id, from which file
/// -- and how small, for one drawn smaller than itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Job {
    pub slot: Slot,
    pub id: u64,
    pub path: PathBuf,
    /// Decoded to fit within this many pixels across and down, its
    /// proportions kept (`imagecodec::decode_scaled`): a photo frame's
    /// picture, which a photograph at its own size would fill the upload
    /// budget with pixels nobody sees. `None` at its own size -- the
    /// wallpaper, which is drawn the size of the screen.
    pub fit: Option<(u32, u32)>,
}

/// A decode's outcome, handed back to the loop's thread.
pub(crate) struct Decoded {
    /// What was asked for, so the session can tell whether it still wants it.
    pub job: Job,
    /// The picture, or why there is none, as a sentence naming the file.
    ///
    /// Shared rather than owned because one decode can answer two slots; the
    /// session only ever reads it, to upload it.
    pub result: Result<Arc<imagecodec::Image>, String>,
}

/// The session's end of the decoding thread.
pub(crate) struct PictureWorker {
    requests: Sender<Job>,
    results: Receiver<Decoded>,
    /// Requests sent and not yet answered: what [`Self::settle`] waits for.
    #[cfg(test)]
    outstanding: std::cell::Cell<usize>,
}

impl PictureWorker {
    /// Start the thread. `waker`, when there is one, is woken after each
    /// result is sent, so a loop parked with nothing on the wire comes round
    /// to collect it; with none, results wait for the loop's next pass.
    ///
    /// A thread the system will not start is not fatal: the worker is then
    /// one that never answers, which leaves the desktop on its plain
    /// background -- the same as a picture that would not decode.
    pub(crate) fn spawn(waker: Option<Waker>) -> Self {
        let (requests, jobs) = mpsc::channel::<Job>();
        let (done, results) = mpsc::channel::<Decoded>();
        let started = std::thread::Builder::new()
            .name("desktop-pictures".into())
            .spawn(move || work(&jobs, &done, waker.as_ref()));
        // A thread that could not start: the error is the outcome described
        // above, and the channels it would have used are simply dropped.
        drop(started);
        Self {
            requests,
            results,
            #[cfg(test)]
            outstanding: std::cell::Cell::new(0),
        }
    }

    /// Ask for `path` to be decoded for `slot` under `id`, at its own size.
    pub(crate) fn request(&self, slot: Slot, id: u64, path: PathBuf) {
        self.request_fitted(slot, id, path, None);
    }

    /// Ask for `path` to be decoded for `slot` under `id`, scaled to fit
    /// `fit` when there is one (see [`Job::fit`]).
    pub(crate) fn request_fitted(
        &self,
        slot: Slot,
        id: u64,
        path: PathBuf,
        fit: Option<(u32, u32)>,
    ) {
        // A send fails only if the thread has gone, which leaves the picture
        // undecoded: the desktop keeps its plain background, as it does for a
        // file that will not decode, rather than stopping.
        if self
            .requests
            .send(Job {
                slot,
                id,
                path,
                fit,
            })
            .is_ok()
        {
            #[cfg(test)]
            self.outstanding
                .set(self.outstanding.get().saturating_add(1));
        }
    }

    /// Every result that has arrived, oldest first, without waiting.
    pub(crate) fn take(&self) -> Vec<Decoded> {
        let ready: Vec<Decoded> = self.results.try_iter().collect();
        #[cfg(test)]
        self.outstanding
            .set(self.outstanding.get().saturating_sub(ready.len()));
        ready
    }

    /// Every answer still owed, waiting up to `timeout` for the last of them.
    ///
    /// For tests, which drive the session by hand and need the picture before
    /// the next assertion. Every request is answered exactly once -- a
    /// superseded one with [`SUPERSEDED`] -- so this knows when it is done.
    #[cfg(test)]
    pub(crate) fn settle(&self, timeout: std::time::Duration) -> Vec<Decoded> {
        let start = std::time::Instant::now();
        let mut ready = self.take();
        while self.outstanding.get() > 0 {
            let left = timeout.saturating_sub(start.elapsed());
            match self.results.recv_timeout(left) {
                Ok(decoded) => {
                    self.outstanding
                        .set(self.outstanding.get().saturating_sub(1));
                    ready.push(decoded);
                }
                Err(_) => break,
            }
        }
        ready
    }
}

/// The thread: take what is queued, decode the newest request for each slot,
/// send the results and wake the loop, until the session hangs up.
fn work(jobs: &Receiver<Job>, done: &Sender<Decoded>, waker: Option<&Waker>) {
    let mut recent = Recent::default();
    loop {
        let first = match jobs.try_recv() {
            Ok(job) => job,
            Err(TryRecvError::Empty) => {
                // Nothing behind the last decode, so nothing to share it
                // with: an idle worker holds no full-screen picture.
                recent.forget();
                match jobs.recv() {
                    Ok(job) => job,
                    Err(_) => return,
                }
            }
            Err(TryRecvError::Disconnected) => return,
        };
        let mut newest: Vec<Job> = vec![first];
        for job in jobs.try_iter() {
            match newest.iter_mut().find(|held| held.slot == job.slot) {
                Some(held) => {
                    let older = core::mem::replace(held, job);
                    let answer = Decoded {
                        job: older,
                        result: Err(SUPERSEDED.to_owned()),
                    };
                    if done.send(answer).is_err() {
                        return;
                    }
                }
                None => newest.push(job),
            }
        }
        for job in newest {
            let result = recent.decode(&job.path, job.fit);
            if done.send(Decoded { job, result }).is_err() {
                // The session is gone; nobody wants the rest.
                return;
            }
            if let Some(waker) = waker {
                waker.wake_by_ref();
            }
        }
    }
}

/// The last picture decoded, for a request for the same file straight after.
///
/// Held only while there is more work queued behind it -- [`work`] forgets it
/// whenever the queue runs dry -- so the one case it serves is two requests
/// in a row for one file, which is the greeter following the desktop.
///
/// The file is recognised by its path *and* its length and modification time,
/// so a picture edited in place and chosen again is decoded again rather than
/// served from before the edit. A file whose times cannot be read is never
/// shared: a second decode costs a second, and a stale picture costs trust in
/// the setting.
#[derive(Default)]
struct Recent {
    held: Option<Held>,
}

/// The picture [`Recent`] holds, and what it was decoded from: which file,
/// at which fit, and which version of the file.
struct Held {
    path: PathBuf,
    fit: Option<(u32, u32)>,
    stamp: Stamp,
    image: Arc<imagecodec::Image>,
}

/// What identifies a version of a file: its length and when it was written.
type Stamp = (u64, SystemTime);

impl Recent {
    /// `path`, decoded to `fit` -- or the picture just decoded from it to the
    /// same fit, if the file is unchanged since. A picture decoded small is
    /// never handed to a request for it at its own size, nor the other way.
    fn decode(
        &mut self,
        path: &Path,
        fit: Option<(u32, u32)>,
    ) -> Result<Arc<imagecodec::Image>, String> {
        let stamp = stamp(path);
        if let (Some(held), Some(now)) = (&self.held, stamp)
            && held.path == path
            && held.fit == fit
            && held.stamp == now
        {
            return Ok(Arc::clone(&held.image));
        }
        // Dropped before decoding, not after: holding one full-screen picture
        // while the next is inflated is two at once for nothing.
        self.held = None;
        let image = Arc::new(decode(path, fit)?);
        if let Some(now) = stamp {
            self.held = Some(Held {
                path: path.to_path_buf(),
                fit,
                stamp: now,
                image: Arc::clone(&image),
            });
        }
        Ok(image)
    }

    /// Hold nothing.
    fn forget(&mut self) {
        self.held = None;
    }
}

/// The length and modification time of `path`, if both can be read.
///
/// `None` rather than the error: all a failure here means is that the file is
/// not shared between two requests, and the decode that follows reads the file
/// itself and reports whatever is wrong with it, naming it.
fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Read and decode `path` -- scaled to fit `fit` when there is one -- or say
/// why not, naming the file.
fn decode(path: &Path, fit: Option<(u32, u32)>) -> Result<imagecodec::Image, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // The default limit is the compositor's own buffer ceiling, so a picture
    // refused here is one the compositor would have refused anyway -- and
    // refusing it from the header costs a header rather than a decompressed
    // framebuffer.
    let limits = imagecodec::Limits::default();
    match fit {
        Some((width, height)) => imagecodec::decode_scaled(&bytes, limits, width, height),
        None => imagecodec::decode(&bytes, limits),
    }
    .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;
    use std::time::Duration;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(format!(
            "{}/../imagecodec/tests/data/{name}.png",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    /// A waker that counts its wakes.
    struct Count(AtomicUsize);

    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Run the worker over `jobs`, queued in full before it starts, and
    /// collect every answer.
    fn run_over(jobs: Vec<Job>) -> Vec<Decoded> {
        let (requests, queue) = mpsc::channel::<Job>();
        let (done, results) = mpsc::channel::<Decoded>();
        for job in jobs {
            requests.send(job).unwrap();
        }
        drop(requests);
        work(&queue, &done, None);
        results.try_iter().collect()
    }

    fn job(slot: Slot, id: u64, path: PathBuf) -> Job {
        Job {
            slot,
            id,
            path,
            fit: None,
        }
    }

    /// **A picture is decoded off the caller's thread, and the loop is woken
    /// when it is ready.**
    #[test]
    fn a_picture_comes_back_decoded_and_the_loop_is_woken() {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let worker = PictureWorker::spawn(Some(Waker::from(Arc::clone(&count))));
        worker.request(Slot::Wallpaper, 7, fixture("rgb8"));
        let done = worker.settle(Duration::from_secs(30));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].job.id, 7);
        assert_eq!(done[0].job.slot, Slot::Wallpaper);
        let image = done[0].result.as_ref().expect("the fixture decodes");
        assert!(image.width > 0 && image.height > 0);
        // The wake follows the send -- a loop woken before the result was on
        // the channel would find nothing and park again -- so the result can
        // be here a moment before the wake is. Waited for, with a bound.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while count.0.load(Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the loop was not woken"
            );
            std::thread::yield_now();
        }
    }

    /// **A fitted request comes back within its fit, proportions kept; each
    /// photo frame is a slot of its own; and a picture decoded small is not
    /// handed to a request for it at its own size**, nor the other way,
    /// though the two ask for one file in a row.
    #[test]
    fn a_fitted_picture_fits_and_is_never_shared_with_a_full_one() {
        let full_size = decode(&fixture("rgb8"), None).expect("the fixture decodes");
        let fit = ((full_size.width / 2).max(1), (full_size.height / 3).max(1));
        let answers = run_over(vec![
            Job {
                fit: Some(fit),
                ..job(Slot::Frame(4), 1, fixture("rgb8"))
            },
            job(Slot::Wallpaper, 2, fixture("rgb8")),
            Job {
                fit: Some(fit),
                ..job(Slot::Frame(5), 3, fixture("rgb8"))
            },
        ]);
        let size = |id: u64| {
            let answer = answers.iter().find(|d| d.job.id == id).expect("answered");
            let image = answer.result.as_ref().expect("decoded");
            (image.width, image.height)
        };
        let (w, h) = size(1);
        assert!(w <= fit.0 && h <= fit.1, "{w}x{h} over {fit:?}");
        assert!(w == fit.0 || h == fit.1, "{w}x{h} smaller than it need be");
        assert_eq!(size(3), (w, h), "two frames, two slots, both decoded");
        assert_eq!(
            size(2),
            (full_size.width, full_size.height),
            "the wallpaper was handed the frame's small picture"
        );
    }

    /// A file that is not there comes back as a reason naming it.
    #[test]
    fn a_missing_file_comes_back_as_a_reason_naming_it() {
        let worker = PictureWorker::spawn(None);
        let path = PathBuf::from("/definitely/not/a/wallpaper.png");
        worker.request(Slot::Greeter, 3, path.clone());
        let done = worker.settle(Duration::from_secs(30));
        assert_eq!(done.len(), 1);
        let why = done[0].result.as_ref().expect_err("not a picture");
        assert!(why.contains("wallpaper.png"), "{why}");
    }

    /// **Only the newest request for a slot is decoded when several are
    /// queued**, and each slot keeps its own.
    #[test]
    fn a_burst_of_requests_decodes_the_newest_for_each_slot() {
        let answers = run_over(vec![
            job(Slot::Wallpaper, 1, fixture("rgb8")),
            job(Slot::Wallpaper, 2, fixture("rgb8")),
            job(Slot::Greeter, 10, fixture("rgb8")),
            job(Slot::Wallpaper, 3, fixture("rgb8")),
        ]);
        let decoded: Vec<(Slot, u64)> = answers
            .iter()
            .filter(|d| d.result.is_ok())
            .map(|d| (d.job.slot, d.job.id))
            .collect();
        assert_eq!(decoded, [(Slot::Wallpaper, 3), (Slot::Greeter, 10)]);
        // And the two it skipped are answered, not dropped: every request
        // gets exactly one answer.
        let skipped: Vec<u64> = answers
            .iter()
            .filter(|d| d.result.as_ref().err().is_some_and(|why| why == SUPERSEDED))
            .map(|d| d.job.id)
            .collect();
        assert_eq!(skipped, [1, 2]);
    }

    /// **The greeter following the desktop costs one decode, not two**: the
    /// same file for both slots, back to back, is the same pixels.
    #[test]
    fn one_file_for_both_slots_is_decoded_once() {
        let answers = run_over(vec![
            job(Slot::Wallpaper, 1, fixture("rgb8")),
            job(Slot::Greeter, 2, fixture("rgb8")),
        ]);
        assert_eq!(answers.len(), 2);
        let wallpaper = answers[0].result.as_ref().expect("decodes");
        let greeter = answers[1].result.as_ref().expect("decodes");
        assert!(
            Arc::ptr_eq(wallpaper, greeter),
            "the same file was decoded twice for two slots"
        );
    }

    /// Two different files are two decodes, however close together.
    #[test]
    fn two_files_are_two_pictures() {
        let answers = run_over(vec![
            job(Slot::Wallpaper, 1, fixture("rgb8")),
            job(Slot::Greeter, 2, fixture("gray8")),
        ]);
        let wallpaper = answers[0].result.as_ref().expect("decodes");
        let greeter = answers[1].result.as_ref().expect("decodes");
        assert!(!Arc::ptr_eq(wallpaper, greeter));
        assert_ne!(
            **wallpaper, **greeter,
            "one file's pixels answered for another"
        );
    }

    /// **A picture edited in place is decoded again**, not served from before
    /// the edit.
    #[test]
    fn a_file_changed_since_is_decoded_again() {
        let dir = scratchdir::ScratchDir::new("desktop-pictures");
        let path = dir.path("wall.png");
        std::fs::copy(fixture("rgb8"), &path).expect("copy the fixture");
        let mut recent = Recent::default();
        let before = recent.decode(&path, None).expect("decodes");
        assert!(
            Arc::ptr_eq(&before, &recent.decode(&path, None).expect("decodes")),
            "an unchanged file was decoded again"
        );

        // A different picture under the same name, written as an editor
        // saving in place would: new contents, new length, new time.
        let edited = std::fs::read(fixture("gray8")).expect("read the fixture");
        std::fs::write(&path, edited).expect("replace the fixture");
        let after = recent.decode(&path, None).expect("decodes");
        assert!(!Arc::ptr_eq(&before, &after), "the edit was not noticed");
        assert_ne!(
            *before, *after,
            "the picture from before the edit came back"
        );
    }

    /// Nothing is held once it is forgotten -- which `work` does whenever the
    /// queue runs dry, so an idle worker holds no picture.
    #[test]
    fn a_forgotten_picture_is_decoded_afresh() {
        let mut recent = Recent::default();
        let first = recent.decode(&fixture("rgb8"), None).expect("decodes");
        recent.forget();
        assert!(recent.held.is_none());
        let second = recent.decode(&fixture("rgb8"), None).expect("decodes");
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(*first, *second);
    }

    /// A file that would not decode is not remembered as a picture -- and
    /// the picture before it is not kept either.
    #[test]
    fn a_failure_is_not_held() {
        let mut recent = Recent::default();
        recent.decode(&fixture("rgb8"), None).expect("decodes");
        assert!(recent.held.is_some());
        let missing = PathBuf::from("/definitely/not/here.png");
        assert!(recent.decode(&missing, None).is_err());
        assert!(
            recent.held.is_none(),
            "a picture was held across a decode that replaced it"
        );
    }

    /// The thread ends when the session hangs up, without being joined.
    #[test]
    fn the_thread_ends_when_the_session_is_dropped() {
        let (requests, jobs) = mpsc::channel::<Job>();
        let (done, _results) = mpsc::channel::<Decoded>();
        let thread = std::thread::spawn(move || work(&jobs, &done, None));
        drop(requests);
        thread.join().expect("the worker ended cleanly");
    }
}
