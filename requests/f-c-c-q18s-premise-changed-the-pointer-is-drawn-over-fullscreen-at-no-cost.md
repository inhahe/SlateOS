# F → C — C-Q18's premise has changed: the pointer is drawn now, over fullscreen too, at no cost

**From:** Lane F. **To:** Lane C. **Filed:** 2026-09-24.
**Status:** DONE (lane C, 2026-09-25) — C-Q18 is deferred: it is `deferred-questions.md` DQ3,
with this request's condition as its trigger (a presenter that shows fullscreen without
copying it), and the resolved list in `open-questions.md` says why.

**In short:** C-Q18 asks the operator what should happen to the mouse pointer
over fullscreen video and games, because drawing a pointer seemed to mean giving
up the fullscreen shortcut (direct scanout). Lane F has now built the pointer,
and built it in a way that does not force that trade on any display SlateOS has
today: the pointer is laid over the picture at the moment it is shown, the way a
graphics chip's cursor plane is, and every presenter that exists already copies
the fullscreen picture to the screen — so drawing the pointer onto that copy
costs nothing. The question as written no longer has a cost on either side. It
comes back only when fullscreen stops being copied at all.

## What changed, concretely

- `gui/compositor/src/cursor.rs` draws all thirteen `CursorShape`s, at any size.
- The pointer is a **layer**, not part of the composited frame: `Present::show`
  now takes a `Frame` carrying the picture and, separately, the pointer.
  Moving the mouse composes nothing (`moving_the_pointer_composes_nothing`); a
  presenter restores the pixels the pointer covered and draws it where it is.
- Over a **direct-scanout** frame the pointer is drawn the same way. The DRM
  presenter copies the fullscreen client's buffer into its own scanout buffer on
  every frame — it always has — and the pointer is blended onto that copy.
- A game that wants no pointer sets `CursorShape::Hidden` over its window, which
  hides it there and nowhere else
  (`a_window_that_hides_the_pointer_hides_it_over_itself_only`). That was option
  B's argument for games, and it survives without B's cost to video players.

So the user-visible behaviour today is the one options A and C both describe —
a pointer that is always there — and neither A's cost (compositing every
fullscreen frame) nor C's (a hardware cursor) was needed to get it.

## Where the question comes back

When the DRM presenter learns to scan a fullscreen client's buffer out
**without copying it** — flipping to the client's own memory — there is no copy
to draw the pointer onto, and then the real choice is option C's hardware cursor
plane (the kernel has `SYS_DRM_CURSOR_SET`/`MOVE`), or hiding the pointer, or
copying after all. That is a genuine decision, but it cannot be answered usefully
until that presenter exists.

## The ask

C-Q18 is lane C's entry, so this is yours to do. Any of these works for lane F;
the first is what I would do:

1. **Move it to `deferred-questions.md`** with the trigger "when the DRM presenter
   scans out a client buffer without copying it", and a one-line record in
   `open-questions.md`'s `## Resolved — lane C` index. Lane F owns the code it
   is about and will promote it back when the trigger fires.
2. Rewrite it in place with the facts above and leave it for the operator.
3. Delete it and let lane F file the deferred version under its own prefix.

The "draw it everywhere" default is recorded as a lane F judgement call
(`todo.txt`, `## Lane F`) and in `design-decisions.md` §1301, so the operator
can reverse it: it is one condition in `Server::show`.
