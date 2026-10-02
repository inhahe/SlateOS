## MODULE 83 (lane C, 2026-08-25) — the pointer tranche, and four ways a green test can be empty

**In short:** the shell's pointer-handling test file — 69 tests covering the
start menu, the taskbar, the calendar popup and the tiling chooser — had never
had a single one of its tests *proved*. Proving a test means breaking the code
it is supposed to guard and watching it go red; until that happens, all a green
test tells you is that it ran. Twenty-two deliberate faults were introduced one
at a time. Eighteen were caught. The four that were not each pointed at a
different way a test can be green without being a check, and one of them turned
out to be guarding code the program could never execute.

**Why this file.** Chosen by measurement rather than by taste: ranking every
swept file by unproved tests put `calendar.rs` (76 of 114) and `wallpaper.rs`
(75 of 92) above it numerically, but `pointer_tests.rs` was **69 of 69**, and a
file that is entirely unproved is a stronger signal than a larger partial hole.
A partial hole means the defects written so far ran along some dimension and
missed others; a whole file means nothing has ever been asked of it.

**The first pass, 22 defects:**

```
22 defects: 18 caught, 4 escaped, 0 never asked, 0 under-caught,
11 under-declared
restored: all files match their recorded SHA-256
```

### Lesson 31: a test that takes its aim from the code it is testing cannot miss

Defect `A` made the start button a fixed size at every display scaling —
`self.scale(START_BUTTON_WIDTH).min(bar.w)` became
`START_BUTTON_WIDTH.min(bar.w)`, so on a 200%-scaled display the button is half
the width of the icon drawn in it. The test named for exactly that:

```rust
fn the_start_button_is_clickable_where_it_is_drawn_at_every_scale() {
    for percent in [100, 125, 150, 200] {
        let mut shell = scaled(percent);
        let start = shell.start_button_rect();
        assert_eq!(click_at(&mut shell, start), ShellAction::Consumed);
        assert!(shell.start_menu_open, "at {percent}% scaling");
    }
}
```

It clicks the middle of `start_button_rect()` and asks whether the start button
was hit. Shrink the button and the aim shrinks with it: the middle of a small
rectangle is still inside the small rectangle. **The expectation is computed
from the function under test, so no value of that function can falsify it.**
This is lesson 22 (*derived expectation*) again, and the reason it is worth a
second number is the structural finding underneath it.

**There is no second view of this geometry anywhere in the crate.** `hit_test`
decides between chrome by calling the accessors —
`self.start_button_rect().contains(x, y)`, `self.start_menu_row_rect(row)
.contains(x, y)`, and so on down the list — and `render_taskbar` fills
`self.start_button_rect()`. Renderer and hit test are not two witnesses that
could disagree; they are one witness called twice. So the whole
"clickable-where-it-is-drawn" family of tests can only ever prove that
`hit_test` *scans in the right order and returns the right variant*, which is
real but is not what the names say.

The fix is the only independent view available: arithmetic written down in the
test.

```rust
let expected = START_BUTTON_WIDTH * f32::from(percent) / 100.0;
assert!((start.w - expected).abs() < 0.01, ...);
assert!((start.h - shell.taskbar_thickness()).abs() < 0.01, ...);
```

Not elegant, and deliberately so — it duplicates `scale()`'s multiplication,
which is the point. A test that re-derives the answer is a test that agrees
with the code by construction.

Two other members of the family were checked and are sound.
`the_clocks_target_covers_the_reading_that_is_drawn_at_every_scaling` reads the
clock's coordinates **out of the render tree** and compares them with
`clock_rect()` — two views, because the renderer places the text from
`bar.w - padding - clock_width()` rather than from the rect.
`every_power_action_is_clickable_where_it_is_drawn_at_every_scale` makes
independent claims (the popup is on the screen, the power button is inside the
menu, every action fits without a scroll). Those two needed nothing.

### Lesson 32: checking that two rows *meet* does not check that they meet *once*

Defect `E` laid the start-menu rows out a pixel short — `row as f32 * height`
became `row as f32 * (height - 1.0)` — so every row overlaps its neighbour and a
click near a boundary is ambiguous between two programs. The test is called
`adjacent_rows_do_not_share_a_pixel` and it escaped.

```rust
let first = shell.start_menu_row_rect(0);
let boundary = first.y + first.h;
assert!(!first.contains(first.x + 4.0, boundary));
assert!(shell.start_menu_row_rect(1).contains(first.x + 4.0, boundary));
```

Row 0 is unaffected (`0 * anything` is 0). Row 1 starts one pixel *early*, so it
still contains the boundary — it simply contains the pixel before it as well.
Both assertions hold. **The test states that there is no gap and says nothing at
all about there being no overlap**, which is the half its name is about.

A seam has two failure modes and they need two assertions. The repaired test
walks the first four rows and, for each pair, checks the pixel *at* the boundary
belongs to the next row and not this one, the pixel *before* it belongs to this
row and not the next, and — because two `contains` pairs still admit a pitch
wrong by less than half a pixel, which walks the last row off the bottom of a
long menu — that `next.y` equals `this.y + this.h` as arithmetic.

### Lesson 33: a clamp has two ends, and the far one is the end that runs away

Defect `C` deleted the taskbar's height clamp: `height.min(self.screen_height as
f32)` became `height`. The bar's thickness is what the *user* asked for (48
logical pixels by default, 96 at 200% scaling), so on a screen shorter than that
the bar hangs off the bottom — a rectangle that answers `contains` for
coordinates the display does not have. The test is
`a_screen_smaller_than_the_taskbar_does_not_invert_the_geometry`, it builds a
200×20 shell, and it escaped.

It asserts `bar.y == 0.0`. It never mentions `bar.h`. Every other thing it
checks is derived from `bar.y` — `work_area().3`, the menu height, the visible
row count — so removing the clamp on the *height* leaves all of them intact.

The property worth stating is not "y is 0" but that the bar sits on the bottom
edge, `bar.y + bar.h == screen_height`, which is exactly true in both regimes:
on a normal screen `y = h_screen − h_bar` and on a tiny one `y = 0` with `h`
clamped. Its sibling `the_taskbar_grows_with_the_scaling_and_takes_the_room_from
_the_work_area` already asserted it for normal screens; the small-screen test
now asserts the same thing, which is the shape a boundary test should have —
*the same invariant, at the boundary*, not a different and weaker one.

### Lesson 34: an unreachable arm looks exactly like a covered one

Defect `S` made the shell leak a click when the button under it had vanished:
in `handle_press`, the taskbar-button arm's `None => ShellAction::Consumed`
became `ShellAction::Pass`. There is a test whose whole subject is that case —
`a_taskbar_button_whose_window_has_gone_swallows_the_click` — and it escaped,
because **the arm the defect patched cannot execute.**

```rust
// hit_test
for index in 0..self.taskbar_windows().len() {
    if self.taskbar_button_rect(index).contains(x, y) {
        return Hit::TaskbarButton(index);
    }
}
return Hit::TaskbarPanel;

// handle_press, in the same call, on the same &mut self
Hit::TaskbarButton(index) => match self.taskbar_windows().get(index).map(|w| w.id) {
    Some(id) => ...,
    // The button is gone from under the click -- the window closed between
    // the frame it was drawn in and this press.
    None => ShellAction::Consumed,
}
```

`hit_test` only ever reports an index it has just found a window for, and
`handle_press` re-reads the same list in the same call with no opportunity for
anything to change it — `ShellSession` deliberately dispatches input *before*
folding in a new window list, which is design-decisions §503 and is why the
race the comment describes cannot happen. So the `None` arm is dead code with a
plausible-sounding comment, and the test named after it walks straight past:
after the window list empties, `hit_test` finds no button at all and returns
`Hit::TaskbarPanel`, whose arm is a different `Consumed` on a different line.

**The test was green, the behaviour was real, and the route between them was
imaginary.** That is the trap: an unreachable arm is indistinguishable from a
covered one in every artefact a test suite produces. Only a defect injected
*into that arm specifically* can tell them apart — which is what the harness is
for, and is the second unreachable branch it has surfaced in this file this
week (the `Activate`-rather-than-`Restore` care in the same match was unreachable
for as long as minimising a window deleted its taskbar button; see
`BUG-C-MINIMIZING-A-WINDOW-MADE-IT-UNREACHABLE`).

**The fix is to make the state unrepresentable rather than merely unvisited.**
`Hit::TaskbarButton` now carries the `WindowId` instead of the slot number:

```rust
for (index, window) in self.taskbar_windows().iter().enumerate() {
    if self.taskbar_button_rect(index).contains(x, y) {
        return Hit::TaskbarButton(window.id);
    }
}
```

The slot is resolved to a window while the list that produced the rectangle is
still in hand, so there is no second lookup, no `None`, and no arm to write a
comment about. Deleting the arm and leaving the index would have been the
smaller change and the wrong one: it leaves the representation able to express
"a slot with nothing in it" and relies on nobody ever constructing one.

**A tautology fell out with it.** `a_taskbar_button_is_clickable_where_it_is
_drawn` said:

```rust
for index in 0..shell.taskbar_windows().len() {
    let (x, y) = centre(shell.taskbar_button_rect(index));
    assert_eq!(shell.hit_test(x, y), Hit::TaskbarButton(index));
}
```

which compares the slot number with itself. No arrangement of windows can make
it false — it is lesson 31's derived aim carried all the way to the assertion.
With ids it becomes a claim about the *pairing*: capture `a`, `b`, `c` from
`open()` and require the button in slot 0 to name `a`. That is the thing that
would actually strand a click on the wrong program, and it is now guarded by a
new defect `W` (every button reports the first window on the bar), which five
tests catch.

Two consequential renames. `a_taskbar_button_whose_window_has_gone_swallows_the
_click` became `a_click_where_a_button_used_to_be_is_still_the_taskbars`,
because the panel is what answers and the button never did; the behaviour it
checks — bare taskbar keeps a click rather than raising whatever is behind the
bar — is real and worth a test, and defect `S` was retargeted to the line that
produces it (`return Hit::TaskbarPanel` → `Hit::Desktop`).

### A test that the bug fix gave teeth to

`a_click_off_the_calendar_closes_it_without_acting` opens a window, minimises
it, opens the calendar, and clicks the minimised window's taskbar button,
asserting the click is spent dismissing the popup and the window stays away.
Before `1b773bb71` the minimised window had *no button*, so the click landed on
bare taskbar and "stays minimised" was true because nothing could possibly have
acted on it — vacuous, in the same file and the same area as the two tests that
asserted the stranded-window bug outright. Fixing the bug gave the test its
subject back without a line of it changing. Recorded because it is the pleasant
case of the same phenomenon: **a test's strength depends on the code around it,
so a test can silently lose its meaning when unrelated code changes, and
silently regain it.** Nothing in a green suite reports either direction.

### Result

```
23 defects: 23 caught, 0 escaped, 0 never asked, 0 under-caught,
0 under-declared
restored: all files match their recorded SHA-256
```

Every repair was re-run **in contact** — the defect reintroduced against the
fixed test — rather than merely re-checked at preflight, because a test that
has been strengthened is a test whose new assertion has itself never been seen
to fail.

**Net:** `pointer_tests.rs` goes from **69 of 69 unproved** to **26 of 72** (the
three extra tests came with the minimised-window fix). Across the swept
packages, unproved falls **2419 → 2371** and proved rises **17.7 % → 19.3 %**,
with single-prover tests up 135 → 166. The defect list is **1760 defects, 0
stale, 0 ambiguous, 0 no-op**. 2882 desktop tests green, clippy `--all-targets`
clean, `cargo fmt` clean.

**The shape of the four escapes is worth keeping.** None of them was a logic
defect — the eighteen logic defects (scroll direction, scroll clamping, menu
category filters, dismissal rules, press/release/right-click routing, window-list
replacement, layer filtering) were all caught, several by five or more tests.
All four escapes were **geometry or reachability**: two tests that aimed with
the code they tested, one that checked half a boundary, one that guarded a
branch the program cannot enter. A file at 0 % proved is not uniformly weak;
it is weak in whichever dimension nobody has yet pushed on, and this file's
weak dimension was *where things are*, not *what happens when you click them*.
