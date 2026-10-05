## 515. A CRTC drives at most one monitor, each monitor copies out its own rectangle of the one composited frame, and a monitor that fails is dropped rather than fatal

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** §514 taught the compositor that a second monitor exists; this
entry makes one actually light up. Before it, the code that hands finished
pictures to the graphics card (the *scanout*) picked a single monitor, drove
it, and ignored every other one plugged in — so the desktop was composited at
the full two-screen width and only the left screen ever showed anything. Now
every connected monitor gets its own pair of buffers and its own flip, and each
one copies out the part of the desktop it occupies. Three rules make that work,
and each is recorded below because each had a plausible alternative.

**Context.** `DrmScanout::new` walked the card's connectors, stopped at the
first one it could drive, and built one buffer pair for it. `size()` — which is
what the binary builds the compositor at — was that one monitor's mode. §514
had already fixed the model above it, so the *combination* was worse than
either half alone: with §514 in place and this not, `main.rs` would compose a
desktop the width of one screen while `DisplayManager` believed there were two.

**Decision, in three parts.**

**1. One frame, a viewport per head.** `Present::show` still takes a single
buffer — the whole desktop — and each head blits the rectangle at its own
`(x, y)`. This is §514's decision arriving at the other end of the pipeline and
it is what keeps `Present` a three-method trait: adding a second monitor
changed no signature. The arithmetic that makes it true is four lines in
`blit`, which is a free function precisely so it can be tested without a
graphics card.

**2. Heads are laid out left to right in enumeration order — the same rule the
compositor already used, not a rule communicated to it.** `DrmScanout::new`
places each head to the right of the last; `DisplayManager::add_display` does
exactly the same with the displays `main.rs` attaches in the order `heads()`
returns them. So the two agree *by construction* rather than by protocol.

The alternative was to make the scanout authoritative: pass each head's offset
into `attach_display` and have the compositor honour it. That is more general —
it is what a future "drag your monitors into any arrangement" feature needs —
and it was rejected for now because §514's surface-origin invariant (the
surface's pixel (0,0) is the desktop's (0,0)) does not yet permit a display at
a negative offset. Publishing an offset the compositor cannot honour would be a
worse lie than not publishing one. `main.rs` therefore cross-checks
`compositor.frame_size()` against `screen.size()` and warns loudly if they ever
disagree, which is the guard that turns "agree by construction" from an
assumption into something observable.

**3. A CRTC drives at most one connector.** A CRTC (the hardware that reads a
framebuffer out to a cable) scans out one framebuffer at a time. The
pre-existing `resolve_crtc` preferred the CRTC a connector was already bound to
at boot, and on a machine where the firmware left two connectors sharing one,
that returned the same CRTC twice. The result would not be a second monitor
lighting up: it would be the *first* monitor's picture being replaced by the
second's, alternating every frame. So `choose_displays` now carries a `taken`
list, and a connector it cannot find a free CRTC for is declined.

Declining is the interesting half. The alternatives were to fail the whole
setup (one unroutable monitor blanks a working one — plainly wrong) or to share
the CRTC anyway and let the driver sort it out (it cannot). Declining leaves
one working monitor instead of two broken ones, and the number of heads is then
bounded by the card's CRTC count — a tighter and more meaningful bound than any
constant this module could have picked, and the hardware's own limit on how
many pictures it can scan out at once.

**A failing monitor is dropped, not fatal.** This is the third place the "one
head" assumption was load-bearing, and the least obvious. The single-head code
closed the display on the first real flip error, which is correct when there is
one screen and catastrophic when there are two: unplugging a DisplayPort cable
would have blanked the *other* monitor and exited the display server. Now:

* a head whose **first** flip fails never enters the layout, so the desktop is
  sized as if it were not there and the surviving heads shift left to fill the
  gap;
* a head whose flips start failing **mid-session** is marked dead and stops
  being drawn on, while the others carry on at their existing offsets — the
  layout deliberately does *not* re-flow, because the compositor's idea of the
  desktop has not changed and a scanout that silently moved the remaining
  monitor would put every window on the wrong screen;
* `is_open()` is false only when no head is left, which is when
  `Server::run_with` should exit.

Either way the head **stays in the vector**, holding its two dumb buffers and
two framebuffer ids, so `Drop` still gives them back. Removing it on failure
was the obvious implementation and leaks four kernel objects for the life of
the process — which matters because this type does not own the card and a
second scanout can share it.

**A monitor whose *buffers* will not allocate is declined on the same terms.**
This was caught reviewing the diff above rather than by a test, and it is worth
recording because the first draft got it wrong in a way that reads as correct:
a head that could not *flip* was declined, but a head that could not *allocate*
returned `Err` from `new()` and so blanked the monitor next to it that had
allocated fine. That is the same wrong answer sharing a CRTC gives, arrived at
from the other direction. `new()` now records the first such failure and
carries on, returning it only when **no** head was built — so a card on which
nothing works still reports why rather than reporting "no display".

The half-allocated head is the sharper edge. A head is all-or-nothing (it
cannot flip with one buffer), so when the second `CREATE_DUMB` fails the first
buffer is owned by nobody: the head never reaches `DrmScanout::heads`, which is
what `Drop` walks. It has to be handed back at the point of failure or its
framebuffer id and GEM handle leak for the life of the process. Hence
`make_head`, whose entire reason to exist is that failure path, and
`release_buffer`, extracted so the constructor's teardown and `Drop`'s cannot
drift apart — including the ordering rule that `RMFB` precedes `DESTROY_DUMB`,
because the other order asks the kernel to free an object a framebuffer still
points at.

**Keeping a dead head costs an index space, so the per-head accessors are keyed
on the connector id.** This falls straight out of the decision above and is the
one place it bites. Because a dead head stays in `self.heads` so `Drop` can give
its buffers back, but `heads()` reports only the live ones, a *position* means
two different things depending on which of the two you got it from. A caller
doing the obvious thing — `for (i, h) in scanout.heads().iter().enumerate()`,
then passing `i` back to ask that head's pitch — reads the wrong monitor the
moment one dies, and reads it silently. So `pitch_for(connector_id)` and
`scanned_out_for(connector_id)` replaced `pitch_of(i)`/`scanned_out_on(i)`: a
connector id is the same in both worlds, is already carried by `HeadInfo`, and
answers "nothing" rather than "the neighbour" for a monitor that has gone.

The single-head accessors (`crtc_id`, `connector_id`, `pitch`, `scanned_out`)
are the same bug in miniature — they read position zero, which after a failure
is the corpse. They now resolve through one `first_live()` helper, which exists
in the singular so the four cannot drift apart. The alternative — dropping dead
heads from the vector so the two index spaces coincide — is the leak this entry
already rejected.

**Verification.** 18 new tests. 17 were proved to be regression tests by
reintroducing the defect each names through the guarded patcher and confirming
a deterministic failure that names it back, with the file verified restored
byte-for-byte after each: `firstconnectoronly`, `sharedcrtcbitmask`,
`sharedcrtcbound`, `headblitsfromorigin`, `blitrowignoresorigin`,
`headpitchisfirst`, `onedeadheadkillsall`, `dropdeadhead`,
`blitclipsatviewport`, `allocfailurefatal`, `partialheadleaks`,
`deadheadisstillthefirst`, `deadheadstillreadable`.

The last two needed a capability the fake card did not have: its failure queue
matched only its front entry, so "fail the *second* monitor's allocation" was
inexpressible — the first monitor's ioctls sat in front of it. `fail_nth`
injects at a *counted* occurrence instead, which is what lets a test say "the
fourth `CREATE_DUMB`" and mean the second head's second buffer. Without it
`partialheadleaks` could not have been proved at all, and an untestable leak is
one that comes back.

Two of those markers are worth naming for what they catch that nothing else
did. `headpitchisfirst` — writing every head at the *first* head's row stride —
is invisible on any pair of monitors whose widths pad to the same pitch, which
is why the fake card's two heads are 1024 wide (4096 bytes, unpadded) and 1366
wide (5464, padded to 5504). `sharedcrtcbound` is reachable only through
`resolve_crtc`'s early return, so a test that exercises only the bitmask path
would have passed while the boot-bound path handed the same CRTC out twice.

The eighteenth — `a_viewport_starting_past_the_end_of_the_frame_copies_nothing`
— **survives every marker above and is recorded as additional coverage rather
than the sole guard on an invariant**, for the same reason §513 and §514
recorded theirs. It is double-guarded: the copy width saturates to zero *and*
the row read goes through `src.get()`, whose absence is already pinned by
`a_frame_shorter_than_it_claims_to_be_does_not_take_the_display_server_down`. It
is kept because it pins the documented contract for a monitor the composited
desktop does not reach, which is a real state during a resize.

**What this does not do.** Per-head scale factor and rotation remain out of
reach, for the reason §514 gives: one surface has one pixel grid. Monitors can
only be arranged in a row, because the surface's origin is the desktop's
origin. And there is still no `SETCRTC` in the kernel, so each monitor runs at
the mode it came up in — `TD-COMPOSITOR-CANNOT-CHANGE-MODE`. None of these are
newly true; they are §514's deferred costs, now paid at both ends of the
pipeline instead of one.
