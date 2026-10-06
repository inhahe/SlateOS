//! `wallvideo FILE` -- a video as the desktop's background: the background
//! program (`gui/backdrop`) that plays `FILE`, looping, writing its pictures
//! on standard output at the size the desktop says on standard input. See
//! the library's documentation (`wallvideo::cover_size`, `shrink`, `due`)
//! for the arithmetic, and `design-decisions.md` §1489 for why a video is a
//! program of its own.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::Instant;

use backdrop::Event;
use videocodec::{SeekMode, Video};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("usage: wallvideo FILE");
        return ExitCode::from(2);
    };
    let events = listen();
    match play(Path::new(path), &events) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            // Said on standard error, which the desktop reads when the
            // pictures stop, and shows as why the background did.
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// The desktop's events, as they arrive on standard input, a thread reading
/// them; the channel ends when standard input does -- the desktop is gone.
fn listen() -> Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            // A line this does not know is passed over: events may be added.
            if let Some(event) = Event::parse(&line)
                && tx.send(event).is_err()
            {
                break;
            }
        }
    });
    rx
}

/// What the desktop has said so far.
#[derive(Default)]
struct Said {
    /// The background's size, once said.
    size: Option<(u32, u32)>,
    /// Whether nobody can see it.
    paused: bool,
}

impl Said {
    /// Take `event` in, answering whether it ended a pause -- after which
    /// the playing carries on from where it was, not from where the clock
    /// says it would be by now.
    fn take(&mut self, event: Event) -> bool {
        match event {
            Event::Size { width, height } => self.size = Some((width, height)),
            Event::Pause => self.paused = true,
            Event::Resume => {
                let was = self.paused;
                self.paused = false;
                return was;
            }
            // A film does not answer the pointer, the windows or the theme.
            Event::Pointer { .. } | Event::Desktop(_) | Event::Windows(_) | Event::Theme { .. } => {
            }
        }
        false
    }
}

/// Play `path` until the desktop goes away -- `Ok` then -- or the file
/// cannot be played on.
fn play(path: &Path, events: &Receiver<Event>) -> Result<(), String> {
    // The path is the user's own and said back to no one: the reason alone
    // goes in the message, which the desktop shows beside the setting that
    // names the file.
    let file = File::open(path).map_err(|e| format!("the video could not be opened: {e}"))?;
    let mut video = Video::open(BufReader::new(file))
        .map_err(|e| format!("the file is not a video this plays: {e}"))?;
    let mut out = BufWriter::new(io::stdout().lock());
    let mut said = Said::default();
    // When the playing (re)started, and the file's time of the first frame
    // shown since.
    let mut start = Instant::now();
    let mut first: Option<i64> = None;
    // Whether a frame has been shown since the start of the file: a file
    // that ends without one would loop for ever.
    let mut shown_this_pass = false;
    loop {
        loop {
            match events.try_recv() {
                Ok(event) => {
                    if said.take(event) {
                        first = None;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if said.paused || said.size.is_none() {
            // Nothing to draw for: wait for the desktop to say something.
            match events.recv() {
                Ok(event) => {
                    if said.take(event) {
                        first = None;
                    }
                    continue;
                }
                Err(_) => return Ok(()),
            }
        }
        let frame = match video.next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) if shown_this_pass => {
                video
                    .seek(0, SeekMode::KeyFrame)
                    .map_err(|e| format!("the video could not be played again: {e}"))?;
                first = None;
                shown_this_pass = false;
                continue;
            }
            Ok(None) => return Err("the video has no pictures".to_owned()),
            Err(e) => return Err(format!("the video stopped: {e}")),
        };
        shown_this_pass = true;
        let first_time = *first.get_or_insert_with(|| {
            start = Instant::now();
            frame.time
        });
        let due = wallvideo::due(start, first_time, frame.time);
        // Wait until it is due, listening the while.
        loop {
            let now = Instant::now();
            if now >= due || said.paused {
                break;
            }
            match events.recv_timeout(due.saturating_duration_since(now)) {
                Ok(event) => {
                    if said.take(event) {
                        first = None;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        if said.paused {
            continue;
        }
        let Some(screen) = said.size else {
            continue;
        };
        let (width, height) = wallvideo::cover_size((frame.width, frame.height), screen);
        let pixels = if (width, height) == (frame.width, frame.height) {
            frame.pixels
        } else {
            wallvideo::shrink(&frame.pixels, (frame.width, frame.height), (width, height))
        };
        match backdrop::write_frame(&mut out, width, height, &pixels).and_then(|()| out.flush()) {
            Ok(()) => {}
            // The desktop stopped reading: it has gone, or chose another
            // background.
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
            Err(e) => return Err(format!("a picture could not be written: {e}")),
        }
    }
}
