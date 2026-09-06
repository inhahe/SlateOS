//! The compositor's side of the `guiremote` wire protocol.
//!
//! Everything needed for an application to talk to the compositor existed
//! before this module and none of it was joined up. `guiremote` could encode a
//! `CREQ` control request and decode a `CRSP` reply; `oswindow` could drive
//! that from an event loop; the compositor could hit-test, track focus, build
//! correctly addressed per-window events and hand back an `INPT` frame. What
//! was missing was the piece in the middle: nothing turned a byte that arrived
//! from a client into a [`CompositorRequest`], and nothing turned the answer
//! back into bytes. See `known-issues.md` →
//! `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`.
//!
//! ## What a client sends, and what it gets back
//!
//! One connection carries everything, which is the Wayland model and was argued
//! for in `design-decisions.md` §457: a transport per window would need the
//! compositor to hand a fresh channel back inside a reply, and this protocol has
//! no capability passing to do that with. So a client's stream interleaves two
//! frame kinds and the compositor's stream interleaves two others:
//!
//! | Direction | Frame | Carries |
//! |---|---|---|
//! | client → compositor | `CREQ` | window control requests, each with a `seq` |
//! | client → compositor | `SURF` | a picture, addressed to one of its windows |
//! | compositor → client | `CRSP` | one reply per request, echoing its `seq` |
//! | compositor → client | `INPT` | input events, addressed to a window |
//!
//! [`decode_any`](guiremote::frame::decode_any) reads the leading magic and
//! picks the decoder, so the two inbound kinds need no framing of their own on
//! top.
//!
//! ## Why a link owns the windows opened over it
//!
//! [`ClientLink`] records the id of every window created on it. That set is
//! what routes input: a notification is written to the link that owns its
//! window and to no other. Routing by `client_pid` instead would look
//! equivalent and is not — one process may hold two connections, and two
//! processes may share a pid namespace — and getting it wrong means sending one
//! application's keystrokes to another, which is a password-shaped bug rather
//! than a cosmetic one.
//!
//! The same set is what makes a request safe to honour. A window id on the wire
//! is a number a client chose to send; nothing about it proves the sender owns
//! the window it names. Every request that carries one is checked against the
//! link's own set before it reaches the compositor, so a client cannot close,
//! move, or draw into a window belonging to someone else.

use guiremote::DecodeError;
use guiremote::control::{
    DisplayInfo, Request, RequestBody, Response, ResponseBody, encode_responses_into,
};
use guiremote::frame::{Frame, try_decode_any};
use guiremote::submit::Submission;
use guiremote::window_list::encode_window_list_into;
use guitk::render::RenderTree;

use crate::{Compositor, CompositorRequest, CompositorResponse, WindowId};

/// One client's connection to the compositor, as seen from the compositor.
///
/// Holds the two half-buffers and the set of windows opened over it. It does
/// not own a transport: the compositor is not the thing that decides how bytes
/// move (a socket, a channel, a test's `Vec`), and coupling it to one would
/// make it untestable without that one.
#[derive(Clone, Debug)]
pub struct ClientLink {
    /// The owning process, recorded on each window for the taskbar and for
    /// process-level policy. Not used for routing — see the module docs.
    client_pid: u64,
    /// Bytes received from the client and not yet decoded. A frame may arrive
    /// in pieces, so what is left over after a decode pass stays here.
    inbox: Vec<u8>,
    /// Bytes to be written back to the client: replies and input frames, in
    /// the order they were produced.
    outbox: Vec<u8>,
    /// Windows opened over this link, in creation order.
    ///
    /// A `Vec` rather than a set: a client with more than a handful of windows
    /// is already unusual, and a linear scan of three ids beats hashing one.
    windows: Vec<WindowId>,
    /// Whether the client has hung up. A closed link is drained but not read
    /// from again.
    closed: bool,
    /// Whether this client asked to be told about the whole desktop's windows.
    ///
    /// False for every ordinary application, which is what makes the whole
    /// mechanism free when nobody wants it: an unsubscribed link never builds a
    /// list at all.
    wants_window_list: bool,
    /// The exact bytes of the last window-list frame written to this link.
    ///
    /// Compared against a freshly built list to decide whether to send, rather
    /// than a change-counter bumped by every operation that touches a window.
    /// A counter has to be bumped at every such site, and the failure mode of
    /// missing one is a taskbar that is silently wrong until something else
    /// happens to change — the "counting instead of searching" trap that
    /// `design-decisions.md` §494 flagged in the stacking order. Comparing the
    /// output cannot be forgotten, because there is nowhere to forget it.
    ///
    /// Empty until the first list is sent, which is why an empty desktop still
    /// produces a frame: `[]` never equals the encoding of an empty list, which
    /// carries a header.
    window_list_sent: Vec<u8>,
    /// Most bytes of uploaded image pixels this connection may hold at once.
    ///
    /// Per link rather than a bare constant because the right number depends on
    /// the machine and on what the connection is: a desktop with 64 GiB can
    /// afford more than a thin client, and a server that knows one link is its
    /// own trusted shell may reasonably give it more than an application it
    /// just accepted a socket from. Defaults to
    /// [`MAX_IMAGE_BYTES_PER_LINK`], so a server that has no opinion gets one.
    ///
    /// It is the *server's* to set and not the client's: nothing on the wire
    /// can change it, which is what keeps it a limit rather than a suggestion.
    image_budget: u64,
}

/// Default ceiling on the uploaded image pixels one connection may keep
/// resident. A server may raise or lower it per link with
/// [`ClientLink::set_image_budget`].
///
/// ## Why there is a limit at all
///
/// [`RequestBody::UploadImage`](guiremote::control::RequestBody::UploadImage) is
/// the only request on this wire whose cost to the compositor is chosen by the
/// sender. `MAX_IMAGE_BYTES` bounds *one* upload; nothing bounds their number,
/// so without this a client with one window could hand over a 126 MiB picture
/// under a fresh id in a loop until the machine is out of memory — and the
/// compositor is the process every other program's windows depend on, so it is
/// the worst process on the desktop to have killed by the allocator.
///
/// ## Why per link, and not per window
///
/// A per-window budget is bypassed by opening a second window. The connection is
/// already the unit of accountability everywhere else here — `design-decisions.md`
/// §458 makes a link own the windows opened over it — so it is the unit a
/// resource limit has to use as well.
///
/// ## Why refuse rather than evict
///
/// The alternative is to drop the connection's least-recently-drawn asset and
/// accept the new one. That is worse *because* it succeeds: a draw command
/// naming an id with no pixels behind it renders nothing, silently and by
/// design, so an evicted thumbnail is a picture that stops appearing with no
/// error anywhere and no way for the client to learn it happened. Refusing
/// returns [`ResponseBody::Error`] to the request that went over, which the
/// client can log, retry smaller, or answer by dropping something itself. It is
/// also what `design.txt` asks for in general — committed memory, no silent
/// overcommit — and it costs no per-frame bookkeeping, where eviction would need
/// a last-drawn timestamp maintained on the compositing hot path.
///
/// ## The number
///
/// 256 MiB is two full-screen 4K pictures at four bytes a pixel with room to
/// spare, which is an image viewer showing one and pre-decoding the next, or a
/// file manager with a very large folder of thumbnails. It is not a measurement
/// — no application yet uploads anything — and it is deliberately generous,
/// because the failure it exists to prevent is unbounded growth rather than
/// large-but-bounded use.
pub const MAX_IMAGE_BYTES_PER_LINK: u64 = 256 * 1024 * 1024;

/// What went wrong serving a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The client's stream could not be decoded. The connection is not
    /// recoverable: a stream that has lost frame sync cannot be resynchronised
    /// by skipping, because there is no way to know how far to skip.
    Malformed(DecodeError),
    /// The client sent a frame only the compositor is supposed to send.
    ///
    /// `SCEN`, `ORDR` and `WLST` travel the other way — a scene and a window
    /// list are the compositor's description of the desktop, and a bare `ORDR`
    /// has no addressee, which is why `SURF` exists. Receiving one means the
    /// peer is confused about which end it is, and treating it as a no-op would
    /// hide that.
    WrongDirection(&'static str),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "malformed client stream: {e}"),
            Self::WrongDirection(kind) => {
                write!(
                    f,
                    "client sent a {kind} frame, which only a compositor sends"
                )
            }
        }
    }
}

impl std::error::Error for WireError {}

impl From<DecodeError> for WireError {
    fn from(e: DecodeError) -> Self {
        Self::Malformed(e)
    }
}

impl ClientLink {
    /// A fresh link for a client process.
    #[must_use]
    pub const fn new(client_pid: u64) -> Self {
        Self {
            client_pid,
            inbox: Vec::new(),
            outbox: Vec::new(),
            windows: Vec::new(),
            closed: false,
            wants_window_list: false,
            window_list_sent: Vec::new(),
            image_budget: MAX_IMAGE_BYTES_PER_LINK,
        }
    }

    /// Most bytes of uploaded image pixels this connection may hold at once.
    #[must_use]
    pub const fn image_budget(&self) -> u64 {
        self.image_budget
    }

    /// Set this connection's image budget.
    ///
    /// Lowering it below what the link already holds does not evict anything —
    /// see [`MAX_IMAGE_BYTES_PER_LINK`] for why nothing here evicts — it only
    /// means the next upload is refused until the client frees enough itself.
    pub const fn set_image_budget(&mut self, bytes: u64) {
        self.image_budget = bytes;
    }

    /// Whether this client is receiving the desktop's window list.
    #[must_use]
    pub const fn wants_window_list(&self) -> bool {
        self.wants_window_list
    }

    /// Start or stop sending this client the desktop's window list.
    ///
    /// Subscribing always forgets what was last sent, so a re-subscribe
    /// re-sends the list rather than being a no-op. That is the useful reading
    /// of a repeated subscribe — "I may have lost track" — and it is what makes
    /// the first list after subscribing arrive at all.
    ///
    /// Unsubscribing also forgets it, so that a later re-subscribe cannot be
    /// answered with silence because the list happens not to have changed while
    /// the client was not listening.
    pub fn set_window_list_subscription(&mut self, on: bool) {
        self.wants_window_list = on;
        self.window_list_sent.clear();
    }

    /// The process on the other end.
    #[must_use]
    pub const fn client_pid(&self) -> u64 {
        self.client_pid
    }

    /// Append bytes read from the client's transport.
    ///
    /// Any number of bytes is fine, including a fraction of a frame: what
    /// cannot be decoded yet is kept and reconsidered on the next call.
    pub fn receive(&mut self, bytes: &[u8]) {
        self.inbox.extend_from_slice(bytes);
    }

    /// Take everything queued for the client, leaving the outbox empty.
    ///
    /// Returns owned bytes rather than a slice so the caller can hand them to a
    /// transport without holding a borrow on the link across the write, which
    /// would stop the compositor being served in the meantime.
    #[must_use]
    pub fn take_outgoing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbox)
    }

    /// Whether anything is waiting to be written.
    #[must_use]
    pub fn has_outgoing(&self) -> bool {
        !self.outbox.is_empty()
    }

    /// The windows opened over this link.
    #[must_use]
    pub fn windows(&self) -> &[WindowId] {
        &self.windows
    }

    /// Whether this link opened the named window.
    #[must_use]
    pub fn owns(&self, window: WindowId) -> bool {
        self.windows.contains(&window)
    }

    /// Note that the client has hung up.
    pub const fn close(&mut self) {
        self.closed = true;
    }

    /// Whether the client has hung up.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// How many bytes are buffered awaiting a complete frame.
    ///
    /// Exposed so a server can notice a client that sends an enormous partial
    /// frame and never finishes it, which is otherwise an unbounded allocation
    /// driven entirely by the peer.
    #[must_use]
    pub fn pending_input(&self) -> usize {
        self.inbox.len()
    }

    /// Resolve a window id a client sent, refusing one it does not own.
    ///
    /// `Err` carries the reply to send: naming someone else's window and naming
    /// a window that no longer exists are answered identically and on purpose.
    /// A distinguishable "that exists but is not yours" would tell any client
    /// how many windows the rest of the desktop has open, by probing.
    fn resolve(&self, window: u64) -> Result<WindowId, ResponseBody> {
        let id = WindowId::from_raw(window);
        if self.owns(id) {
            Ok(id)
        } else {
            Err(ResponseBody::Error {
                message: format!("no such window: {window}"),
            })
        }
    }

    /// The single place the compositor asks "is this connection a shell?".
    ///
    /// **It does not currently check anything, and saying so is the point.**
    /// A handful of requests are a shell's and not an application's — reading
    /// the whole desktop's window list, acting on windows the sender does not
    /// own, reserving a panel edge, showing a different virtual desktop and
    /// filing a window on one — and the honest gate for them does not exist
    /// yet: the answer has to come
    /// from a capability the kernel attests at connection accept, and kernel
    /// channel IPC does not yet carry one to the compositor. A check written
    /// against a value the *client* supplies would not be a gate but the
    /// appearance of one, which is worse than none because it looks solved.
    /// `design-decisions.md` §495 has the full reasoning; the consequence is
    /// tracked as `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`.
    ///
    /// What it buys today is that the privileged requests are named, greppable
    /// and routed through **one** function, so the day the capability arrives
    /// the fix is a body here rather than a hunt for every place that should
    /// have asked. That was already the shape of the recorded proper fix when
    /// there was one such request, and every one added since has gone through
    /// here rather than growing a check of its own.
    ///
    /// Returns the refusal to send, so a caller writes `link.require_shell()?`
    /// exactly as it writes `link.resolve(window)?`.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the signature is the seam; the day it checks, it returns Err"
    )]
    fn require_shell(&self) -> Result<(), ResponseBody> {
        Ok(())
    }
}

/// Translate a control request into the compositor's own vocabulary.
///
/// `Err` is a reply to send instead of dispatching: a request naming a window
/// the sender does not own never reaches the compositor at all.
fn to_compositor_request(
    link: &ClientLink,
    body: RequestBody,
) -> Result<CompositorRequest, ResponseBody> {
    Ok(match body {
        RequestBody::CreateWindow(spec) => CompositorRequest::CreateWindow {
            spec,
            // From the link, never from the frame: a client that could state
            // its own pid could claim another process's windows in the taskbar.
            client_pid: link.client_pid,
        },
        RequestBody::DestroyWindow { window } => CompositorRequest::DestroyWindow {
            window_id: link.resolve(window)?,
        },
        RequestBody::SetTitle { window, title } => CompositorRequest::SetTitle {
            window_id: link.resolve(window)?,
            title,
        },
        RequestBody::Move { window, x, y } => CompositorRequest::Move {
            window_id: link.resolve(window)?,
            x,
            y,
        },
        RequestBody::Resize {
            window,
            width,
            height,
        } => CompositorRequest::Resize {
            window_id: link.resolve(window)?,
            width,
            height,
        },
        RequestBody::Minimize { window } => CompositorRequest::Minimize {
            window_id: link.resolve(window)?,
        },
        RequestBody::Maximize { window } => CompositorRequest::Maximize {
            window_id: link.resolve(window)?,
        },
        RequestBody::Restore { window } => CompositorRequest::Restore {
            window_id: link.resolve(window)?,
        },
        RequestBody::SetVisible { window, visible } => CompositorRequest::SetVisible {
            window_id: link.resolve(window)?,
            visible,
        },
        RequestBody::SetCursor { window, shape } => CompositorRequest::SetCursor {
            window_id: link.resolve(window)?,
            cursor: shape,
        },
        RequestBody::SetFullscreen { window, enable } => CompositorRequest::SetFullscreen {
            window_id: link.resolve(window)?,
            enable,
        },
        RequestBody::SetOpacity { window, opacity } => CompositorRequest::SetOpacity {
            window_id: link.resolve(window)?,
            opacity,
        },
        RequestBody::GetDisplayInfo => CompositorRequest::GetDisplayInfo,
        // Unlike every window request above there is no `link.resolve` on
        // either of these, and nothing to resolve: a reload names no window and
        // carries no settings, so there is no ownership question to ask. They
        // are the two requests whose whole safety argument is what they do
        // *not* contain.
        RequestBody::ReloadAppearance => CompositorRequest::ReloadAppearance,
        RequestBody::ReloadInput => CompositorRequest::ReloadInput,
        // Handled by `answer_requests` before it reaches here, because it
        // changes the *link*, not the compositor: nothing about a subscription
        // belongs in the window/display state a `CompositorRequest` describes,
        // and giving it a variant would mean adding one the compositor cannot
        // serve without being handed the connection that asked. Reaching this
        // arm is a bug in that dispatch, so it says so rather than answering.
        RequestBody::SubscribeWindowList { .. } => {
            return Err(ResponseBody::Error {
                message: "window-list subscription is a link-level request".to_string(),
            });
        }
        // The one window request that is deliberately *not* resolved against
        // the sender's own windows: a taskbar button exists to act on somebody
        // else's window, so `resolve` would refuse every legitimate use. What
        // stands in its place is `require_shell`, which asks a different
        // question — not "is this yours" but "are you the shell" — and is the
        // only thing between any connected program and every window on the
        // desktop. See its doc for why it does not yet answer.
        RequestBody::ShellControl { window, action } => {
            link.require_shell()?;
            CompositorRequest::ShellControl {
                window_id: WindowId::from_raw(window),
                action,
            }
        }
        // The only request that is **both** resolved against the sender's own
        // windows *and* privileged, and it needs both for different reasons.
        // `resolve` because the window named is the panel's own and a client
        // reserving out of somebody else's window would be nonsense — the
        // monitor and the lifetime are read off it. `require_shell` because the
        // *effect* lands on every other client: an unprivileged program could
        // take a third of each edge and leave the whole desktop unable to tile.
        // Ownership is checked first, so a program that is not a shell learns
        // nothing about which window ids exist that it did not already know.
        RequestBody::ReserveEdge { window, edge, size } => {
            let window_id = link.resolve(window)?;
            link.require_shell()?;
            CompositorRequest::ReserveEdge {
                window_id,
                edge,
                size,
            }
        }
        // Privileged and unresolved, for the two halves of the same reason as
        // `ShellControl`: a switch names no window at all, and filing a window
        // on a desktop is a thing done to somebody else's window by definition
        // -- a client may not choose which desktop its own window opens on.
        RequestBody::SwitchWorkspace { workspace } => {
            link.require_shell()?;
            CompositorRequest::SwitchWorkspace { workspace }
        }
        // Resolved *and* privileged, the same pair as `ReserveEdge` and for the
        // same shape of reason. `resolve` because the window named is the
        // sender's own -- it is where the keystroke will be delivered and it is
        // the grab's lifetime. `require_shell` because the effect lands on
        // everyone else: a chord one client holds is a chord no other client can
        // ever see, and a program able to claim Super+L could show a convincing
        // fake lock screen and collect the password. Ownership first, so a
        // program that is not a shell learns nothing about which window ids
        // exist that it did not already know.
        RequestBody::GrabKey {
            window,
            key,
            modifiers,
        } => {
            let window_id = link.resolve(window)?;
            link.require_shell()?;
            CompositorRequest::GrabKey {
                window_id,
                key,
                modifiers,
            }
        }
        RequestBody::UngrabKey {
            window,
            key,
            modifiers,
        } => {
            let window_id = link.resolve(window)?;
            link.require_shell()?;
            CompositorRequest::UngrabKey {
                window_id,
                key,
                modifiers,
            }
        }
        RequestBody::SetWindowWorkspace { window, workspace } => {
            link.require_shell()?;
            CompositorRequest::SetWindowWorkspace {
                window_id: WindowId::from_raw(window),
                workspace,
            }
        }
        // Resolved like any other window request, and *not* privileged: an
        // application uploading a thumbnail into its own window is the ordinary
        // case. What makes this one different is that it is the only request on
        // this wire whose cost to the compositor is chosen by the sender, which
        // is why `answer_requests` puts a second gate in front of it — see
        // `image_budget_refusal`. Nothing about that gate can live here: it
        // needs to know how many bytes this link already holds, and that is a
        // question only the compositor can answer.
        RequestBody::UploadImage {
            window,
            image_id,
            width,
            height,
            stride,
            format,
            bytes,
        } => CompositorRequest::RegisterImage {
            window_id: link.resolve(window)?,
            image_id,
            width,
            height,
            stride,
            format,
            bytes,
        },
        RequestBody::DropImage { window, image_id } => CompositorRequest::UnregisterImage {
            window_id: link.resolve(window)?,
            image_id,
        },
    })
}

/// Translate the compositor's answer into the reply body a client receives.
///
/// The stream responses have no wire form: `StreamStart`/`StreamCapture`/
/// `StreamStop` are how a *remote viewer* subscribes to the whole desktop, not
/// something an application asks about its own window, and there is no
/// `RequestBody` that can produce them. They are mapped to an error rather than
/// given a tag, so that the day one does become reachable the failure is a
/// message a developer can read instead of a silent `Ok`.
fn to_response_body(response: CompositorResponse) -> ResponseBody {
    match response {
        CompositorResponse::WindowCreated { window_id } => ResponseBody::WindowCreated {
            window: window_id.raw(),
        },
        CompositorResponse::Ok => ResponseBody::Ok,
        CompositorResponse::Error { message } => ResponseBody::Error { message },
        CompositorResponse::DisplayInfo {
            width,
            height,
            refresh_rate,
            scale_factor,
        } => ResponseBody::Display(DisplayInfo {
            width,
            height,
            refresh_rate,
            scale_factor,
        }),
        CompositorResponse::WorkArea {
            x,
            y,
            width,
            height,
        } => ResponseBody::WorkArea {
            x,
            y,
            width,
            height,
        },
        CompositorResponse::StreamStarted { .. } | CompositorResponse::StreamFrame { .. } => {
            ResponseBody::Error {
                message: "stream responses have no client-facing wire form".to_string(),
            }
        }
    }
}

impl Compositor {
    /// Decode and act on everything a client has sent, queueing the replies.
    ///
    /// Returns how many requests were answered, so a caller can tell a link
    /// that did something from one that only delivered a partial frame.
    ///
    /// # Errors
    ///
    /// [`WireError::Malformed`] if the client's stream cannot be decoded, and
    /// [`WireError::WrongDirection`] if it sent a frame only the compositor
    /// sends. Both are terminal for the connection: there is no way to
    /// resynchronise a stream whose framing is wrong, because the length that
    /// would say how far to skip is itself part of what is not trusted.
    pub fn serve(&mut self, link: &mut ClientLink) -> Result<usize, WireError> {
        // A client that has hung up cannot be replied to, so acting on what it
        // sent before hanging up would create windows nobody can drive and
        // queue replies nobody will read. The bytes stay in the inbox for a
        // server that wants to look at them; they are not obeyed.
        if link.closed {
            return Ok(0);
        }

        let mut served = 0usize;
        let mut consumed = 0usize;

        // `consumed` is a running sum of lengths the decoder reported for
        // frames it found *in this buffer*, so the slice always exists; the
        // default is an unreachable branch written as an empty stream rather
        // than a panic, because a front end that faces untrusted bytes should
        // not contain the word `expect` at all.
        while let Some((frame, used)) =
            try_decode_any(link.inbox.get(consumed..).unwrap_or_default())?
        {
            // Saturating rather than plain `+`: both counters are bounded by the
            // inbox this loop is walking, so neither can really overflow, but
            // the bound is an argument about a decoder that reads bytes a
            // *client* chose, and a front end facing untrusted input should not
            // rest on one. Saturating misreports a count that cannot occur;
            // wrapping would misreport one as zero and re-serve the buffer.
            consumed = consumed.saturating_add(used);

            match frame {
                Frame::Requests(requests) => {
                    served = served.saturating_add(requests.len());
                    self.answer_requests(link, &requests);
                }
                Frame::Submit(submission) => {
                    served = served.saturating_add(1);
                    self.accept_submission(link, submission);
                }
                // These four travel outwards. A client sending one is confused
                // about which end of the connection it is on, and pretending
                // otherwise would leave that confusion to be discovered later,
                // as a window that never draws.
                Frame::Responses(_) => return Err(WireError::WrongDirection("control response")),
                Frame::Scene(_) => return Err(WireError::WrongDirection("scene")),
                Frame::Render(_) => return Err(WireError::WrongDirection("unaddressed render")),
                Frame::Input(_) => return Err(WireError::WrongDirection("input")),
                Frame::WindowList(_) => return Err(WireError::WrongDirection("window list")),
            }
        }

        // Drop what was decoded, keeping any partial frame at the tail. Done
        // once at the end rather than per frame so a burst of small frames is
        // one memmove rather than one per frame.
        if consumed > 0 {
            link.inbox.drain(..consumed);
        }
        Ok(served)
    }

    /// Dispatch a batch of control requests and append the replies.
    fn answer_requests(&mut self, link: &mut ClientLink, requests: &[Request]) {
        let mut replies = Vec::with_capacity(requests.len());
        for req in requests {
            // Subscription is the one request whose subject is the connection
            // rather than a window, so it is answered here and never converted.
            // It goes through the same privilege seam as `ShellControl`: the
            // desktop's window list is a shell's to read, and asking in one
            // place is what keeps the eventual capability check to one edit.
            if let RequestBody::SubscribeWindowList { subscribe } = req.body {
                let body = match link.require_shell() {
                    Ok(()) => {
                        link.set_window_list_subscription(subscribe);
                        ResponseBody::Ok
                    }
                    Err(refusal) => refusal,
                };
                replies.push(Response::new(req.seq, body));
                continue;
            }
            let body = match to_compositor_request(link, req.body.clone()) {
                // Ownership was settled by the conversion; what is left is the
                // second gate, and only one request has one. An upload is the
                // only thing on this wire whose cost to the compositor the
                // *sender* chooses, so it is weighed against what this link
                // already holds before it is dispatched — and refused whole, so
                // that a refusal cannot leave a half-replaced image behind.
                Ok(request) => match self.image_budget_refusal(link, &request) {
                    Some(refusal) => refusal,
                    None => {
                        // Destruction is noted before dispatch and creation
                        // after, because both read the link's window set and the
                        // id of a new window does not exist until the compositor
                        // answers.
                        let destroying = match request {
                            CompositorRequest::DestroyWindow { window_id } => Some(window_id),
                            _ => None,
                        };
                        let body = to_response_body(self.handle_request(request));
                        if let Some(id) = destroying {
                            link.windows.retain(|&w| w != id);
                        }
                        if let ResponseBody::WindowCreated { window } = body {
                            link.windows.push(WindowId::from_raw(window));
                        }
                        body
                    }
                },
                Err(refusal) => refusal,
            };
            replies.push(Response::new(req.seq, body));
        }
        encode_responses_into(&mut link.outbox, &replies);
    }

    /// Weigh an image upload against what its connection already holds, and
    /// return the refusal to send if it does not fit.
    ///
    /// `None` for every other request, so the caller can put one call in front
    /// of the whole dispatch rather than a special case beside it.
    ///
    /// ## The arithmetic, and the subtlety in it
    ///
    /// The budget is measured against the total this link would hold **after**
    /// the upload, not before it plus the upload. Those differ whenever the id
    /// already exists, because re-registering replaces: overwriting a 40 MiB
    /// asset with a 41 MiB one costs one megabyte. Adding without subtracting
    /// would refuse every in-place update — a video frame, a re-rendered chart —
    /// from the moment a client passed half its allowance, which is the point at
    /// which such a client is doing exactly what it is supposed to.
    ///
    /// The size weighed is the *resident* size, `width * height * 4`, not the
    /// number of bytes on the wire. They differ when a client uploads a
    /// sub-rectangle of a larger picture with the original's stride: the wire
    /// carries the padding and the compositor does not keep it. The budget is a
    /// limit on what the compositor holds, so it counts what the compositor
    /// holds. Four bytes because that is what an [`ImageAsset`](crate::ImageAsset)
    /// stores per pixel whatever format it was handed, not
    /// `format.bytes_per_pixel()`, which describes the sender's layout.
    ///
    /// Nothing here validates the geometry: absurd dimensions either fail this
    /// check by being enormous or fail `ImageAsset::import` a moment later. A
    /// second copy of the stride and coverage rules would be a second place for
    /// them to be wrong, and only one of the two is the one a hostile client has
    /// to get past.
    fn image_budget_refusal(
        &self,
        link: &ClientLink,
        request: &CompositorRequest,
    ) -> Option<ResponseBody> {
        let CompositorRequest::RegisterImage {
            window_id,
            image_id,
            width,
            height,
            ..
        } = *request
        else {
            return None;
        };

        // `u64` throughout, and saturating: every term is a number a client
        // chose, and a budget check that overflowed to a small total would admit
        // exactly the upload it exists to refuse.
        let incoming = u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4);
        let held = link
            .windows
            .iter()
            .filter_map(|&w| self.window_image_bytes(w))
            .map(|n| u64::try_from(n).unwrap_or(u64::MAX))
            .fold(0u64, u64::saturating_add);
        let freed = self
            .image_size_bytes(window_id, image_id)
            .map_or(0u64, |n| u64::try_from(n).unwrap_or(u64::MAX));
        let after = held.saturating_sub(freed).saturating_add(incoming);

        if after > link.image_budget {
            // The numbers are in the message because the client can act on
            // them: it knows how much it asked for and now knows how much room
            // there is, which is what it needs to decide between uploading a
            // smaller picture and dropping one it no longer shows.
            let limit = link.image_budget;
            Some(ResponseBody::Error {
                message: format!(
                    "image upload of {incoming} bytes would put this connection at {after} bytes, \
                     over the {limit}-byte limit"
                ),
            })
        } else {
            None
        }
    }

    /// Take a picture a client submitted for one of its windows.
    ///
    /// Silently dropped — not answered — when the client does not own the
    /// window it named. `SURF` has no `seq` and so no reply to carry a refusal,
    /// which is deliberate: a draw is the highest-frequency thing a client
    /// sends, and making every frame cost a round trip would put the
    /// compositor's acknowledgement on the critical path of every repaint.
    fn accept_submission(&mut self, link: &ClientLink, submission: Submission) {
        let id = WindowId::from_raw(submission.window);
        if !link.owns(id) {
            return;
        }
        let Submission { commands, .. } = submission;
        let RenderTree { commands } = commands;
        // The error is the same "no such window" that `owns` just ruled out,
        // save for a window destroyed between the check and here, which is not
        // something a client can be told about on a frame with no reply.
        let _ = self.submit_render(id, commands);
    }

    /// Queue the input events addressed to this link's windows.
    ///
    /// Events for windows this link does not own are left pending for the link
    /// that does. Call this once per link per tick; whatever no link claims is
    /// dropped by [`discard_unrouted_input`](Self::discard_unrouted_input).
    ///
    /// Returns how many events were routed. A closed link routes nothing: its
    /// events are dropped rather than encoded, because an outbox no one will
    /// read is just a leak with a queue in front of it. The windows themselves
    /// outlive the link until the server destroys them, so their events are
    /// this link's to discard and no one else's to claim.
    pub fn route_input(&mut self, link: &mut ClientLink) -> usize {
        let mut mine = Vec::new();
        let mut theirs = std::collections::VecDeque::new();
        for note in self.pending_notifications.drain(..) {
            if link.owns(note.window_id()) {
                mine.push(crate::wire_event(note));
            } else {
                theirs.push_back(note);
            }
        }
        self.pending_notifications = theirs;

        if link.closed {
            return 0;
        }
        let routed = mine.len();
        if routed > 0 {
            guiremote::encode_input_frame_into(&mut link.outbox, &mine);
        }
        routed
    }

    /// Send this link the desktop's window list, if it wants one and the list
    /// has changed since it last got one. Returns whether a frame was queued.
    ///
    /// Call once per link per tick, alongside [`route_input`](Self::route_input).
    /// Calling it more often is harmless — the second call finds nothing to
    /// send — and calling it less often costs latency, not correctness, because
    /// each frame is a whole snapshot rather than a step in a sequence.
    ///
    /// ## Why this compares bytes instead of watching for changes
    ///
    /// The obvious design is a counter the compositor bumps whenever a window
    /// is created, destroyed, retitled, focused, minimized, maximized or
    /// hidden, with each link remembering the value it last saw. That is one
    /// bump per site and about eight sites today, every one of which is a place
    /// a future change can forget — and a forgotten bump does not fail, it
    /// leaves a taskbar quietly showing yesterday's title until something
    /// unrelated happens to move the counter. Deriving the answer from the list
    /// itself has no site to forget: if the list a client would receive is
    /// different, it is sent.
    ///
    /// The cost is building and encoding the list once per tick *per subscribed
    /// link*, and subscribed links are shells — one, on a normal desktop. An
    /// unsubscribed link, which is every application, does no work at all.
    pub fn route_window_list(&mut self, link: &mut ClientLink) -> bool {
        // Checked before anything is built, so an ordinary application pays a
        // single boolean test per tick for a feature it does not use.
        if !link.wants_window_list || link.closed {
            return false;
        }
        // Reuses one buffer across ticks rather than allocating a fresh Vec for
        // a frame that is usually discarded as unchanged.
        self.window_list_scratch.clear();
        let list = self.window_list();
        encode_window_list_into(&mut self.window_list_scratch, &list);

        if link.window_list_sent == self.window_list_scratch {
            return false;
        }
        link.outbox.extend_from_slice(&self.window_list_scratch);
        link.window_list_sent.clear();
        link.window_list_sent
            .extend_from_slice(&self.window_list_scratch);
        true
    }

    /// Drop input that no client claimed, and say how much there was.
    ///
    /// A non-zero count is a real symptom rather than housekeeping: it means
    /// the compositor addressed an event to a window no live connection owns —
    /// a window destroyed between the hit test and the flush, or a link that
    /// was never served. Letting it accumulate instead would grow the queue
    /// without bound and deliver stale events to whoever next opened a window
    /// with a recycled id.
    pub fn discard_unrouted_input(&mut self) -> usize {
        let n = self.pending_notifications.len();
        self.pending_notifications.clear();
        n
    }
}

#[cfg(test)]
mod tests {
    // The defensive lints the workspace turns on are for production code: a
    // test that indexes a fixed-size fixture, or unwraps a value it just
    // constructed, is *asserting*. CLAUDE.md's lint policy says as much.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use crate::SettingsGroup;
    use guiremote::control::{
        BufferFormat, CursorShape, Layer, Request, RequestBody, ShellControlAction, WindowSpec,
        decode_responses,
    };
    use guiremote::input::decode_input_frame;
    use guiremote::window_list::{WindowInfo, WindowList};
    use guiremote::{InputEvent, encode_requests, encode_submit};
    use guitk::color::Color;
    use guitk::event::{Event, Key, Modifiers};

    use crate::EventNotification;

    use super::*;

    /// A compositor and one connected client.
    fn wired() -> (Compositor, ClientLink) {
        (
            Compositor::new(800, 600, 60).expect("compositor"),
            ClientLink::new(4242),
        )
    }

    /// Send a batch of requests and read back the decoded replies.
    fn exchange(
        comp: &mut Compositor,
        link: &mut ClientLink,
        bodies: Vec<RequestBody>,
    ) -> Vec<Response> {
        let requests: Vec<Request> = bodies
            .into_iter()
            .enumerate()
            .map(|(i, b)| Request::new(u32::try_from(i).unwrap() + 1, b))
            .collect();
        link.receive(&encode_requests(&requests));
        comp.serve(link).expect("serves");
        let bytes = link.take_outgoing();
        let (responses, used) = decode_responses(&bytes).expect("decodes");
        assert_eq!(used, bytes.len(), "the reply frame must consume its bytes");
        responses
    }

    /// Open one window over the link and return its id.
    fn open(comp: &mut Compositor, link: &mut ClientLink, title: &str) -> u64 {
        let responses = exchange(
            comp,
            link,
            vec![RequestBody::CreateWindow(WindowSpec::new(title, 200, 150))],
        );
        match responses.as_slice() {
            [
                Response {
                    body: ResponseBody::WindowCreated { window },
                    ..
                },
            ] => *window,
            other => panic!("expected one WindowCreated, got {other:?}"),
        }
    }

    #[test]
    fn a_window_is_created_over_the_wire_and_the_client_is_told_its_id() {
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Editor");

        // The id is the compositor's, and it names a real window.
        let win = comp
            .window_ref(WindowId::from_raw(window))
            .expect("the id names a window");
        assert_eq!(win.title, "Editor");
        assert_eq!(win.client_pid, 4242, "the pid comes from the link");
        assert!(link.owns(WindowId::from_raw(window)));
    }

    #[test]
    fn the_whole_spec_survives_the_trip_through_the_wire() {
        // The point of (f1): a spec that crosses this seam must arrive intact,
        // not be reduced to a title and a size on the way.
        let (mut comp, mut link) = wired();
        let mut spec = WindowSpec::new("Menu", 120, 80);
        spec.decorations = false;
        spec.resizable = false;
        spec.transparent = true;
        spec.position = Some((17, 23));
        spec.min_size = Some((100, 60));
        spec.max_size = Some((300, 200));

        let responses = exchange(&mut comp, &mut link, vec![RequestBody::CreateWindow(spec)]);
        let ResponseBody::WindowCreated { window } = responses[0].body else {
            panic!("expected WindowCreated");
        };

        let win = comp.window_ref(WindowId::from_raw(window)).expect("window");
        assert!(!win.decorations);
        assert!(!win.resizable);
        assert!(win.transparent);
        assert_eq!((win.x, win.y), (17, 23));
        assert_eq!(win.min_size, Some((100, 60)));
        assert_eq!(win.max_size, Some((300, 200)));
    }

    #[test]
    fn every_request_gets_exactly_one_reply_carrying_its_own_seq() {
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Editor");

        let responses = exchange(
            &mut comp,
            &mut link,
            vec![
                RequestBody::SetTitle {
                    window,
                    title: "Renamed".to_string(),
                },
                RequestBody::Move {
                    window,
                    x: 300,
                    y: 40,
                },
                RequestBody::GetDisplayInfo,
                RequestBody::SetOpacity {
                    window,
                    opacity: 0.5,
                },
            ],
        );

        assert_eq!(responses.len(), 4);
        assert_eq!(
            responses.iter().map(|r| r.seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4],
            "a reply carries the seq of the request it answers, in order"
        );
        assert!(matches!(responses[0].body, ResponseBody::Ok));
        assert!(matches!(responses[2].body, ResponseBody::Display(_)));

        let win = comp.window_ref(WindowId::from_raw(window)).expect("window");
        assert_eq!(win.title, "Renamed");
        assert_eq!((win.x, win.y), (300, 40));
        assert!((win.opacity - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_client_cannot_touch_a_window_it_did_not_open() {
        let (mut comp, mut link) = wired();
        let mine = open(&mut comp, &mut link, "Mine");

        // A second client's window, opened over its own link.
        let mut other = ClientLink::new(99);
        let theirs = open(&mut comp, &mut other, "Theirs");
        assert_ne!(mine, theirs);

        let responses = exchange(
            &mut comp,
            &mut link,
            vec![
                RequestBody::DestroyWindow { window: theirs },
                RequestBody::SetTitle {
                    window: theirs,
                    title: "Hijacked".to_string(),
                },
                // A window id that never existed is refused the same way, so
                // probing cannot tell the two apart.
                RequestBody::Minimize {
                    window: 0xDEAD_BEEF,
                },
            ],
        );

        for r in &responses {
            assert!(
                matches!(r.body, ResponseBody::Error { .. }),
                "expected a refusal, got {:?}",
                r.body
            );
        }
        let victim = comp
            .window_ref(WindowId::from_raw(theirs))
            .expect("still there");
        assert_eq!(victim.title, "Theirs");
    }

    #[test]
    fn a_picture_is_accepted_for_an_owned_window_and_ignored_for_anyone_elses() {
        let (mut comp, mut link) = wired();
        let mine = open(&mut comp, &mut link, "Mine");
        let mut other = ClientLink::new(99);
        let theirs = open(&mut comp, &mut other, "Theirs");

        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 10.0, 10.0, Color::from_hex(0x11_22_33));

        link.receive(&encode_submit(mine, &tree));
        link.receive(&encode_submit(theirs, &tree));
        comp.serve(&mut link).expect("serves");

        assert_eq!(
            comp.window_ref(WindowId::from_raw(mine))
                .expect("window")
                .render_tree
                .commands
                .len(),
            1
        );
        assert!(
            comp.window_ref(WindowId::from_raw(theirs))
                .expect("window")
                .render_tree
                .commands
                .is_empty(),
            "a submission for someone else's window must not land"
        );
        assert!(
            !link.has_outgoing(),
            "a submission has no seq and so gets no reply, refused or not"
        );
    }

    #[test]
    fn requests_and_pictures_interleave_on_one_connection() {
        // The reason the demultiplexer exists: a client draws and controls its
        // windows over the same stream, in whatever order it likes.
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Editor");

        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 5.0, 5.0, Color::from_hex(0x00_00_FF));

        let mut stream = encode_submit(window, &tree);
        stream.extend_from_slice(&encode_requests(&[Request::new(
            9,
            RequestBody::SetTitle {
                window,
                title: "Mid-stream".to_string(),
            },
        )]));
        stream.extend_from_slice(&encode_submit(window, &tree));

        link.receive(&stream);
        assert_eq!(comp.serve(&mut link).expect("serves"), 3);
        assert_eq!(link.pending_input(), 0, "the whole stream was consumed");
        assert_eq!(
            comp.window_ref(WindowId::from_raw(window))
                .expect("window")
                .title,
            "Mid-stream"
        );
    }

    #[test]
    fn a_frame_split_across_reads_is_assembled_rather_than_rejected() {
        // A transport delivers bytes, not frames. Feeding one byte at a time is
        // the worst case a real socket can produce, so it is the case tested.
        let (mut comp, mut link) = wired();
        let bytes = encode_requests(&[Request::new(
            1,
            RequestBody::CreateWindow(WindowSpec::new("Slow", 100, 100)),
        )]);

        for (i, b) in bytes.iter().enumerate() {
            link.receive(&[*b]);
            let served = comp.serve(&mut link).expect("serves");
            if i + 1 < bytes.len() {
                assert_eq!(served, 0, "an incomplete frame must not be acted on");
                assert!(!link.has_outgoing());
            } else {
                assert_eq!(served, 1);
            }
        }
        assert_eq!(link.windows().len(), 1);
        assert_eq!(link.pending_input(), 0);
    }

    #[test]
    fn input_goes_only_to_the_client_that_owns_the_window() {
        let (mut comp, mut link) = wired();
        let mine = open(&mut comp, &mut link, "Mine");
        let mut other = ClientLink::new(99);
        let theirs = open(&mut comp, &mut other, "Theirs");

        // Opening a window focuses it, and a focus change is an event in its
        // own right — the first link is told it gained and then lost focus, the
        // second that it gained it. Deliver those before the keystroke so the
        // counts below are about the keystroke and nothing else.
        assert_eq!(comp.route_input(&mut link), 2, "gained then lost focus");
        assert_eq!(comp.route_input(&mut other), 1, "gained focus");
        drop(link.take_outgoing());
        drop(other.take_outgoing());

        // Typing goes to whichever window has focus; here, the one opened last.
        comp.handle_key(0x1E, true, Some('a'));

        assert_eq!(comp.route_input(&mut link), 0, "not my window, not my keys");
        assert!(!link.has_outgoing());

        assert_eq!(comp.route_input(&mut other), 1);
        let bytes = other.take_outgoing();
        let (events, _) = decode_input_frame(&bytes).expect("decodes");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].window, theirs);
        assert!(matches!(events[0].event, Event::Key(_)));

        // And the compositor's queue is empty afterwards, not doubly delivered.
        assert_eq!(comp.discard_unrouted_input(), 0);
        assert_ne!(mine, theirs);
    }

    #[test]
    fn input_for_a_window_no_client_claims_is_reported_rather_than_hoarded() {
        let (mut comp, mut link) = wired();
        // A window the compositor knows about but that no link owns — what a
        // server sees if it forgets to serve a connection.
        let orphan = comp.create_window("Orphan".to_string(), 100, 100, 7);
        // Opening it focused it, which is itself an event with nowhere to go.
        assert_eq!(comp.discard_unrouted_input(), 1, "focus gained by nobody");

        comp.handle_key(0x1E, true, Some('a'));

        assert_eq!(comp.route_input(&mut link), 0);
        assert_eq!(
            comp.discard_unrouted_input(),
            1,
            "unrouted input must be counted, not silently accumulated"
        );
        assert_eq!(comp.discard_unrouted_input(), 0);
        assert!(comp.window_ref(orphan).is_some());
    }

    #[test]
    fn destroying_a_window_releases_the_links_claim_on_it() {
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Transient");
        assert!(link.owns(WindowId::from_raw(window)));

        let responses = exchange(
            &mut comp,
            &mut link,
            vec![RequestBody::DestroyWindow { window }],
        );
        assert!(matches!(responses[0].body, ResponseBody::Ok));
        assert!(!link.owns(WindowId::from_raw(window)));

        // And a second attempt is a refusal, not a second destruction.
        let responses = exchange(
            &mut comp,
            &mut link,
            vec![RequestBody::DestroyWindow { window }],
        );
        assert!(matches!(responses[0].body, ResponseBody::Error { .. }));
    }

    #[test]
    fn a_client_that_hung_up_is_neither_obeyed_nor_written_to() {
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Doomed");
        drop(link.take_outgoing());
        assert_eq!(comp.discard_unrouted_input(), 1, "focus on creation");

        link.close();
        assert!(link.is_closed());

        // A request that arrived before the hang-up (or after it, from a peer
        // that has not noticed) is not acted on: there is no one to reply to.
        link.receive(&encode_requests(&[Request::new(
            9,
            RequestBody::CreateWindow(WindowSpec::new("Too late", 100, 100)),
        )]));
        assert_eq!(comp.serve(&mut link).expect("no decode error"), 0);
        assert_eq!(link.windows().len(), 1, "no second window was opened");
        assert!(!link.has_outgoing(), "nothing was queued for a dead peer");

        // Nor does its window's input pile up in an outbox nobody reads.
        comp.handle_key(0x1E, true, Some('a'));
        assert_eq!(comp.route_input(&mut link), 0);
        assert!(!link.has_outgoing());
        assert_eq!(
            comp.discard_unrouted_input(),
            0,
            "a closed link's events are dropped, not left for another link"
        );
        assert!(comp.window_ref(WindowId::from_raw(window)).is_some());
    }

    // ------------------------------------------------------------------
    // The window list: how a shell learns about windows it did not open
    // ------------------------------------------------------------------

    /// Every window-list frame in a byte stream, ignoring the input frames and
    /// replies interleaved with them.
    fn window_lists(bytes: &[u8]) -> Vec<WindowList> {
        let mut lists = Vec::new();
        let mut at = 0usize;
        while at < bytes.len() {
            let (frame, used) = guiremote::decode_any(&bytes[at..]).expect("decodes");
            if let Frame::WindowList(list) = frame {
                lists.push(list);
            }
            at += used;
        }
        assert_eq!(at, bytes.len(), "no bytes left over");
        lists
    }

    /// Route everything pending to `link` and return the window lists sent,
    /// header and all.
    fn pump_full_lists(comp: &mut Compositor, link: &mut ClientLink) -> Vec<WindowList> {
        comp.route_input(link);
        comp.route_window_list(link);
        let bytes = link.take_outgoing();
        window_lists(&bytes)
    }

    /// The same, reduced to just the windows -- what most of these tests are
    /// asking about, and the shape they were written against.
    fn pump_lists(comp: &mut Compositor, link: &mut ClientLink) -> Vec<Vec<WindowInfo>> {
        pump_full_lists(comp, link)
            .into_iter()
            .map(|l| l.windows)
            .collect()
    }

    /// Open a window in a named stacking band over `link`.
    fn open_in(comp: &mut Compositor, link: &mut ClientLink, title: &str, layer: Layer) -> u64 {
        let mut spec = WindowSpec::new(title, 200, 150);
        spec.layer = layer;
        let responses = exchange(comp, link, vec![RequestBody::CreateWindow(spec)]);
        match responses[0].body {
            ResponseBody::WindowCreated { window } => window,
            ref other => panic!("expected WindowCreated, got {other:?}"),
        }
    }

    #[test]
    fn a_shell_is_told_about_windows_it_did_not_open() {
        // The whole point of the protocol. Before it, a taskbar could stay in
        // front of the windows it was supposed to list (that is what `Layer`
        // bought) and had no way at all to find out what they were.
        let (mut comp, mut shell) = wired();
        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        assert!(matches!(responses[0].body, ResponseBody::Ok));
        assert!(shell.wants_window_list());

        let mut app = ClientLink::new(99);
        let theirs = open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        let lists = pump_lists(&mut comp, &mut shell);
        assert_eq!(lists.len(), 1, "exactly one list, not one per change");
        let list = &lists[0];
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, theirs);
        assert_eq!(list[0].title, "Editor");
        assert_eq!(
            list[0].pid, 99,
            "the owner, so a shell can group by program"
        );
        assert!(list[0].focused, "a new window takes focus");
        assert!(!shell.owns(WindowId::from_raw(theirs)));
    }

    #[test]
    fn an_application_that_never_subscribed_is_told_nothing_about_the_desktop() {
        // A window list is a description of every other program on the machine.
        // An app that did not ask must not receive one — and, just as much, must
        // not pay for building one.
        let (mut comp, mut app) = wired();
        open_in(&mut comp, &mut app, "Mine", Layer::Normal);
        let mut other = ClientLink::new(99);
        open_in(&mut comp, &mut other, "Theirs", Layer::Normal);

        assert!(!comp.route_window_list(&mut app));
        assert!(window_lists(&app.take_outgoing()).is_empty());
    }

    #[test]
    fn an_unchanged_desktop_costs_nothing_after_the_first_list() {
        // A shell ticks at frame rate; a desktop changes a few times a minute.
        // Re-sending the same list 60 times a second would be the whole cost of
        // the feature.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        assert!(comp.route_window_list(&mut shell), "the first list is sent");
        drop(shell.take_outgoing());

        for tick in 0..10 {
            assert!(
                !comp.route_window_list(&mut shell),
                "tick {tick} re-sent an unchanged list"
            );
        }
        assert!(!shell.has_outgoing());
    }

    #[test]
    fn a_retitle_reaches_the_shell() {
        // The change with no other signal. Creation and destruction move the
        // window count, focus moves a flag — but a title changing is invisible
        // to anything except the list itself, so a design that watched for
        // "interesting" events instead of comparing the list would drop it.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        let window = open_in(&mut comp, &mut app, "untitled", Layer::Normal);
        assert_eq!(pump_lists(&mut comp, &mut shell)[0][0].title, "untitled");

        exchange(
            &mut comp,
            &mut app,
            vec![RequestBody::SetTitle {
                window,
                title: "notes.txt — saved".to_string(),
            }],
        );

        let lists = pump_lists(&mut comp, &mut shell);
        assert_eq!(lists.len(), 1, "the retitle produced a list");
        assert_eq!(lists[0][0].title, "notes.txt — saved");
    }

    #[test]
    fn minimizing_and_restoring_both_reach_the_shell() {
        // A taskbar button's whole job: it must know which windows are
        // minimized, because those are the ones clicking it restores.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        let window = open_in(&mut comp, &mut app, "Editor", Layer::Normal);
        assert!(!pump_lists(&mut comp, &mut shell)[0][0].minimized);

        exchange(&mut comp, &mut app, vec![RequestBody::Minimize { window }]);
        let lists = pump_lists(&mut comp, &mut shell);
        assert_eq!(lists.len(), 1);
        assert!(lists[0][0].minimized);

        exchange(&mut comp, &mut app, vec![RequestBody::Restore { window }]);
        let lists = pump_lists(&mut comp, &mut shell);
        assert_eq!(lists.len(), 1);
        assert!(!lists[0][0].minimized);
    }

    #[test]
    fn a_shell_can_act_on_a_window_it_does_not_own_and_an_application_still_cannot() {
        // The other half of the window list, and the thing that makes a taskbar
        // button possible: a shell must be able to *do* something to a window it
        // only learned about by being told. The two halves are asserted in one
        // test on purpose — `ShellControl` is only correct if it is the sole
        // exception, so the same foreign window is driven twice, once through
        // the shell's request and once through the ordinary owned verb, and the
        // second must be refused. A `ShellControl` that quietly went through
        // `resolve` would fail the first assertion; an ownership check that had
        // been loosened to let it through would fail the second.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        let theirs = open_in(&mut comp, &mut app, "Editor", Layer::Normal);
        assert!(
            !shell.owns(WindowId::from_raw(theirs)),
            "the fixture is pointless if the shell owns the window"
        );
        assert!(!pump_lists(&mut comp, &mut shell)[0][0].minimized);

        // The shell minimizes somebody else's window, as a taskbar button does.
        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::ShellControl {
                window: theirs,
                action: ShellControlAction::Minimize,
            }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Ok),
            "a shell was refused a window it was just told about: {:?}",
            responses[0].body
        );
        // And it actually happened — the reply is checked against the desktop
        // rather than taken at its word.
        assert!(
            pump_lists(&mut comp, &mut shell)[0][0].minimized,
            "the compositor said Ok and did nothing"
        );

        // Clicking the button again brings it back, focused: the round trip a
        // user performs, not just the outbound half.
        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::ShellControl {
                window: theirs,
                action: ShellControlAction::Activate,
            }],
        );
        assert!(matches!(responses[0].body, ResponseBody::Ok));
        let list = pump_lists(&mut comp, &mut shell).remove(0);
        assert!(!list[0].minimized, "activate left it minimized");
        assert!(list[0].focused, "activate gave it back without focus");

        // The exception is exactly one request wide. The same client asking for
        // the same window through the ordinary verb is still refused, because
        // that one is resolved against the sender's own windows.
        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::Minimize { window: theirs }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Error { .. }),
            "the owned-window verb reached a window the sender does not own: {:?}",
            responses[0].body
        );
        assert!(
            !comp
                .window_ref(WindowId::from_raw(theirs))
                .expect("window")
                .minimized,
            "a refused request minimized the window anyway"
        );
    }

    #[test]
    fn a_shell_moves_a_window_to_another_desktop_and_the_list_says_so() {
        // The two halves of virtual desktops as a *shell* meets them: the
        // request that files a window elsewhere, and the field in the list that
        // is the only way to find out where it went. Without the second, a
        // taskbar has to remember what it asked for, and it will be wrong the
        // first time anything else moves a window.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        let theirs = open_in(&mut comp, &mut app, "Editor", Layer::Normal);
        let list = pump_full_lists(&mut comp, &mut shell).remove(0);
        assert_eq!(list.current_workspace, 0);
        assert_eq!(
            list.windows[0].workspace, 0,
            "a new window did not open on the desktop the user is looking at"
        );

        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SetWindowWorkspace {
                window: theirs,
                workspace: 2,
            }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Ok),
            "a shell was refused a window it was just told about: {:?}",
            responses[0].body
        );

        let list = pump_full_lists(&mut comp, &mut shell).remove(0);
        assert_eq!(
            list.current_workspace, 0,
            "moving a window away moved the user with it"
        );
        assert_eq!(
            list.windows[0].workspace, 2,
            "the compositor said Ok and the list still files it here"
        );
        assert!(
            list.windows[0].visible,
            "a window on another desktop was reported as unmapped -- hiding is only hiding"
        );
    }

    #[test]
    fn the_list_says_which_desktop_is_showing_after_a_switch_the_shell_did_not_ask_for() {
        // The reason the showing desktop is a field rather than something a
        // shell tracks itself. Activating a window that is filed elsewhere is a
        // *switch*, decided by the compositor; a shell that remembered its own
        // last request would now list desktop 0's windows over desktop 2's
        // screen -- a taskbar disagreeing with the glass, which is the bug
        // virtual desktops were built to remove.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        let theirs = open_in(&mut comp, &mut app, "Editor", Layer::Normal);
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SetWindowWorkspace {
                window: theirs,
                workspace: 2,
            }],
        );
        let _ = pump_full_lists(&mut comp, &mut shell);

        // The taskbar button for a window the user cannot see.
        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::ShellControl {
                window: theirs,
                action: ShellControlAction::Activate,
            }],
        );
        assert!(matches!(responses[0].body, ResponseBody::Ok));

        let list = pump_full_lists(&mut comp, &mut shell).remove(0);
        assert_eq!(
            list.current_workspace, 2,
            "activating a window elsewhere did not take the user to it"
        );
        assert!(list.windows[0].focused, "it arrived without the keyboard");
    }

    #[test]
    fn switching_desktop_off_the_wire_reaches_the_compositor() {
        // A switch changes what *every* client on the machine is showing, so it
        // goes through `require_shell` -- which today refuses nobody, and says
        // so at length. What this can assert is the other half: that the
        // request is routed rather than dropped, and that the compositor's own
        // idea of the showing desktop actually moved.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SwitchWorkspace { workspace: 3 }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Ok),
            "a shell was refused a switch: {:?}",
            responses[0].body
        );
        assert_eq!(comp.current_workspace(), 3);
    }

    /// The grab path end to end, over the real codec, with the shell *not*
    /// focused — which is the only interesting case and the one that was
    /// impossible before. Asserting on `Compositor::grab_key` alone would leave
    /// the wire translation untested, and it is the translation that carries the
    /// two checks: resolve the window against the sender, then `require_shell`.
    #[test]
    fn a_shell_grabs_a_chord_over_the_wire_and_receives_it_from_another_window() {
        let (mut comp, mut shell) = wired();
        let panel = open_in(&mut comp, &mut shell, "Taskbar", Layer::Overlay);
        let mut app = ClientLink::new(99);
        open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::GrabKey {
                window: panel,
                key: Key::Tab,
                modifiers: Modifiers::alt(),
            }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Ok),
            "the grab was refused: {:?}",
            responses[0].body
        );

        comp.drain_notifications();
        comp.handle_input(crate::InputEvent::KeyDown {
            scancode: 0x38, // left Alt
            character: None,
        });
        comp.handle_input(crate::InputEvent::KeyDown {
            scancode: 0x0F, // Tab
            character: None,
        });

        let tab = comp
            .drain_notifications()
            .into_iter()
            .find_map(|n| match n {
                EventNotification::KeyEvent {
                    window_id,
                    key: Key::Tab,
                    ..
                } => Some(window_id.raw()),
                _ => None,
            })
            .expect("Alt+Tab reached nobody");
        assert_eq!(
            tab, panel,
            "Alt+Tab went to the focused editor: the shortcut is still unreachable"
        );
    }

    /// The window in a grab is the *sender's*, resolved like any other window
    /// request. Without that, a client could aim another program's shortcuts at
    /// itself — or, more simply, register a grab whose deliveries would go to a
    /// window it cannot read.
    #[test]
    fn a_grab_naming_somebody_elses_window_is_refused() {
        let (mut comp, mut shell) = wired();
        open_in(&mut comp, &mut shell, "Taskbar", Layer::Overlay);
        let mut app = ClientLink::new(99);
        let editor = open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        let responses = exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::GrabKey {
                window: editor,
                key: Key::Tab,
                modifiers: Modifiers::alt(),
            }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Error { .. }),
            "a grab was accepted for a window the sender does not own"
        );
        assert_eq!(comp.grab_count(), 0);
    }

    #[test]
    fn closing_the_last_window_sends_an_empty_list_rather_than_nothing() {
        // "Nothing to report" and "there is nothing left" are different, and a
        // shell told the first when the second happened leaves a taskbar button
        // for a program that has exited.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        let window = open_in(&mut comp, &mut app, "Editor", Layer::Normal);
        assert_eq!(pump_lists(&mut comp, &mut shell)[0].len(), 1);

        exchange(
            &mut comp,
            &mut app,
            vec![RequestBody::DestroyWindow { window }],
        );
        let lists = pump_lists(&mut comp, &mut shell);
        assert_eq!(lists.len(), 1, "the desktop emptying is itself news");
        assert!(lists[0].is_empty());
    }

    #[test]
    fn unsubscribing_stops_the_traffic_and_resubscribing_resends() {
        // Re-subscribing must not be answered with silence just because the
        // list happens not to have changed while the client was not listening.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let mut app = ClientLink::new(99);
        open_in(&mut comp, &mut app, "Editor", Layer::Normal);
        assert_eq!(pump_lists(&mut comp, &mut shell).len(), 1);

        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: false }],
        );
        assert!(!shell.wants_window_list());
        open_in(&mut comp, &mut app, "Second", Layer::Normal);
        assert!(
            !comp.route_window_list(&mut shell),
            "an unsubscribed link gets nothing even when the desktop changes"
        );
        drop(shell.take_outgoing());

        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        let lists = pump_lists(&mut comp, &mut shell);
        assert_eq!(lists.len(), 1, "re-subscribing re-sends");
        assert_eq!(lists[0].len(), 2, "including what changed while away");
    }

    #[test]
    fn the_list_carries_the_band_so_a_taskbar_can_leave_itself_out_of_it() {
        // Without `layer` a taskbar lists the wallpaper and itself, which is
        // the reason this field is on the frame rather than left to be inferred.
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        open_in(&mut comp, &mut shell, "Taskbar", Layer::Overlay);
        open_in(&mut comp, &mut shell, "Wallpaper", Layer::Background);
        let mut app = ClientLink::new(99);
        open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        let lists = pump_lists(&mut comp, &mut shell);
        let list = &lists[lists.len() - 1];
        assert_eq!(list.len(), 3);
        // Bottom-to-top, which is the stacking order and so is banded.
        assert_eq!(
            list.iter().map(|w| w.layer).collect::<Vec<_>>(),
            vec![Layer::Background, Layer::Normal, Layer::Overlay]
        );
        let taskbar_would_show: Vec<&str> = list
            .iter()
            .filter(|w| w.layer == Layer::Normal)
            .map(|w| w.title.as_str())
            .collect();
        assert_eq!(taskbar_would_show, vec!["Editor"]);
    }

    #[test]
    fn two_shells_each_get_their_own_copy_and_their_own_staleness() {
        // The state that decides whether to send lives on the link, not on the
        // compositor: one shell having been told must not count as the other
        // having been told.
        let (mut comp, mut first) = wired();
        let mut second = ClientLink::new(77);
        for link in [&mut first, &mut second] {
            exchange(
                &mut comp,
                link,
                vec![RequestBody::SubscribeWindowList { subscribe: true }],
            );
        }
        let mut app = ClientLink::new(99);
        open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        assert_eq!(pump_lists(&mut comp, &mut first).len(), 1);
        assert_eq!(
            pump_lists(&mut comp, &mut second).len(),
            1,
            "the second shell is told too"
        );
        assert!(!comp.route_window_list(&mut first));
        assert!(!comp.route_window_list(&mut second));
    }

    #[test]
    fn a_shell_that_hung_up_is_not_queued_a_list_it_will_never_read() {
        let (mut comp, mut shell) = wired();
        exchange(
            &mut comp,
            &mut shell,
            vec![RequestBody::SubscribeWindowList { subscribe: true }],
        );
        shell.close();
        let mut app = ClientLink::new(99);
        open_in(&mut comp, &mut app, "Editor", Layer::Normal);

        assert!(!comp.route_window_list(&mut shell));
        assert!(!shell.has_outgoing());
    }

    #[test]
    fn a_window_list_from_a_client_is_rejected_rather_than_ignored() {
        // It travels the other way. A client sending one is confused about
        // which end it is, and a silent no-op would leave that to be found later
        // as a taskbar that never updates.
        let (mut comp, mut link) = wired();
        link.receive(&guiremote::window_list::encode_window_list(
            &guiremote::WindowList::new(0, vec![WindowInfo::new(1, 2, "Forged")]),
        ));
        assert_eq!(
            comp.serve(&mut link),
            Err(WireError::WrongDirection("window list"))
        );
    }

    #[test]
    fn a_frame_only_the_compositor_sends_is_rejected_rather_than_ignored() {
        let (mut comp, mut link) = wired();
        link.receive(&guiremote::control::encode_responses(&[Response::new(
            1,
            ResponseBody::Ok,
        )]));
        assert_eq!(
            comp.serve(&mut link),
            Err(WireError::WrongDirection("control response"))
        );
    }

    #[test]
    fn a_corrupt_stream_is_an_error_and_not_a_resynchronisation_attempt() {
        let (mut comp, mut link) = wired();
        link.receive(b"XXXXnot a frame at all");
        assert!(matches!(
            comp.serve(&mut link),
            Err(WireError::Malformed(DecodeError::BadMagic))
        ));
    }

    #[test]
    fn a_cursor_request_names_the_window_it_is_for() {
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Editor");
        let responses = exchange(
            &mut comp,
            &mut link,
            vec![RequestBody::SetCursor {
                window,
                shape: CursorShape::Text,
            }],
        );
        assert!(matches!(responses[0].body, ResponseBody::Ok));
        assert_eq!(
            comp.window_ref(WindowId::from_raw(window))
                .expect("window")
                .cursor,
            CursorShape::Text
        );
    }

    #[test]
    fn a_reply_and_an_input_frame_share_the_outbox_in_order() {
        // The client's reader is a demultiplexer, so the compositor is free to
        // interleave — but the order must be the order things happened, or a
        // resize event could arrive before the reply that caused it.
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Editor");

        link.receive(&encode_requests(&[Request::new(
            1,
            RequestBody::Resize {
                window,
                width: 640,
                height: 480,
            },
        )]));
        comp.serve(&mut link).expect("serves");
        comp.route_input(&mut link);

        let bytes = link.take_outgoing();
        let (responses, used) = decode_responses(&bytes).expect("reply first");
        assert!(matches!(responses[0].body, ResponseBody::Ok));
        let (events, rest) = decode_input_frame(&bytes[used..]).expect("then the event");
        assert_eq!(used + rest, bytes.len(), "nothing left over");
        assert!(
            events
                .iter()
                .any(|e: &InputEvent| matches!(e.event, Event::Resize { .. })),
            "the resize the client asked for comes back as the event that confirms it"
        );
    }

    #[test]
    fn a_reload_request_off_the_wire_reaches_the_users_settings_file() {
        // The whole point of the verb, end to end: bytes a client wrote, over
        // the same decode path every other request takes, ending in the
        // compositor holding what the *file* says. Asserted here rather than in
        // `lib.rs` because this is the only test module with both halves of the
        // protocol in scope, and the two halves are exactly where a new verb
        // gets half-wired — a variant that encodes but never decodes, or one
        // that decodes to a request nothing maps.
        appearance::config::testing::with_scratch_config("wire-reload", |_root| {
            let (mut comp, mut link) = wired();
            assert!(
                comp.appearance().drop_shadows,
                "the compositor should start from the defaults"
            );

            let mut file = appearance::AppearanceFile::new();
            file.settings.drop_shadows = false;
            file.save().expect("write scratch appearance.yaml");

            let responses = exchange(&mut comp, &mut link, vec![RequestBody::ReloadAppearance]);
            assert!(
                matches!(
                    responses.as_slice(),
                    [Response {
                        body: ResponseBody::Ok,
                        ..
                    }]
                ),
                "a reload is answered Ok, got {responses:?}"
            );
            assert!(
                !comp.appearance().drop_shadows,
                "the reload request did not reach the settings the compositor draws from"
            );
        });
    }

    /// Every settings group announced since the last look.
    fn announced(comp: &mut Compositor) -> Vec<(u64, SettingsGroup)> {
        comp.drain_notifications()
            .into_iter()
            .filter_map(|n| match n {
                EventNotification::SettingsChanged { window_id, group } => {
                    Some((window_id.0, group))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_reload_request_is_passed_on_to_everybody_else() {
        // The reason the request exists at all. Before this, the compositor
        // re-read the file and told nobody, so a theme change reached the
        // window decorations the compositor draws and *nothing inside them*
        // until the next login -- including the desktop shell, which is where
        // the taskbar and the start menu live.
        appearance::config::testing::with_scratch_config("wire-announce", |_root| {
            let (mut comp, mut link) = wired();
            let a = comp.create_window("a".to_string(), 100, 100, 7);
            let b = comp.create_window("b".to_string(), 100, 100, 7);
            let _ = announced(&mut comp);

            exchange(&mut comp, &mut link, vec![RequestBody::ReloadAppearance]);

            let mut got = announced(&mut comp);
            got.sort_by_key(|(id, _)| *id);
            let mut want = vec![
                (a.0, SettingsGroup::Appearance),
                (b.0, SettingsGroup::Appearance),
            ];
            want.sort_by_key(|(id, _)| *id);
            assert_eq!(got, want, "every window's program should have been told");
        });
    }

    #[test]
    fn an_input_reload_announces_input_and_not_appearance() {
        // The two verbs are two tags, two decode arms and two mappings, and the
        // announcement adds a third place they can be confused. A `ReloadInput`
        // that announced `Appearance` would make every window re-read the
        // wrong file and would leave the one that actually changed unread --
        // and it would pass every test that only checks *that* something was
        // announced.
        inputsettings::config::testing::with_scratch_config("wire-announce-input", |_root| {
            let (mut comp, mut link) = wired();
            let w = comp.create_window("a".to_string(), 100, 100, 7);
            let _ = announced(&mut comp);

            exchange(&mut comp, &mut link, vec![RequestBody::ReloadInput]);
            assert_eq!(announced(&mut comp), vec![(w.0, SettingsGroup::Input)]);
        });
    }

    #[test]
    fn a_reload_with_no_windows_open_announces_nothing() {
        // The Settings app can send a reload before opening a window -- the
        // test above this one asserts it may -- and an announcement addressed
        // to a window that does not exist has nowhere to be routed. It must
        // simply not be made, rather than be made against a placeholder id.
        appearance::config::testing::with_scratch_config("wire-announce-empty", |_root| {
            let (mut comp, mut link) = wired();
            exchange(&mut comp, &mut link, vec![RequestBody::ReloadAppearance]);
            assert!(announced(&mut comp).is_empty());
        });
    }

    #[test]
    fn a_reload_request_names_no_window_and_so_needs_no_window_to_name() {
        // Every other request that reaches the compositor carries a window id
        // the link must vouch for. This one carries nothing, which means a
        // client that has never opened a window can still send it — and should,
        // because the Settings app is exactly such a client. If someone later
        // routes it through the ownership check by reflex, this fails.
        appearance::config::testing::with_scratch_config("wire-reload-windowless", |_root| {
            let (mut comp, mut link) = wired();
            let responses = exchange(&mut comp, &mut link, vec![RequestBody::ReloadAppearance]);
            assert!(
                matches!(
                    responses.as_slice(),
                    [Response {
                        body: ResponseBody::Ok,
                        ..
                    }]
                ),
                "a windowless client's reload was refused: {responses:?}"
            );
        });
    }

    #[test]
    fn an_input_reload_request_off_the_wire_reaches_the_users_settings_file() {
        // The same end-to-end claim as for appearance, and worth making
        // separately rather than trusting the symmetry: the two verbs are two
        // tags, two decode arms and two mappings, and a new one is exactly the
        // thing that gets half-wired. A `ReloadInput` that silently decoded to
        // `ReloadAppearance` would pass every test in `guiremote` and every
        // test in `lib.rs`, and would still leave the double-click speed
        // unreachable while repainting the desktop for no reason.
        inputsettings::config::testing::with_scratch_config("wire-reload-input", |_root| {
            let (mut comp, mut link) = wired();
            assert_eq!(
                comp.double_click_ms(),
                400,
                "the compositor should start from the defaults"
            );

            let mut file = inputsettings::InputFile::new();
            file.settings.mouse.set_double_click_ms(1200);
            file.save().expect("write scratch input.yaml");

            let responses = exchange(&mut comp, &mut link, vec![RequestBody::ReloadInput]);
            assert!(
                matches!(
                    responses.as_slice(),
                    [Response {
                        body: ResponseBody::Ok,
                        ..
                    }]
                ),
                "an input reload is answered Ok, got {responses:?}"
            );
            assert_eq!(
                comp.double_click_ms(),
                1200,
                "the reload request did not reach the interval the compositor measures with"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Image upload
    // -----------------------------------------------------------------------

    /// An `UploadImage` for `window` of `w × h` opaque pixels.
    fn upload(window: u64, image_id: u64, w: u32, h: u32) -> RequestBody {
        RequestBody::UploadImage {
            window,
            image_id,
            width: w,
            height: h,
            stride: w * 4,
            format: BufferFormat::Argb8888,
            bytes: vec![0xFF; (w * h * 4) as usize],
        }
    }

    #[test]
    fn an_image_uploaded_over_the_wire_reaches_the_windows_image_store() {
        // The whole point of the request: before it existed, `register_image`
        // could only be called by code inside the compositor's own address
        // space, so a picture drawn by a program in another process drew
        // nothing — with no error either way. See `known-issues.md` →
        // `TD-C-AN-IMAGE-CAN-ONLY-BE-UPLOADED-IN-PROCESS`.
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Viewer");

        let responses = exchange(&mut comp, &mut link, vec![upload(window, 1, 4, 4)]);
        assert!(
            matches!(
                responses.as_slice(),
                [Response {
                    body: ResponseBody::Ok,
                    ..
                }]
            ),
            "upload was not accepted: {responses:?}"
        );

        let id = WindowId::from_raw(window);
        assert_eq!(comp.image_count(id), Some(1));
        assert_eq!(comp.window_image_bytes(id), Some(4 * 4 * 4));
        assert_eq!(comp.image_size_bytes(id, 1), Some(4 * 4 * 4));
    }

    #[test]
    fn re_uploading_an_id_replaces_it_rather_than_adding_to_it() {
        // The in-place update a video frame or a re-rendered chart is made of.
        // If this ever *added*, a client redrawing at 60 Hz would exhaust its
        // budget in seconds while holding exactly one picture.
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Player");
        let id = WindowId::from_raw(window);

        exchange(&mut comp, &mut link, vec![upload(window, 1, 8, 8)]);
        exchange(&mut comp, &mut link, vec![upload(window, 1, 4, 4)]);

        assert_eq!(comp.image_count(id), Some(1), "one id, one image");
        assert_eq!(
            comp.window_image_bytes(id),
            Some(4 * 4 * 4),
            "the replacement's size, not the sum of both"
        );
    }

    #[test]
    fn dropping_an_image_frees_it_and_dropping_it_twice_is_not_an_error() {
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Viewer");
        let id = WindowId::from_raw(window);
        exchange(&mut comp, &mut link, vec![upload(window, 1, 4, 4)]);

        for round in 0..2 {
            let responses = exchange(
                &mut comp,
                &mut link,
                vec![RequestBody::DropImage {
                    window,
                    image_id: 1,
                }],
            );
            assert!(
                matches!(
                    responses.as_slice(),
                    [Response {
                        body: ResponseBody::Ok,
                        ..
                    }]
                ),
                "drop round {round} was not answered Ok: {responses:?}"
            );
        }
        assert_eq!(comp.image_count(id), Some(0));
        assert_eq!(comp.window_image_bytes(id), Some(0));
    }

    #[test]
    fn an_upload_naming_another_links_window_is_refused_exactly_like_a_missing_one() {
        // The §458 rule, applied to the newest request that carries a window
        // id. The two refusals must be *identical text*: a distinguishable
        // "that exists but is not yours" is a way to enumerate the desktop's
        // window ids one probe at a time.
        let (mut comp, mut link) = wired();
        let mut other = ClientLink::new(9999);
        let theirs = open(&mut comp, &mut other, "Somebody else's");

        let refused = exchange(&mut comp, &mut link, vec![upload(theirs, 1, 2, 2)]);
        let nonexistent = exchange(&mut comp, &mut link, vec![upload(theirs, 1, 2, 2)]);
        let ResponseBody::Error { message: a } = &refused[0].body else {
            panic!("uploading into another link's window was allowed: {refused:?}");
        };
        // Same id, now asked for after establishing it is not ours: the
        // messages must not differ, and neither must have left pixels behind.
        let ResponseBody::Error { message: b } = &nonexistent[0].body else {
            panic!("expected a refusal, got {nonexistent:?}");
        };
        assert_eq!(a, b);
        assert_eq!(
            comp.image_count(WindowId::from_raw(theirs)),
            Some(0),
            "the refused upload must not have reached the other link's window"
        );
    }

    /// Bytes an `n × n` upload costs once resident: four per pixel, whatever
    /// format it arrived in.
    const fn resident(side: u32) -> u64 {
        (side as u64) * (side as u64) * 4
    }

    /// A compositor and a link whose image budget is exactly two 8×8 pictures.
    ///
    /// The budget is set on the link rather than the tests being written
    /// against [`MAX_IMAGE_BYTES_PER_LINK`] because filling 256 MiB for real —
    /// twice, in each of four tests, with the compositor keeping its own copy —
    /// is gigabytes of allocation to prove arithmetic that does not care how
    /// big the numbers are. The budget being a per-link field is what makes
    /// that substitution honest: these tests exercise the same code path a
    /// 256 MiB link does, with the same comparison against the same field.
    fn wired_with_small_budget() -> (Compositor, ClientLink) {
        let (comp, mut link) = wired();
        link.set_image_budget(resident(8) * 2);
        (comp, link)
    }

    #[test]
    fn a_fresh_link_starts_at_the_default_budget() {
        // The substitution above is only sound if a link nobody configured gets
        // the documented ceiling.
        let (_comp, link) = wired();
        assert_eq!(link.image_budget(), MAX_IMAGE_BYTES_PER_LINK);
    }

    #[test]
    fn an_upload_over_the_links_budget_is_refused_and_changes_nothing() {
        let (mut comp, mut link) = wired_with_small_budget();
        let window = open(&mut comp, &mut link, "Hog");
        let id = WindowId::from_raw(window);

        // Half the budget, twice: the second fits exactly.
        exchange(&mut comp, &mut link, vec![upload(window, 1, 8, 8)]);
        exchange(&mut comp, &mut link, vec![upload(window, 2, 8, 8)]);
        assert_eq!(
            comp.window_image_bytes(id).map(|n| n as u64),
            Some(link.image_budget()),
            "the two halves should exactly fill the budget"
        );

        // One more pixel does not.
        let responses = exchange(&mut comp, &mut link, vec![upload(window, 3, 1, 1)]);
        let ResponseBody::Error { message } = &responses[0].body else {
            panic!("an over-budget upload was accepted: {responses:?}");
        };
        assert!(
            message.contains("limit"),
            "the refusal should say what it hit: {message}"
        );
        assert_eq!(comp.image_count(id), Some(2), "nothing new was stored");
        assert_eq!(
            comp.window_image_bytes(id).map(|n| n as u64),
            Some(link.image_budget()),
            "a refused upload must not change what is held"
        );
    }

    #[test]
    fn replacing_an_image_is_weighed_against_the_total_after_the_replacement() {
        // The subtlety the budget arithmetic exists for. A client holding most
        // of its allowance in one asset re-uploads *that same asset* at the same
        // size: the total afterwards is unchanged, so it must be accepted.
        // Computing "held + incoming" instead would refuse it — and would
        // therefore refuse every in-place update from the moment a client passed
        // half its budget, which is when a video player is doing precisely what
        // it is supposed to.
        let (mut comp, mut link) = wired_with_small_budget();
        let window = open(&mut comp, &mut link, "Player");
        let id = WindowId::from_raw(window);

        // 3/4 of the budget: 12×8 pixels against a two-8×8-picture ceiling.
        exchange(&mut comp, &mut link, vec![upload(window, 1, 12, 8)]);
        let responses = exchange(&mut comp, &mut link, vec![upload(window, 1, 12, 8)]);
        assert!(
            matches!(
                responses.as_slice(),
                [Response {
                    body: ResponseBody::Ok,
                    ..
                }]
            ),
            "an in-place replacement at three-quarters of budget was refused: {responses:?}"
        );
        assert_eq!(comp.image_count(id), Some(1));
        assert_eq!(comp.window_image_bytes(id), Some(12 * 8 * 4));
    }

    #[test]
    fn the_budget_is_per_link_and_a_second_window_does_not_double_it() {
        // The reason the budget is not per window: if it were, this would
        // succeed, and "how much may one connection hold" would be answered by
        // how many windows it felt like opening.
        let (mut comp, mut link) = wired_with_small_budget();
        let a = open(&mut comp, &mut link, "One");
        let b = open(&mut comp, &mut link, "Two");

        let first = exchange(&mut comp, &mut link, vec![upload(a, 1, 12, 8)]);
        assert!(matches!(first[0].body, ResponseBody::Ok));

        let second = exchange(&mut comp, &mut link, vec![upload(b, 1, 12, 8)]);
        assert!(
            matches!(second[0].body, ResponseBody::Error { .. }),
            "a second window bought a second budget: {second:?}"
        );
        assert_eq!(comp.image_count(WindowId::from_raw(b)), Some(0));
    }

    #[test]
    fn two_links_have_separate_budgets() {
        // The other half of "per link": one connection filling its allowance
        // must not refuse another's first upload. A single global total would
        // let any program on the machine deny images to every other one.
        let (mut comp, mut link) = wired_with_small_budget();
        let mut other = ClientLink::new(5150);
        other.set_image_budget(resident(8) * 2);

        let mine = open(&mut comp, &mut link, "Mine");
        let theirs = open(&mut comp, &mut other, "Theirs");
        exchange(&mut comp, &mut link, vec![upload(mine, 1, 8, 8)]);
        exchange(&mut comp, &mut link, vec![upload(mine, 2, 8, 8)]);

        let responses = exchange(&mut comp, &mut other, vec![upload(theirs, 1, 8, 8)]);
        assert!(
            matches!(responses[0].body, ResponseBody::Ok),
            "one link's full budget refused another link's first upload: {responses:?}"
        );
    }

    #[test]
    fn dropping_an_image_makes_room_for_the_next_one() {
        // The client-side answer to a refusal, and the reason refusing is not a
        // dead end: a program told it is over budget can free something and try
        // again, which is the exchange an eviction policy would have replaced
        // with a picture that silently stopped appearing.
        let (mut comp, mut link) = wired_with_small_budget();
        let window = open(&mut comp, &mut link, "Viewer");

        exchange(&mut comp, &mut link, vec![upload(window, 1, 8, 8)]);
        exchange(&mut comp, &mut link, vec![upload(window, 2, 8, 8)]);
        let refused = exchange(&mut comp, &mut link, vec![upload(window, 3, 1, 1)]);
        assert!(matches!(refused[0].body, ResponseBody::Error { .. }));

        exchange(
            &mut comp,
            &mut link,
            vec![RequestBody::DropImage {
                window,
                image_id: 2,
            }],
        );
        let accepted = exchange(&mut comp, &mut link, vec![upload(window, 3, 1, 1)]);
        assert!(
            matches!(accepted[0].body, ResponseBody::Ok),
            "the room freed by a drop was not reusable: {accepted:?}"
        );
    }

    #[test]
    fn an_upload_whose_bytes_do_not_cover_its_geometry_is_refused_by_the_importer() {
        // The wire deliberately does *not* check that the stride covers the
        // width or that the bytes cover the rows — `ImageAsset::import` does,
        // and one copy of that arithmetic is the whole argument. This is the
        // test that the request actually reaches it.
        let (mut comp, mut link) = wired();
        let window = open(&mut comp, &mut link, "Liar");

        let responses = exchange(
            &mut comp,
            &mut link,
            vec![RequestBody::UploadImage {
                window,
                image_id: 1,
                width: 64,
                height: 64,
                stride: 256,
                format: BufferFormat::Argb8888,
                bytes: vec![0; 16], // nowhere near 64 rows of 256 bytes
            }],
        );
        assert!(
            matches!(responses[0].body, ResponseBody::Error { .. }),
            "a frame that lied about its size was accepted: {responses:?}"
        );
        assert_eq!(comp.image_count(WindowId::from_raw(window)), Some(0));
    }
}
