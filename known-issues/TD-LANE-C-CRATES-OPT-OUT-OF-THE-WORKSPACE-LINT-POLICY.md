## TD-LANE-C-CRATES-OPT-OUT-OF-THE-WORKSPACE-LINT-POLICY

**Found 2026-08-14.** CLAUDE.md requires every crate to build under
`clippy::all` deny / `pedantic` warn plus the five defensive lints
(`unwrap_used`, `expect_used`, `panic`, `indexing_slicing`,
`arithmetic_side_effects`). The workspace declares exactly that in
`[workspace.lints.clippy]`, but a crate only receives it by opting in with
`[lints] workspace = true` in its own `Cargo.toml` — and **131 of lane C's
crates do not**: all 123 under `apps/`, and `gui/clipboard`, `gui/compositor`,
`gui/credentials`, `gui/desktop`, `gui/notifications`, `gui/remote`,
`gui/toolkit`, `gui/window`. The only lane-C crate that opts in is the new
`gui/appearance`.

So "clippy clean" for those crates means clean under *rustc's defaults* —
no pedantic lints, and none of the five that exist specifically to catch the
panic and overflow risks an AI author is prone to. This is not theoretical:
the very first file to cross into an opted-in crate brought a real one with
it. `color_from_hex` had `digits.get(i..i + 2)`, an unchecked add that had
lived in `gui/desktop` without complaint and warned immediately in
`gui/appearance` (fixed in the same commit, 91f1a15cf).

**Proper fix.** Add `[lints]\nworkspace = true` to each crate and fix what it
reports. This is not a mechanical sweep: the count above means a large
backlog of genuine findings, and test modules will need the standard
`#[allow(...)]` block (panicking on bad data is a test's job) — `gui/desktop`
already carries an equivalent as a crate-level `cfg_attr(test, allow(...))`.
Do it crate by crate, smallest first, so each addition is one reviewable
commit rather than a thousand-warning wall. Prioritise the crates that parse
untrusted input — `gui/compositor` (protocol messages), `gui/remote`,
`gui/credentials` — over the games.

**Where.** `Cargo.toml` (`[workspace.lints.clippy]` at :105) and the
`Cargo.toml` of every crate listed above. Reproduce the list with:
`for f in gui/*/Cargo.toml apps/*/Cargo.toml; do grep -q "workspace = true" "$f" || echo "$f"; done`
