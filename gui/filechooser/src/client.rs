//! A program asking for a file: [`Picker`], the toolkit's
//! [`FilePicker`] with the file explorer in front of it.
//!
//! # How a program uses it
//!
//! As it used [`FilePicker`], whose calls these are:
//! [`put_up`](Picker::put_up) (or [`open_to_read`](Picker::open_to_read) and
//! [`open_to_write`](Picker::open_to_write)), every event offered to
//! [`handle`](Picker::handle) first, [`render`](Picker::render) last. Two
//! things are new, and both are for the explorer's answer, which arrives on
//! a connection of its own rather than as an event:
//!
//! - [`with_waker`](Picker::with_waker): the event loop's waker
//!   (`oswindow::EventLoop::waker`), which the picker wakes when the answer
//!   comes. Without one the answer waits for the program's next event.
//! - [`poll`](Picker::poll): ask, when the loop wakes, whether the answer
//!   has come -- [`Picked::Chose`] or [`Picked::Cancelled`] if it has.
//!
//! # While the explorer has it
//!
//! The explorer's window is the dialog, and the program's window is behind
//! it as behind any dialog: a key or a click on it is taken and does
//! nothing, except Escape, which gives up asking and is
//! [`Picked::Cancelled`] -- the way out if the explorer never answers. Time
//! and size changes still reach the program, as with the toolkit's dialog.
//!
//! # Where there is no explorer
//!
//! Where nothing serves [`SERVICE`](crate::protocol::SERVICE) -- a
//! development host; a session the explorer is not serving -- or asking
//! fails, the program draws the toolkit's own dialog with the same settings,
//! as before. If the explorer fails after it was asked (it exits, or answers
//! something that is not an answer), the toolkit's dialog comes up then: the
//! user asked to choose a file, and still gets to.

use guiremote::client::Transport;
use guitk::dialog::{DialogMode, FileDialog, FilePicker, Picked};
use guitk::event::{Event, Key};
use guitk::palette::Palette;
use guitk::render::RenderCommand;
use std::ffi::OsStr;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::Waker;

use svcconn::{Connect, SystemConnect, Waiting};

use crate::protocol::{self, Filter, Mode, Reply, Request};

/// What the worker and the picker share: the answer, once it has come, and
/// whether the picker still wants it.
#[derive(Debug, Default)]
struct Shared {
    outcome: Mutex<Option<io::Result<Reply>>>,
    abandoned: AtomicBool,
}

/// A request the explorer has.
#[derive(Debug)]
struct Asking {
    shared: Arc<Shared>,
    /// The dialog the program asked for, kept to draw if the explorer fails.
    dialog: FileDialog,
    saving: bool,
}

impl Drop for Asking {
    fn drop(&mut self) {
        // The worker sees this within a slice of its wait, and closes the
        // connection, which tells the explorer to take its window down.
        self.shared.abandoned.store(true, Ordering::Release);
    }
}

/// The toolkit's [`FilePicker`] with the file explorer in front of it: see
/// the [module documentation](self).
#[derive(Debug)]
pub struct Picker<C: Connect = SystemConnect> {
    connect: C,
    local: FilePicker,
    asking: Option<Asking>,
    waker: Option<Waker>,
    owner: u64,
}

impl Default for Picker<SystemConnect> {
    fn default() -> Self {
        Self::new()
    }
}

impl Picker<SystemConnect> {
    /// A picker with nothing up, asking the system's file chooser.
    #[must_use]
    pub fn new() -> Self {
        Self::with_connect(SystemConnect)
    }
}

impl<C: Connect> Picker<C> {
    /// A picker with nothing up, reaching the chooser through `connect`.
    #[must_use]
    pub fn with_connect(connect: C) -> Self {
        Self {
            connect,
            local: FilePicker::new(),
            asking: None,
            waker: None,
            owner: 0,
        }
    }

    /// The waker to wake when the explorer answers: the program's event
    /// loop's, so the answer is handled the moment it comes.
    #[must_use]
    pub fn with_waker(mut self, waker: Waker) -> Self {
        self.waker = Some(waker);
        self
    }

    /// Set the window the dialog belongs to, as the compositor numbers it,
    /// so the explorer's window can be kept above it.
    pub fn set_owner(&mut self, window: u64) {
        self.owner = window;
    }

    /// Whether a dialog is up: the toolkit's, or the explorer's for this
    /// program. While one is, events go to [`handle`](Self::handle) first.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.asking.is_some() || self.local.is_open()
    }

    /// Whether the explorer has the request, rather than the toolkit's dialog.
    #[must_use]
    pub fn is_asking(&self) -> bool {
        self.asking.is_some()
    }

    /// Whether the dialog up is for saving.
    #[must_use]
    pub fn is_saving(&self) -> bool {
        self.asking
            .as_ref()
            .map_or_else(|| self.local.is_saving(), |asking| asking.saving)
    }

    /// Ask for a file to open, starting where [`FilePicker::default_start`]
    /// says.
    pub fn open_to_read(&mut self) {
        self.put_up(
            FileDialog::open().with_initial_path(FilePicker::default_start()),
            false,
        );
    }

    /// Ask for a place to save, offering `filename`.
    pub fn open_to_write(&mut self, filename: impl AsRef<OsStr>) {
        let dialog = FileDialog::save()
            .with_initial_path(FilePicker::default_start())
            .with_filename(filename);
        self.put_up(dialog, true);
    }

    /// Ask for what `dialog` asks for: of the explorer if there is one to
    /// ask, and otherwise by drawing `dialog`.
    pub fn put_up(&mut self, dialog: FileDialog, saving: bool) {
        self.close();
        match self.ask(&request_for(&dialog, self.owner)) {
            Some(shared) => {
                self.asking = Some(Asking {
                    shared,
                    dialog,
                    saving,
                });
            }
            None => self.local.put_up(dialog, saving),
        }
    }

    /// Take the dialog down without choosing anything: the toolkit's, or
    /// the explorer's -- whose window goes when it sees no one is asking.
    pub fn close(&mut self) {
        self.asking = None;
        self.local.close();
    }

    /// Offer `event`, as to [`FilePicker::handle`]. While the explorer has
    /// the request, its answer is collected here as well as by
    /// [`poll`](Self::poll); the program's own input is taken and does
    /// nothing, except Escape, which gives up asking.
    pub fn handle(&mut self, event: &Event, width: f32, height: f32) -> Picked {
        if self.asking.is_none() {
            return self.local.handle(event, width, height);
        }
        match event {
            Event::Key(_) | Event::Mouse(_) => {
                let answered = self.poll();
                if answered != Picked::Ignored {
                    return answered;
                }
                if let Event::Key(key) = event
                    && key.pressed
                    && key.key == Key::Escape
                {
                    self.close();
                    return Picked::Cancelled;
                }
                Picked::Handled
            }
            // Time and size: the program still needs them.
            _ => Picked::Ignored,
        }
    }

    /// Whether the explorer has answered: [`Picked::Chose`] or
    /// [`Picked::Cancelled`] if it has, [`Picked::Handled`] if it failed and
    /// the toolkit's dialog is up instead -- draw again -- and
    /// [`Picked::Ignored`] while it has not, or when nothing is being asked.
    pub fn poll(&mut self) -> Picked {
        let Some(asking) = self.asking.as_ref() else {
            return Picked::Ignored;
        };
        let outcome = asking
            .shared
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(outcome) = outcome else {
            return Picked::Ignored;
        };
        let Some(asking) = self.asking.take() else {
            return Picked::Ignored;
        };
        match outcome {
            Ok(Reply::Chosen { path, .. }) => Picked::Chose(path),
            Ok(Reply::Cancelled) => Picked::Cancelled,
            // The explorer went, or answered something that is not an
            // answer: the user asked to choose a file, and still gets to.
            Err(_) => {
                let Asking {
                    ref dialog, saving, ..
                } = asking;
                self.local.put_up(dialog.clone(), saving);
                Picked::Handled
            }
        }
    }

    /// The toolkit dialog's commands, when it is the one up; nothing while
    /// the explorer has the request, whose window is its own.
    #[must_use]
    pub fn render(&self, palette: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {
        self.local.render(palette, width, height)
    }

    /// The toolkit's picker underneath, for what [`FilePicker`] offers beyond
    /// these calls -- its dialog, to read or adjust while it is up.
    #[must_use]
    pub fn local(&self) -> &FilePicker {
        &self.local
    }

    /// The toolkit's picker underneath, mutably. See [`local`](Self::local).
    pub fn local_mut(&mut self) -> &mut FilePicker {
        &mut self.local
    }

    /// Send `request` to the chooser and set a worker waiting for the
    /// answer -- or `None` where there is no chooser to ask, or asking it
    /// failed, and the toolkit's dialog is to be drawn instead.
    fn ask(&self, request: &Request) -> Option<Arc<Shared>> {
        // A request past the protocol's bounds -- a start folder longer than
        // a path may be -- cannot be asked, and is drawn here instead.
        let frame = protocol::encode_request(request).ok()?;
        // No chooser, or one that cannot be reached: the toolkit's dialog,
        // which is the answer to every failure here.
        let mut conn = self.connect.connect(protocol::SERVICE).ok()??;
        conn.write(&frame).ok()?;
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        let waker = self.waker.clone();
        std::thread::Builder::new()
            .name("file chooser".into())
            .spawn(move || {
                if let Some(outcome) = await_reply(conn, &worker) {
                    *worker
                        .outcome
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some(outcome);
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }
            })
            .ok()?;
        Some(shared)
    }
}

/// The request `dialog` makes, from the window `owner`.
fn request_for(dialog: &FileDialog, owner: u64) -> Request {
    let mode = match dialog.mode() {
        DialogMode::Open => Mode::Open,
        DialogMode::Save => Mode::Save,
        DialogMode::SelectFolder => Mode::Folder,
    };
    Request {
        mode,
        owner,
        title: String::new(),
        start: dialog.current_path().to_path_buf(),
        name: if mode == Mode::Save {
            dialog.filename()
        } else {
            std::ffi::OsString::new()
        },
        filters: dialog
            .filters()
            .into_iter()
            .map(|filter| Filter {
                label: filter.description,
                patterns: filter.patterns,
            })
            .collect(),
        filter: dialog.filter_index(),
    }
}

/// Wait on `conn` for the chooser's answer: `None` if the picker stopped
/// wanting it first, and otherwise the answer or why there is none.
fn await_reply<T: Transport<Error = io::Error>>(
    mut conn: T,
    shared: &Shared,
) -> Option<io::Result<Reply>> {
    let waiting = Waiting {
        patience: None,
        abandoned: Some(&shared.abandoned),
    };
    svcconn::read_frame(&mut conn, protocol::decode_reply, waiting).transpose()
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
