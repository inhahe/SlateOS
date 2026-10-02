### TD-C-THREE-TEST-MODULES-HAND-ROLL-THE-ACCENT-LIST-THAT-A-HELPER-ALREADY-RETURNS — 2026-08-24 — OPEN

**In short.** Small duplication, no user-visible symptom, but it is the exact
shape of defect that produced the two switch/slider bugs above: a correct
answer exists in one place, callers did not find it, so they wrote their own
copy. `appearance::AccentColor::presets()` returns the fourteen accent colours
the appearance page offers. Three test modules declare their own
`const OFFERED: [AccentColor; 14]` listing the same fourteen by hand.

**Where:** `gui/desktop/src/accessibility_settings.rs:1177`,
`gui/desktop/src/display_settings.rs:1390`, `gui/desktop/src/snap.rs:1121`.

**Why it matters despite being test-only.** These lists exist so that a test
sweeps every accent a user can pick. A fifteenth accent added to `presets()`
would be covered by every test that calls the helper and silently *not* covered
by these three, which would keep passing while checking a stale set — a test
that has quietly stopped testing what its name says. That is worse than a
compile error.

**The proper fix:** delete all three constants and iterate
`AccentColor::presets()`. `gui/desktop/src/switch.rs` and
`gui/desktop/src/slider.rs` already do, and their doc comments state the reason
("a hue added there is covered here without anyone remembering to"), so the
pattern to copy exists.

**If never fixed:** nothing breaks today. The cost lands entirely on whoever
adds the fifteenth accent, in the form of three tests that pass and should not
have.
