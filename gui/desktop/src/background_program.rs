//! The desktop's background drawn by a program: started by the desktop, its
//! pictures shown as the wallpaper, and the desktop's events told to it
//! (`gui/backdrop`, `design-decisions.md` §1489).
//!
//! Two kinds of background are this one thing: a **video** chosen as the
//! wallpaper is played by `wallvideo`, a program beside the desktop's own
//! ([`wallvideo_path`]), and a **program** chosen as the background
//! (`wallpaper.program` in the appearance settings) is started as it is.
//! Either way the desktop reads the pictures the program writes, on a thread
//! of its own that keeps only the newest and wakes the desktop's loop for it,
//! and the session puts each up as the wallpaper's picture -- the same
//! picture, under the same number, so the wallpaper's fit and placement are
//! the user's as for a picture file.
//!
//! **Out of the desktop's process, on purpose.** A video is a file from
//! anywhere, and its decoders are large: a fault in one costs the background
//! and not the desktop, which holds the user's session. The same for a
//! program somebody else wrote. What the program is told is listed in
//! `backdrop`'s documentation, and it is nothing of what is in the windows.
//!
//! The session talks to a [`Source`], so its tests can stand a scripted one
//! in for a process.

use std::ffi::OsString;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::task::Waker;

pub use backdrop::{Event, Frame, Rect};

/// The program that plays a video as the background.
pub const WALLVIDEO: &str = "wallvideo";

/// The file kinds played as a video background, by their names' endings --
/// what `videocodec` reads: Matroska and WebM, MP4 and QuickTime's.
const VIDEO_KINDS: [&str; 5] = ["webm", "mkv", "mp4", "m4v", "mov"];

/// How much of what a program says on its standard error is kept, to say
/// why its background stopped: the end of it, where the reason is.
const SAID_KEPT: usize = 2048;

/// Whether `path` names a video, which is played as the background rather
/// than decoded as a picture: by its name's ending, as the file chooser
/// offers it.
#[must_use]
pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_KINDS.iter().any(|k| e.eq_ignore_ascii_case(k)))
}

/// Where `wallvideo` is: beside the desktop's own program, as the image
/// installs both (`/usr/bin`), and as a build leaves both (`target/...`).
#[must_use]
pub fn wallvideo_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(dir.join(format!("{WALLVIDEO}{}", std::env::consts::EXE_SUFFIX)))
}

/// Something drawing the background: a program, or a test's stand-in.
pub trait Source {
    /// The newest picture since the last asked, if a new one came.
    fn take_frame(&mut self) -> Option<Frame>;
    /// Why the pictures stopped, once they have: the program ended, or
    /// wrote something that is not a picture.
    fn ended(&self) -> Option<String>;
    /// Tell it `event`. A program that has stopped listening is not an
    /// error here: its end is reported through [`Self::ended`].
    fn tell(&mut self, event: &Event);
}

/// Between the thread reading a program's pictures and the desktop.
#[derive(Default)]
struct Shared {
    /// The newest picture not yet taken.
    frame: Option<Frame>,
    /// Why the pictures stopped.
    ended: Option<String>,
    /// The end of what the program said on standard error.
    said: Vec<u8>,
}

/// A background program, running.
pub struct Program {
    child: Child,
    /// Its standard input: the events. `None` once it would take no more.
    stdin: Option<ChildStdin>,
    shared: Arc<Mutex<Shared>>,
}

impl Program {
    /// Start `program` with `args`, its pictures read on a thread of their
    /// own, which wakes `waker` after each -- the desktop's loop, so that a
    /// picture arriving is put up without waiting for an event.
    ///
    /// # Errors
    ///
    /// The program could not be started: it is not there, or not a program.
    pub fn start(program: &Path, args: &[OsString], waker: Option<Waker>) -> io::Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("the program's output was not given to the desktop"))?;
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        let shared = Arc::new(Mutex::new(Shared::default()));
        if let Some(stderr) = stderr {
            let said = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("background-said".into())
                .spawn(move || keep_said(stderr, &said))?;
        }
        let pictures = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("background-pictures".into())
            .spawn(move || read_pictures(stdout, &pictures, waker.as_ref()))?;
        Ok(Self {
            child,
            stdin,
            shared,
        })
    }
}

/// Read pictures from `stdout` into `shared` until they stop, waking `waker`
/// after each and after the end.
fn read_pictures(stdout: impl Read, shared: &Mutex<Shared>, waker: Option<&Waker>) {
    let mut input = BufReader::with_capacity(1 << 20, stdout);
    let why = loop {
        match backdrop::read_frame(&mut input) {
            Ok(Some(frame)) => {
                // A poisoned lock is the desktop's thread panicking, which
                // ends this program's background anyway.
                if let Ok(mut shared) = shared.lock() {
                    shared.frame = Some(frame);
                }
                if let Some(waker) = waker {
                    waker.wake_by_ref();
                }
            }
            Ok(None) => break "the background program ended".to_owned(),
            Err(e) => break format!("the background program stopped: {e}"),
        }
    };
    // What it said as it went, on standard error, is the reason a user can
    // act on: wait a moment for the thread keeping it, which ends with the
    // program.
    std::thread::sleep(std::time::Duration::from_millis(100));
    if let Ok(mut shared) = shared.lock() {
        let said = std::str::from_utf8(&shared.said)
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        shared.ended = Some(if said.is_empty() {
            why
        } else {
            format!("{why}: {said}")
        });
    }
    if let Some(waker) = waker {
        waker.wake_by_ref();
    }
}

/// Keep the end of what `stderr` says in `shared`, until it closes.
fn keep_said(mut stderr: impl Read, shared: &Mutex<Shared>) {
    let mut buf = [0u8; 512];
    loop {
        match stderr.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                if let (Ok(mut shared), Some(got)) = (shared.lock(), buf.get(..n)) {
                    shared.said.extend_from_slice(got);
                    let over = shared.said.len().saturating_sub(SAID_KEPT);
                    if over > 0 {
                        shared.said.drain(..over);
                    }
                }
            }
        }
    }
}

impl Source for Program {
    fn take_frame(&mut self) -> Option<Frame> {
        self.shared.lock().ok()?.frame.take()
    }

    fn ended(&self) -> Option<String> {
        self.shared.lock().ok()?.ended.clone()
    }

    fn tell(&mut self, event: &Event) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let sent = writeln!(stdin, "{}", event.line()).and_then(|()| stdin.flush());
        if sent.is_err() {
            // It stopped listening: it has ended or is ending, which the
            // picture thread reports. Nothing more is sent.
            self.stdin = None;
        }
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        // Closing its input is the polite word -- a program reading events
        // ends at it -- and the kill the certain one. Both name this child
        // and no other process: it is the desktop's own, started above.
        drop(self.stdin.take());
        // Not running any more is the outcome either way; a kill that fails
        // because it has already ended is that outcome.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;

    /// A scripted background: the pictures to give, the end to report, and
    /// what it was told -- shared, so a test keeps a handle on it after the
    /// session takes it.
    #[derive(Clone, Default)]
    pub(crate) struct Scripted {
        pub(crate) frames: Rc<RefCell<VecDeque<Frame>>>,
        pub(crate) ended: Rc<RefCell<Option<String>>>,
        pub(crate) told: Rc<RefCell<Vec<Event>>>,
    }

    impl Source for Scripted {
        fn take_frame(&mut self) -> Option<Frame> {
            // The newest, as a program's reader keeps it.
            let mut frames = self.frames.borrow_mut();
            let newest = frames.pop_back();
            frames.clear();
            newest
        }

        fn ended(&self) -> Option<String> {
            self.ended.borrow().clone()
        }

        fn tell(&mut self, event: &Event) {
            self.told.borrow_mut().push(event.clone());
        }
    }

    /// A `width` by `height` picture of one colour.
    pub(crate) fn frame(width: u32, height: u32, argb: u32) -> Frame {
        Frame {
            width,
            height,
            pixels: vec![argb; (width * height) as usize],
        }
    }

    /// **A video is told by its name's ending**, in any case; a picture, or
    /// a name with no ending, is not one.
    #[test]
    fn a_video_is_told_by_its_ending() {
        for name in ["a.webm", "b.MKV", "c.mp4", "d.m4v", "e.Mov"] {
            assert!(is_video(Path::new(name)), "{name}");
        }
        for name in ["a.png", "b.jpg", "webm", "c.webm.png", "d"] {
            assert!(!is_video(Path::new(name)), "{name}");
        }
    }

    /// **`wallvideo` is looked for beside the desktop's own program.**
    #[test]
    fn wallvideo_is_looked_for_beside_the_desktop() {
        let path = wallvideo_path().expect("the test binary has a directory");
        let exe = std::env::current_exe().unwrap();
        assert_eq!(path.parent(), exe.parent());
        assert!(
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(WALLVIDEO)),
            "{path:?}"
        );
    }

    /// **The pictures a program writes are read on their own thread, the
    /// newest kept, and the desktop's loop woken for each; the end of them
    /// is reported with what the program said.**
    #[test]
    fn pictures_are_read_and_the_end_reported() {
        let mut stream = Vec::new();
        backdrop::write_frame(&mut stream, 1, 1, &[0xFF00_0001]).unwrap();
        backdrop::write_frame(&mut stream, 2, 1, &[0xFF00_0002, 0xFF00_0003]).unwrap();
        let shared = Mutex::new(Shared::default());
        let wakes = Arc::new(Wakes::default());
        let waker = Waker::from(Arc::clone(&wakes));
        shared.lock().unwrap().said = b"  the file is gone \n".to_vec();
        read_pictures(&stream[..], &shared, Some(&waker));
        let shared = shared.into_inner().unwrap();
        assert_eq!(shared.frame.map(|f| f.width), Some(2), "the newest");
        assert_eq!(
            shared.ended.as_deref(),
            Some("the background program ended: the file is gone")
        );
        assert_eq!(
            wakes.0.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "a wake for each picture, and one for the end"
        );
        let shared = Mutex::new(Shared::default());
        read_pictures(&b"not pictures at all"[..], &shared, None);
        assert_eq!(
            shared.into_inner().unwrap().ended.as_deref(),
            Some("the background program stopped: it wrote something that is not a picture")
        );
    }

    /// **What a program says is kept to its end**, where the reason is.
    #[test]
    fn what_a_program_says_is_kept_to_its_end() {
        let shared = Mutex::new(Shared::default());
        let long = vec![b'x'; SAID_KEPT * 2];
        let mut said = long.clone();
        said.extend_from_slice(b"the reason");
        keep_said(&said[..], &shared);
        let kept = shared.into_inner().unwrap().said;
        assert_eq!(kept.len(), SAID_KEPT);
        assert!(kept.ends_with(b"the reason"));
    }

    /// A waker that counts its wakes.
    #[derive(Default)]
    struct Wakes(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}
