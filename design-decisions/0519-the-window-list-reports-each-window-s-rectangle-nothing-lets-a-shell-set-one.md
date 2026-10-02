## 519. The window list reports each window's rectangle; nothing lets a shell set one

**Lane:** C
**Decided by:** Claude (autonomous) — flagged to the operator before starting; see
"Flagged, and proceeded on the default" below.

**In short:** SlateOS has an "overview" screen — the Exposé-style view that shrinks
every open window to a thumbnail so you can pick one — and 1856 lines of code for
it that nothing calls. It was never wired up because it *cannot* be: drawing a
thumbnail in proportion to the real window needs the real window's size, and the
list of windows the desktop shell receives did not carry one. So the window-list
frame now carries each window's position and size. The shell can read a
rectangle; it still has no way to set one.

### The problem

`gui/desktop/src/overview.rs` builds its grid of thumbnails through
`compute_grid_layout`, which calls `fit_aspect(thumb.width, thumb.height, …)`.
With no geometry available every thumbnail is zero by zero, and `fit_aspect`
returns `(0.0, 0.0)` for zero input — so the grid would be a screen full of
rectangles that rasterise to no pixels and match no click. That is precisely the
failure `Hit::WindowContent` had before §506 deleted it: a feature that looks
present in the code and is absent on the screen. It is why the overview was
demoted from `[x]` to `[-]` in the roadmap and filed as
`TD-C-THE-OVERVIEW-SCREEN-IS-1856-LINES-NOBODY-CALLS`.

So the question was never "should the overview have geometry". It was "should the
shell be told rectangles at all".

### Why this is not a reversal of §506

§506 *deleted* per-window geometry from the shell's own window list, which reads
like a flat contradiction of this section. It is worth being exact about why it
is not.

What §506 removed was geometry the shell **wrote and then acted on**: the shell
kept its own rectangles, decided where a window belonged, and the compositor
followed. Two copies of one truth with the shell's copy authoritative — so every
drift between them was a window in the wrong place, and the shell was a window
manager rather than a client of one.

What version 3 adds is geometry that is:

- **read-only.** No verb in `ShellControl` accepts a rectangle. A snap names a
  *named edge* (§505), a maximize names nothing, a move-to-desktop names a
  number. There is deliberately no `SetWindowRect`, and adding one would be a
  separate decision that this section does not make.
- **replaced wholesale by every list.** The shell accumulates no copy that can
  drift; each `WLST` frame supersedes the last one entirely.
- **read off the compositor's own `Window` at the instant the list is built**
  (`w.x`, `w.y`, `w.width`, `w.height`), not out of a cache.

The distinction is *report* versus *accept*. A client that can see where the
windows are can draw a picture of the desktop. A client that can move them by
naming coordinates is the thing §506 refused. Only the second is authority.

### Alternatives considered

**(1) Uniform thumbnails — no geometry on the wire.** Draw every window as the
same box. *Against:* an overview whose tiles are all one size and carry no
picture is a list of window titles, and the taskbar is already a list of window
titles. The feature would exist and be pointless, which is worse than not having
it — it costs the same screen and teaches the user that the overview shows less
than the thing it covers up.

**(2) A separate "overview" request returning geometry only when asked.** *For:*
keeps the common `WLST` frame smaller, and makes the extra reporting explicit and
opt-in. *Against:* it is a second source of truth for the same windows arriving at
a different instant, so the overview would routinely draw a window where it was
one frame ago. And the saving is not real: 16 bytes per window against a title
that is typically 20–60.

**(3) Geometry, reported, in the list that already exists — chosen.** One list,
one instant, one truth. The cost is 16 bytes per window and a version bump.

### The wire

Four fields per window, placed **after** the state byte and the workspace, so
that the existing decoder tests' byte arithmetic (`state_at = HEADER + 8 + 8 + 1`)
keeps naming the fields it was written for:

```
x       : i32    top-left in desktop coordinates
y       : i32
width   : u32    outer size, decorations included
height  : u32
```

`x` and `y` are **signed**, which is the one non-obvious choice here. The desktop
origin is the primary monitor's top-left corner, so a monitor arranged to its
left occupies negative x across its whole width. Narrowing that to unsigned turns
x = −1920 into 4 293 047 296 — not a clipped window but one two million pixels
off-screen, which draws as nothing and reads to the user as "that application
closed".

`WINDOW_LIST_VERSION` goes 2 → 3, and a v2 shell reading a v3 frame is refused
rather than allowed to short-read, on the same reasoning the 1 → 2 bump used: a
shell that saw zero-by-zero windows would look like an empty desktop rather than
like a decoding error, and an empty desktop is a state that legitimately happens.

`write_i32` and `Reader::read_i32` were hoisted out of `control.rs` into `lib.rs`
and `reader.rs`, because two codecs now need them, and two spellings of one
scalar is how the two ends of a wire come to disagree about it.

### Verification

Seven markers, applied one at a time through the guarded reversible patcher and
reverted with a byte-for-byte SHA-256 check, each run against all four GUI
crates:

| Marker | The defect put back | New tests that failed |
|---|---|---|
| `wl3swapxy` | decoder reads y before x | negative-origin, not-interchangeable, extremes |
| `wl3swapsize` | decoder reads height before width | not-interchangeable, extremes |
| `wl3xunsigned` | encoder writes `unsigned_abs()` — "a position cannot be negative" | negative-origin |
| `wl3decodedropsgeometry` | bytes consumed, rectangle discarded (v3 frame, v2 struct) | negative-origin, not-interchangeable, extremes |
| `wl3newinventsasize` | an unplaced window given a plausible default size | nobody-placed |
| `i32signmagnitude` | the shared `write_i32` as sign-magnitude, not two's complement | negative-origin |
| `compnogeometry` | the compositor reports a rectangle it never filled in | reports-where-it-is, moves-in-the-next-list |

Every marker was caught, and every one of the six new tests was failed by at
least one marker. Three results are worth recording as *weaker* than the table
makes them look, because a proof round is only useful if what it did not prove
is written down too:

- **`i32signmagnitude` did not fail the extremes test, and the comment inside
  that test was wrong about why it would.** The comment claimed `i32::MIN` was
  the witness against sign-magnitude. It is the opposite: `i32::MIN` is the one
  negative value where the two encodings agree, because
  `i32::MIN.unsigned_abs()` is `0x8000_0000`, which is already the sign bit and
  nothing else. The ordinary negative in the other test is what catches it. The
  comment has been corrected to say so, and to say that the reintroduction run
  is what settled it. This is exactly the class of error the exercise exists to
  find — a test whose *stated* reason for existing is not the reason it passes.
- **`i32signmagnitude` also failed four `control.rs` tests**, including
  `a_negative_position_is_not_read_as_a_huge_one` and
  `a_reserve_reply_carries_a_signed_origin_so_a_left_hand_monitor_survives`.
  That is the hoist working as intended: the two codecs really do share one
  scalar, and a defect in it surfaces on both wires rather than on one.
- **`moving_a_window_moves_it_in_the_next_window_list` is proved by the same
  single marker as its neighbour.** Its distinct claim — that the list is
  rebuilt per call rather than cached at window creation — has no marker of its
  own, because a stale cache cannot be expressed as a one-string replacement in
  a function that has no cache to make stale. It is honest additional coverage
  against a defect that does not exist yet, not a proved regression test for one
  that did.

### Flagged, and proceeded on the default

This rests on a reading of §506 — report versus accept — and the operator may
read it differently. The question was put before the work began: *if you would
rather the shell never see rectangles at all, say so and I will stop; the
fallback is alternative (1), uniform boxes, which makes the overview a list,
which is what the taskbar already is.* No objection came back, and the change is
small and wholly revertible — one version bump, four fields, no new verb — so it
proceeded on the default. If the operator prefers (1), the reversal is this
commit backed out and the overview left filed as dead code.
