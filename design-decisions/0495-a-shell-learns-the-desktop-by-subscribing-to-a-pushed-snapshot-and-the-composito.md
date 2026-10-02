## §495 — A shell learns the desktop by subscribing to a pushed snapshot, and the compositor decides it is stale by comparing bytes rather than counting changes

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A taskbar has to show a button per open window, but until now a
client could only see the windows it opened itself — so the shell could draw a
taskbar with nothing in it and no way to find out what was missing. This adds a
way for a program to say "tell me about everyone's windows", after which the
compositor sends the whole list whenever it changes. Two things had to be
chosen: *when* the compositor decides the list changed, and *what* stops any
program from using this to read every window title on the desktop. The first is
decided here; the second is not solved and is written down as a known issue.

### What was added

A new frame kind, `WLST`, carrying one entry per window: id, owning process,
band (`Layer`), title, and four state flags (visible, minimized, maximized,
focused). A new request, `SubscribeWindowList { subscribe: bool }`, turns the
stream on and off. The client stores the latest list and a revision counter; a
shell redraws when the counter moves.

Three properties, each deliberate:

- **Push, not poll.** A taskbar that polled would either lag by up to a frame
  or ask every frame for an answer that almost never changes.
- **Snapshot, not delta.** A shell that missed one delta would be wrong
  forever, with nothing in the protocol to tell it so. A snapshot is
  self-correcting: the next one is the whole truth regardless of what came
  before. At desktop scale — tens of windows, one subscribed shell — the cost
  of re-sending is a few hundred bytes.
- **Unfiltered.** The compositor sends *every* window: invisible ones,
  minimized ones, every band. Filtering is the shell's decision and different
  jobs need different subsets — a taskbar wants minimized windows and not
  overlays; an Alt-Tab switcher wants neither. A compositor that pre-filtered
  would have to pick one, be wrong for the other, and leave the loser with no
  way to recover what was dropped.

### The decision: comparing bytes vs. counting changes

The obvious design is an epoch counter on the compositor, bumped wherever the
window set is touched, with each link remembering the epoch it last sent. It is
cheap and it is what most compositors do.

It was rejected because of *where the bug lives*. The bump sites are
`create_window_from_spec`, `destroy_window`, `set_title`, `focus_window`,
`minimize_window`, `restore_window`, `maximize_window`, `set_layer` — eight
today and more later. A future edit that changes a title without bumping does
not fail: it silently leaves a taskbar showing a stale name until something
unrelated moves the counter. That is a bug with no test that could plausibly
catch it, in a place nobody would think to look. It is the same trap §494
named in the stacking order: *counting* a property is correct only while its
precondition holds, and a lapsed precondition returns a plausible wrong answer
rather than an error.

What ships instead: `route_window_list` encodes the list every tick and
compares the bytes to the exact bytes it last sent that link. Identical means
nothing goes out. **There is no site to forget** — staleness is derived from
the thing that would actually be wrong, so a new window-mutating method is
covered the moment it exists, without its author knowing this protocol is
there.

*Cost:* one encode per subscribed client per frame — a few hundred bytes of
formatting for tens of windows, against a frame budget of milliseconds. If
that ever matters, the fix is to encode once and compare per link, not to go
back to counting.

*Byte comparison, not a hash:* a hash collision means a *missed* update, i.e.
a permanently stuck taskbar, which is far worse than the bytes it saves. The
compared buffer is the message that would be sent anyway.

### What is deliberately not solved: any client may enumerate every title

A window title is often a filename, a URL, or a subject line. Subscribing is
open to any client, so this is a real information leak, recorded in
`known-issues.md` as `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`. Real
Wayland gates the equivalent behind a privileged protocol that only a
compositor-trusted shell can bind.

The alternatives were: (a) ship it ungated and record the debt, (b) gate it on
something available today, or (c) not ship it, leaving the shell unable to
draw a taskbar. (b) is the right answer and cannot be written yet — the honest
gate is a capability, and kernel channel IPC does not yet carry capabilities to
the compositor, so anything written today would be a check against a value the
client itself supplies, which is not a gate but the appearance of one. (c)
blocks the shell on a kernel feature. So (a), with the debt named and the
proper fix stated: one capability check in `answer_requests`, at the one place
the subscription is granted.

**Reversible:** yes, and cheaply. The subscription is link state in
`ClientLink`, the query is one method on `Compositor`, and the frame kind is
additive — a peer that does not know `WLST` never subscribes and never
receives one.
