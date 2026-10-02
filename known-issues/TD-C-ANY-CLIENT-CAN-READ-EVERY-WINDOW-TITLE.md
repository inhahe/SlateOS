## TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE

**In short:** Any program connected to the compositor can ask to be sent the
list of every window on the desktop — including each window's title. Titles are
usually filenames, URLs or email subject lines, so a program with no business
knowing what you have open can watch all of it, live. Nothing stops it, because
the compositor currently has no way to tell a trusted shell apart from an
ordinary application.

**Where:** `gui/compositor/src/wire.rs` — `ClientLink::answer_requests`
intercepts `RequestBody::SubscribeWindowList` and grants the subscription.

**Corrected 2026-09-13: it is no longer *unconditional*, and the difference
matters for whoever picks this up.** The grant now runs through
`link.require_shell()` — one of sixteen call sites of the single privilege
seam — and refuses with that function's `Err` when it ever returns one. So the
plumbing this entry asks for is in place; what is missing is only the
capability that would make `require_shell` answer. That function's own doc
says as much, and says why a check written today against a client-supplied
value would be worse than none. The remaining work is in the kernel, not here,
and the fix when it lands is a body in `require_shell` rather than an edit at
this call site. `gui/remote/src/control.rs` —
the request itself. `gui/compositor/src/lib.rs` — `Compositor::window_list`,
which returns the whole desktop by design (see below).

**Reproduce:** any `oswindow` client, including `apps/editor`, can call
`EventLoop::watch_desktop(true)` and then read `desktop_windows()`. There is no
check of any kind between the request arriving and the subscription being
granted.

**Why it shipped this way (2026-08-21):** the shell cannot draw a taskbar
without it, and the honest gate does not exist yet. A capability is the right
mechanism — the shell is handed one at launch, an application is not — and
kernel channel IPC does not yet deliver capabilities to the compositor. Any
check written today would test a value the *client* supplies, which is not a
gate but the appearance of one, and worse than none because it would look
solved. Full reasoning in `design-decisions.md` §495.

**Note that the unfiltered list is not the bug.** The compositor deliberately
sends every window in every band including minimized ones, because filtering is
the subscriber's decision — a taskbar and an Alt-Tab switcher want different
subsets, and a compositor that picked one would leave the other unable to
recover what was dropped. Narrowing what is sent is not the fix; deciding *who*
may ask is.

**Proper fix:** one capability check at the single place the subscription is
granted — the `SubscribeWindowList` arm in `answer_requests` — refusing with
`ResponseBody::Error` when the link holds no window-list capability. The
compositor learns the caller's capabilities from the kernel at connection
accept, not from the connection's own claims. One site, because the
subscription is granted in exactly one place; that was part of why the
interception lives in `answer_requests` rather than being spread through the
compositor.

**Trigger to fix:** when kernel channel IPC carries capabilities across an
accepted connection. Lane A owns that; no request has been filed yet because
the feature is not near enough to specify an interface against.

**If never fixed:** every window title on the desktop is readable by every
program the user runs, silently and continuously. The desktop works; the
privacy property a user would assume it has is simply absent.

**Scope note, 2026-09-09 — the title list is the example, not the extent.**
Found while wiring the Settings dynamic-DNS page, and recorded here rather than
as a new entry because the cause is the one above, not a second one. Two facts
this entry did not state:

- **The missing gate is not specific to `SubscribeWindowList`.**
  `Server::accept_pending` (`gui/compositor/src/server.rs`) adopts every
  connection that arrives, assigns it a client id and pushes a `Client` — it
  does not look at the peer at all, and `Listener::accept` discards the peer
  address before it could. `require_shell` (`wire.rs:361`) then returns
  `Ok(())` unconditionally by design. So a connected client can also inject
  input, move and resize windows it does not own, and destroy them. Reading
  titles is simply the cheapest thing to demonstrate.
- **`SLATE_DISPLAY` can move the listener off loopback.** The default is
  `127.0.0.1:7373`, so out of the box only local processes reach it. But the
  address comes from the environment (`socket.rs`, `display_addr`), and setting
  it to `0.0.0.0:7373` exposes the same ungated surface to the network, with
  nothing warning that it has been. The proper fix below covers this too — the
  gate is at accept either way — but until it exists, the loopback default is
  the only thing standing between an unauthenticated peer and the desktop.

**The one place authentication for this was ever written down** is
`apps/settings/src/remote.rs` — `RemoteDesktopConfig`'s `require_authentication`
and `allowed_users`, together with a port and an encryption level. That file is
unreachable (`TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`), so
the design exists and nothing reads it. It is retained for that reason: it is
the only statement in the tree of what the gate should ask.
