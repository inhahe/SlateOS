//! SlateOS window library — the API an application uses to have windows.
//!
//! An application should no more name the display protocol than a Unix program
//! names the socket layer. This crate is that boundary: it speaks
//! [`guiremote`]'s frames on one side and offers windows, properties and an
//! event loop on the other. Nothing above it should mention `SURF`, `CREQ` or
//! a correlation id.
//!
//! ```text
//!   application            ← windows, events, "set the title"
//!        │
//!   oswindow (here)        ← the translation
//!        │
//!   guiremote::Connection  ← frames, correlation ids, one socket
//!        │
//!   compositor
//! ```
//!
//! ## What changed, and why the previous version had to go
//!
//! Until this rewrite this crate did not connect to anything. It allocated
//! window ids from a local counter, answered its own requests from a
//! `simulate_response` function, and its `run` loop exited on its first
//! iteration with a comment saying a real one would block. Every application
//! written against it therefore *appeared* to work and could never have shown a
//! pixel. It also defined a third event vocabulary — `WindowEvent`, alongside
//! [`guitk::event::Event`] and the compositor's — so that connecting it later
//! would have required a lossy translation in the middle of the input path.
//!
//! So: ids come from the compositor and nowhere else, requests go out on a real
//! transport and their answers come back over it, `run` really blocks, and
//! there is exactly one event type — `guitk::event::Event` — end to end.
//!
//! ## Ownership
//!
//! One connection carries every window a process owns (see
//! [`guiremote::frame`] for why one rather than several), so the connection
//! cannot live inside a `Window`. [`EventLoop`] owns it, and a [`Window`] is a
//! record of what the compositor last told us about one window. Mutating one
//! goes through [`EventLoop::window_mut`], which hands back a [`WindowHandle`]
//! borrowing both.
//!
//! ## Usage
//!
//! ```rust
//! use oswindow::{EventLoop, EventResponse, Event, WindowBuilder};
//! use guiremote::pipe;
//!
//! // A real application connects to the compositor; this doc test uses an
//! // in-process pipe so it can run without one.
//! let (client_end, _server_end) = pipe();
//! let mut events = EventLoop::new(client_end);
//!
//! // `build` blocks until the compositor answers with the window's id.
//! # if false {
//! let id = WindowBuilder::new("My App", 800, 600)
//!     .resizable(true)
//!     .build(&mut events)
//!     .expect("the compositor refused the window");
//!
//! events.run(|events, window, event| match event {
//!     Event::CloseRequested => EventResponse::Exit,
//!     Event::Resize { width, height } => {
//!         let _ = events.submit(window, &guitk::render::RenderTree::new());
//!         EventResponse::Continue
//!     }
//!     _ => EventResponse::Continue,
//! })
//! .expect("the connection failed");
//! # }
//! ```

use std::collections::VecDeque;

use guiremote::client::{ClientError, Connection, Transport};
use guiremote::control::{CursorShape, DisplayInfo, RequestBody, ResponseBody, WindowSpec};

pub use guiremote::client::{ClientError as ConnectionError, Transport as ConnectionTransport};
pub use guiremote::control::{
    CursorShape as Cursor, DisplayInfo as Display, Layer, ShellControlAction, WindowSpec as Spec,
};
/// What a shell learns about the windows it does not own. See
/// [`EventLoop::watch_desktop`].
pub use guiremote::window_list::WindowInfo;
// An addressed event, as it travels. Applications never build one — they
// receive `(window, Event)` pairs from the loop — but anything driving an
// application synthetically does, which is what [`testing`] is for.
pub use guiremote::input::InputEvent;
pub use guiremote::{Pipe, pipe};
pub use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
pub use guitk::render::{RenderCommand, RenderTree};

// ---------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------

/// The environment variable naming the display to connect to.
///
/// Re-exported so an application can *mention* it in a diagnostic — "set
/// `SLATE_DISPLAY`" is the useful half of a failure to connect — without
/// depending on `guiremote` to learn its spelling.
pub use guiremote::socket::DISPLAY_VAR;

/// A live link to the compositor.
///
/// An alias, deliberately: the whole point of this crate is that an application
/// does not name the display protocol, and it should not name the *carrier*
/// either. Today this is a TCP socket (see `design-decisions.md` §460 for why
/// that, rather than a local-only pipe); on SlateOS it becomes a kernel channel.
/// Code written against `oswindow::Link` survives that change, and code written
/// against `guiremote::socket::Socket` does not.
///
/// It implements [`ConnectionTransport`], which is what [`EventLoop::new`]
/// wants. Named `Link` rather than `Connection` because
/// [`guiremote::client::Connection`] — the frame-level machinery this crate is
/// built on — already has that name, and two `Connection`s one layer apart is
/// exactly the confusion this crate exists to prevent.
pub type Link = guiremote::socket::Socket;

/// Connect to the compositor named by the environment.
///
/// The address comes from `SLATE_DISPLAY` if it is set and from
/// [`guiremote::socket::DEFAULT_DISPLAY`] otherwise, so a program started from a
/// normal session needs no arguments and a second compositor is reachable by
/// setting one variable.
///
/// # Errors
///
/// Fails if `SLATE_DISPLAY` is set to something that is not valid UTF-8, if the
/// address does not resolve, or if nothing is listening on it — the last being
/// overwhelmingly the common case, and meaning simply that no compositor is
/// running. That is worth saying in those words when reporting it: the raw
/// `ConnectionRefused` reads like a fault in the application.
pub fn connect() -> std::io::Result<Link> {
    Link::connect_display()
}

/// Connect to a compositor at an explicit address, ignoring the environment.
///
/// For a program given a display on its command line, and for tests that start
/// a compositor on an ephemeral port.
///
/// # Errors
///
/// As [`connect`], minus the environment-variable failure.
pub fn connect_to<A: std::net::ToSocketAddrs>(addr: A) -> std::io::Result<Link> {
    Link::connect(addr)
}

/// What can go wrong, for a given transport.
///
/// An alias rather than a new type: an application that wants to report a
/// failure needs the same information [`guiremote`] already distinguishes —
/// a transport failure, a protocol violation, a refusal with a reason — and
/// wrapping them again would only add a layer to unwrap.
pub type Error<T> = ClientError<<T as Transport>::Error>;

/// An application's answer to an event, controlling the loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResponse {
    /// Keep running.
    Continue,
    /// Stop the event loop and return.
    Exit,
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// The terms a window was created on.
///
/// Separate from [`Window`] because these are a different kind of fact. The
/// geometry and focus in a `Window` are *reports* — the compositor sends an
/// event whenever they change, and the record is rewritten to match. These are
/// *terms*: agreed once when the window was created, never contradicted
/// afterwards, and not folded from any event. Storing them beside the reported
/// state made `is_resizable()` read like live state sitting next to
/// `is_focused()`, when only one of the two is ever updated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowAttributes {
    /// Whether the user may resize the window.
    pub resizable: bool,
    /// Whether the compositor draws a frame and title bar for it.
    pub decorations: bool,
    /// Whether its client area may be see-through.
    pub transparent: bool,
    /// The smallest size the user may resize it to, if constrained.
    pub min_size: Option<(u32, u32)>,
    /// The largest size the user may resize it to, if constrained.
    pub max_size: Option<(u32, u32)>,
}

/// What is known about one window.
///
/// A record, not a handle: it holds no connection, so reading a property costs
/// nothing and cannot fail. The geometry and focus track what the compositor has
/// told us — [`EventLoop`] folds `Resize`, `Moved` and the focus events into
/// them as they arrive, so a size read here is the size the window actually has
/// and not the size it was last asked for. The creation terms live in
/// [`WindowAttributes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    id: u64,
    title: String,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    visible: bool,
    focused: bool,
    attributes: WindowAttributes,
}

impl Window {
    /// The compositor-assigned id. Stable for the window's lifetime.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// The title as last set.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Client-area size in pixels.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Client-area width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Client-area height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Screen position of the top-left corner.
    ///
    /// Signed because a multi-monitor desktop puts displays left of and above
    /// the primary one's origin.
    #[must_use]
    pub const fn position(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// Whether the window is mapped.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Whether this window currently has keyboard focus.
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// The terms this window was created on.
    #[must_use]
    pub const fn attributes(&self) -> WindowAttributes {
        self.attributes
    }

    /// Whether the user may resize it.
    #[must_use]
    pub const fn is_resizable(&self) -> bool {
        self.attributes.resizable
    }

    /// Whether the compositor draws a frame and title bar for it.
    #[must_use]
    pub const fn has_decorations(&self) -> bool {
        self.attributes.decorations
    }

    /// Whether its client area may be see-through.
    #[must_use]
    pub const fn is_transparent(&self) -> bool {
        self.attributes.transparent
    }

    /// The smallest size the user may resize it to, if constrained.
    #[must_use]
    pub const fn min_size(&self) -> Option<(u32, u32)> {
        self.attributes.min_size
    }

    /// The largest size the user may resize it to, if constrained.
    #[must_use]
    pub const fn max_size(&self) -> Option<(u32, u32)> {
        self.attributes.max_size
    }

    /// Fold an event into what we know about this window.
    fn apply(&mut self, event: &Event) {
        match *event {
            Event::Resize { width, height } => {
                self.width = width;
                self.height = height;
            }
            Event::Moved { x, y } => {
                self.x = x;
                self.y = y;
            }
            Event::FocusIn => self.focused = true,
            Event::FocusOut => self.focused = false,
            _ => {}
        }
    }
}

/// A window together with the connection that can change it.
///
/// Every method here sends a request and waits for the compositor to confirm
/// it. Waiting rather than firing and forgetting is deliberate: a `set_title`
/// the compositor refused would otherwise fail silently, and the local record
/// would then disagree with the screen with nothing to detect it. The cost is a
/// round trip on operations a program performs a handful of times, not on the
/// per-frame path — [`EventLoop::submit`] does not wait.
pub struct WindowHandle<'a, T: Transport> {
    events: &'a mut EventLoop<T>,
    /// The window's id rather than its index. An index would be a reference
    /// into a `Vec` by another name, and would silently address a different
    /// window if the vector were ever reordered.
    id: u64,
}

impl<T: Transport> WindowHandle<'_, T> {
    /// What is known about this window.
    ///
    /// # Panics
    ///
    /// Never: a handle cannot outlive the window it names, because holding one
    /// borrows the loop that would have to remove it.
    #[must_use]
    pub fn get(&self) -> &Window {
        self.events
            .window(self.id)
            .unwrap_or_else(|| unreachable!("a handle borrows the loop that owns its window"))
    }

    /// The compositor-assigned id.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Send this window's picture. Does not wait — this is the per-frame path.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the write fails.
    pub fn submit(&mut self, tree: &RenderTree) -> Result<(), Error<T>> {
        self.events.submit(self.id, tree)
    }

    /// Rename the window.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn set_title(&mut self, title: impl Into<String>) -> Result<(), Error<T>> {
        let title = title.into();
        self.events.confirm(RequestBody::SetTitle {
            window: self.id,
            title: title.clone(),
        })?;
        self.events.record_mut(self.id, |w| w.title = title);
        Ok(())
    }

    /// Move the window.
    ///
    /// The local position is *not* updated here: the compositor may place the
    /// window elsewhere — snapped to an edge, constrained to a monitor — and it
    /// reports where it actually put it with [`Event::Moved`]. Recording the
    /// requested position would make [`Window::position`] a record of what was
    /// asked rather than what happened, which is the bug this whole layer
    /// exists to avoid.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn set_position(&mut self, x: i32, y: i32) -> Result<(), Error<T>> {
        self.events.confirm(RequestBody::Move {
            window: self.id,
            x,
            y,
        })
    }

    /// Resize the window.
    ///
    /// As with [`Self::set_position`], the new size arrives as an
    /// [`Event::Resize`] rather than being assumed here.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn set_size(&mut self, width: u32, height: u32) -> Result<(), Error<T>> {
        self.events.confirm(RequestBody::Resize {
            window: self.id,
            width,
            height,
        })
    }

    /// Minimise the window.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn minimize(&mut self) -> Result<(), Error<T>> {
        self.events
            .confirm(RequestBody::Minimize { window: self.id })
    }

    /// Maximise the window.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn maximize(&mut self) -> Result<(), Error<T>> {
        self.events
            .confirm(RequestBody::Maximize { window: self.id })
    }

    /// Return the window to its pre-minimised/maximised geometry.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn restore(&mut self) -> Result<(), Error<T>> {
        self.events
            .confirm(RequestBody::Restore { window: self.id })
    }

    /// Map or unmap the window.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn set_visible(&mut self, visible: bool) -> Result<(), Error<T>> {
        self.events.confirm(RequestBody::SetVisible {
            window: self.id,
            visible,
        })?;
        self.events.record_mut(self.id, |w| w.visible = visible);
        Ok(())
    }

    /// Choose the cursor shown over this window's client area.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn set_cursor(&mut self, shape: CursorShape) -> Result<(), Error<T>> {
        self.events.confirm(RequestBody::SetCursor {
            window: self.id,
            shape,
        })
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Describes a window to be created.
///
/// ```rust
/// use oswindow::{EventLoop, WindowBuilder};
/// use guiremote::pipe;
///
/// let (client_end, _server) = pipe();
/// let mut events = EventLoop::new(client_end);
/// let request = WindowBuilder::new("Settings", 640, 480)
///     .position(100, 100)
///     .min_size(320, 240);
/// # let _ = (request, &mut events);
/// ```
#[derive(Clone, Debug)]
pub struct WindowBuilder {
    spec: WindowSpec,
}

impl WindowBuilder {
    /// A titled window of the given size, with the ordinary defaults.
    #[must_use]
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            spec: WindowSpec::new(title, width, height),
        }
    }

    /// Ask for a specific screen position. Left unset, the compositor places
    /// it — which is what a window should normally allow, since only the
    /// compositor knows what else is on screen.
    #[must_use]
    pub const fn position(mut self, x: i32, y: i32) -> Self {
        self.spec.position = Some((x, y));
        self
    }

    /// Whether the user may resize it.
    #[must_use]
    pub const fn resizable(mut self, resizable: bool) -> Self {
        self.spec.resizable = resizable;
        self
    }

    /// Whether the compositor draws a frame and title bar.
    #[must_use]
    pub const fn decorations(mut self, decorations: bool) -> Self {
        self.spec.decorations = decorations;
        self
    }

    /// Constrain how small the user may make it.
    #[must_use]
    pub const fn min_size(mut self, width: u32, height: u32) -> Self {
        self.spec.min_size = Some((width, height));
        self
    }

    /// Constrain how large the user may make it.
    #[must_use]
    pub const fn max_size(mut self, width: u32, height: u32) -> Self {
        self.spec.max_size = Some((width, height));
        self
    }

    /// Whether the client area may be see-through.
    #[must_use]
    pub const fn transparent(mut self, transparent: bool) -> Self {
        self.spec.transparent = transparent;
        self
    }

    /// Which band of the stacking order the window lives in.
    ///
    /// Almost every application wants the default, [`Layer::Normal`], and
    /// should not call this. It exists for the surfaces that are not ordinary
    /// windows: a wallpaper ([`Layer::Background`]) and the shell's own chrome
    /// — taskbar, start menu, popups — which has to stay in front of
    /// application windows however often they are clicked
    /// ([`Layer::Overlay`]).
    #[must_use]
    pub const fn layer(mut self, layer: Layer) -> Self {
        self.spec.layer = layer;
        self
    }

    /// The underlying protocol description, for a caller that needs it.
    #[must_use]
    pub fn spec(&self) -> &WindowSpec {
        &self.spec
    }

    /// Create the window and register it with the loop, returning its id.
    ///
    /// Blocks until the compositor answers. The id is the compositor's to
    /// choose — a client that minted its own would be naming windows that do
    /// not exist.
    ///
    /// # Errors
    ///
    /// As [`Connection::create_window`]: [`ClientError::Refused`] if the
    /// compositor declined, [`ClientError::Closed`] if it went away first.
    pub fn build<T: Transport>(self, events: &mut EventLoop<T>) -> Result<u64, Error<T>> {
        events.create(self.spec)
    }
}

// ---------------------------------------------------------------------------
// Event loop
// ---------------------------------------------------------------------------

/// One connection, every window on it, and the loop that drives them.
pub struct EventLoop<T: Transport> {
    conn: Connection<T>,
    windows: Vec<Window>,
    /// Events read from the connection but not yet handed to the application.
    pending: VecDeque<InputEvent>,
    running: bool,
    /// Events addressed to a window this loop does not know about.
    ///
    /// Counted rather than dropped: a nonzero value means the compositor is
    /// misrouting, or that a window was closed while its events were in
    /// flight — the second is ordinary and the first is a bug, and neither is
    /// visible if the events simply vanish.
    unrouted: u64,
}

impl<T: Transport> EventLoop<T> {
    /// Take over a transport. No traffic happens until something asks for it.
    pub fn new(transport: T) -> Self {
        Self::over(Connection::new(transport))
    }

    /// Take over an existing connection.
    pub const fn over(conn: Connection<T>) -> Self {
        Self {
            conn,
            windows: Vec::new(),
            pending: VecDeque::new(),
            running: false,
            unrouted: 0,
        }
    }

    /// The connection underneath, for requests this crate does not wrap.
    pub const fn connection(&mut self) -> &mut Connection<T> {
        &mut self.conn
    }

    /// Ask the compositor about the display.
    ///
    /// # Errors
    ///
    /// As [`Connection::round_trip`], plus [`ClientError::Mismatched`] if the
    /// answer is not display information.
    pub fn display_info(&mut self) -> Result<DisplayInfo, Error<T>> {
        match self.conn.round_trip(RequestBody::GetDisplayInfo)? {
            ResponseBody::Display(info) => Ok(info),
            ResponseBody::Error { message } => Err(ClientError::Refused(message)),
            ResponseBody::Ok | ResponseBody::WindowCreated { .. } => Err(ClientError::Mismatched),
        }
    }

    /// Create a window from a protocol spec. [`WindowBuilder::build`] is the
    /// usual way in.
    ///
    /// # Errors
    ///
    /// As [`Connection::create_window`].
    pub fn create(&mut self, spec: WindowSpec) -> Result<u64, Error<T>> {
        let id = self.conn.create_window(spec.clone())?;
        self.windows.push(Window {
            id,
            title: spec.title,
            width: spec.width,
            height: spec.height,
            // The compositor places an unpositioned window and reports where
            // with `Event::Moved`; until then the origin is a placeholder and
            // is documented as such rather than guessed at.
            x: spec.position.map_or(0, |p| p.0),
            y: spec.position.map_or(0, |p| p.1),
            visible: true,
            focused: false,
            attributes: WindowAttributes {
                resizable: spec.resizable,
                decorations: spec.decorations,
                transparent: spec.transparent,
                min_size: spec.min_size,
                max_size: spec.max_size,
            },
        });
        Ok(id)
    }

    /// What is known about one window.
    #[must_use]
    pub fn window(&self, id: u64) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// A handle that can change one window.
    pub fn window_mut(&mut self, id: u64) -> Option<WindowHandle<'_, T>> {
        // Checked before the handle is built so that the handle's own accessor
        // can be infallible.
        if self.windows.iter().any(|w| w.id == id) {
            Some(WindowHandle { events: self, id })
        } else {
            None
        }
    }

    /// Every window this loop owns.
    #[must_use]
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Start or stop being told about *other* clients' windows.
    ///
    /// For shells — a taskbar, a window switcher, an accessibility tool. An
    /// ordinary application has no use for this and should not call it: the
    /// windows it owns are already in [`windows`](Self::windows), and what the
    /// rest of the desktop has open is none of its business.
    ///
    /// While subscribed, [`poll`](Self::poll) and [`run`](Self::run) keep
    /// [`desktop_windows`](Self::desktop_windows) current.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn watch_desktop(&mut self, on: bool) -> Result<(), Error<T>> {
        self.conn.subscribe_window_list(on)
    }

    /// Activate, minimise, restore, maximise or close a window this loop does
    /// not own.
    ///
    /// The other half of [`watch_desktop`](Self::watch_desktop), and for the
    /// same callers: a list of the desktop's windows is only useful to a shell
    /// that can then act on one. `window` is an id from
    /// [`desktop_windows`](Self::desktop_windows) — this loop's own windows go
    /// through [`WindowHandle`], which needs no such privilege.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`]. A refusal usually means the window closed
    /// between the list the button was drawn from and the click, which is an
    /// ordinary race: a shell should repaint rather than treat it as a fault.
    pub fn control_window(
        &mut self,
        window: u64,
        action: ShellControlAction,
    ) -> Result<(), Error<T>> {
        self.conn.shell_control(window, action)
    }

    /// Tell the compositor the user's appearance settings have changed on disk.
    ///
    /// For the one application that edits them — Settings — to call *after* it
    /// has written `appearance.yaml`, so that window corners and drop shadows
    /// change on the windows already open instead of at the next login.
    ///
    /// Note what this does not take: the settings. It is a notification, not a
    /// setter, and the compositor answers it by re-reading the user's own file.
    /// The reason is that any process which can open the display socket could
    /// otherwise dictate how every window on the desktop is drawn — a title bar
    /// the colour of its own text is a perfectly legal set of settings. Here the
    /// worst a hostile client achieves is making the compositor re-read a file
    /// it may have no permission to write. See
    /// [`RequestBody::ReloadAppearance`] for the wire form.
    ///
    /// Sending this when nothing changed is harmless: the compositor compares
    /// what it read against what it holds and repaints only on a difference.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`].
    pub fn appearance_changed(&mut self) -> Result<(), Error<T>> {
        self.conn.confirm(RequestBody::ReloadAppearance)
    }

    /// Every window on the desktop, bottom-to-top, as of the last update.
    ///
    /// Empty until the first list arrives — which, for a client that never
    /// called [`watch_desktop`](Self::watch_desktop), is never.
    ///
    /// These are *not* [`Window`]s: a `Window` is one this loop owns and can
    /// draw into, and these are mostly other people's. Keeping the types apart
    /// is deliberate, so that no code can drift into submitting a picture for a
    /// window it merely knows about.
    #[must_use]
    pub fn desktop_windows(&self) -> &[WindowInfo] {
        self.conn.window_list().unwrap_or(&[])
    }

    /// How many desktop window lists have arrived.
    ///
    /// A shell redraws its taskbar when this changes, rather than diffing the
    /// list against its own copy every frame.
    #[must_use]
    pub fn desktop_revision(&self) -> u64 {
        self.conn.window_list_revision()
    }

    /// How many windows this loop owns.
    #[must_use]
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// How many events arrived for a window this loop does not know about.
    #[must_use]
    pub const fn unrouted_events(&self) -> u64 {
        self.unrouted
    }

    /// Send one window's picture. Does not wait.
    ///
    /// # Errors
    ///
    /// [`ClientError::Transport`] if the write fails.
    pub fn submit(&mut self, window: u64, tree: &RenderTree) -> Result<(), Error<T>> {
        self.conn.submit(window, tree)
    }

    /// Destroy a window and forget it.
    ///
    /// # Errors
    ///
    /// As [`Connection::confirm`]. The window is forgotten locally even if the
    /// compositor reports an error, because an id it refuses to destroy is one
    /// this process can no longer usefully address.
    pub fn close(&mut self, window: u64) -> Result<(), Error<T>> {
        let result = self.conn.confirm(RequestBody::DestroyWindow { window });
        self.windows.retain(|w| w.id != window);
        result
    }

    /// Read whatever is available and take the next event, if any.
    ///
    /// Returns `Ok(None)` when nothing is waiting — the ordinary state of an
    /// idle desktop, not an error.
    ///
    /// # Errors
    ///
    /// As [`Connection::pump`].
    pub fn poll(&mut self) -> Result<Option<(u64, Event)>, Error<T>> {
        if self.pending.is_empty() {
            self.conn.pump()?;
            self.pending.extend(self.conn.drain_events());
        }
        while let Some(ev) = self.pending.pop_front() {
            let Some(window) = self.windows.iter_mut().find(|w| w.id == ev.window) else {
                self.unrouted = self.unrouted.saturating_add(1);
                continue;
            };
            // Folded in before the application sees it, so a handler that asks
            // the window how big it is during a `Resize` gets the new answer.
            window.apply(&ev.event);
            return Ok(Some((ev.window, ev.event)));
        }
        Ok(None)
    }

    /// Run until the handler asks to stop, [`Self::quit`] is called, or the
    /// connection closes.
    ///
    /// The handler receives the loop itself, because responding to an event
    /// almost always means drawing, and drawing needs the connection. A handler
    /// that could not reach it would force every application to keep its
    /// windows in a second place outside the loop that owns them.
    ///
    /// # Errors
    ///
    /// As [`Connection::pump`] and [`Connection::wait`].
    pub fn run<F>(&mut self, mut handler: F) -> Result<(), Error<T>>
    where
        F: FnMut(&mut Self, u64, Event) -> EventResponse,
    {
        self.running = true;
        while self.running && self.conn.is_open() {
            let mut dispatched = false;
            while let Some((window, event)) = self.poll()? {
                dispatched = true;
                // A close request the handler does not act on still closes the
                // window. A title-bar X that does nothing is worse than an
                // application that quits when it would rather not have.
                let requested_close = matches!(event, Event::CloseRequested);
                if handler(self, window, event) == EventResponse::Exit || requested_close {
                    self.running = false;
                    break;
                }
            }
            // Only block when there was nothing to do. Waiting after a burst
            // would add a frame of latency to the next one for no benefit.
            if !dispatched && self.running {
                self.conn.wait()?;
            }
        }
        self.running = false;
        Ok(())
    }

    /// Stop [`Self::run`] at the end of the current batch.
    pub const fn quit(&mut self) {
        self.running = false;
    }

    /// Whether [`Self::run`] is executing.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }

    /// Place an event at the front of the queue as though it had arrived.
    ///
    /// For synthetic input — a test, a scripted demo, an accessibility tool
    /// driving an application. It does not reach the compositor, so it cannot
    /// be used to fake something the compositor would have had to agree to.
    pub fn inject_event(&mut self, window: u64, event: Event) {
        self.pending.push_back(InputEvent::new(window, event));
    }

    /// Send a request and wait for the acknowledgement.
    fn confirm(&mut self, body: RequestBody) -> Result<(), Error<T>> {
        self.conn.confirm(body)
    }

    /// Update the local record of a window, if it is still known.
    fn record_mut(&mut self, id: u64, f: impl FnOnce(&mut Window)) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            f(w);
        }
    }
}

/// A compositor that answers, for testing an application's event loop.
///
/// An application cannot have a window without a compositor to grant one, so
/// until this module was public there was no way to test an application's event
/// loop at all: [`EventLoop::create`] blocks for an id that nothing would send.
/// Every application in the tree would otherwise have had to write its own
/// stand-in compositor, and each copy would be a fresh chance to get the wire
/// format subtly wrong in a way that made the *test* pass.
///
/// It is **not a simulation of the protocol**. Requests are decoded and
/// responses encoded with the real `guiremote` codecs, so a test written
/// against this fails when the wire format is wrong. Only the *policy* behind
/// the answers is stubbed — which window id to hand out, whether to refuse —
/// and that is what a test of an application is entitled to stand in for.
///
/// It is compiled unconditionally rather than behind `#[cfg(test)]`, for the
/// same reason `guiremote::loopback` is: a `#[cfg(test)]` item is invisible to
/// every *other* crate's tests, which are precisely the ones that need it.
///
/// ```rust
/// use oswindow::{Event, EventResponse, InputEvent, WindowBuilder, testing};
///
/// let (mut events, desktop) = testing::desktop();
/// let id = WindowBuilder::new("Test", 800, 600).build(&mut events).unwrap();
///
/// // Queue a batch of input to arrive the next time the client blocks.
/// desktop
///     .borrow_mut()
///     .script
///     .push_back(vec![InputEvent::new(id, Event::CloseRequested)]);
///
/// let mut seen = 0;
/// events
///     .run(|_, _, _| {
///         seen += 1;
///         EventResponse::Continue
///     })
///     .unwrap();
/// assert_eq!(seen, 1, "the close request was delivered");
/// ```
pub mod testing {
    // A test double's contract is to fail loudly the instant the code under
    // test is wrong, so panicking on bad input is the feature and not the
    // oversight: a harness that swallowed a malformed frame and returned an
    // empty `Vec` would turn an encoder bug into a test that passes. The
    // panics are documented per-function under `# Panics`.
    //
    // Note what is *not* allowed here: `indexing_slicing` and
    // `arithmetic_side_effects` stay on. Those are not the harness reporting a
    // fault in its subject — they are faults in the harness itself, and one of
    // them (a decode loop that can spin forever) fails in the single way a
    // test harness must never fail, by hanging instead of reporting.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use guiremote::control::{
        DisplayInfo, Request, RequestBody, Response, ResponseBody, decode_requests,
        encode_responses,
    };
    use guiremote::input::InputEvent;
    use guiremote::loopback::{Pipe, pipe};
    use guiremote::submit::decode_submit;
    use guiremote::window_list::WindowInfo;

    use crate::{EventLoop, Transport};

    /// The compositor's side of the pipe.
    ///
    /// Not a simulation of the *protocol* — it runs the real decoders and
    /// encoders, so a test here fails if the wire format is wrong. Only the
    /// policy behind the answers is stubbed, which is what a test of this crate
    /// is entitled to stand in for.
    pub struct TestDesktop {
        pub pipe: Pipe,
        pub next_window: u64,
        /// Requests seen, so a test can assert what actually went out.
        pub seen: Vec<Request>,
        /// Frames drawn, as `(window, command count)`, in arrival order.
        pub submitted: Vec<(u64, usize)>,
        /// When set, every request is refused with this message.
        pub refuse: Option<String>,
        /// Input to deliver, one batch per turn.
        pub script: VecDeque<Vec<InputEvent>>,
    }

    impl TestDesktop {
        /// A desktop that will answer on `pipe`.
        #[must_use]
        pub fn new(pipe: Pipe) -> Self {
            Self {
                pipe,
                next_window: 100,
                seen: Vec::new(),
                submitted: Vec::new(),
                refuse: None,
                script: VecDeque::new(),
            }
        }

        /// Read everything waiting, recording requests and submissions.
        ///
        /// Both are recorded rather than one being answered and the other
        /// discarded, because the two arrive interleaved on one connection and
        /// a read consumes whatever it finds. A version that dropped
        /// submissions here would make them visible only to a test that never
        /// let the compositor take a turn — which excludes every test of an
        /// event loop, since taking turns is how the loop makes progress.
        fn absorb(&mut self) -> Vec<Request> {
            let mut wire = Vec::new();
            self.pipe.read(&mut wire).unwrap();

            let mut at = 0usize;
            let mut requests = Vec::new();
            // `while let Some(rest)` rather than `while at < wire.len()`: the
            // slice is taken once, checked once, and then reused, so there is
            // no second place where `at` has to be independently known to be
            // in range. A `None` here means `at` ran past the end, which the
            // arithmetic below makes unreachable — but breaking is the right
            // answer either way, since there is nothing left to decode.
            while let Some(rest) = wire.get(at..) {
                if rest.is_empty() {
                    break;
                }
                // Control requests and submissions share one connection; only
                // the former want an answer. Each direction has its own magic,
                // so telling them apart is a four-byte test and not a guess.
                let used = if rest.starts_with(&guiremote::SUBMIT_MAGIC) {
                    let (sub, used) = decode_submit(rest).unwrap();
                    self.submitted
                        .push((sub.window, sub.commands.commands.len()));
                    used
                } else {
                    let (reqs, used) = decode_requests(rest).unwrap();
                    requests.extend(reqs);
                    used
                };
                // A decoder reporting that it consumed nothing would leave
                // `at` where it was and spin here forever. That is the one
                // failure a test harness must not have: a hang reports
                // nothing, so the suite dies on a timeout naming no test,
                // whereas this names the byte and the decoder.
                assert!(
                    used > 0,
                    "a decoder consumed no bytes at offset {at} of {} — the \
                     frame would be re-decoded forever",
                    wire.len()
                );
                at = at
                    .checked_add(used)
                    .expect("decoded length ran past the end of the buffer");
            }
            requests
        }

        /// Answer every pending request. Reports whether anything was said.
        ///
        /// # Panics
        ///
        /// If the client sent something that will not decode. That is a test
        /// harness's job: a malformed frame here means the encoder under test
        /// is wrong, and failing loudly at the byte that proves it is the
        /// diagnosis.
        pub fn serve(&mut self) -> bool {
            let mut replies = Vec::new();
            for req in self.absorb() {
                let body = if let Some(why) = &self.refuse {
                    ResponseBody::Error {
                        message: why.clone(),
                    }
                } else {
                    match &req.body {
                        RequestBody::CreateWindow(_) => {
                            let window = self.next_window;
                            // `checked_add`, not `wrapping_add`: wrapping would
                            // hand out an id already in use, and a harness that
                            // silently aliases two windows makes the test it
                            // breaks look like a bug in the crate under test.
                            self.next_window = self
                                .next_window
                                .checked_add(1)
                                .expect("ran out of window ids");
                            ResponseBody::WindowCreated { window }
                        }
                        RequestBody::GetDisplayInfo => ResponseBody::Display(DisplayInfo {
                            width: 2560,
                            height: 1440,
                            refresh_rate: 144,
                            scale_factor: 1.5,
                        }),
                        _ => ResponseBody::Ok,
                    }
                };
                replies.push(Response::new(req.seq, body));
                self.seen.push(req);
            }
            if replies.is_empty() {
                return false;
            }
            self.pipe.write(&encode_responses(&replies)).unwrap();
            true
        }

        /// Deliver input to the client immediately.
        ///
        /// # Panics
        ///
        /// Never in practice: the loopback pipe's write is infallible while the
        /// pipe is open, and a closed one is a test that has already ended.
        pub fn send_input(&mut self, events: &[InputEvent]) {
            self.pipe
                .write(&guiremote::encode_input_frame(events))
                .unwrap();
        }

        /// Push a desktop window list, as a compositor does to a subscribed
        /// shell.
        ///
        /// Unconditional on purpose: the harness does *not* check that the
        /// client subscribed first. Whether an unsubscribed client is sent a
        /// list is the compositor's rule, tested where the compositor is; here
        /// the question is only what the client does with one that arrives.
        ///
        /// # Panics
        ///
        /// As [`Self::send_input`].
        pub fn send_window_list(&mut self, windows: &[WindowInfo]) {
            self.pipe
                .write(&guiremote::encode_window_list(windows))
                .unwrap();
        }

        /// One turn of the other end, taken whenever the client blocks.
        ///
        /// A turn in which the server has nothing to answer and nothing left to
        /// say ends the conversation. That rule is what makes every blocking
        /// call in these tests terminate: a test where neither side has
        /// anything further is over, and hanging up is how a compositor would
        /// say so. Without it a client waiting for input that will never come
        /// would spin until the harness timed out.
        pub fn turn(&mut self) {
            let answered = self.serve();
            if let Some(batch) = self.script.pop_front() {
                self.send_input(&batch);
            } else if !answered {
                self.pipe.close();
            }
        }

        /// Everything the client has drawn so far, as `(window, command count)`,
        /// in the order it was drawn.
        ///
        /// Cumulative over the desktop's whole life, including frames that
        /// arrived during a [`Self::turn`]. Requests waiting alongside them are
        /// recorded but *not* answered — call [`Self::serve`] for that.
        ///
        /// # Panics
        ///
        /// As [`Self::serve`]: on a frame that will not decode.
        pub fn drawn(&mut self) -> Vec<(u64, usize)> {
            let pending = self.absorb();
            self.seen.extend(pending);
            self.submitted.clone()
        }
    }

    /// A transport that gives the other end a turn when the client blocks.
    ///
    /// Both halves of a [`Pipe`] live on one thread, so a client blocked in a
    /// request would never see its answer: nothing can run while it waits. On a
    /// socket the kernel is what runs the other side; here the test is. So
    /// `wait` — whose whole contract is "block until there is plausibly
    /// something to read" — is implemented as "let the compositor act", which
    /// is exactly what blocking means when there is only one thread.
    pub struct TestConnection {
        pub pipe: Pipe,
        pub server: Rc<RefCell<TestDesktop>>,
    }

    impl Transport for TestConnection {
        type Error = std::convert::Infallible;

        fn read(&mut self, buf: &mut Vec<u8>) -> Result<usize, Self::Error> {
            self.pipe.read(buf)
        }

        fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.pipe.write(bytes)
        }

        fn is_open(&self) -> bool {
            self.pipe.is_open()
        }

        fn wait(&mut self) -> Result<(), Self::Error> {
            self.server.borrow_mut().turn();
            Ok(())
        }
    }

    /// A loop wired to a compositor that answers when the client waits.
    ///
    /// The desktop is shared so a test can script input into it and read back
    /// what the application drew, while the loop holds its own end of the pipe.
    #[must_use]
    pub fn desktop() -> (EventLoop<TestConnection>, Rc<RefCell<TestDesktop>>) {
        let (client_end, server_end) = pipe();
        let server = Rc::new(RefCell::new(TestDesktop::new(server_end)));
        let transport = TestConnection {
            pipe: client_end,
            server: Rc::clone(&server),
        };
        (EventLoop::new(transport), server)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::testing::{TestConnection, desktop as wired};
    use super::*;

    /// Create a window through the real path, blocking until the id arrives.
    fn open(events: &mut EventLoop<TestConnection>, title: &str) -> u64 {
        events
            .create(WindowSpec::new(title, 800, 600))
            .expect("the compositor should have created it")
    }

    #[test]
    fn a_window_id_comes_from_the_compositor_and_not_from_a_local_counter() {
        // The defect that made the previous version of this crate a
        // simulation: ids were minted locally, so a client named windows that
        // did not exist and could never have been told otherwise.
        let (mut events, server) = wired();
        server.borrow_mut().next_window = 0x00C0_FFEE;
        assert_eq!(open(&mut events, "A"), 0x00C0_FFEE);
        assert_eq!(events.window_count(), 1);
    }

    #[test]
    fn creating_a_window_blocks_until_the_compositor_answers() {
        // Not a formality: the id is the compositor's to choose, so there is
        // nothing to return until it has spoken. A version that returned early
        // would have to invent one.
        let (mut events, server) = wired();
        assert!(events.window(100).is_none(), "nothing exists yet");
        let id = open(&mut events, "A");
        assert!(events.window(id).is_some());
        assert!(matches!(
            server.borrow().seen[0].body,
            RequestBody::CreateWindow(_)
        ));
    }

    #[test]
    fn building_a_window_sends_the_spec_the_builder_describes() {
        let (mut events, server) = wired();
        let id = WindowBuilder::new("Settings", 640, 480)
            .position(10, -20)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .min_size(320, 240)
            .max_size(1280, 960)
            .build(&mut events)
            .unwrap();

        {
            let borrowed = server.borrow();
            let RequestBody::CreateWindow(sent) = &borrowed.seen[0].body else {
                panic!("expected a create");
            };
            assert_eq!(sent.title, "Settings");
            assert_eq!(sent.position, Some((10, -20)));
            assert!(!sent.resizable);
            assert!(!sent.decorations);
            assert!(sent.transparent);
            assert_eq!(sent.min_size, Some((320, 240)));
            assert_eq!(sent.max_size, Some((1280, 960)));
        }

        // And the local record repeats the spec rather than inventing defaults.
        let w = events.window(id).unwrap();
        assert_eq!(w.size(), (640, 480));
        assert_eq!(w.position(), (10, -20));
        assert!(!w.is_resizable());
        assert!(!w.has_decorations());
        assert!(w.is_transparent());
        assert_eq!(w.min_size(), Some((320, 240)));
        assert_eq!(w.max_size(), Some((1280, 960)));
        assert_eq!(
            w.attributes(),
            WindowAttributes {
                resizable: false,
                decorations: false,
                transparent: true,
                min_size: Some((320, 240)),
                max_size: Some((1280, 960)),
            }
        );
    }

    #[test]
    fn a_resize_updates_what_the_window_reports_before_the_handler_sees_it() {
        // A handler that asks "how big am I?" while handling a resize must get
        // the new answer; the old one is a frame drawn at the wrong size.
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        assert_eq!(events.window(id).unwrap().size(), (800, 600));

        server.borrow_mut().send_input(&[InputEvent::new(
            id,
            Event::Resize {
                width: 1024,
                height: 768,
            },
        )]);
        let (w, ev) = events.poll().unwrap().expect("an event");
        assert_eq!(w, id);
        assert!(matches!(ev, Event::Resize { .. }));
        assert_eq!(events.window(id).unwrap().size(), (1024, 768));
    }

    #[test]
    fn a_move_the_client_did_not_ask_for_still_updates_its_position() {
        // A user dragging the title bar. Without `Event::Moved` a window could
        // only know where it last asked to be, and anything placed in screen
        // coordinates would be placed against a stale answer.
        let (mut events, server) = wired();
        let id = open(&mut events, "A");

        server
            .borrow_mut()
            .send_input(&[InputEvent::new(id, Event::Moved { x: -300, y: 40 })]);
        events.poll().unwrap().expect("an event");
        assert_eq!(events.window(id).unwrap().position(), (-300, 40));
    }

    #[test]
    fn a_move_request_does_not_pretend_to_know_where_the_window_landed() {
        // The compositor may snap the window to an edge or clamp it to a
        // monitor. Recording the asked-for position would make `position()` a
        // record of the request rather than of the result, which is the bug
        // this whole layer exists to avoid.
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        events
            .window_mut(id)
            .unwrap()
            .set_position(5000, 5000)
            .unwrap();
        assert_eq!(
            events.window(id).unwrap().position(),
            (0, 0),
            "still what was last reported, not what was asked"
        );

        server
            .borrow_mut()
            .send_input(&[InputEvent::new(id, Event::Moved { x: 1900, y: 0 })]);
        events.poll().unwrap();
        assert_eq!(events.window(id).unwrap().position(), (1900, 0));
    }

    #[test]
    fn focus_is_tracked_from_the_events_that_report_it() {
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        assert!(!events.window(id).unwrap().is_focused());

        server
            .borrow_mut()
            .send_input(&[InputEvent::new(id, Event::FocusIn)]);
        events.poll().unwrap();
        assert!(events.window(id).unwrap().is_focused());

        server
            .borrow_mut()
            .send_input(&[InputEvent::new(id, Event::FocusOut)]);
        events.poll().unwrap();
        assert!(!events.window(id).unwrap().is_focused());
    }

    #[test]
    fn a_submission_reaches_the_compositor_addressed_to_the_right_window() {
        // The reason `SURF` exists: two windows on one connection, and a bare
        // `ORDR` frame says nothing about which one it is for.
        let (mut events, server) = wired();
        let a = open(&mut events, "A");
        let b = open(&mut events, "B");
        assert_ne!(a, b);

        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 4.0, 4.0, guitk::color::Color::WHITE);
        events.submit(a, &tree).unwrap();
        events.submit(b, &RenderTree::new()).unwrap();

        assert_eq!(server.borrow_mut().drawn(), vec![(a, 1), (b, 0)]);
    }

    #[test]
    fn setting_a_title_waits_for_the_compositor_to_agree() {
        // Fire-and-forget would let a refusal pass unnoticed and leave the
        // local record disagreeing with the screen.
        let (mut events, server) = wired();
        let id = open(&mut events, "Before");
        events.window_mut(id).unwrap().set_title("After").unwrap();

        assert_eq!(events.window(id).unwrap().title(), "After");
        assert!(matches!(
            server.borrow().seen.last().unwrap().body,
            RequestBody::SetTitle { .. }
        ));
    }

    #[test]
    fn a_refused_property_change_leaves_the_local_record_alone() {
        let (mut events, server) = wired();
        let id = open(&mut events, "Before");
        server.borrow_mut().refuse = Some("no".to_string());

        let err = events
            .window_mut(id)
            .unwrap()
            .set_title("After")
            .expect_err("a refusal must not read as success");
        assert!(matches!(err, ClientError::Refused(_)));
        assert_eq!(
            events.window(id).unwrap().title(),
            "Before",
            "the record must not claim a change the compositor rejected"
        );
    }

    #[test]
    fn a_handle_names_a_window_by_id_so_it_cannot_address_the_wrong_one() {
        let (mut events, _server) = wired();
        let a = open(&mut events, "A");
        let b = open(&mut events, "B");
        assert!(events.window_mut(b.wrapping_add(1)).is_none());

        let mut handle = events.window_mut(b).unwrap();
        assert_eq!(handle.id(), b);
        assert_eq!(handle.get().title(), "B");
        handle.set_title("renamed").unwrap();
        assert_eq!(events.window(a).unwrap().title(), "A");
        assert_eq!(events.window(b).unwrap().title(), "renamed");
    }

    #[test]
    fn an_event_for_an_unknown_window_is_counted_not_dispatched() {
        let (mut events, server) = wired();
        let id = open(&mut events, "A");

        server.borrow_mut().send_input(&[
            InputEvent::new(id.wrapping_add(999), Event::FocusIn),
            InputEvent::new(id, Event::CloseRequested),
        ]);
        let (w, ev) = events.poll().unwrap().expect("the good one still arrives");
        assert_eq!(w, id);
        assert!(matches!(ev, Event::CloseRequested));
        assert_eq!(events.unrouted_events(), 1);
    }

    #[test]
    fn run_stops_when_the_handler_says_so() {
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        server.borrow_mut().script.push_back(vec![
            InputEvent::new(id, Event::FocusIn),
            InputEvent::new(
                id,
                Event::Key(KeyEvent {
                    key: Key::Q,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                    text: None,
                }),
            ),
            InputEvent::new(id, Event::FocusOut),
        ]);

        let mut seen = Vec::new();
        events
            .run(|_loop, _w, event| {
                let quit = matches!(event, Event::Key(_));
                seen.push(event);
                if quit {
                    EventResponse::Exit
                } else {
                    EventResponse::Continue
                }
            })
            .unwrap();
        assert_eq!(seen.len(), 2, "the focus, then the key — not what follows");
        assert!(!events.is_running());
    }

    #[test]
    fn run_ends_on_a_close_request_the_handler_ignores() {
        // A title-bar X that does nothing is worse than an application that
        // quits when it would rather not have.
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        server
            .borrow_mut()
            .script
            .push_back(vec![InputEvent::new(id, Event::CloseRequested)]);

        let mut count = 0u32;
        events
            .run(|_loop, _w, _event| {
                count += 1;
                EventResponse::Continue
            })
            .unwrap();
        assert_eq!(count, 1);
        assert!(!events.is_running());
    }

    #[test]
    fn quit_stops_the_loop_from_inside_the_handler() {
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        server
            .borrow_mut()
            .script
            .push_back(vec![InputEvent::new(id, Event::FocusIn)]);

        events
            .run(|ev_loop, _w, _event| {
                ev_loop.quit();
                EventResponse::Continue
            })
            .unwrap();
        assert!(!events.is_running());
    }

    #[test]
    fn run_ends_when_the_connection_closes() {
        // The previous version of this crate exited its loop on the first
        // iteration with a comment saying a real one would block. This one runs
        // until there is a reason to stop.
        let (client_end, server_end) = pipe();
        let mut events = EventLoop::new(client_end);
        server_end.close();
        events
            .run(|_loop, _w, _event| EventResponse::Continue)
            .unwrap();
        assert!(!events.is_running());
    }

    #[test]
    fn a_handler_can_draw_because_it_is_given_the_loop() {
        // The old handler signature took only `(id, event)`, so an application
        // responding to an event had no way to submit the frame that response
        // called for.
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        server
            .borrow_mut()
            .script
            .push_back(vec![InputEvent::new(id, Event::CloseRequested)]);

        events
            .run(|ev_loop, window, _event| {
                let mut tree = RenderTree::new();
                tree.fill_rect(0.0, 0.0, 1.0, 1.0, guitk::color::Color::WHITE);
                ev_loop.submit(window, &tree).unwrap();
                EventResponse::Continue
            })
            .unwrap();
        assert_eq!(server.borrow_mut().drawn(), vec![(id, 1)]);
    }

    #[test]
    fn display_info_comes_from_the_compositor() {
        let (mut events, _server) = wired();
        let info = events.display_info().unwrap();
        assert_eq!(info.width, 2560);
        assert_eq!(info.height, 1440);
        assert_eq!(info.refresh_rate, 144);
        assert!((info.scale_factor - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_refused_request_is_reported_rather_than_swallowed() {
        let (mut events, server) = wired();
        server.borrow_mut().refuse = Some("out of memory".to_string());
        let err = events
            .create(WindowSpec::new("A", 1, 1))
            .expect_err("a refusal must not read as success");
        assert!(matches!(err, ClientError::Refused(ref why) if why == "out of memory"));
        assert_eq!(events.window_count(), 0, "and nothing is recorded");
    }

    #[test]
    fn closing_a_window_forgets_it_and_its_late_events_are_counted() {
        let (mut events, server) = wired();
        let id = open(&mut events, "A");
        assert_eq!(events.window_count(), 1);

        // Events already in flight when a window closes are ordinary, not a
        // bug — but they must not be dispatched to a window that is gone.
        server
            .borrow_mut()
            .send_input(&[InputEvent::new(id, Event::FocusIn)]);
        events.close(id).unwrap();
        assert_eq!(events.window_count(), 0);
        assert!(events.poll().unwrap().is_none());
        assert_eq!(events.unrouted_events(), 1);
    }

    #[test]
    fn injected_events_are_dispatched_like_real_ones() {
        let (mut events, _server) = wired();
        let id = open(&mut events, "A");

        events.inject_event(id, Event::ScaleChanged { scale: 2.0 });
        let (w, ev) = events.poll().unwrap().expect("an event");
        assert_eq!(w, id);
        assert!(matches!(ev, Event::ScaleChanged { .. }));
    }

    #[test]
    fn there_is_exactly_one_event_type_end_to_end() {
        // The previous version defined a third vocabulary alongside
        // `guitk::event::Event` and the compositor's, so connecting it would
        // have needed a lossy translation in the middle of the input path.
        // What the compositor encodes is what the handler receives, unchanged.
        let (mut events, server) = wired();
        let id = open(&mut events, "A");

        let sent = Event::Mouse(MouseEvent {
            x: 12.5,
            y: -3.25,
            kind: MouseEventKind::Scroll { dx: 1.0, dy: -2.0 },
        });
        server
            .borrow_mut()
            .send_input(&[InputEvent::new(id, sent.clone())]);
        let (_, got) = events.poll().unwrap().expect("an event");
        assert_eq!(got, sent);
    }

    #[test]
    fn many_windows_share_one_connection() {
        // The design this crate is built on: one transport, demultiplexed by
        // window id, rather than a socket per window.
        let (mut events, server) = wired();
        let ids: Vec<u64> = (0..4)
            .map(|i| open(&mut events, &format!("W{i}")))
            .collect();
        assert_eq!(events.window_count(), 4);

        for &id in &ids {
            server
                .borrow_mut()
                .send_input(&[InputEvent::new(id, Event::FocusIn)]);
        }
        let mut focused = Vec::new();
        while let Some((w, _)) = events.poll().unwrap() {
            focused.push(w);
        }
        assert_eq!(focused, ids);
        assert!(events.windows().iter().all(Window::is_focused));
    }

    /// Two windows differing in every field, so a codec or a store that
    /// confused one for the other cannot pass.
    fn two_desktop_windows() -> Vec<WindowInfo> {
        vec![
            WindowInfo {
                id: 7,
                pid: 1234,
                layer: Layer::Background,
                title: "Wallpaper".to_owned(),
                visible: true,
                minimized: false,
                maximized: false,
                focused: false,
            },
            WindowInfo {
                id: 9,
                pid: 5678,
                layer: Layer::Normal,
                title: "Editor".to_owned(),
                visible: false,
                minimized: true,
                maximized: true,
                focused: true,
            },
        ]
    }

    #[test]
    fn watching_the_desktop_asks_the_compositor_instead_of_assuming() {
        // A shell cannot see other clients' windows by default, and nothing
        // local could make it: the answer lives in the compositor. So the
        // observable effect of `watch_desktop` must be a request on the wire.
        let (mut events, server) = wired();
        events.watch_desktop(true).unwrap();
        assert!(
            server
                .borrow()
                .seen
                .iter()
                .any(|r| matches!(r.body, RequestBody::SubscribeWindowList { subscribe: true })),
            "watch_desktop should have subscribed on the wire"
        );

        events.watch_desktop(false).unwrap();
        assert!(
            server.borrow().seen.iter().any(|r| matches!(
                r.body,
                RequestBody::SubscribeWindowList { subscribe: false }
            )),
            "unwatching should have unsubscribed on the wire"
        );
    }

    #[test]
    fn telling_the_compositor_the_settings_changed_sends_it_nothing_but_the_news() {
        // Two claims in one, and both are the point of the call. First that it
        // is on the wire at all: an application writing `appearance.yaml`
        // changes nothing about a compositor that has already read it, so a
        // local no-op here would look identical until the user noticed their
        // window corners did not change until they logged out.
        let (mut events, server) = wired();
        events.appearance_changed().unwrap();
        assert!(
            server
                .borrow()
                .seen
                .iter()
                .any(|r| matches!(r.body, RequestBody::ReloadAppearance)),
            "appearance_changed should have asked the compositor to re-read the file"
        );

        // Second that it works on a loop that owns no windows — which this one
        // does not, and which is not incidental: the application that has cause
        // to send it is a settings dialog, and requiring it to have opened a
        // window first would be requiring it for no reason the protocol has.
        assert_eq!(events.window_count(), 0);
    }

    #[test]
    fn a_client_that_never_watched_sees_an_empty_desktop() {
        // Not an error and not a guess: a client with no subscription has been
        // told nothing, and the honest report of that is "nothing", not a list
        // assembled from the windows it happens to own itself.
        let (mut events, _server) = wired();
        open(&mut events, "A");
        assert!(events.desktop_windows().is_empty());
        assert_eq!(events.desktop_revision(), 0);
    }

    #[test]
    fn a_desktop_list_arrives_field_for_field() {
        let (mut events, server) = wired();
        events.watch_desktop(true).unwrap();
        let sent = two_desktop_windows();
        server.borrow_mut().send_window_list(&sent);

        // No input in that frame, so there is no event to return — the point
        // is the side effect the pump had while reading it.
        assert!(events.poll().unwrap().is_none());
        assert_eq!(events.desktop_windows(), sent.as_slice());
        assert_eq!(events.desktop_revision(), 1);
    }

    #[test]
    fn a_later_list_replaces_the_earlier_one_rather_than_adding_to_it() {
        // Each list is the whole desktop, so appending would leave a window
        // that has closed visible in a taskbar forever.
        let (mut events, server) = wired();
        events.watch_desktop(true).unwrap();
        server.borrow_mut().send_window_list(&two_desktop_windows());
        assert!(events.poll().unwrap().is_none());

        let only = vec![WindowInfo::new(9, 5678, "Editor")];
        server.borrow_mut().send_window_list(&only);
        assert!(events.poll().unwrap().is_none());

        assert_eq!(events.desktop_windows(), only.as_slice());
        assert_eq!(
            events.desktop_revision(),
            2,
            "the revision counts lists received, so a shell can redraw on change"
        );
    }

    #[test]
    fn a_closing_desktop_is_reported_as_empty_and_not_as_unchanged() {
        // The last window closing is exactly when a taskbar must clear itself,
        // and an empty list is a legal frame rather than a no-op.
        let (mut events, server) = wired();
        events.watch_desktop(true).unwrap();
        server.borrow_mut().send_window_list(&two_desktop_windows());
        assert!(events.poll().unwrap().is_none());
        assert_eq!(events.desktop_windows().len(), 2);

        server.borrow_mut().send_window_list(&[]);
        assert!(events.poll().unwrap().is_none());
        assert!(events.desktop_windows().is_empty());
        assert_eq!(events.desktop_revision(), 2);
    }

    #[test]
    fn a_desktop_list_does_not_disturb_the_windows_this_client_owns() {
        // The two window sets are separate stores. A list naming ids this
        // client does not own must not create, drop or re-route anything in
        // its own, and must not count as an unrouted event either.
        let (mut events, server) = wired();
        let mine = open(&mut events, "Mine");
        events.watch_desktop(true).unwrap();

        server.borrow_mut().send_window_list(&two_desktop_windows());
        assert!(events.poll().unwrap().is_none());

        assert_eq!(events.window_count(), 1);
        assert!(events.window(mine).is_some());
        assert_eq!(events.unrouted_events(), 0);

        // And input still routes normally afterwards.
        server
            .borrow_mut()
            .send_input(&[InputEvent::new(mine, Event::FocusIn)]);
        let (w, _) = events.poll().unwrap().expect("an event");
        assert_eq!(w, mine);
    }
}
