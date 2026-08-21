//! Window-control protocol — the third direction, and the one that lets a
//! window exist at all.
//!
//! [`crate`] carries drawing out ([`encode_frame`](crate::encode_frame)) and
//! input back ([`input`](crate::input)). Neither can *create* a window, name
//! it, move it, or ask how big the screen is. Those are requests with replies
//! rather than a stream of one-way notifications, and they are what this module
//! encodes.
//!
//! Until it existed the gap was filled by `oswindow`, which declared exactly
//! these messages and then answered them itself — `Connection::send` pushed a
//! request and immediately called `simulate_response()` to pop it and invent
//! the reply. That is a convincing-looking client with no compositor behind it,
//! and window ids came from a process-local counter, so two applications would
//! have used the same one.
//!
//! ## Two magics, not one frame type with a direction byte
//!
//! Requests are `b"CREQ"`, responses are `b"CRSP"`. The alternative — one magic
//! and a direction flag — costs a byte less and detects a wrong-way frame one
//! field later, after a decoder has already accepted the frame as its own kind.
//! Since a control channel is duplex by construction, that check is worth its
//! four bytes; the same reasoning gave `INPT` its own magic rather than a flag
//! on `ORDR`.
//!
//! ## Correlation
//!
//! Every [`Request`] and [`Response`] carries a `seq`. A synchronous
//! simulation never needed one — it had the answer before `send` returned — but
//! a real channel can deliver a `Resize` reply after a `GetDisplayInfo` reply
//! that was asked for later, and a client with two requests outstanding
//! otherwise cannot tell which answer is which. The client owns the numbering;
//! the compositor only echoes it.
//!
//! ## Wire format
//!
//! ```text
//! magic  : [u8;4] = b"CREQ" | b"CRSP"
//! version: u8     = CONTROL_VERSION
//! flags  : u8     = 0 (reserved)
//! n_msgs : u32                        message count, little-endian
//!   per message:
//!     seq : u32                       client-assigned correlation id
//!     tag : u8                        RequestTag / ResponseTag
//!     body: variable
//! ```
//!
//! Scalars are little-endian, `f32` is the `to_le_bytes` of its IEEE-754 bits,
//! strings are u32-length-prefixed UTF-8, and a signed integer is the
//! two's-complement bit pattern of its unsigned counterpart — the same
//! primitive conventions as every other frame in this crate.
//!
//! ## Robustness
//!
//! Malformed input is an error, never a panic. Unknown tags, reserved flag
//! bits, oversized counts and non-UTF-8 strings are all [`DecodeError`]s
//! naming what was wrong.

use crate::{DecodeError, Reader, capacity_hint, write_f32, write_string, write_u32, write_u64};

/// Request-frame magic: `b"CREQ"` (client → compositor).
pub const REQUEST_MAGIC: [u8; 4] = *b"CREQ";

/// Response-frame magic: `b"CRSP"` (compositor → client).
pub const RESPONSE_MAGIC: [u8; 4] = *b"CRSP";

/// Control protocol version. Bump on any incompatible layout change; never
/// reuse a number.
pub const CONTROL_VERSION: u8 = 1;

/// Control-frame header: magic + version + flags + message count.
const CONTROL_HEADER_LEN: usize = 4 + 1 + 1 + 4;

/// Upper bound on messages in a single control frame, so a hostile sender
/// cannot make the decoder pre-allocate unboundedly. Control traffic is a
/// handful of messages at startup and then almost nothing; this is generous by
/// three orders of magnitude.
pub const MAX_MESSAGES_PER_FRAME: u32 = 1 << 12;

// ============================================================================
// Cursor
// ============================================================================

/// Cursor shape the compositor should display over a window.
///
/// This is the one the *wire* uses, and deliberately the only one: the tree
/// previously had three near-identical cursor enums — `oswindow::CursorShape`,
/// the compositor's own, and `guitk::style::Cursor` — which is three
/// translation tables to keep in step and three chances for `Help` and
/// `NotAllowed` to swap places. `guitk::style::Cursor` survives because it is a
/// different thing: a *styling* property a widget declares, not a request to
/// the compositor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// Default pointer arrow.
    #[default]
    Arrow,
    /// Text insertion beam (I-beam).
    Text,
    /// Pointing hand, for something clickable.
    Hand,
    /// Vertical resize (north–south).
    ResizeNS,
    /// Horizontal resize (east–west).
    ResizeEW,
    /// Diagonal resize (north-east–south-west).
    ResizeNESW,
    /// Diagonal resize (north-west–south-east).
    ResizeNWSE,
    /// Move or drag.
    Move,
    /// Busy; the application is not accepting input.
    Wait,
    /// Context help.
    Help,
    /// Crosshair, for precision selection.
    Crosshair,
    /// The drop or action under the pointer is not permitted.
    NotAllowed,
    /// No cursor drawn at all.
    Hidden,
}

impl CursorShape {
    const fn to_byte(self) -> u8 {
        match self {
            Self::Arrow => 0x01,
            Self::Text => 0x02,
            Self::Hand => 0x03,
            Self::ResizeNS => 0x04,
            Self::ResizeEW => 0x05,
            Self::ResizeNESW => 0x06,
            Self::ResizeNWSE => 0x07,
            Self::Move => 0x08,
            Self::Wait => 0x09,
            Self::Help => 0x0A,
            Self::Crosshair => 0x0B,
            Self::NotAllowed => 0x0C,
            Self::Hidden => 0x0D,
        }
    }

    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::Arrow,
            0x02 => Self::Text,
            0x03 => Self::Hand,
            0x04 => Self::ResizeNS,
            0x05 => Self::ResizeEW,
            0x06 => Self::ResizeNESW,
            0x07 => Self::ResizeNWSE,
            0x08 => Self::Move,
            0x09 => Self::Wait,
            0x0A => Self::Help,
            0x0B => Self::Crosshair,
            0x0C => Self::NotAllowed,
            0x0D => Self::Hidden,
            _ => return None,
        })
    }
}

// ============================================================================
// Window creation parameters
// ============================================================================

/// Which band of the stacking order a window belongs to.
///
/// A desktop needs three kinds of surface that no amount of raising and
/// lowering can express with one flat stack: a wallpaper that is always behind
/// everything, ordinary application windows, and shell chrome — a taskbar, a
/// start menu, a popup — that is always in front. Without this, clicking an
/// application window raises it over the taskbar, because to the compositor
/// the taskbar *is* an application window.
///
/// The bands are totally ordered and a window never leaves the one it was
/// created in. Raising, focusing and stacking all happen strictly *within* a
/// band, so an ordinary window cannot climb above the shell and a wallpaper
/// cannot climb above anything. That is the whole guarantee — inside a band
/// the rules are exactly what they were before this type existed.
///
/// Deliberately three and not an integer depth: an open-ended depth invites
/// each surface to pick a number, and the numbers then encode a policy that
/// nobody wrote down and every new surface has to guess at. Three named roles
/// are what the shell actually distinguishes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// Behind every ordinary window: the wallpaper, the desktop icon surface.
    Background,
    /// Ordinary application windows. The default, and where a client that has
    /// never heard of this type lands.
    #[default]
    Normal,
    /// In front of every ordinary window: taskbar, start menu, popups, OSD.
    Overlay,
}

impl Layer {
    /// The wire byte for this layer.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        match self {
            Self::Background => 0,
            Self::Normal => 1,
            Self::Overlay => 2,
        }
    }

    /// The layer a wire byte names, or `None` if it names none of them.
    ///
    /// Returning `None` rather than defaulting to [`Layer::Normal`] is
    /// deliberate: a byte we do not recognise means the peer is speaking a
    /// protocol we do not, and silently placing its taskbar in with the
    /// application windows would be a wrong desktop rather than a refused
    /// connection.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Background),
            1 => Some(Self::Normal),
            2 => Some(Self::Overlay),
            _ => None,
        }
    }
}

// ============================================================================
// Acting on somebody else's window
// ============================================================================

/// What a shell asks the compositor to do to a window it does not own.
///
/// Every other window request in this protocol is resolved against the sending
/// connection's own windows, which is the whole ownership model: a client
/// cannot name a window it did not create. A taskbar's entire job is the
/// opposite — the button exists precisely to act on somebody else's window —
/// so those verbs cannot be reused, and this is a separate request rather than
/// a flag on them, so that "names a window I own" stays a property you can read
/// off the variant.
///
/// The actions are the ones a shell surface actually offers: a taskbar button
/// (activate, minimise), its context menu (maximise, restore, close), and an
/// Alt-Tab switcher (activate). Deliberately not move/resize: placing windows
/// is the compositor's, and a shell that could move any window would be a
/// second window manager — the duplication this part of the tree exists to
/// remove.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellControlAction {
    /// Un-minimise if minimised, then focus and raise within the window's band.
    ///
    /// One action rather than restore-then-focus because the two are not
    /// independent: the compositor refuses to focus a minimised window, so a
    /// shell issuing them separately would be relying on the order it happened
    /// to send them in.
    Activate,
    /// Minimise to the taskbar.
    Minimize,
    /// Return from minimised or maximised to the previous geometry.
    Restore,
    /// Fill the work area.
    Maximize,
    /// *Ask* the window to close — the same request its own close button makes.
    ///
    /// Not a destroy: the client is told, and an editor with unsaved changes
    /// gets to put up its dialog. A shell that could destroy a window would be
    /// able to discard a user's work from a context menu.
    Close,
}

impl ShellControlAction {
    /// The wire byte for this action.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        match self {
            Self::Activate => 0,
            Self::Minimize => 1,
            Self::Restore => 2,
            Self::Maximize => 3,
            Self::Close => 4,
        }
    }

    /// The action a wire byte names, or `None` if it names none of them.
    ///
    /// `None` rather than a default, for [`Layer::from_byte`]'s reason and one
    /// of its own: the actions are not interchangeable, and guessing would let
    /// a peer speaking a later protocol have a window minimised when it asked
    /// for something else.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Activate),
            1 => Some(Self::Minimize),
            2 => Some(Self::Restore),
            3 => Some(Self::Maximize),
            4 => Some(Self::Close),
            _ => None,
        }
    }
}

/// What a client asks for when it creates a window.
///
/// Every field is a *request*: the compositor answers with the id it assigned
/// and is free to have honoured none of the rest. A client that assumes it got
/// what it asked for will draw at the wrong size on any compositor with a tiling
/// policy, which is why the size a window is actually given arrives as an
/// [`Event::Resize`](guitk::event::Event::Resize) rather than being read back
/// from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSpec {
    /// Title for the decoration bar and the taskbar.
    pub title: String,
    /// Requested client-area width in pixels.
    pub width: u32,
    /// Requested client-area height in pixels.
    pub height: u32,
    /// Requested top-left position, or `None` to let the compositor place it.
    ///
    /// `None` rather than `(0, 0)` because they mean different things: a client
    /// that does not care should not be pinned to the top-left corner, which is
    /// where every window that does not care would then pile up.
    pub position: Option<(i32, i32)>,
    /// Whether the user may resize the window.
    pub resizable: bool,
    /// Whether the compositor draws a title bar and borders.
    pub decorations: bool,
    /// Whether the window's background may be transparent.
    pub transparent: bool,
    /// Smallest client area the window can usefully be shown at.
    pub min_size: Option<(u32, u32)>,
    /// Largest client area the window wants to be shown at.
    pub max_size: Option<(u32, u32)>,
    /// Which band of the stacking order the window belongs to.
    ///
    /// Unlike the rest of this struct this one is not merely advisory: the
    /// compositor either honours it or refuses the window, because a shell
    /// panel silently demoted to [`Layer::Normal`] would be worse than no
    /// panel — it would disappear behind the first window the user opened.
    pub layer: Layer,
}

impl WindowSpec {
    /// A titled window of the given size, with the ordinary defaults:
    /// resizable, decorated, opaque, placed by the compositor.
    #[must_use]
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            position: None,
            resizable: true,
            decorations: true,
            transparent: false,
            min_size: None,
            max_size: None,
            layer: Layer::Normal,
        }
    }
}

/// What the compositor knows about the display a client is being shown on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayInfo {
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// Refresh rate in Hz.
    pub refresh_rate: u32,
    /// DPI scale factor: 1.0 is 96 DPI, 2.0 is 192 DPI.
    pub scale_factor: f32,
}

// ============================================================================
// Requests
// ============================================================================

/// A control request with the correlation id its reply will carry.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Client-assigned correlation id, echoed in the [`Response`].
    pub seq: u32,
    /// What is being asked for.
    pub body: RequestBody,
}

impl Request {
    /// A request with the given correlation id.
    #[must_use]
    pub const fn new(seq: u32, body: RequestBody) -> Self {
        Self { seq, body }
    }
}

/// The requests a client can make of the compositor.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestBody {
    /// Create a window. Answered with [`ResponseBody::WindowCreated`].
    CreateWindow(WindowSpec),
    /// Destroy a window and release its id.
    DestroyWindow { window: u64 },
    /// Change a window's title.
    SetTitle { window: u64, title: String },
    /// Move a window's top-left corner.
    Move { window: u64, x: i32, y: i32 },
    /// Ask for a different client-area size.
    Resize {
        window: u64,
        width: u32,
        height: u32,
    },
    /// Minimise a window to the taskbar.
    Minimize { window: u64 },
    /// Maximise a window to fill the work area.
    Maximize { window: u64 },
    /// Return a window from minimised or maximised to its previous geometry.
    Restore { window: u64 },
    /// Show or hide a window without destroying it.
    SetVisible { window: u64, visible: bool },
    /// Set the cursor drawn while the pointer is over a window.
    ///
    /// Carries the window, which the shape-only form this replaced did not: a
    /// process with two windows had no way to say which one the text cursor
    /// belonged to.
    SetCursor { window: u64, shape: CursorShape },
    /// Enter or leave fullscreen: the window owns the whole display, with no
    /// decorations, and the compositor may scan it out directly.
    ///
    /// Separate from [`Maximize`](Self::Maximize) because they are different
    /// states with different restore geometry — a maximized window keeps its
    /// title bar and respects panel reservations, a fullscreen one does not —
    /// and a client toggling one must not disturb the other.
    SetFullscreen { window: u64, enable: bool },
    /// Set whole-window opacity, from 0.0 (invisible) to 1.0 (opaque).
    ///
    /// Uniform over the window *including its decorations*, which is what makes
    /// it different from [`WindowSpec::transparent`]: that one says the client
    /// paints its own background and the compositor should not undercoat it.
    SetOpacity { window: u64, opacity: f32 },
    /// Ask about the display. Answered with [`ResponseBody::DisplayInfo`].
    GetDisplayInfo,
    /// Start or stop receiving the desktop's window list.
    ///
    /// A shell — a taskbar, a window switcher, an accessibility tool — needs to
    /// know about windows it did not open, which nothing else in this protocol
    /// will tell it. While subscribed, the compositor sends a
    /// [`WLST`](crate::window_list) frame whenever the list it would send
    /// differs from the one this client last received; see that module for why
    /// it is a push rather than a query.
    ///
    /// Answered with [`ResponseBody::Ok`], and the first list follows
    /// separately rather than riding in the reply: a `WLST` frame is what
    /// arrives on every *later* change, so making the first one arrive by a
    /// different route would give a shell two code paths to the same state.
    ///
    /// Subscribing twice is not an error and does not double the traffic. It
    /// does re-send the list, which is the useful reading of a repeated
    /// subscribe — "I may have lost track, tell me again".
    SubscribeWindowList { subscribe: bool },
    /// Tell the compositor its copy of the user's appearance settings is out
    /// of date, so that it re-reads `appearance.yaml` and redraws.
    ///
    /// **Carries no data, and that is the whole point.** The compositor draws
    /// every window frame on the desktop, so a request that *set* the
    /// appearance would let any process able to open this socket restyle the
    /// entire machine — invisible title-bar text, a close button the same
    /// colour as the bar behind it. A notification cannot do that: the
    /// compositor goes and reads the *user's* file, which the sender may well
    /// have no permission to write. The worst a hostile client achieves is a
    /// redundant re-read and a repaint of a screen that already looks the way
    /// it looks.
    ///
    /// It is also why this is not `SetAppearance(AppearanceSettings)` even
    /// though that would save a file read: the settings are one document with
    /// one owner (`gui/appearance`), and a wire form for them would be a
    /// second copy of that model, free to drift from the crate that defines it.
    ///
    /// Answered with [`ResponseBody::Ok`], including when the file turns out
    /// not to have changed — "I have re-read it" is the truthful answer either
    /// way, and a client asking has no business learning what the user's
    /// settings say from the shape of the reply.
    ReloadAppearance,
    /// Act on a window the sender does not own — the request a taskbar, an
    /// Alt-Tab switcher or a window menu is made of.
    ///
    /// The only request in this protocol that names somebody else's window, and
    /// therefore the only one the compositor does not resolve against the
    /// sender's own. See [`ShellControlAction`] for why the ordinary verbs
    /// could not be reused, and `ClientLink::require_shell` in the compositor
    /// for the still-open question of *who* may send it.
    ///
    /// Answered with [`ResponseBody::Ok`], or an error. A shell acting on a
    /// window that closed a moment ago is an ordinary race rather than a fault
    /// — the click happens after the list snapshot the button was drawn from —
    /// so the error is for the shell's log, not for the user.
    ///
    /// Unlike the owned-window requests, the error text here *does* distinguish
    /// "no such window" from "that window refuses this" (a non-resizable window
    /// cannot be maximised). That would be a way to probe which ids exist, were
    /// the sender not already entitled to the whole window list by the same
    /// privilege that let it send this at all.
    ShellControl {
        window: u64,
        action: ShellControlAction,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum RequestTag {
    CreateWindow = 0x01,
    DestroyWindow = 0x02,
    SetTitle = 0x03,
    Move = 0x04,
    Resize = 0x05,
    Minimize = 0x06,
    Maximize = 0x07,
    Restore = 0x08,
    SetVisible = 0x09,
    SetCursor = 0x0A,
    GetDisplayInfo = 0x0B,
    SetFullscreen = 0x0C,
    SetOpacity = 0x0D,
    SubscribeWindowList = 0x0E,
    ReloadAppearance = 0x0F,
    ShellControl = 0x10,
}

impl RequestTag {
    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::CreateWindow,
            0x02 => Self::DestroyWindow,
            0x03 => Self::SetTitle,
            0x04 => Self::Move,
            0x05 => Self::Resize,
            0x06 => Self::Minimize,
            0x07 => Self::Maximize,
            0x08 => Self::Restore,
            0x09 => Self::SetVisible,
            0x0A => Self::SetCursor,
            0x0B => Self::GetDisplayInfo,
            0x0C => Self::SetFullscreen,
            0x0D => Self::SetOpacity,
            0x0E => Self::SubscribeWindowList,
            0x0F => Self::ReloadAppearance,
            0x10 => Self::ShellControl,
            _ => return None,
        })
    }
}

// ============================================================================
// Responses
// ============================================================================

/// A control response, carrying the `seq` of the request it answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    /// The [`Request::seq`] this answers.
    pub seq: u32,
    /// The answer.
    pub body: ResponseBody,
}

impl Response {
    /// A response to the request with the given correlation id.
    #[must_use]
    pub const fn new(seq: u32, body: ResponseBody) -> Self {
        Self { seq, body }
    }
}

/// The compositor's answers.
#[derive(Clone, Debug, PartialEq)]
pub enum ResponseBody {
    /// A window was created, with the id the *compositor* assigned.
    ///
    /// The id comes from here and nowhere else. It is the compositor's
    /// namespace, and a client that mints its own — as the simulation this
    /// replaced did, from a process-local counter — collides with every other
    /// client on the machine at window number one.
    WindowCreated { window: u64 },
    /// The request succeeded and had nothing to return.
    Ok,
    /// The request failed.
    Error { message: String },
    /// Answer to [`RequestBody::GetDisplayInfo`].
    Display(DisplayInfo),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum ResponseTag {
    WindowCreated = 0x01,
    Ok = 0x02,
    Error = 0x03,
    Display = 0x04,
}

impl ResponseTag {
    const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::WindowCreated,
            0x02 => Self::Ok,
            0x03 => Self::Error,
            0x04 => Self::Display,
            _ => return None,
        })
    }
}

// ============================================================================
// Encoding
// ============================================================================

/// Encode a batch of requests as one `CREQ` frame.
#[must_use]
pub fn encode_requests(requests: &[Request]) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity_hint(CONTROL_HEADER_LEN, requests.len(), 24));
    encode_requests_into(&mut out, requests);
    out
}

/// Encode requests into a caller-provided buffer, appending to what it holds.
pub fn encode_requests_into(out: &mut Vec<u8>, requests: &[Request]) {
    write_header(out, REQUEST_MAGIC, requests.len());
    for req in requests {
        write_u32(out, req.seq);
        encode_request_body(out, &req.body);
    }
}

/// Encode a batch of responses as one `CRSP` frame.
#[must_use]
pub fn encode_responses(responses: &[Response]) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity_hint(CONTROL_HEADER_LEN, responses.len(), 16));
    encode_responses_into(&mut out, responses);
    out
}

/// Encode responses into a caller-provided buffer, appending to what it holds.
pub fn encode_responses_into(out: &mut Vec<u8>, responses: &[Response]) {
    write_header(out, RESPONSE_MAGIC, responses.len());
    for resp in responses {
        write_u32(out, resp.seq);
        encode_response_body(out, &resp.body);
    }
}

fn write_header(out: &mut Vec<u8>, magic: [u8; 4], count: usize) {
    out.extend_from_slice(&magic);
    out.push(CONTROL_VERSION);
    out.push(0); // flags
    // Saturating rather than panicking, as elsewhere in this crate: the decoder
    // rejects anything past MAX_MESSAGES_PER_FRAME regardless, so a caller who
    // assembled four billion messages gets a rejected frame, not a downed
    // compositor.
    write_u32(out, u32::try_from(count).unwrap_or(u32::MAX));
}

fn write_i32(out: &mut Vec<u8>, v: i32) {
    write_u32(out, v.cast_unsigned());
}

fn write_optional_point(out: &mut Vec<u8>, p: Option<(i32, i32)>) {
    match p {
        Some((x, y)) => {
            out.push(1);
            write_i32(out, x);
            write_i32(out, y);
        }
        None => out.push(0),
    }
}

fn write_optional_size(out: &mut Vec<u8>, s: Option<(u32, u32)>) {
    match s {
        Some((w, h)) => {
            out.push(1);
            write_u32(out, w);
            write_u32(out, h);
        }
        None => out.push(0),
    }
}

fn encode_request_body(out: &mut Vec<u8>, body: &RequestBody) {
    match body {
        RequestBody::CreateWindow(spec) => {
            out.push(RequestTag::CreateWindow as u8);
            write_string(out, &spec.title);
            write_u32(out, spec.width);
            write_u32(out, spec.height);
            write_optional_point(out, spec.position);
            out.push(u8::from(spec.resizable));
            out.push(u8::from(spec.decorations));
            out.push(u8::from(spec.transparent));
            write_optional_size(out, spec.min_size);
            write_optional_size(out, spec.max_size);
            out.push(spec.layer.as_byte());
        }
        RequestBody::DestroyWindow { window } => {
            out.push(RequestTag::DestroyWindow as u8);
            write_u64(out, *window);
        }
        RequestBody::SetTitle { window, title } => {
            out.push(RequestTag::SetTitle as u8);
            write_u64(out, *window);
            write_string(out, title);
        }
        RequestBody::Move { window, x, y } => {
            out.push(RequestTag::Move as u8);
            write_u64(out, *window);
            write_i32(out, *x);
            write_i32(out, *y);
        }
        RequestBody::Resize {
            window,
            width,
            height,
        } => {
            out.push(RequestTag::Resize as u8);
            write_u64(out, *window);
            write_u32(out, *width);
            write_u32(out, *height);
        }
        RequestBody::Minimize { window } => {
            out.push(RequestTag::Minimize as u8);
            write_u64(out, *window);
        }
        RequestBody::Maximize { window } => {
            out.push(RequestTag::Maximize as u8);
            write_u64(out, *window);
        }
        RequestBody::Restore { window } => {
            out.push(RequestTag::Restore as u8);
            write_u64(out, *window);
        }
        RequestBody::SetVisible { window, visible } => {
            out.push(RequestTag::SetVisible as u8);
            write_u64(out, *window);
            out.push(u8::from(*visible));
        }
        RequestBody::SetCursor { window, shape } => {
            out.push(RequestTag::SetCursor as u8);
            write_u64(out, *window);
            out.push(shape.to_byte());
        }
        RequestBody::SetFullscreen { window, enable } => {
            out.push(RequestTag::SetFullscreen as u8);
            write_u64(out, *window);
            out.push(u8::from(*enable));
        }
        RequestBody::SetOpacity { window, opacity } => {
            out.push(RequestTag::SetOpacity as u8);
            write_u64(out, *window);
            write_f32(out, *opacity);
        }
        RequestBody::GetDisplayInfo => out.push(RequestTag::GetDisplayInfo as u8),
        RequestBody::SubscribeWindowList { subscribe } => {
            out.push(RequestTag::SubscribeWindowList as u8);
            out.push(u8::from(*subscribe));
        }
        RequestBody::ReloadAppearance => out.push(RequestTag::ReloadAppearance as u8),
        RequestBody::ShellControl { window, action } => {
            out.push(RequestTag::ShellControl as u8);
            write_u64(out, *window);
            out.push(action.as_byte());
        }
    }
}

fn encode_response_body(out: &mut Vec<u8>, body: &ResponseBody) {
    match body {
        ResponseBody::WindowCreated { window } => {
            out.push(ResponseTag::WindowCreated as u8);
            write_u64(out, *window);
        }
        ResponseBody::Ok => out.push(ResponseTag::Ok as u8),
        ResponseBody::Error { message } => {
            out.push(ResponseTag::Error as u8);
            write_string(out, message);
        }
        ResponseBody::Display(info) => {
            out.push(ResponseTag::Display as u8);
            write_u32(out, info.width);
            write_u32(out, info.height);
            write_u32(out, info.refresh_rate);
            write_f32(out, info.scale_factor);
        }
    }
}

// ============================================================================
// Decoding
// ============================================================================

/// Decode exactly one `CREQ` frame. Returns the requests and bytes consumed.
pub fn decode_requests(input: &[u8]) -> Result<(Vec<Request>, usize), DecodeError> {
    let (mut r, n) = read_header(input, REQUEST_MAGIC)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let seq = r.read_u32()?;
        out.push(Request {
            seq,
            body: decode_request_body(&mut r)?,
        });
    }
    Ok((out, r.position()))
}

/// Streaming decode of a `CREQ` frame: `Ok(None)` when the buffer holds only
/// part of one, so a caller reading from a transport can simply read more.
pub fn try_decode_requests(input: &[u8]) -> Result<Option<(Vec<Request>, usize)>, DecodeError> {
    match decode_requests(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Decode exactly one `CRSP` frame. Returns the responses and bytes consumed.
pub fn decode_responses(input: &[u8]) -> Result<(Vec<Response>, usize), DecodeError> {
    let (mut r, n) = read_header(input, RESPONSE_MAGIC)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let seq = r.read_u32()?;
        out.push(Response {
            seq,
            body: decode_response_body(&mut r)?,
        });
    }
    Ok((out, r.position()))
}

/// Streaming decode of a `CRSP` frame: `Ok(None)` on a partial frame.
pub fn try_decode_responses(input: &[u8]) -> Result<Option<(Vec<Response>, usize)>, DecodeError> {
    match decode_responses(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

fn read_header(input: &[u8], magic: [u8; 4]) -> Result<(Reader<'_>, u32), DecodeError> {
    let mut r = Reader::new(input);
    r.need(CONTROL_HEADER_LEN)?;
    r.expect_magic(magic)?;
    let ver = r.read_u8()?;
    if ver != CONTROL_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let n = r.read_u32()?;
    if n > MAX_MESSAGES_PER_FRAME {
        return Err(DecodeError::TooManyMessages(n));
    }
    Ok((r, n))
}

fn read_i32(r: &mut Reader<'_>) -> Result<i32, DecodeError> {
    Ok(r.read_u32()?.cast_signed())
}

fn read_bool(r: &mut Reader<'_>) -> Result<bool, DecodeError> {
    // Any non-zero byte is true rather than an error: this is a boolean, and
    // there is no encoder in this crate that can produce a 2. Rejecting it
    // would trade a harmless byte for a dropped connection.
    Ok(r.read_u8()? != 0)
}

fn read_optional_point(r: &mut Reader<'_>) -> Result<Option<(i32, i32)>, DecodeError> {
    if read_bool(r)? {
        let x = read_i32(r)?;
        let y = read_i32(r)?;
        Ok(Some((x, y)))
    } else {
        Ok(None)
    }
}

fn read_optional_size(r: &mut Reader<'_>) -> Result<Option<(u32, u32)>, DecodeError> {
    if read_bool(r)? {
        let w = r.read_u32()?;
        let h = r.read_u32()?;
        Ok(Some((w, h)))
    } else {
        Ok(None)
    }
}

fn decode_request_body(r: &mut Reader<'_>) -> Result<RequestBody, DecodeError> {
    let tag_byte = r.read_u8()?;
    let tag = RequestTag::from_byte(tag_byte).ok_or(DecodeError::BadTag(tag_byte))?;
    Ok(match tag {
        RequestTag::CreateWindow => {
            let title = r.read_string()?;
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            let position = read_optional_point(r)?;
            let resizable = read_bool(r)?;
            let decorations = read_bool(r)?;
            let transparent = read_bool(r)?;
            let min_size = read_optional_size(r)?;
            let max_size = read_optional_size(r)?;
            let layer_byte = r.read_u8()?;
            let layer = Layer::from_byte(layer_byte).ok_or(DecodeError::BadTag(layer_byte))?;
            RequestBody::CreateWindow(WindowSpec {
                title,
                width,
                height,
                position,
                resizable,
                decorations,
                transparent,
                min_size,
                max_size,
                layer,
            })
        }
        RequestTag::DestroyWindow => RequestBody::DestroyWindow {
            window: r.read_u64()?,
        },
        RequestTag::SetTitle => {
            let window = r.read_u64()?;
            RequestBody::SetTitle {
                window,
                title: r.read_string()?,
            }
        }
        RequestTag::Move => {
            let window = r.read_u64()?;
            let x = read_i32(r)?;
            let y = read_i32(r)?;
            RequestBody::Move { window, x, y }
        }
        RequestTag::Resize => {
            let window = r.read_u64()?;
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            RequestBody::Resize {
                window,
                width,
                height,
            }
        }
        RequestTag::Minimize => RequestBody::Minimize {
            window: r.read_u64()?,
        },
        RequestTag::Maximize => RequestBody::Maximize {
            window: r.read_u64()?,
        },
        RequestTag::Restore => RequestBody::Restore {
            window: r.read_u64()?,
        },
        RequestTag::SetVisible => {
            let window = r.read_u64()?;
            RequestBody::SetVisible {
                window,
                visible: read_bool(r)?,
            }
        }
        RequestTag::SetCursor => {
            let window = r.read_u64()?;
            let b = r.read_u8()?;
            RequestBody::SetCursor {
                window,
                shape: CursorShape::from_byte(b).ok_or(DecodeError::BadCursorShape(b))?,
            }
        }
        RequestTag::SetFullscreen => {
            let window = r.read_u64()?;
            RequestBody::SetFullscreen {
                window,
                enable: read_bool(r)?,
            }
        }
        RequestTag::SetOpacity => {
            let window = r.read_u64()?;
            RequestBody::SetOpacity {
                window,
                opacity: r.read_f32()?,
            }
        }
        RequestTag::GetDisplayInfo => RequestBody::GetDisplayInfo,
        RequestTag::SubscribeWindowList => RequestBody::SubscribeWindowList {
            subscribe: read_bool(r)?,
        },
        RequestTag::ReloadAppearance => RequestBody::ReloadAppearance,
        RequestTag::ShellControl => {
            let window = r.read_u64()?;
            let b = r.read_u8()?;
            RequestBody::ShellControl {
                window,
                action: ShellControlAction::from_byte(b)
                    .ok_or(DecodeError::BadShellAction(b))?,
            }
        }
    })
}

fn decode_response_body(r: &mut Reader<'_>) -> Result<ResponseBody, DecodeError> {
    let tag_byte = r.read_u8()?;
    let tag = ResponseTag::from_byte(tag_byte).ok_or(DecodeError::BadTag(tag_byte))?;
    Ok(match tag {
        ResponseTag::WindowCreated => ResponseBody::WindowCreated {
            window: r.read_u64()?,
        },
        ResponseTag::Ok => ResponseBody::Ok,
        ResponseTag::Error => ResponseBody::Error {
            message: r.read_string()?,
        },
        ResponseTag::Display => {
            let width = r.read_u32()?;
            let height = r.read_u32()?;
            let refresh_rate = r.read_u32()?;
            let scale_factor = r.read_f32()?;
            ResponseBody::Display(DisplayInfo {
                width,
                height,
                refresh_rate,
                scale_factor,
            })
        }
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly at the line that did
    // it. The defensive lints guard code that runs on a user's data, not this.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    fn spec() -> WindowSpec {
        WindowSpec {
            title: "Text Editor — notes.md".to_string(),
            width: 900,
            height: 600,
            position: Some((-40, 17)),
            resizable: true,
            decorations: false,
            transparent: true,
            min_size: Some((320, 240)),
            max_size: Some((3840, 2160)),
            layer: Layer::Overlay,
        }
    }

    fn round_trip_requests(reqs: &[Request]) -> Vec<Request> {
        let bytes = encode_requests(reqs);
        let (out, used) = decode_requests(&bytes).expect("decodes");
        assert_eq!(
            used,
            bytes.len(),
            "the frame must consume exactly its bytes"
        );
        out
    }

    fn round_trip_responses(resps: &[Response]) -> Vec<Response> {
        let bytes = encode_responses(resps);
        let (out, used) = decode_responses(&bytes).expect("decodes");
        assert_eq!(
            used,
            bytes.len(),
            "the frame must consume exactly its bytes"
        );
        out
    }

    #[test]
    fn every_request_survives_the_wire() {
        // Listed exhaustively rather than sampled: a field dropped by an
        // encoder is invisible to a test that never sends that variant.
        let reqs = vec![
            Request::new(1, RequestBody::CreateWindow(spec())),
            Request::new(2, RequestBody::DestroyWindow { window: 7 }),
            Request::new(
                3,
                RequestBody::SetTitle {
                    window: 7,
                    title: "renamed".to_string(),
                },
            ),
            Request::new(
                4,
                RequestBody::Move {
                    window: 7,
                    x: -1920,
                    y: -1080,
                },
            ),
            Request::new(
                5,
                RequestBody::Resize {
                    window: 7,
                    width: 1,
                    height: u32::MAX,
                },
            ),
            Request::new(6, RequestBody::Minimize { window: 7 }),
            Request::new(7, RequestBody::Maximize { window: 7 }),
            Request::new(8, RequestBody::Restore { window: 7 }),
            Request::new(
                9,
                RequestBody::SetVisible {
                    window: 7,
                    visible: false,
                },
            ),
            Request::new(
                10,
                RequestBody::SetCursor {
                    window: 7,
                    shape: CursorShape::Text,
                },
            ),
            Request::new(11, RequestBody::GetDisplayInfo),
            Request::new(
                12,
                RequestBody::SetFullscreen {
                    window: 7,
                    enable: true,
                },
            ),
            Request::new(
                13,
                RequestBody::SetOpacity {
                    window: 7,
                    opacity: 0.25,
                },
            ),
            // Both polarities: a codec that wrote a constant byte for the flag
            // would round-trip one of these and not the other.
            Request::new(14, RequestBody::SubscribeWindowList { subscribe: true }),
            Request::new(15, RequestBody::SubscribeWindowList { subscribe: false }),
            Request::new(16, RequestBody::ReloadAppearance),
        ];
        assert_eq!(round_trip_requests(&reqs), reqs);
    }

    /// Every action, not a sample: the action is one byte and an encoder that
    /// wrote a constant would round-trip whichever one the sample happened to
    /// pick. Listing them also makes adding a sixth action fail here until it
    /// is added to the list, which is the point of an exhaustive test.
    #[test]
    fn every_shell_control_action_survives_the_wire() {
        let actions = [
            ShellControlAction::Activate,
            ShellControlAction::Minimize,
            ShellControlAction::Restore,
            ShellControlAction::Maximize,
            ShellControlAction::Close,
        ];
        let reqs: Vec<Request> = actions
            .iter()
            .enumerate()
            .map(|(i, &action)| {
                Request::new(
                    u32::try_from(i).expect("small"),
                    RequestBody::ShellControl { window: 7, action },
                )
            })
            .collect();
        assert_eq!(round_trip_requests(&reqs), reqs);

        // And the bytes really are distinct, which is what the round trip
        // above is relying on without saying so.
        let mut seen: Vec<u8> = actions.iter().map(|a| a.as_byte()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), actions.len(), "two actions share a wire byte");
    }

    /// An action byte this decoder does not know is refused, not guessed at.
    /// Silently defaulting would let a peer speaking a later protocol have a
    /// window minimized when it asked for something the peer had no word for.
    #[test]
    fn an_unknown_shell_control_action_is_refused() {
        let bytes = encode_requests(&[Request::new(
            1,
            RequestBody::ShellControl {
                window: 7,
                action: ShellControlAction::Close,
            },
        )]);
        // The action byte is the frame's last, after the tag and the u64.
        let mut corrupt = bytes.clone();
        let last = corrupt.len() - 1;
        assert_eq!(corrupt[last], ShellControlAction::Close.as_byte());
        corrupt[last] = 0xFE;
        assert!(matches!(
            decode_requests(&corrupt),
            Err(DecodeError::BadShellAction(0xFE))
        ));
    }

    #[test]
    fn a_reload_request_carries_nothing_a_client_could_restyle_the_desktop_with() {
        // The security argument for `ReloadAppearance` is that it is a
        // notification and not a setter: the compositor re-reads the user's own
        // file rather than being handed a picture of what to draw. That rests
        // entirely on the request having no payload, so it is asserted on the
        // bytes rather than left to the enum's shape — the day someone adds
        // "just a corner radius, to save a file read" this fails and says why.
        let bytes = encode_requests(&[Request::new(1, RequestBody::ReloadAppearance)]);
        assert_eq!(
            bytes.len(),
            CONTROL_HEADER_LEN + 4 + 1,
            "a reload request should be a header, a seq and a tag byte — nothing \
             else; a payload here is a client dictating how the desktop looks"
        );
    }

    #[test]
    fn every_response_survives_the_wire() {
        let resps = vec![
            Response::new(1, ResponseBody::WindowCreated { window: u64::MAX }),
            Response::new(2, ResponseBody::Ok),
            Response::new(
                3,
                ResponseBody::Error {
                    message: "no such window".to_string(),
                },
            ),
            Response::new(
                4,
                ResponseBody::Display(DisplayInfo {
                    width: 3840,
                    height: 2160,
                    refresh_rate: 144,
                    scale_factor: 1.5,
                }),
            ),
        ];
        assert_eq!(round_trip_responses(&resps), resps);
    }

    #[test]
    fn every_cursor_shape_survives_the_wire() {
        // The tree had three of these enums; if the byte mapping is wrong,
        // `Help` arrives as `NotAllowed` and the pointer is silently wrong.
        const ALL: [CursorShape; 13] = [
            CursorShape::Arrow,
            CursorShape::Text,
            CursorShape::Hand,
            CursorShape::ResizeNS,
            CursorShape::ResizeEW,
            CursorShape::ResizeNESW,
            CursorShape::ResizeNWSE,
            CursorShape::Move,
            CursorShape::Wait,
            CursorShape::Help,
            CursorShape::Crosshair,
            CursorShape::NotAllowed,
            CursorShape::Hidden,
        ];
        for shape in ALL {
            let req = Request::new(1, RequestBody::SetCursor { window: 1, shape });
            assert_eq!(round_trip_requests(std::slice::from_ref(&req)), vec![req]);
        }
        // Byte codes must be distinct, or two shapes collapse into one.
        let mut codes: Vec<u8> = ALL.iter().map(|s| s.to_byte()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), ALL.len());
    }

    #[test]
    fn an_unplaced_window_stays_unplaced() {
        // `None` and `Some((0, 0))` must not collapse: one means "put it
        // somewhere sensible", the other means "the top-left corner", and every
        // window that did not care would otherwise pile up in that corner.
        let mut s = spec();
        s.position = None;
        s.min_size = None;
        s.max_size = None;
        let req = Request::new(1, RequestBody::CreateWindow(s));
        let back = round_trip_requests(std::slice::from_ref(&req));
        let RequestBody::CreateWindow(got) = &back[0].body else {
            panic!("wrong variant back")
        };
        assert_eq!(got.position, None);

        let mut s = spec();
        s.position = Some((0, 0));
        let req = Request::new(1, RequestBody::CreateWindow(s));
        let back = round_trip_requests(std::slice::from_ref(&req));
        let RequestBody::CreateWindow(got) = &back[0].body else {
            panic!("wrong variant back")
        };
        assert_eq!(got.position, Some((0, 0)));
    }

    #[test]
    fn a_negative_position_is_not_read_as_a_huge_one() {
        // A window dragged off the left edge has a negative x. Encoding it
        // through u32 and back must land on the same negative number.
        let req = Request::new(
            1,
            RequestBody::Move {
                window: 1,
                x: i32::MIN,
                y: -1,
            },
        );
        let back = round_trip_requests(std::slice::from_ref(&req));
        assert_eq!(back[0].body, req.body);
    }

    #[test]
    fn the_correlation_id_comes_back_unchanged() {
        // The whole point of `seq`: with two requests outstanding, the client
        // must be able to tell which reply is which.
        let resps = vec![
            Response::new(u32::MAX, ResponseBody::Ok),
            Response::new(0, ResponseBody::WindowCreated { window: 3 }),
        ];
        let back = round_trip_responses(&resps);
        assert_eq!(back[0].seq, u32::MAX);
        assert_eq!(back[1].seq, 0);
    }

    #[test]
    fn a_request_frame_is_not_a_response_frame() {
        // The reason for two magics. A duplex channel that crossed its
        // directions must fail on the first four bytes, not decode a `Move`
        // into a plausible-looking `Display`.
        let reqs = encode_requests(&[Request::new(1, RequestBody::GetDisplayInfo)]);
        assert_eq!(decode_responses(&reqs), Err(DecodeError::BadMagic));

        let resps = encode_responses(&[Response::new(1, ResponseBody::Ok)]);
        assert_eq!(decode_requests(&resps), Err(DecodeError::BadMagic));
    }

    #[test]
    fn a_control_frame_is_not_an_input_or_render_frame() {
        let reqs = encode_requests(&[Request::new(1, RequestBody::GetDisplayInfo)]);
        assert_eq!(
            crate::decode_input_frame(&reqs),
            Err(DecodeError::BadMagic),
            "an input decoder must reject a control frame"
        );
        // `RenderTree` is not `PartialEq`, so this one is matched rather than
        // compared.
        assert!(
            matches!(crate::decode_frame(&reqs), Err(DecodeError::BadMagic)),
            "a render decoder must reject a control frame"
        );
    }

    #[test]
    fn several_frames_arrive_back_to_back() {
        let mut buf = encode_requests(&[Request::new(1, RequestBody::GetDisplayInfo)]);
        let first_len = buf.len();
        encode_requests_into(
            &mut buf,
            &[Request::new(2, RequestBody::Minimize { window: 4 })],
        );

        let (a, used) = decode_requests(&buf).unwrap();
        assert_eq!(used, first_len);
        assert_eq!(a[0].seq, 1);
        let (b, used_b) = decode_requests(&buf[used..]).unwrap();
        assert_eq!(used + used_b, buf.len());
        assert_eq!(b[0].seq, 2);
    }

    #[test]
    fn an_empty_frame_is_legal() {
        assert_eq!(round_trip_requests(&[]), vec![]);
        assert_eq!(round_trip_responses(&[]), vec![]);
    }

    #[test]
    fn every_truncation_reads_as_incomplete_not_corrupt() {
        // A transport delivers a frame in pieces. Every prefix must be "read
        // more", never an error the caller would drop the connection over.
        let full = encode_requests(&[
            Request::new(1, RequestBody::CreateWindow(spec())),
            Request::new(
                2,
                RequestBody::SetCursor {
                    window: 9,
                    shape: CursorShape::Wait,
                },
            ),
        ]);
        for n in 0..full.len() {
            assert_eq!(
                try_decode_requests(&full[..n]),
                Ok(None),
                "prefix of {n} bytes must read as incomplete"
            );
        }
        assert!(try_decode_requests(&full).unwrap().is_some());
    }

    #[test]
    fn a_bad_version_is_rejected_rather_than_guessed() {
        let mut bytes = encode_requests(&[Request::new(1, RequestBody::GetDisplayInfo)]);
        bytes[4] = CONTROL_VERSION.wrapping_add(1);
        assert_eq!(
            decode_requests(&bytes),
            Err(DecodeError::UnsupportedVersion(CONTROL_VERSION + 1))
        );
    }

    #[test]
    fn reserved_flag_bits_are_rejected() {
        // They are reserved so a later version can mean something by them; a
        // decoder that ignored them would silently mis-read that version.
        let mut bytes = encode_requests(&[Request::new(1, RequestBody::GetDisplayInfo)]);
        bytes[5] = 0x80;
        assert_eq!(
            decode_requests(&bytes),
            Err(DecodeError::ReservedFlags(0x80))
        );
    }

    #[test]
    fn an_unknown_tag_is_rejected() {
        let mut bytes = encode_requests(&[Request::new(1, RequestBody::GetDisplayInfo)]);
        let tag_at = CONTROL_HEADER_LEN + 4;
        bytes[tag_at] = 0xEE;
        assert_eq!(decode_requests(&bytes), Err(DecodeError::BadTag(0xEE)));

        let mut bytes = encode_responses(&[Response::new(1, ResponseBody::Ok)]);
        bytes[tag_at] = 0xEE;
        assert_eq!(decode_responses(&bytes), Err(DecodeError::BadTag(0xEE)));
    }

    #[test]
    fn an_unknown_cursor_shape_is_rejected() {
        let mut bytes = encode_requests(&[Request::new(
            1,
            RequestBody::SetCursor {
                window: 1,
                shape: CursorShape::Arrow,
            },
        )]);
        let last = bytes.len() - 1;
        bytes[last] = 0x7F;
        assert_eq!(
            decode_requests(&bytes),
            Err(DecodeError::BadCursorShape(0x7F))
        );
    }

    #[test]
    fn an_absurd_message_count_is_rejected_before_allocating() {
        // Without the bound, this line is where a hostile sender aims: four
        // billion messages declared, four billion slots reserved.
        let mut bytes = encode_requests(&[]);
        bytes[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode_requests(&bytes),
            Err(DecodeError::TooManyMessages(u32::MAX))
        );
    }

    #[test]
    fn no_damaged_byte_of_a_control_frame_ever_panics() {
        // The decoder runs on bytes from another process. Every one of them
        // being wrong must be an error, and never a downed client.
        let full = encode_requests(&[Request::new(3, RequestBody::CreateWindow(spec()))]);
        for i in 0..full.len() {
            for bit in 0..8u32 {
                let mut damaged = full.clone();
                damaged[i] ^= 1u8 << bit;
                let _ = decode_requests(&damaged);
                let _ = decode_responses(&damaged);
            }
        }

        let full = encode_responses(&[Response::new(
            3,
            ResponseBody::Display(DisplayInfo {
                width: 800,
                height: 600,
                refresh_rate: 60,
                scale_factor: 1.0,
            }),
        )]);
        for i in 0..full.len() {
            for bit in 0..8u32 {
                let mut damaged = full.clone();
                damaged[i] ^= 1u8 << bit;
                let _ = decode_responses(&damaged);
                let _ = decode_requests(&damaged);
            }
        }
    }

    #[test]
    fn the_default_spec_is_the_ordinary_window() {
        let s = WindowSpec::new("Untitled", 640, 480);
        assert!(
            s.resizable,
            "a window a user cannot resize is the exception"
        );
        assert!(s.decorations);
        assert!(!s.transparent);
        assert_eq!(s.position, None, "placement is the compositor's job");
    }
}
