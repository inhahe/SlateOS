## TD-C-NO-MODAL-DIALOG-KNOWS-WHERE-IT-IS-WHEN-IT-IS-CLICKED (lane C, 2026-08-24) — FIXED 2026-08-24

`ModalOverlay` has a `content_rect` — the dialog's own rectangle, which
`handle_mouse` tests a click against to decide whether it landed outside the
dialog and should dismiss it. **Nothing ever sets it.** The only callers of
`set_content_rect` in the tree are two unit tests (`gui/toolkit/src/modal.rs`);
no dialog calls it, so every live dialog carries `(0.0, 0.0, 0.0, 0.0)` and
`point_in_content` answers `false` for every point on the screen.

### What breaks

- **`dismiss_on_click_outside` dismisses on a click *anywhere*, including on the
  dialog's own buttons.** `ModalOverlay::new()` turns it on by default, so this
  is the behaviour of any dialog that does not turn it off. `InputDialog` and
  `ProgressDialog` set it to `false` in their constructors and are unaffected;
  `AlertDialog` and `NonModalDialog` need checking, and the default itself is
  the trap for whatever is written next.
- **No dialog can hit-test anything inside itself.** That is why `InputDialog`
  has a caret it can place with the arrows but not with the mouse: the geometry
  it would need is computed inside `render(&self, parent_width, parent_height,
  …)` from the parent's size, discarded when that call returns, and unavailable
  to `handle_mouse`, which is not told the parent's size at all.

### Why it is a design fault and not a missing call

Adding `self.overlay.set_content_rect(...)` inside `render` is impossible as
written — `render` takes `&self`. Threading the parent size into `handle_mouse`
instead would give two independent copies of the layout arithmetic, in two
methods, that have to agree exactly or the click lands in the wrong place; that
is the bug it is trying to fix, moved.

### The proper fix

Give the dialogs a **layout step**, as `Widget`/`WidgetTree` already have: a
`fn layout(&mut self, parent_width: f32, parent_height: f32)` that computes the
rectangles once, stores them (dialog rect, input-field rect, button rects), and
is called before `render` and consulted by `handle_mouse`. `render` then draws
from the stored boxes rather than recomputing them, and `handle_mouse` hit-tests
against the same numbers that were drawn — which is the property that makes a
click land where the user aimed it.

Once that exists, `InputDialog` gains click-to-place-caret in a few lines, using
`text::cursor_at` against the stored field rect exactly as
`WidgetKind::TextInput` does today, and `dismiss_on_click_outside` starts
meaning what it says.

### 2026-08-24, later — the survey before the fix found three more, and they are all the same fault

Reading every `handle_mouse` in the file before starting the layout step turned
up that the missing geometry is not one dialog's problem. It is the file's:

- **`InputDialog::handle_mouse` hit-tests nothing at all.** Its whole body is
  the overlay-dismiss check. So its OK and Cancel buttons — which it draws,
  and highlights, and moves a focus ring between — **cannot be clicked**. The
  dialog is keyboard-only, and nothing says so.
- **`ProgressDialog` has no `handle_mouse` at all**, and its `handle_event`
  has no `Event::Mouse` arm. A `.cancelable()` progress dialog draws a Cancel
  button that can only be worked with Escape.
- **`AlertDialog::handle_mouse` hit-tests against `compute_layout(800.0,
  600.0)`** — a hardcoded parent size, because `handle_mouse` is not told the
  real one. Its buttons therefore land correctly only on an 800×600 parent and
  are offset by half the difference on any other; on a 1920×1080 desktop the
  hit areas sit 560 px left and 240 px above the buttons the user can see.
  This is the same bug as the other two wearing a plausible number.

That last one is the reason the fix is a stored layout and not a parameter.
`AlertDialog` already has the shared `compute_layout` the entry above asks for,
and it *still* clicks in the wrong place — because the input the layout needs
is not available where the click is handled. Threading it in would not have
been "two copies of the arithmetic"; it would have been one copy fed a guess.

### 2026-08-24 — fixed, with one change to the plan above and a fourth bug found on the way

The layout step landed, but **not as the separate `fn layout(&mut self, …)` the
plan asked for.** A separate call is a call that can be forgotten, and a
forgotten one reproduces this entry exactly: hit areas that do not match what is
on screen, compiling and rendering perfectly and failing only under the mouse.
The tree already had the evidence — `ModalOverlay::set_content_rect` *was* that
separate call, and in the whole repository only two unit tests ever made it.
So instead `render` itself took `&mut self` and records where it put things as a
side effect of drawing them; `handle_mouse` can consult nothing else. The
reasoning, and the cost of `&mut self` on a draw method, are `design-decisions.md`
§547.

**The fourth bug, found while fixing the other three:** `content_rect` was a
plain tuple starting at all zeroes, so before the first frame *every* point on
screen is "outside the dialog". A dialog with `dismiss_on_click_outside` on —
which is `ModalOverlay::new()`'s default — therefore dismissed itself if a click
arrived between `show()` and its first frame, before the user had seen it. It is
now `Option`, so "I do not know where I am" classifies no clicks rather than all
of them.

What is fixed:

- `AlertDialog` hit-tests the rectangles it drew, at any parent size, and
  `render_buttons` draws *from* those rectangles rather than re-deriving them.
- `InputDialog` has working OK, Cancel and click-to-place-caret, the last of
  those correct through a horizontal scroll (shared with `WidgetKind::TextInput`
  via the new `textedit::cursor_at_click`) and through password masking (the
  click is resolved against the row of marks and mapped back to a byte offset in
  the secret, so it can never land inside a character).
- `ProgressDialog` has an `Event::Mouse` arm and a clickable Cancel button.
- `dismiss_on_click_outside` means what it says, and means nothing before the
  first frame.

Proved by `scripts/reintro-modal-geometry.py` — nineteen one-line defects, each
of which compiles.

---

### 2026-08-24 — the `cmp`/`diff` verdict: reversing this file's own recommendation, from flat-1 to 0/1/2

The entry above (§"Still outstanding") queued the `cmp`/`diff` verdict fix with
an explicit recommendation: *"Follow the tree's flat-`1` convention rather than
POSIX `cmp`'s 0/1/2: a half-migrated table is worse than a consistent one … so
'differ' and 'could not compare' both report 1."*

**That recommendation was wrong, and the fix as landed uses 0/1/2** for `grep`,
`cmp` and `diff` alike. Recorded here rather than silently overriding, because
this file is where the next reader will look for the rationale and will
otherwise find the superseded one.

**Why the earlier argument does not apply.** It borrowed its force from the
*syntax-error* sites, where the choice really is about a shell-wide status
table: bash reserves 2 for syntax errors, 126 for not-executable, 127 for
not-found, 128+N for signals, and adopting one row of that table without the
rest leaves a caller unable to tell which convention it is reading. That
reasoning is sound and those sites still use flat 1.

But `grep`/`cmp`/`diff`'s 0/1/2 is not a row of the shell's table at all. It is
a **per-command** convention, and 1 in it is not a failure code — it is a
*finding*: "I compared them and they differ", "I searched and it is not there".
The third value exists precisely because that finding must be distinguishable
from "I never got to look". Collapsing 2 into 1 does not lose resolution
uniformly; it loses it in one direction only, replacing *I don't know* with a
confident, wrong *no*, delivered in the exact form the caller is waiting for.

**And the loss the earlier note accepted was larger than it looked.** It called
flat-1 "a real loss of resolution … still strictly better than today". Strictly
better, yes — but the case it waves through is the one that matters most:

```
diff a b || echo "they differ"
```

`diff` prints nothing when files match, so under flat-1 a run that *refused* to
compare (unreadable file, or one past the 2000-line cap) is byte-identical to a
successful comparison and now also status-identical to a real difference. There
is no observation left that separates the three. Under 0/1/2 the status is the
one thing that does.

The full rationale, the alternatives and the rule for deciding whether another
command needs a third value are in `design-decisions.md` §275.

**Scope of the reversal:** `grep`, `cmp`, `diff` only. Usage errors inside those
three moved from 1 to 2 as part of it, for the same reason — inside them, 1 is
spent. Everywhere else in the shell a usage error is still 1, and the 17
`"Syntax error: …"` sites remain queued for flat `set_exit(1)` exactly as
originally planned.
