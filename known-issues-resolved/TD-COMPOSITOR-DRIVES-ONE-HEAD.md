## TD-COMPOSITOR-DRIVES-ONE-HEAD (lane C, 2026-08-21) — ✅ RESOLVED 2026-08-21

**In short:** with two monitors plugged in, one shows the desktop and the other
stays dark. Not a bug in what exists — the second screen was never implemented.
*(Both halves are now done; see the two updates at the end. The original
diagnosis below turned out to be half wrong, and is kept for that reason.)*

**What.** `choose_display` returns the *first* connected connector that has a
usable mode and a CRTC that can reach it, and `DrmScanout` owns exactly one
CRTC, one connector and one pair of buffers. Every later connector is skipped.

**Why it is not a small fix.** The scanout side is genuinely easy: loop over the
connectors, allocate a buffer pair per head, flip each. The hard half is
upstream — `Compositor` composes *one* frame at *one* size, and a second monitor
is not a second copy of that frame but a second viewport onto one desktop with
its own resolution, its own position in a virtual screen, and windows that can
straddle the boundary. `DisplayManager` (in `gui/compositor/src/lib.rs`, not a
`display.rs` — an earlier revision of this entry named a file that does not
exist) already models multiple displays, modes and refresh rates faithfully; it
is the compositing pipeline that assumes one.

**Severity.** Low as a defect, medium as a missing feature. Multi-monitor is
table stakes for a desktop OS, but a one-headed desktop is fully usable and
nothing about the current design has to be unwound to add the second — the
per-head state is already a struct.

**Update 2026-08-21 — the compositor half is done, and it was not a missing
feature but a live bug.** The claim above that the model "already models
multiple displays faithfully" was wrong. `DisplayManager::add_display` extended
the virtual desktop and *nothing extended the surface being composited into*, so
a window placed on the second monitor was drawn past the end of the framebuffer
and clipped away entirely — while the model went on reporting it as visible on
screen 2. Two more followed from the same crack: `set_fullscreen` sized from
`backend.size()` rather than from the window's own monitor (so fullscreening on
the second screen jumped the window to the first and spanned both, disagreeing
with `maximize_window` one line away), and `refit_fullscreen_windows` applied the
resized framebuffer's dimensions to *every* fullscreen window (so a mode change
on one monitor dragged the games on all the others onto it).

Fixed by making the scanout surface exactly the virtual desktop's bounding box,
as an invariant of the only two functions that can change either
(`resize_display` and a new `attach_display`, both of which allocate before they
adopt, so a surface that cannot be allocated leaves the arrangement alone), and
by resolving fullscreen against `work_bounds_for` like maximise does. Rationale
and the rejected per-head-frame alternative: `design-decisions.md` §514. 10 new
tests, each proved by defect reintroduction.

A latent `Rect` defect fell out of that work and is fixed in the same change:
`union` and `intersect` computed far edges with `self.width as i32`, so a
rectangle wider than 2^31 unioned to a bounding box *narrower than either input*
and intersected with anything to nothing. They now use the already-saturating
`right()`/`bottom()`.

**Update 2026-08-21 (2) — RESOLVED; the scanout half is done too.**
`DrmScanout` now holds a `Vec<Head>`, one per connected connector that has a
usable mode and a CRTC no earlier connector claimed, each with its own buffer
pair and its own page flip. `blit` takes a `Viewport` — pitch, size and the
head's `(src_x, src_y)` within the composited frame — so each monitor copies out
its own rectangle of the one desktop-sized picture, and `Present` did not gain a
method or a parameter. `main.rs` builds the compositor at head 0 and declares
the rest with `attach_display`, then cross-checks `frame_size()` against
`screen.size()` and warns if they disagree.

Three things that were latent in the single-head code and became defects the
moment there were two, all fixed here:

* **CRTC exclusivity.** `resolve_crtc`'s preference for the CRTC a connector was
  already bound to at boot would hand the same one to two connectors, which does
  not light the second monitor — it replaces the first monitor's picture with
  the second's, every frame. `choose_displays` now carries a `taken` list and
  declines a connector it cannot give a free CRTC to.
* **A flip failure closed the whole display.** Unplugging one monitor
  mid-session would have blanked the other and exited the display server. A
  failing head is now marked dead individually; `is_open()` goes false only when
  none is left.
* **The destination row stride.** Writing every head at the first head's pitch
  skews any monitor whose width pads differently — invisible on one head, and on
  two whose widths happen to pad alike.
* **An allocation failure was fatal, and a partial one leaked.** A head whose
  flip failed was declined, but a head whose *buffers* would not allocate
  returned `Err` from `new()` and blanked the monitor next to it that had
  allocated fine. Worse, when the second of a head's two `CREATE_DUMB`s failed,
  the first buffer was owned by nobody — the head never reaches
  `DrmScanout::heads`, which is what `Drop` walks — so its framebuffer id and
  GEM handle leaked for the life of the process. `make_head` now releases the
  orphan at the point of failure and `new()` declines the head, returning an
  error only when no head was built at all.
* **The per-head accessors indexed a different list from the one `heads()`
  reports.** Dead heads stay in the vector so `Drop` can return their buffers,
  but `heads()` filters them out, so a caller enumerating `heads()` and passing
  the loop index back to `pitch_of(i)` read the wrong monitor once one died.
  They are now keyed on the connector id — `pitch_for` / `scanned_out_for` —
  and the single-head accessors (`crtc_id`, `connector_id`, `pitch`,
  `scanned_out`) resolve through one `first_live()` helper instead of reading
  position zero, which after a failure is the head that just went away.

A head that fails stays in the vector so `Drop` still returns its two dumb
buffers and two framebuffer ids; dropping it would leak four kernel objects,
which matters because this type does not own the card. Rationale, and the
rejected alternatives for each of the three rules: `design-decisions.md` §515.
18 new tests, 17 proved by defect reintroduction and the eighteenth recorded
there as additional coverage.

**What remains, and is filed elsewhere.** Per-head scale factor and rotation are
not possible under the one-surface arrangement (`design-decisions.md` §514) and
monitors can only be arranged in a row, because the surface's origin is the
desktop's origin. Each monitor runs at the mode it came up in —
`TD-COMPOSITOR-CANNOT-CHANGE-MODE`. Hotplug of a monitor *after* startup is not
handled: heads are enumerated once in `DrmScanout::new`, so plugging a screen in
does nothing until the display server restarts. That last one is new debt and is
filed below as `TD-COMPOSITOR-IGNORES-MONITOR-HOTPLUG` — since resolved, on
2026-08-21, by the polled re-probe in `design-decisions.md` §517.
