## MODULE 85 (lane C, 2026-08-25) — the rest of multimon: the config file, window placement, and one hole in a round trip

**In short:** the other half of `gui/desktop/src/multimon.rs` — the code that
saves your monitor arrangement to a file and reads it back, decides where a new
window opens, and keeps windows from being dragged off the edge of the desktop.
Twenty-one deliberate faults, twenty caught. The one escape was in the
save-and-reload code, and it escaped a test that was *specifically written to
catch exactly that kind of fault* and had already caught three of its siblings.
The reason is worth a lesson: the test round-tripped two of the four screen
rotations, and the fault was in one of the other two.

**The pass, 21 defects:**

```
21 defects: 20 caught, 1 escaped, 0 never asked, 0 under-caught,
0 under-declared
```

Caught: every parser fault the module can have (a bare `[` treated as a section
header, the final section never flushed, a missing resolution, a resolution with
no `x` in it, a position split on the wrong separator, a disabled monitor written
out as enabled, the resolution separator itself), all of window placement
(centring, per-monitor offsets, proportional moves between monitors of different
sizes), all four edges of the keep-on-screen clamp including the
smaller-than-the-minimum panic and the no-monitors case, both branches of
choosing a monitor for a new window, and the manager's demote-the-old-primary,
scale clamp and hotplug bookkeeping. Ten of the twenty were caught by more than
one test.

### Lesson 37: a round trip proves the values you round-tripped, and nothing else

`Rotation` is written to the config with `as_str` and read back with
`from_str_config`. They are two hand-written lists of the same four words, and
the property that matters is that they are mutual inverses. There is a test for
exactly that — `config_save_load_roundtrip` — and it works: it caught defect `B`
(`Left` written as `"right"`), defect `C` (a disabled monitor written out as
enabled) and defect `D` (the resolution separator changed to a comma).

Defect `A` was the same shape as `B`:

```rust
-            Self::Inverted => "inverted",
+            Self::Inverted => "normal",
```

and it escaped, because the fixture the round-trip test uses carries a `Normal`
monitor and a `Left` one. `Right` and `Inverted` are never written, so nothing
observes what word they are written as.

**A serialiser is the worst possible case for sampling.** Most functions
degrade smoothly — get one input wrong and nearby inputs are usually wrong too,
so a sample stands a fair chance of landing on the fault. A serialiser built
from a `match` is the opposite: each arm is independent, and a wrong arm is
wrong for exactly one value and correct for every other. The round trip is a
strong *property*, and it is easy to read a passing round trip as having proved
the property rather than four instances of it. It proved two.

**The escape count understates the hole.** The harness staged one defect in this
family, so the sweep reported one escape — but `Self::Right => "left"` would
have escaped identically, and so would anything touching `from_str_config`'s
`"right"` or `"inverted"` arms. Half the enum was unobserved; the sweep could
only see the half of that half it happened to poke at. Escape counts are a lower
bound on the size of a hole, never a measurement of it.

**The fix is exhaustive, because for an enum it can be.** This is the one
situation where "cover the whole input space" is not an aspiration but a loop of
four iterations, which makes sampling an unforced choice rather than a
compromise:

```rust
const ALL: [Rotation; 4] = [Rotation::Normal, Rotation::Left,
                            Rotation::Right, Rotation::Inverted];

// The collision itself, stated directly rather than inferred from a
// failed round trip.
let mut labels: Vec<&str> = ALL.iter().map(|r| r.as_str()).collect();
let written = labels.len();
labels.sort_unstable();
labels.dedup();
assert_eq!(labels.len(), written, "two rotations share a config label");

for rotation in ALL {
    // Exhaustiveness guard: a fifth variant stops this match compiling
    // until it is listed in `ALL` above.
    match rotation {
        Rotation::Normal | Rotation::Left | Rotation::Right | Rotation::Inverted => {}
    }
    // …save a config carrying `rotation`, load it back, assert it survived.
}
```

Two deliberate details. The **label-collision assertion** is separate from the
round trip because it names the failure directly — "two rotations share a config
label" is a better diagnostic than "expected Inverted, got Normal", and it holds
even for a variant that some future refactor stops round-tripping. The
**exhaustiveness guard** is there because `ALL` is a hand-written list, and a
hand-written list of variants is precisely the thing that silently falls behind
the enum; the `match` makes adding a fifth rotation a compile error here rather
than a fresh gap.

**Where this generalises.** Any `T -> String -> T` pair over a closed set — the
rotation labels here, and by inspection the same pattern in the theme-mode,
scaling-mode and panel-position settings elsewhere in the crate — should be
tested over the whole set, not over a plausible-looking sample. The cost is a
`for` loop; the thing it buys is that a collision, which is invisible in every
other artefact, becomes a compile-time-adjacent certainty.

### Result

```
21 defects: 21 caught, 0 escaped, 0 never asked, 0 under-caught,
0 under-declared
```

`multimon.rs` is now 50 of 75 tests proved (it was 0 of 75 before module 84).
The swept corpus stands at **2943 tests, 2325 unproved — 21.0 % proved, 0
dangling, 202 single-prover**; the last figure recorded above, before modules
83–85, was 15.7 %.
