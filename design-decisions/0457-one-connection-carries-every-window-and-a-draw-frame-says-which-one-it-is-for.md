## §457 — One connection carries every window, and a draw frame says which one it is for

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A program can have several windows open at once — a text editor
with two documents, a browser with two tabs torn off. All of them talk to the
compositor (the program that actually puts pixels on the screen) down a single
pipe, rather than one pipe per window. That choice created a problem: a picture
travelling down that pipe didn't say which window it was a picture *of*, so the
compositor would have had no way to know where to put it. The fix is a small
envelope around each picture carrying the window's number.

**Context.** `guiremote` had two frame kinds going in opposite directions:
`ORDR` (a `RenderTree` — a list of drawing commands — from client to
compositor) and `INPT` (input events back). Neither names a window: `ORDR` is a
bare tree, and `INPT` names one per *event*. `oswindow`'s `EventLoop` owns a
`Vec<Window>`, so the question "which window did this `ORDR` come from?" has to
have an answer before it can send a second one.

Two prior decisions bear on this. `INPT` and `ORDR` have distinct four-byte
magics specifically so a frame sent the wrong way "fails on its first four
bytes instead of decoding into plausible nonsense". And `SceneFrame` (`SCEN`)
already embeds an `ORDR` per window, under a `SceneWindow` record that carries
the window id.

### Decision 1 — one connection per process, demultiplexed by window id

**Options.**

| | *What changes:* |
|---|---|
| **(1) One transport per process** ✅ | A program opens one connection at startup; every window it later creates rides on it. |
| (2) One transport per window | Each `create_window` returns a fresh channel, and a program with three windows holds three. |

**Chose (1)**, which is also what Wayland and X11 do.

*For (1):* the connection is established once, before any window exists, so
window creation is an ordinary request-and-reply on an existing channel.
Backpressure and connection loss are one thing to reason about rather than *n*.
A `select`-style wait has one thing to wait on.

*For (2):* no demultiplexing at all — a frame's window is implied by which
channel it arrived on, so no id is needed anywhere and a misrouted frame is
impossible by construction. Per-window backpressure is genuinely better: one
window flooding the compositor cannot delay another's input.

*Against (2), decisively:* it requires the compositor to hand a **fresh channel
back inside a reply**, and this protocol has no way to express that. Passing a
capability (an unforgeable handle to a kernel object) through a message is a
kernel-IPC feature the wire format does not have and would have to grow first.
So (2) is not merely costlier — it is not currently expressible.

*Cost accepted:* the demultiplexing has to be written and every message that
concerns a window has to name it. `Connection::pump` sorts incoming frames into
an event queue and a reply table keyed by correlation id, and counts what it
cannot place (`unsolicited_replies`, `misdirected_frames`) rather than dropping
it silently. `EventLoop` counts events for windows it does not know
(`unrouted_events`) — ordinary when a window closes with events in flight, a
compositor bug otherwise, and invisible if they simply vanished.

### Decision 2 — the id goes in a wrapper frame, not in `ORDR`'s header

**Options.**

| | *What changes:* |
|---|---|
| **(1) A new `SURF` frame wrapping `ORDR`** ✅ | A draw frame on the wire is `SURF`[id + `ORDR`[tree]]; 18 bytes of overhead. |
| (2) Add a `window` field to `ORDR`'s header | A draw frame stays one frame, 8 bytes smaller, and every `ORDR` everywhere gains the field. |

**Chose (1).** (2) is fewer bytes and is the wrong shape.

The reason is that `ORDR` is not only a top-level frame. It is nested *inside*
`SceneFrame`, once per window, under a `SceneWindow` that already carries
`id`. Putting a window id in `ORDR`'s header duplicates that field, and two
fields that must agree eventually disagree — at which point every reader has to
decide which one wins, and different readers will decide differently. Keeping
`ORDR` a pure "here is a picture" and letting the *context* name the subject
means there is exactly one id per window per frame, wherever the tree appears.

The 18 bytes are 10 of `SURF` header plus 8 of id, against an `ORDR` payload
that is a whole window's drawing commands. `SURF` also gets its own magic, so
the demultiplexer in `frame.rs` sorts it by the same four-byte test as
everything else, and a compositor reading a stream that mixes control requests
and submissions tells them apart without guessing.

*Reversibility.* Easy. `SURF` is additive: nothing that existed changed, and
removing it would mean deleting one module and one `Frame` variant.

### Decision 3 — a window is told where it moved, rather than assuming

**Options.**

| | *What changes:* |
|---|---|
| **(1) `Event::Moved { x, y }` on the wire** ✅ | `window.position()` reports where the window *is*. |
| (2) Cache the requested position in `set_position` | `window.position()` reports where the window last *asked* to be. |

**Chose (1).** (2) is a lie-by-cache. The compositor may snap a window to a
screen edge, clamp it to a monitor, or rearrange everything when a display is
unplugged — and a user dragging the title bar moves a window the client never
asked to move at all. Under (2) a menu, a tooltip, or a remembered window
position would all be computed against a stale answer, and nothing would
detect it.

The cost was measured before it was paid rather than guessed: grepping for an
existing variant (`Event::ScaleChanged`) found four matches, all in
`gui/remote/src/input.rs`. Only the codec matches `Event` exhaustively — 500-odd
other sites construct events or match a variant or two — so a new variant is a
codec change and nothing else. It came to about twenty lines, and `guitk`'s 759
tests confirmed it after the fact.

`WindowHandle::set_position` therefore sends the request and does *not* touch
the local record; the new position arrives as an event, like any other. Same
for `set_size`. `set_title` and `set_visible` do update the record, but only
after the compositor acknowledges — a refusal leaves the old value, which a
test pins.

### Decision 4 — property changes wait for acknowledgement; frames do not

`WindowHandle`'s property setters block until the compositor answers.
Fire-and-forget would let a refusal pass unnoticed and leave the local record
disagreeing with the screen with nothing able to detect the divergence. The
cost is a round trip on operations a program performs a handful of times.

`EventLoop::submit` does *not* wait, because that is the per-frame path and a
round trip per frame would tie the client's frame rate to the compositor's
scheduling latency.

**Reversibility.** Easy for 2–4, moderate for 1: moving to a channel per window
would change `Connection`'s shape, though the window ids it carries would remain
correct and could simply become redundant.
