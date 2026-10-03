## TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR (lane C, 2026-08-17) -- FIXED 2026-09-13

*(Filed 2026-08-17 as `TD-EDITOR-HAS-NO-INPUT-LOOP`, and renamed the same day.
The original entry blamed `apps/editor` for a gap that turned out to be
tree-wide, and claimed `apps/markdowneditor` had a working event loop to copy
from — it does not; its `main()` says `// In a real application, this would
enter the event loop`. Both errors are corrected below. `roadmap.md` still
refers to the old name at ~967.)*

**What.** No application in this tree is connected to the compositor, and the
protocol they would connect over has no input direction. Three separate
findings, each verified:

1. **`guiremote` is one-directional.** Its entire public API is
   `encode_frame`, `encode_frame_to_vec`, `decode_frame`, `try_decode_frame`
   (`gui/remote/src/lib.rs:352`, `:377`, `:747`, `:756`) — `RenderTree` → bytes
   and back, and nothing else. There is no wire encoding for a key press, a
   mouse move, a resize, or a focus change. Input cannot travel over it because
   there is nothing to travel *as*.
2. **Nothing depends on it but the compositor.** `guiremote` appears in exactly
   one `Cargo.toml` besides its own: `gui/compositor/Cargo.toml:8`. Of the 142
   crates in `apps/`, 138 depend on `guitk` — so they build `RenderTree`s — and
   **zero** depend on `guiremote`, so none of those trees can reach a screen.
3. **The compositor's input routing is complete and then discarded.** It
   hit-tests, tracks focus, and builds properly addressed per-window events —
   `handle_key` (`gui/compositor/src/main.rs:3129`) queues an
   `EventNotification::KeyEvent { window_id, scancode, pressed, character }`,
   the mouse handlers queue `MouseEvent`s with window-local coordinates
   (`:2995`, `:3021`, `:3088`, `:3103`, `:3119`), and focus/resize/close
   changes queue their own (`:2521`, `:2768`, `:2785`, `:3045`). Then
   `main()`'s stub loop drains them into a `for` body whose entire content is
   `// In production: send via IPC channel to the owning client.` (`:4305`).
   The only other caller of `drain_notifications` is a test (`:4592`).

So the two halves exist and the seam between them does not: the compositor
produces events it cannot deliver, and 138 apps produce frames they cannot
send.

**How it was found.** Wiring horizontal auto-scroll for
`TD-EDITOR-IS-NOT-BIDIRECTIONAL` step (d) needed a caller, and there was none.
`Document::ensure_cursor_visible` turned out to have *zero* callers and to have
had none since it was written: vertical auto-scroll is written, tested by
nobody, and never runs. `EditorState::ensure_caret_visible_horizontally`, added
by step (d), is in exactly the same position — correct, tested, and uncalled.
Looking for another app to copy a loop from is what turned a one-app problem
into this one: there was no app to copy from.

**Why this matters more than it looks.** Two uncalled scroll functions is the
visible symptom; the real cost is that *nothing* anywhere is exercised end to
end. A model-level test can assert that `ensure_cursor_visible` moves
`scroll_line`, but not that anything ever asks it to — and that is precisely
the kind of defect that survives a green test suite. Multiply that by 138
crates and the exposure is the whole userland: every one of them is written
against an imagined caller, and the interface each imagined may differ. The
longer that goes on, the more of the tree has to be revised when a real caller
finally arrives — which is the argument for building the seam *now*, while the
cost of a mismatch is small.

**The proper fix**, in the order the dependencies force:

- **(a) Give `guiremote` an input direction.** — **DONE 2026-08-17.**
  `gui/remote/src/input.rs`: an `INPT`-magic frame carrying batched
  `InputEvent`s, each a `guitk::event::Event` plus its addressee window. Its own
  magic, so a frame sent the wrong way over a duplex transport fails on its
  first four bytes rather than decoding into plausible nonsense. 23 tests,
  including an exhaustive sweep of all 88 named keys, all 16 modifier
  combinations, and every mouse kind × button — and a corruption sweep that
  flips every byte of a frame in turn to confirm the decoder never panics on
  input from another process.
- **(b) Decide what a key looks like on the wire.** — **DECIDED 2026-08-17,
  `design-decisions.md` §456.** The compositor translates scancode→`Key`
  centrally, so one system keymap governs every app, *and* forwards the raw
  scancode so games and remappers can still see physical key positions. The
  scancode rides on the wire event rather than on `guitk::event::KeyEvent`,
  which would have obliged all ~539 `KeyEvent` construction sites in the tree to
  invent one.
- **(c) Give the compositor the keymap and the modifier state** that (b) assigns
  it. — **DONE 2026-08-17.** `gui/compositor/src/keymap.rs`: an 88-key
  scan-code-set-1 table (extended keys keeping their `0xE0` prefix, so Left
  arrow stays distinct from keypad 4) and a `ModifierState` tracking each
  modifier *per side*, so releasing one Shift while the other is held does not
  drop the modifier mid-capital. Caps Lock is a latch that cancels with Shift
  rather than combining. `handle_key` folds the state *before* building the
  notification, so `Ctrl+S` arrives as a chord rather than one event behind.
  `Compositor::drain_input_frame` encodes the pending notifications as a
  `guiremote` input frame. 21 tests, including compositor→wire→client round
  trips for a key press, a chord, the arrow keys and a window-local mouse click.
  **Still open under this item:** the transport itself — each window needs a
  channel to its owning process, and until that exists `main()` builds the frame
  and drops it. Also `TD-ONLY-ONE-KEYBOARD-LAYOUT`: the structure supports
  layouts, and exactly one table occupies it.
- **(d) Write the client-side loop once,** not once per app: read events,
  dispatch to a handler, push a `RenderTree` per frame. 138 hand-written loops
  is 138 chances to get the seam subtly different. — **DONE 2026-08-17.**
  `gui\remote\src\client.rs`. Note the correction to this step's own text: it
  said "in `guitk`", which is impossible — the loop needs both directions of the
  `guiremote` wire format and `guiremote` already depends on `guitk`, so putting
  it in `guitk` closes a dependency cycle. A third crate was the other option
  and is not available to lane C, which may not edit the workspace-root
  `Cargo.toml`. So it lives in `guiremote`.

  An app implements `App` (`on_event` → `Idle`/`Redraw`/`Exit`, plus `render`);
  the transport is a second trait, so the loop is testable without a kernel
  channel and can later sit on a channel, a socket or a pipe unchanged. The
  behaviours worth naming, each pinned by a test: a batch of events produces
  **at most one** frame (ten mouse moves are not ten frames); a `Resize` is
  applied *before* the frame that answers it, and the very first paint folds
  into the first batch for that same reason — painting first would draw the
  window at a size it does not have; a `CloseRequested` the app ignores still
  closes, because a close button that does nothing is worse than an app that
  wanted to stay; an event addressed to another window is counted, not
  dispatched. 19 tests.
- **(d½) Fold `gui/window` (`oswindow`) onto the protocol — found while starting
  (e), and it changes (e).** — **DONE 2026-08-17.** There is already a crate whose stated job is
  "compositor client for creating windows and receiving events", 1254 lines of
  it, and **nothing in the tree depends on it** — the same count as `guiremote`
  had before this work. It is not a client; it is a simulation of one:

  * `Connection::send` calls `simulate_response()`, which pops the request it
    just pushed and fabricates the reply. Nothing is serialised and no
    transport is involved.
  * `WindowBuilder::build` gets its window id from a local
    `allocate_window_id()` counter, so two processes would confidently use the
    same id.
  * `EventLoop::run` — the blocking loop an app is supposed to live in — ends
    with `// For now, break to avoid infinite loops in tests` and `break`. It
    runs zero or one iterations.
  * It declares a *third* event vocabulary, `oswindow::WindowEvent`, parallel
    to `guitk::event::Event` and the wire `InputEvent`. Three enums for one
    concept is three translation tables to keep in step.

  This is the band-aid pattern `CLAUDE.md` names: the shape of the right
  design with the substance stubbed, and the stub is what everything would be
  written against. The fix is not to leave it beside `guiremote::client` as a
  second answer — it is to make `oswindow` the app-facing crate *implemented
  on* `guiremote`: `guitk::event::Event` as the only vocabulary, real frames
  on a real `Transport`, and any simulation confined to a loopback transport
  that is named as one.

  **It also exposes a piece this plan never listed.** (a)–(d) cover two
  directions: render frames out, input frames back. A window cannot be
  *created* over either. `oswindow`'s `CompositorRequest`/`CompositorResponse`
  are exactly that missing third message set — create/destroy window, set
  title, resize, minimise, set cursor, and the replies — and they have no wire
  encoding at all. So there is a **(c½): give `guiremote` a control frame**,
  and the count of what is missing between here and a usable desktop was one
  larger than this entry claimed.

  **What landed.** `gui/window/src/lib.rs` is now 1200 lines of client rather
  than 1254 of simulation. `WindowEvent` is gone; `guitk::event::Event` is the
  only vocabulary end to end, pinned by a test that sends a mouse-scroll event
  from the server side and asserts the handler receives the identical value.
  Window ids come from the compositor — the test that would have caught the
  old defect sets the fake compositor's counter to `0x00C0_FFEE` and asserts
  that is the id returned. `EventLoop::run` blocks on `Transport::wait` and
  ends only when the handler says so, `quit()` is called, or the connection
  closes. 23 tests.

  Three pieces the rewrite forced, each a design choice in its own right:

  * **`SURF`, an addressed draw frame** (`gui/remote/src/submit.rs`). Designing
    `Connection` for more than one window raised "which window is this `ORDR`
    for?", and `ORDR` has no answer — it is a bare `RenderTree`. Adding an id
    to its header would have been fewer bytes and the wrong shape: `ORDR` is
    also nested *inside* a `SceneFrame`, under a `SceneWindow` that already
    names the window, so the id would be duplicated, and two fields that must
    agree eventually disagree. `SURF` wraps `ORDR` with the id instead.
  * **`Event::Moved`** (`gui/toolkit/src/event.rs`). A window could otherwise
    only know where it last *asked* to be. The alternative — caching the
    requested position — is a lie-by-cache: the compositor may snap the window
    to an edge or clamp it to a monitor, and anything placed in screen
    coordinates would then be placed against a position the window never had.
    Cost measured before writing it: only the codec matches `Event`
    exhaustively (4 sites, all in `input.rs`), so the variant was ~20 lines.
  * **`guiremote::loopback::Pipe`**, an in-process duplex pipe implementing
    `Transport`. Both halves speak the real wire protocol; the only thing
    missing versus a socket is the kernel. It is explicitly *not* a substitute
    for a transport — `wait` does nothing, because on one thread nothing can
    arrive while a caller is blocked, and `Rc` rather than `Arc` says so in the
    type system. `oswindow`'s tests wrap it in a transport whose `wait` gives
    the fake compositor a turn, which is what blocking *means* with one thread,
    and that is what lets them exercise the genuinely blocking paths
    (`create`, `set_title`, `run`) rather than reaching around them.

- **(f) Give the compositor a wire front end — found while finishing (d½), and
  it blocks (e).** — **DONE 2026-08-17**, in the three parts described at the
  end of this item. The compositor has `handle_request(CompositorRequest) ->
  CompositorResponse` and `drain_notifications()`, and **nothing that decodes
  or encodes a frame on the request path**: no `CREQ` → `CompositorRequest`, no
  `CompositorResponse` → `CRSP`, no `SURF` → `submit_render`. `drain_input_frame`
  (added by (c)) is the one piece that exists, and `main()` drops what it
  builds. So the client half of the protocol is now complete and has nothing to
  talk to: `oswindow`'s tests pass against a fake compositor written in their
  own test module, and there is no way to point one at the real thing.

  The work is a translation layer, and its shape is fixed by what already
  exists on both sides — but it is not mechanical, because the two enums do not
  correspond one-to-one. `CompositorRequest` has `SetFullscreen`, `SetOpacity`
  and the three `Stream*` variants that `RequestBody` does not; `RequestBody`
  has `SetVisible` that `CompositorRequest` does not; `CompositorRequest::CreateWindow`
  carries a `client_pid` and only title/width/height, where `WindowSpec` also
  carries position, resizability, decorations, transparency and size limits —
  which the compositor would currently discard. Deciding whether to widen
  `CompositorRequest`, widen `RequestBody`, or accept a lossy edge is the
  substance of this step.

  There is also a **third cursor enum** to delete on the way past:
  `compositor::CursorShape`, `guiremote::control::CursorShape` and the one
  `oswindow` used to have cannot represent the same set — `Help` existed only
  in `oswindow`'s, `NotAllowed` only in the compositor's. `oswindow`'s is gone;
  the compositor's should become `guiremote`'s.

  **What landed, in the order the work forced.** The question this step posed —
  widen `CompositorRequest`, widen `RequestBody`, or accept a lossy edge — was
  answered *both*: each side gained what the other had, because a lossy edge
  here means a client asking for an undecorated window and silently getting a
  title bar, and a defect that only shows up as pixels is the worst kind to
  leave in a protocol.

  * **(f1) The compositor honours the whole `WindowSpec`** (commit
    `83d408c7d`). It previously stored title, width and height and dropped
    position, decorations, resizability, transparency and both size limits.
    Storing them was the small half; the real defect was that
    `TITLE_BAR_HEIGHT`/`BORDER_WIDTH` were read *directly* in six `Window`
    helpers, in `detect_border_drag`, in `maximize_window` and again in
    `render_title_bar`, so "undecorated" could only ever have been honoured
    where someone remembered to check. They are now behind
    `Window::frame_insets()`/`shadow_extent()`, which return zeroes for an
    undecorated or fullscreen window, and every consumer derives from those —
    so hit testing, damage, drag detection and painting agree by construction.
    Button rectangles became `Option<Rect>` (an empty rect contains no point
    only by accident of arithmetic) and are placed by *slot*, so a
    non-resizable window's minimise button moves up into the missing maximise
    button's place rather than leaving a dead patch of title bar.
    `clamp_size` puts a hard 100×50 floor beneath any client minimum and
    resolves a contradictory `max < min` in favour of the minimum, so a client
    cannot strand a window too small to grab. `transparent` and `opacity` are
    kept distinct: the first skips the opaque white client-area undercoat that
    would otherwise defeat the request, the second fades the whole window
    including its frame. 13 tests.
  * **(f2) One `CursorShape`, stored per window** (commit `f4f2db88d`). The
    compositor's enum is deleted in favour of the wire's, which fixes the
    `Help`/`NotAllowed` mismatch above, and the shape is stored on the
    `Window`. This found a live bug: `handle_request`'s `SetCursor` arm was
    `SetCursor { cursor, .. }` — it discarded the window id and wrote the
    *global* cursor, so any client could repaint the desktop's cursor from
    anywhere, including while the pointer was over another application.
    `cursor_at(x, y)` now resolves it in the same order the user perceives:
    a resize border wins, then the client area's own shape, then the frame's
    arrow.
  * **(f3) The front end itself** — `gui/compositor/src/wire.rs`, plus
    `RequestBody::SetFullscreen`/`SetOpacity` and
    `CompositorRequest::SetVisible` to close the last gaps between the two
    enums. `Compositor::serve(&mut ClientLink)` decodes whatever a client has
    sent — `CREQ` and `SURF` interleaved on one connection, in any framing,
    including one byte per read — dispatches it, and queues the replies;
    `route_input` writes each pending event to the link that owns its window.
    The design decision that carried the most weight is in
    `design-decisions.md` §458: **a link owns the windows opened over it**, and
    that set both routes input and authorises every request naming a window.
    15 tests here; 31 across the three parts, and the compositor suite went
    from 105 to 136.

- **(e) Wire `apps/editor` to it** as the first real client, calling
  `ensure_cursor_visible` and `ensure_caret_visible_horizontally` after any
  cursor movement. Its demo `main()` should become an example or a test, not be
  deleted — it is a compact tour of the model's API. **Amended by (d½):** wire
  it to `oswindow`, not to `guiremote::Client` directly — an app should not
  name the wire format any more than a Unix program names the socket layer —
  and do (c½) and (d½) first so the editor is not rewired twice.
  — **DONE 2026-08-17** (commit `7f8c7b514`).

  **What landed.** `apps/editor/src/input.rs`, ~800 lines and 31 tests: the
  editor previously had *no* input handling at all, so this is the whole layer
  rather than a rewiring of one. Its organising rule is that nothing in it
  moves the caret directly — every motion goes through `EditorState::moving`,
  which settles the selection anchor and then calls **both**
  `ensure_cursor_visible` and `ensure_caret_visible_horizontally`. That is what
  this step asked for, and routing it through one function rather than
  appending two calls to each binding is what stops the thirty-second binding
  from forgetting them. Both functions now have callers for the first time
  since they were written, which closes the specific symptom that opened this
  entry.

  Three input modes are checked in order — external-change prompt, find bar,
  document — and the find bar returns an `Option` rather than a bool, so a
  chord it does not claim (`Ctrl+S`) falls through to the document's bindings
  instead of being swallowed by the bar that happens to have focus.

  `main` became `run(&mut EventLoop<T>, window, &mut EditorState)`, generic
  over `oswindow::ConnectionTransport` and therefore testable end to end
  against `oswindow::testing` — which is the point of the step, since a model
  test can assert a scroll function *works* but not that anything ever calls
  it. `mod loop_tests` drives the editor through the real protocol and asserts
  the one thing `run` adds over `handle_event`: *when* a frame is sent. A
  no-op mouse move, a keystroke and a close produce **two** frames — the
  initial paint and the keystroke's — not one per event.

  The demo `main()` is preserved as `mod api_tour` with its prints turned into
  assertions, as this step required.

  **`connect()` returns `None`.** There is still no transport, so the binary
  prints a four-line diagnostic naming this entry and exits 1. It is a
  function rather than an `unimplemented!` main precisely so everything above
  it is real, compiled code: the day it returns a socket, the editor works
  with no other change in `apps/editor`.

  **One piece this step forced.** `oswindow`'s compositor stand-in was
  `#[cfg(test)]`-private, so no other crate could test an event loop at all —
  an app cannot obtain a window id without a compositor answering
  `CreateWindow`, and (d½) deliberately removed the locally-minted ids that
  used to make that possible. It is now a public `oswindow::testing`, compiled
  unconditionally, for the same reason `guiremote::loopback` is: a per-app copy
  of the fake is a fresh chance to get the wire format wrong in a way that
  makes the *test* pass. Promoting it also exposed a defect in it — `serve()`
  consumed submissions while decoding, so `drawn()` only ever reported frames
  sent since the last turn, which is useless for an event-loop test where
  taking turns is how the loop makes progress. Submissions now accumulate in
  `absorb()`.

  **What (e) does not close.** The editor cannot yet run: `connect()` has
  nothing to return. Both halves of the protocol are complete and there is no
  channel between them — the compositor's `main()` has no listener and no
  client has an address to dial. That is the next step and is now the only
  thing between this tree and a window on a screen. Two smaller gaps found on
  the way: `guitk::event::MouseEvent` carries no modifiers, so shift-click is
  recognised from state recorded on the last `KeyEvent` (correct, but it means
  a mouse event that arrives before any key event cannot know the shift
  state); and Save As needs a file dialog, which does not exist, so an unnamed
  buffer reports "No file name — Save As needs a file dialog" and cannot be
  saved.

- **(g) Put a channel between the two ends** — the step (e) named as "the only
  thing between this tree and a window on a screen". — **DONE 2026-08-17**
  (commits `675dfc72c`, `4094767c1`, `f81aaec1b`).

  Both halves of the protocol were finished and there was nothing to carry the
  bytes: the compositor's `main()` had no listener, and no client had an address
  to dial. Three pieces closed it.

  * **A transport** (`gui/remote/src/socket.rs`). A TCP socket implementing
    `guiremote::client::Transport`, plus the `Listener` the compositor accepts
    on. The choice of TCP over a Unix socket or a named pipe is
    `design-decisions.md` §460; the short version is that this crate's first
    line calls itself a *remote* display protocol, and a carrier that cannot
    leave the machine removes a capability on purpose. `SLATE_DISPLAY` names the
    display and defaults to `127.0.0.1:7373` — loopback, so a default install is
    not listening to the network. The whole of the awkwardness is in reconciling
    two contradictory obligations on one socket: `read` **must not** block and
    `wait` **must**. It is held non-blocking, and `wait` switches to blocking,
    peeks one byte, and restores the mode *on every path including the failing
    one* — which has its own test, because forgetting the failing path leaves a
    socket whose next `read` blocks the whole application. 14 tests.
  * **A front end** (`gui/compositor/src/server.rs`). `Server` binds the
    listener, gives each connection its own `ClientLink` and identity, serves,
    routes input, and paces frames on the display's refresh interval. A departed
    client's windows are destroyed on reap, so a crash cannot leave a window
    nobody can close; a client that speaks nonsense is dropped without
    disturbing the others. Two limits found writing it are logged separately:
    `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING` (the standard library has no
    readiness primitive, so the loop polls) and `TD-COMPOSITOR-HAS-NO-SCANOUT`
    (the composited frame is finished and then dropped, because nothing here
    owns a framebuffer).
  * **A dialer** (`oswindow::connect`, `connect_to`, `Link`), and the editor's
    `connect()` replaced by it. An application still never names TCP.

  **What this step forced, and it is the part worth remembering.**
  `gui/compositor` was a binary with no library, so *nothing outside its own
  source file could link it*. Every test that existed put the shipped code on
  **one** end of the protocol and a stand-in on the other — the editor against
  `oswindow::testing`, the compositor against a hand-written client in its own
  test module — and two stand-ins can both be satisfied while the two real
  halves disagree. That is exactly the class of defect this whole entry is
  about, reproduced one level up. The compositor is now a library plus a thin
  binary, and `apps/editor`'s `mod against_the_real_compositor` starts a genuine
  `Server` on a thread and an ephemeral port and drives the editor into it: a
  window is created and its id comes back over the socket, the editor's real
  render tree crosses the wire and is composited, a scancode injected at the
  compositor arrives as a character in the document, and the window is reclaimed
  when the client goes away.

  It found something immediately, which is the argument for having written it:
  `ServerStats::frames` was incremented inline in `run()`, so any *other* driver
  of the loop reported zero frames composited while the screen was demonstrably
  being drawn. Composition is now `Server::compose`, counted for every caller.

  **What is left between here and a visible desktop:** scanout
  (`TD-COMPOSITOR-HAS-NO-SCANOUT`) and an input source. The hosted build has no
  keyboard or mouse driver feeding `Compositor::handle_input`, so outside a test
  that injects them, `route_input` has nothing to route.

  *(Update 2026-08-21: scanout is done — `TD-COMPOSITOR-HAS-NO-SCANOUT` is
  closed and `gui/compositor/src/present/drm.rs` page-flips composited frames
  onto a real display. The input half of that sentence still stands, and is now
  its own entry: `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`.)*

**Severity.** High as a *blocker* — it is the single gap between "142 app
crates" and "an OS with any usable application at all". Low as a *defect*:
nothing that exists is wrong, it is only unreachable, so the fix is additive
and nothing has to be unwound first — with the one exception found in (d½),
where `oswindow` is not merely unreachable but actively misleading, and does
have to be unwound.

**Status 2026-08-17: the transport chain is closed.** (a)–(g) are done, and one
application really opens a window on a real compositor and receives a keystroke
from it. This entry stays open for the remaining 137 apps, none of which is
wired yet, and because a window whose pixels never reach a screen is not yet a
usable desktop — `TD-COMPOSITOR-HAS-NO-SCANOUT` is now the last link in the
chain from an app's `RenderTree` to a photon.

*(Update 2026-08-21: that last link is closed. The chain from a `RenderTree` to
a photon is complete on SlateOS. This entry stays open for the remaining 137
unwired applications, which is now the only thing it is about.)*


**One of the 137 now has something specific it cannot call (2026-08-21).**
`apps/settings` is the only application in the tree that edits the user's
appearance settings, and the compositor now accepts a `ReloadAppearance`
request so that a change to window corners or drop shadows reaches windows that
are already open (`design-decisions.md` §500). The sender API exists —
`oswindow::EventLoop::appearance_changed()` — and Settings cannot call it,
because it is one of the unwired 137: its `Cargo.toml` names only `guitk` and
`appearance`, and its `main()` says *"In a real Slate OS environment, this
would enter the compositor event loop. For now, render one frame to verify the
UI builds correctly."*

So the live-reload path is complete on the compositor's side and has no caller.
That is recorded here rather than papered over: opening a socket inside a
program with no event loop to service it would put a connection in a process
that cannot answer anything that arrives on it, which is the `oswindow`
simulation mistake of (d½) in a new place. Until Settings is wired, changing an
appearance setting still requires restarting the compositor, and the fix is
step (e) applied to Settings — an `oswindow` event loop, a window, and one
`appearance_changed()` call after `AppearanceFile::save()` in
`save_appearance` (`apps/settings/src/main.rs`, ~line 880).

**Update 2026-08-22 — Settings is wired; 136 to go.** `apps/settings` now has a
real `main()` that dials the compositor with `oswindow::connect()`/`connect_to()`,
a real window, and a real event loop (`run`), and the `appearance_changed()`
call this paragraph asked for is in place — with one correction to the recipe
above. It is *not* "one `appearance_changed()` call after `AppearanceFile::save()`
in `save_appearance`": `save_appearance` has no access to the event loop, and
putting the notification there would either thread the loop into the model or
notify from a function that cannot report a transport failure. Instead
`save_appearance` sets a private `appearance_dirty` flag and the loop drains it
with `take_appearance_change()`, which also makes the notification track *the
file changing* rather than *an event being consumed* — the two are not the same,
and both mistakes are pinned by tests. `design-decisions.md` §523 has the
reasoning.

Fifteen new tests, all proved regression tests by reintroducing the defect each
names (eighteen markers, every test earned at least one). Four of them are
`mod against_the_real_compositor`, which puts the shipped `Compositor` on one end
of a real socket and Settings' shipped `run` on the other: a physical mouse click
at a theme card changes the appearance the compositor draws with, end to end,
with no stand-in on either side.

Settings was the application this entry singled out because it had something
specific it could not call. That is closed. The entry stays open for the other
136 unwired applications.

**Update 2026-08-25 — the strap is written once, and the recipe below is the
whole of what converting an app now costs.**

Three applications had by then been wired — editor, Settings, and (as of this
update) metronome — and each had its own hand-written `run`. That is the failure
step (d) named at the top of this entry, arriving from the other direction: the
loop was written once *for the tree* and then written again, per app, because
nothing had extracted the parts that are identical in all of them. Three copies
is where the divergence starts being cheap to fix; 136 is where it is not.

`gui/window/src/app.rs` (`oswindow::app`) now holds it: an `App` trait
(`title`, `initial_size`, `resizable`, `tick_interval`, `on_event`, `render`,
the first three and the fourth defaulted), `open`, `drive`, `Args`, and
`launch`/`launch_with`. Editor and metronome are on it; Settings is not yet.

**The recipe.** For an ordinary single-window application:

1. Add `oswindow = { path = "../../gui/window" }` to its `Cargo.toml`. An app
   names `oswindow` and never `guiremote`, for the reason a Unix program does
   not name the socket layer.
2. Delete `#![allow(dead_code)]` if it has one. It is almost always there
   because the app was never wired, and it is what hides the fact — see
   `lesson 46`. Expect to delete genuinely dead code once the lint works again,
   and expect some of it to be *test-only* dead code, which is lesson 45 exactly
   and is invisible unless you compare `cargo build` against `cargo test`.
3. `impl oswindow::app::App` for the app's state type.
   - If it has an inherent `render`, **rename it** (`render_commands`,
     `render_tree`). At equal arity an inherent method silently wins method
     lookup over the trait's and every existing call keeps compiling while
     testing the other function.
   - **`tick_interval` is the one to get right.** Anything the app ages — a
     beat, a blink, a countdown, a toast — needs it, and the default `None`
     ships the feature frozen with its tests still passing (`lesson 47`).
     Return `None` only when the app genuinely ages nothing, and gate it on
     what is actually moving so an idle desktop can park.
   - Do **not** answer `Redraw` to `Event::Resize`/`ScaleChanged`; `drive` takes
     that decision for every app, because 137 apps each remembering to is 137
     chances to forget.
   - If the app's renderer reads its own stored window size, have `App::render`
     reconcile it with the size it is *handed* — a compositor may grant a size
     that was never requested, and the first frame is drawn before any event.
4. `fn main() -> ExitCode` becomes one line, `app::launch("name", &mut state)`
   — or, for an app with arguments of its own, `Args::from_env()` +
   `launch_with(name, args.display.as_deref(), &mut state)`, because `launch`
   rejects unexpected arguments with exit 2.
5. Delete the app's own display parsing, dialer, diagnostic and window build.
   All four are in `oswindow::app` and were identical in every copy.
6. Run `python scripts/check-tick-wiring.py` and
   `python scripts/scan-unwired.py`. The second reports reach per binary; a
   conversion should raise it, and if it *falls*, suspect the scanner's
   `ENTRY_POINTS` list before suspecting the app (that list has already lagged
   the code once).

**What the extraction cost, which is the argument for having done it before app
four rather than after app forty.** `guiremote::client::Client` — step (d)'s
original home for the loop — was retired in the same work. It had been
superseded by `oswindow::EventLoop` within a week of being written and had sat
since with **no production caller of its own**, which is this entry's own defect
arriving inside its own cure. It also structurally could not tick: no wake-up
list, so it parked in `Transport::wait` until the compositor spoke, and any app
built on it would have reproduced lesson 47 through no fault of its own.

Two live defects surfaced in the two conversions, both of them in `main` and
both invisible until `main` had something in it worth testing: `apps/metronome`
never advanced (its `Event::Tick` fell into a `_ => {}` arm), and
`apps/editor` showed a stray blank tab beside every file named on its command
line. That is the yield to expect per app, and it is the reason the conversions
are worth doing individually rather than by a mechanical sweep.

**Update 2026-08-25 (later) — Settings converted; the harness grew the one thing
an application can need that a loop cannot guess.**

Settings was the third and last hand-written strap, and it is the reason step 3
of the recipe now has a fifth bullet. It edits `appearance.yaml` and
`input.yaml`, which the *compositor* reads — window corners, drop shadows,
whether two clicks are one double click — so it must tell the compositor when it
has rewritten one. Its own loop did that; `drive` had no way to express it, and
converting Settings without adding one would have been a silent downgrade
producing exactly the defect this entry is about: a setting that appears to work
and does not, its own preview updating while every real window keeps its old
corners until the next login.

`App::take_reloads` is the hook, drained after every event and turned into the
`ReloadAppearance`/`ReloadInput` requests `EventLoop` already had. Two flags,
because they become two requests. **Per event and not once per batch**, which is
the one placement decision here with a wrong answer: a batch ending in
`CloseRequested` never reaches `Dispatch::Settled`, so news held for the boundary
is lost exactly when a user makes their last change and closes the window.

Add to step 3 of the recipe:

  - If the app writes a file another process reads, implement `take_reloads` —
    and make it *drain*, not peek, or the compositor re-reads the file on every
    event for the rest of the session.

Two further notes from this conversion, both worth having before the next app:

- **An app whose handlers speak `guitk::EventResult` should map at the seam, not
  convert the handlers.** Settings' internal handlers use `Consumed`/`Ignored`
  for what it genuinely means — whether an event keeps propagating up a widget
  tree — so the four-line mapping in `on_event` is right and pushing `Response`
  down into the hierarchy would be wrong. The mapping is imprecise in the
  harmless direction only (a click on an already-selected item repaints an
  identical frame) and never in the harmful one.
- **A claim in a commit message is a claim to falsify like any other.** This
  conversion deleted Settings' `split_args` tests on the grounds that `launch`
  now carried the rule. It did not — `launch` needs a socket, so nothing in it
  had ever been executed by a test. Fixed by extracting `leftover_complaint`,
  which is the only part of `launch` that can be tested without a compositor.

**Count: 135 to go.** All three applications that open a window — editor,
Settings, metronome — are now on `oswindow::app`, so there is no fourth copy of
the strap to retire and the remaining work is the placeholder `main`s. The next
one is a judgement call rather than a forced move: pick an app whose state type
already exists and already has a `handle_event`, since that is most of the work,
and expect roughly one live defect per app (three conversions, three defects).

**Count corrected 2026-09-03 — and moved somewhere it cannot go stale again.**
The line above said "135 to go". It was written on 2026-08-25 and never revised
while the conversions continued; by 2026-09-03 the true figure was 55, and three
conversions that same day took it to 52. A number maintained by hand in prose is
a number that is wrong most of the time, which is the same defect as the
hand-written palettes this file is full of, one medium over.

**Do not read a count from this entry. Ask the ratchet.**
`scripts/check-window-wiring.py` already measures it on every boot test and
prints it: "N program(s) open a window, M do not". `BASELINE` in that file is
the current M, and it is lowered in the same commit as each conversion — so it
is both the count and the thing that stops the count going back up. As of
2026-09-03 it is 46, against 92 programs that do open a window.

(The two figures do not have to sum to 143. `check-window-wiring` counts
*programs that draw*, which is not the same set as crates under `apps/`, and it
is the more useful denominator: a crate with no renderer has no window to open.)

**`apps/fontmanager` is number 88, and the "one live defect per app" rule held.**
The defect: **there was no `Event::Mouse` arm anywhere in the file.** A font
manager whose list of fonts could not be clicked — only arrowed through.
`MouseEvent`, `MouseButton` and `MouseEventKind` were imported and never named
again, and a file-wide `#[allow(unused_imports)]` is what kept that quiet. It
was invisible for the reason this entry exists: `main` rendered one frame and
asserted it was non-empty, so nothing had ever delivered the app an event.

Mouse handling now selects a sidebar filter, a category, or a font family. The
row geometry was inline in the renderer, so it was extracted to
`sidebar_rows()`/`font_list_top()` which the renderer and the hit test both
walk — hand-writing a second copy of the arithmetic is how `apps/mixer` got a
slider handler taking arguments nothing in the program computed. Seven tests,
including a sweep asserting *every* drawn row is clickable (an off-by-one in the
accumulator would strand exactly one row, which sampling two would miss) and one
asserting a click above or below the list selects nothing, since a hit test that
clamps looks like the program choosing for you. Mutation-checked: deleting the
`Event::Mouse` arm fails exactly the four click tests.

Three other things the conversion turned up, all pre-existing:

- **Two latent panic sites in production code.** `select_next_font` and
  `select_prev_font` indexed `visible[0]`, `visible[next_pos]` and computed
  `visible.len() - 1`; `uninstall` indexed `self.fonts[idx]`. Each is in range
  today because of a check a few lines above it, which is the guarantee that
  stops holding the moment someone moves the check. All are now `get`/`first`/
  `last`/`checked_sub`.
- **The `render()` name collision the recipe warns about, from the other side.**
  Renaming the inherent `render` to `render_tree` made five *test* call sites
  resolve to the trait's `render(&mut self, w, h)` instead — the compiler found
  every one, which is the argument for renaming rather than relying on care.
- **A tail of hidden lint debt**: `float_cmp`, `unwrap_used` and
  `indexing_slicing` in the test module (now carrying the standard allow block),
  and three `sort()` on primitives (now `sort_unstable`).

`check-window-wiring.py`'s baseline went 49 → 48 in the same commit, which is
what a ratchet is for: ground gained and not held is ground that can be lost
again without anything objecting.

**`apps/procexplorer` is number 89**, and its defect was the other one the
recipe singles out. It already handled mouse, resize *and* `Event::Tick` — so
the conversion was mechanical except for `tick_interval`, which defaults to
`None`, which means no tick is ever delivered. Shipping that default would have
given a **process explorer whose numbers never change**: a system monitor that
monitors nothing, with all 67 of its existing tests still green, because a model
test can assert the refresh works without asserting anything ever asks for it.
That is lesson 47 again, and the recipe calls `tick_interval` "the one to get
right" for exactly this reason.

It returns the *user's* refresh interval rather than a constant or a frame rate,
so changing the setting changes the clock, and an idle desktop parks between
refreshes instead of being woken sixty times a second to redraw the same
numbers. Three tests: the clock follows the setting; the refresh is driven by
*elapsed time* rather than tick count (the interval is a floor, not a promise —
`Event::Tick` carries what actually elapsed); and one long tick after a busy
loop still refreshes. Mutation-checked: returning `None` fails exactly the first.

Pre-existing debt this one exposed was larger than fontmanager's, and in a file
the conversion did not otherwise touch: **31 clippy findings under
`-D warnings`**, across `main.rs` and `features.rs`. Seven were latent
production sites — `(1u64 << count) - 1` in two places, `cpu % cols` and
`cpu / cols`, `scroll_offset + i`, `current + delta` before a clamp that cannot
rescue an addition that already overflowed, and `handles.len() - max_handles`
guarded by a `>` three lines above it. Each is safe today because of a check
somewhere nearby, which is the guarantee that stops holding when someone moves
the check. The rest were the two test modules, which now carry the standard
allow block.

Baseline 48 → 47.

**A note for the next conversion:** budget for the lint tail. Both apps so far
were clippy-dirty under `-D warnings` *before* being touched, and the debt is
not visible until the crate is looked at. It is worth fixing — these are real
overflow and panic sites — but it is most of the work, not a footnote to it.

**Corrected 2026-09-03, after measuring instead of extrapolating.** Three apps
for three looked like a tree-wide condition and I wrote it up as one. It is not:
**114 of the 143 crates are clean**, and the debt is 588 production findings
concentrated in 26. The apps left to convert are the ones nobody has been into
recently, which is the same property that predicts lint debt — a biased sample,
not a tree-wide fact. Full numbers, and the command to re-measure them, are in
`TD-C-APPS-CARRY-1812-CLIPPY-FINDINGS-AND-588-OF-THEM-ARE-IN-PRODUCTION`. The
advice stands; the scale was wrong.

**`apps/musicplayer` is number 90, and it is why that note is there.** Its
conversion was small; its lint tail was **71 findings, every one in production
code**, and the bulk of them were in `parse_wav_header`, `parse_flac_header`,
`parse_id3v2` and `decode_id3_text` — three audio-format parsers reading
untrusted file bytes with unchecked arithmetic on offsets taken *from those
bytes*.

**One of them was a hang, not a lint.** The WAV chunk loop advanced with
`offset += 8 + chunk_size as usize`, and `chunk_size` is four bytes out of the
file. A crafted size wraps `offset` back to a small number, a small offset
passes the loop guard, and the parser reads the same chunks forever. The ID3
extended-header advance (`pos += 4 + ext_size`, a full 32-bit field) has the
identical shape. A media player that hangs on a malformed file is the cheapest
denial of service there is and it arrives as an email attachment.
`a_wav_with_an_absurd_chunk_size_does_not_loop_forever` pins it, and the
mutation check is unusual: restoring the wrapping advance does not fail the
test, it **times out at 120 s**, which is the finding.

Four tests, including a sweep that feeds every truncation of a plausible WAV,
ID3 and FLAC header to all three parsers — a missed bound shows up only at the
length that reaches it, so sampling would not do.

The conversion's own defect was the visualiser: `advance_visualizer` eased the
bars by a flat per-*call* blend, ignoring the `elapsed_secs` it was handed. That
is `apps/mixer`'s peak-meter defect one call site over, and it only becomes
reachable when the app is given a real clock — `tick_interval` is a floor, so
ticks arrive irregularly and a busy frame would ease twice as far as a quiet
one. It is now an exponential rate whose time constant is solved to reproduce
the old look at the rate the player asks for. `tick_interval` is gated on
`playing`, so a paused player lets the desktop park.

Baseline 47 → 46. Running total: three conversions today, three live defects,
and 102 clippy findings cleared across them.

**`apps/paint` is number 91, and its gap was the largest yet: it had no event
handling of any kind.** Not a missing arm — `Event` did not appear once in four
thousand lines, while `on_canvas_press`, `on_canvas_drag`, `on_canvas_release`,
`handle_key_press` and `handle_special_key` all sat there written and tested,
taking arguments nothing in the program computed. A paint program that cannot be
drawn in, whose drawing code was covered by tests that passed canvas
coordinates straight in. That is `apps/mixer`'s slider, four thousand lines
wide.

`handle_event` now routes mouse and keys. The mouse work is the interesting
half: a press must land on the pixel it points at, which means
`window_to_canvas` and `canvas_to_window` — both of which already existed — have
to agree, through the zoom and the scroll.

**And the test that checks it caught nothing until it was mutation-checked.**
The first version drove clicks on a fresh `PaintApp`, where `scroll_x`,
`scroll_y` are 0 and `zoom` is 1 — so every term of the transform is the
identity. Deleting `+ self.scroll_x` from `window_to_canvas` left all 174 tests
green. The test now sweeps three (zoom, scroll) states, and the same mutation
fails exactly one test. Worth recording as a shape rather than an incident: **a
transform tested only at its identity state is not tested.**

Fixing that exposed a second thing worth having: with the canvas scrolled by 37,
canvas pixel 0 is off the left of the viewport and a press there is *correctly*
refused. The strengthened test failed on its own fixture before it failed on the
code, and asks for offsets from the scroll position instead.

Seven tests, including that a press on the chrome is not a stroke, that a bare
move is not a stroke, and that a drag leaving the viewport keeps drawing while
the release is accepted from anywhere — or a mouse-up outside the window leaves
the app permanently mid-stroke.

**Left undone deliberately:** the toolbar, option bar, colour swatches and
layers panel are not clickable. Each computes its geometry inline in its own
render function, so hit-testing them means extracting that geometry first, and a
second hand-written copy of it is the defect `apps/fontmanager` was just fixed
for. Tools are reachable from the keyboard meanwhile. **And paint's 136
arithmetic findings are not fixed here** — they are the largest single pile in
`apps/` and are their own task, per
`TD-C-APPS-CARRY-1812-CLIPPY-FINDINGS-AND-588-OF-THEM-ARE-IN-PRODUCTION`.

Baseline 46 → 45.


**Update 2026-09-13 — the count was 135 and the truth was two.**

`match3` and `pinball` are wired, and with them every application in this tree
that draws reaches a window. The remaining figure in the heading above ("135 to
go") had been stale since the strap landed: each app converted after it stopped
updating the number, and nothing measured the total again.

Measured, with a rule about *position* rather than shape — a `fn main(` at
column 0, because `find("fn main(")` matches the fixture strings that
`archivemanager`, `filediff` and `snippets` keep in their test data, and that
mistake is what first reported eight unwired apps instead of five:

| | count |
|---|---|
| app crates | 143 |
| launch from `main` | **135** |
| have no `main` at all — `diffcore`, `globmatch`, `safeio` | 3 (libraries) |
| command-line tools, correctly not windowed — `backup`, `indexer`, `installer` | 3 |
| **genuinely unwired** — `match3`, `pinball` | **2**, now 0 |

**What wiring a game costs, now that the strap exists:** an `App` impl of five
methods, a `TICK`, a `launch_with` in `main`, and one rename. Both games had an
inherent `render(&self) -> Vec<RenderCommand>`, and inside the `App` impl a
bare `self.render()` resolves to the *trait* method even at a different arity —
the compiler reports two missing arguments rather than calling the inherent
one. Both are now `render_commands`, which is the same collision
`apps/filediff` solved by naming its own method `render_tree`.

**Each game gained two tests, because 120 model tests apiece could not see
this.** None of them would notice an `App` impl that was deleted, returned
`(0, 0)` from `initial_size`, or handed back an empty tree: the program would
compile, launch and show nothing, which is the exact failure this entry is
about. `mod reaches_a_window` opens a window *through the trait* against
`oswindow::testing`, scripts a tick and a close, and asserts frames came out
with commands in them. Proved able to fail: an empty `RenderTree` makes it
report *the game submitted 2 frame(s) for this window and every one was empty,
so the window would be blank* while all 120 model tests keep passing.

**This entry is now closed.** What it asked for — applications connected to the
compositor — is done from `guiremote`'s input direction through to the last
game. What remains in the neighbourhood is separate and separately filed:
`TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING`, `TD-COMPOSITOR-HAS-NO-SCANOUT`, and
the 43 games' theming, which waits on C-Q16.
