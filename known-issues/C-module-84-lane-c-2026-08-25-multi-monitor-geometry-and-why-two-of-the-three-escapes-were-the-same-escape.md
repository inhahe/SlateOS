## MODULE 84 (lane C, 2026-08-25) — multi-monitor geometry, and why two of the three escapes were the same escape

**In short:** `gui/desktop/src/multimon.rs` is the file that decides where the
desktop is when more than one screen is plugged in — how the monitors line up
into one coordinate space, which one a new window opens on, what happens when
one is unplugged. Twenty-six deliberate faults were introduced into it one at a
time to find out which of its tests would notice. Twenty-three did. The three
that got through were all cases where the test suite had never built the *input*
that makes the code's decision matter, and two of those three were literally the
same mistake in two places.

**The first pass, 26 defects:**

```
26 defects: 23 caught, 3 escaped, 0 never asked, 0 under-caught,
3 under-declared
restored: all files match their recorded SHA-256
```

The twenty-three that were caught cover the substance of the module and are
worth naming, because they are what says the file is in good shape: reversed
rectangle corners, edge-inclusive `contains`, the desktop bounding box over
enabled and disabled monitors, rotation swapping width and height, the DPI
arithmetic in both directions and its divide-by-zero guard, every branch of the
monitor-drag snapping (threshold equality, best-candidate replacement, both
axes, snap-by-gap rather than snap-to-edge, disabled monitors not attracting a
drag), gap detection, horizontal and mirrored arrangement, primary-only mode,
duplicate hotplug, and unplugging the primary. Several were caught by five or
more tests.

### Lesson 35: a mitigation is untested until a test creates the hazard it mitigates

Defect `A` reversed the direction of the module's saturating narrow:

```rust
-    i32::try_from(v).unwrap_or(if v.is_negative() { i32::MIN } else { i32::MAX })
+    i32::try_from(v).unwrap_or(if v.is_negative() { i32::MAX } else { i32::MIN })
```

`narrow` exists because every rectangle computation in the file widens to `i64`
first, so that an intermediate cannot wrap; `narrow` is the single place the
value comes back down, and clamping it the wrong way reintroduces exactly the
wrap the widening was written to prevent — a monitor dragged off the left edge
of the coordinate space reappears at the right. It is a two-line function on the
path of every public constructor in the module, and **not one test noticed.**

Not because the function was unreached — `VirtualRect::from_corners` calls it on
every construction, and dozens of tests construct rectangles. It was unreached
*in the branch that matters*. Every test in the file used screen-shaped numbers:
0, 1920, 3840, −1080. `i32::try_from` succeeds for all of them, so every test
took the `Ok` path, and the `unwrap_or` — the whole point of the function — was
dead as far as the suite was concerned.

**This generalises past this file.** Overflow guards, fallbacks, retry paths,
error branches, `saturating_*`, the `else` of a bounds check: all of them are
code that only runs on input the rest of the program is trying to avoid, which
is the same input a test author is not naturally holding in mind while writing
fixtures. A suite assembled from realistic values tests the realistic path
exhaustively and the defensive path not at all — and the defensive path is
precisely the one whose failure mode is silent and weird rather than loud.

The fix tests `narrow` directly at the four boundaries and then again through
the public surface, so the private helper's contract is pinned and the route
from a caller to it is pinned too:

```rust
assert_eq!(narrow(i64::from(i32::MAX) + 1), i32::MAX);
assert_eq!(narrow(i64::from(i32::MIN) - 1), i32::MIN);
assert_eq!(narrow(i64::MIN), i32::MIN);
assert_eq!(narrow(0), 0);

let r = VirtualRect::from_corners(i64::from(i32::MIN) - 5_000, -9, 10, 11);
assert_eq!(r.x, i32::MIN);   // still on the left
assert_eq!(r.y, -9);
```

### Lesson 36: a compound condition is only proved where its operands disagree

Two of the three escapes were this, and finding the same hole twice in one
module is the reason it gets a number rather than a sentence.

Defect `E` turned an `||` into an `&&`:

```rust
-        self.w == 0 || self.h == 0     // is_empty
+        self.w == 0 && self.h == 0
```

Defect `K` deleted a conjunct:

```rust
-        self.monitors.iter().find(|m| m.primary && m.enabled)
+        self.monitors.iter().find(|m| m.primary)
```

Different operators, opposite directions, same reason for escaping: **no test
ever supplied an input on which the two operands differ.** Every existing
`is_empty` test used a 0×0 rectangle or a fully-sized one, and on those `||` and
`&&` return the same answer. Every existing layout fixture marked its primary
monitor enabled, and on those `primary` and `primary && enabled` return the same
monitor. The tests were not weak in general — they were blind along one specific
axis, and the axis is stated exactly by the boolean structure of the code.

That gives a mechanical rule that does not require insight into the subject
matter: **for `A op B`, a suite that never runs a case with `A != B` cannot
distinguish `&&` from `||` from `A` alone from `B` alone.** It is checkable by
reading the condition, and it says which fixture is missing rather than merely
that one is.

Both escapes also had a real user-visible consequence, which is worth recording
because "a predicate is under-tested" sounds academic until you follow it out:

* A 1920×0 strip is empty — it covers no pixels. Under `&&` it reports itself
  non-empty, and every caller that guards a division by the desktop's size
  (scaling a wallpaper, computing what fraction of the desktop a window
  occupies) divides by zero.
* A laptop panel that is closed for the evening is still the primary display; the
  primary flag deliberately survives being switched off, so that it comes back
  when the lid opens. `suggest_default_monitor` consults `primary()` *before* it
  considers area, so a disabled primary is not filtered downstream — every new
  window opens on a screen showing nothing.

The fixes state the disagreeing case directly:

```rust
assert!(VirtualRect::new(0, 0, 1920, 0).is_empty());
assert!(VirtualRect::new(0, 0, 0, 1080).is_empty());
assert!(!VirtualRect::new(0, 0, 1, 1).is_empty());
```

```rust
layout.monitors[0].enabled = false;                 // still flagged primary
assert!(layout.primary().is_none(), "it is showing nothing");
assert_eq!(WindowPlacement::suggest_default_monitor(&layout), Some(MonitorId(2)));
layout.monitors[0].enabled = true;                  // and it comes back
assert_eq!(layout.primary().map(|m| m.id), Some(MonitorId(1)));
```

### Result

```
26 defects: 26 caught, 0 escaped, 0 never asked, 0 under-caught,
0 under-declared
```

**Contrast with module 83.** That module's escapes were about *reachability* —
tests aiming with the code they tested, an arm the program could not enter.
This module's were about *input coverage* — the code was reached, the assertion
was real, and the value fed in could not tell the right answer from the wrong
one. Both are invisible in a green suite, but they call for opposite responses:
83's wanted the code restructured so the untestable state stopped existing;
84's wanted three more fixtures and no production change at all beyond the two
lines the defects had touched, which were already correct.
