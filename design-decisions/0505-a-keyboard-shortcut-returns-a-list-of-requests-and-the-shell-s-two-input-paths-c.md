## 505. A keyboard shortcut returns a *list* of requests, and the shell's two input paths converge on one request type

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop's keyboard shortcuts (Alt+F4, Super+D, Super+Left…)
used to change the shell's own picture of the windows, which on a real session
is a picture the compositor overwrites a moment later — so the shortcuts did
nothing visible. They now hand back what they want done, for the event loop to
send on. The two choices worth recording are that a shortcut hands back a
*list* rather than one item, and that this list holds the same type a mouse
click already produced rather than a new one.

**A list, because one shortcut names every window.** Super+D minimises
everything on the current desktop. Options considered: return
`Option<WindowRequest>` and special-case Super+D by having it act locally
(rejected — that is the bug, restated); give the shell a callback to send
through (rejected — it puts the connection inside the model half of the crate,
which is what keeps every test in it offline); return a `Vec`. The `Vec` costs
an allocation on every consumed keystroke, which is nothing at keyboard rates,
and it makes the batch visible to a test: `super_d_asks_for_every_window_to_be_
minimised` asserts three requests in order, and a reintroduced
`take(1)` in the session's send loop fails it.

**One request type for both paths.** `ShellAction::Control` was an inline
struct variant carrying `{ window, action }`; the obvious cheap move was to add
a separate `WindowRequest` struct for the keyboard and leave it alone.
Rejected: they are the same ask, they end at the same `control_window` call,
and two spellings of one concept is a second place to forget an action when the
vocabulary grows. `ShellAction::Control` now holds a `WindowRequest`. Six call
sites, all inside this crate.

**Consumed and asked-for are independent, which is not obvious.** A shortcut
pressed with nothing focused is *consumed and asks for nothing* — the key must
not fall through to an application, because the user pressed a desktop
shortcut and the desktop is what should have swallowed it. An early version
collapsed the two (no request ⇒ not consumed) and broke the virtual-desktop
shortcuts on an empty desktop; that mistake is now a reintroduced defect in the
sweep, failing four tests.
