## TD-B-COREUTILS-IS-LINTED-AT-STOCK-CLIPPY-NOT-AT-THE-STANDARD-CLAUDE-MD-MANDATES (lane B, 2026-09-04)

**In short:** `CLAUDE.md` requires every crate to deny `clippy::all` *and*
`clippy::pedantic`, and to warn on five lints that catch panics and overflow
(`unwrap_used`, `expect_used`, `panic`, `indexing_slicing`,
`arithmetic_side_effects`). The workspace declares all of them in
`[workspace.lints]`. `userspace/coreutils` never opted in — its `Cargo.toml`
has no `[lints]` section, so it inherits none of them — and **85 of its 87
binaries carry no lint attribute of their own either**. They are being checked
at stock clippy defaults. `userspace/shell` is in the same state; `posix` is
not, so the correct pattern already exists in this lane.

**How it surfaced.** Not from a checker — from reading the noise. The
2026-09-04 gate run emitted 9,828 warnings, and I nearly dismissed them all as
belonging to the fabricated crates (4,912 are `missing [lints]` on those). Two
of them named `coreutils`. Cargo has a warn-by-default lint,
`cargo::missing_lints_inheritance`, that says precisely this — and it has been
saying it on every run, buried under 4,910 identical lines about other crates.

**Why it matters more than a normal lint gap.** `coreutils` is the largest
crate in lane B and the one whose code most resembles the things those five
lints exist to catch: byte slicing of user-supplied paths (`indexing_slicing`),
arithmetic on sizes and offsets parsed from the command line
(`arithmetic_side_effects`), and `unwrap` on syscall results. Every clean
`scripts/coreutils-check.sh` run I have reported — including the one certifying
the walk extraction in `4bb9da2fd` — graded against the weaker standard. The
runs are not *wrong*; they are narrower than their "clean" implies.

**This is Lane A's TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE shape**, arrived at
from the other end. There the gate was wired and checked one line; here the
gate is thorough but the *lint set it enforces* is a fraction of the mandated
one. Both produce the same artifact: a green result whose scope the reader
overestimates. And it is a sibling of
`TD-B-THE-UNIX-HALF-OF-COREUTILS-IS-NEITHER-LINTED-NOR-TESTED-BY-DEFAULT` —
same crate, same class, different axis (that one is about *which target*, this
is about *which lints*).

**The proper fix,** in the order the work has to happen:

1. Add `[lints] workspace = true` to `userspace/coreutils/Cargo.toml` and
   `userspace/shell/Cargo.toml`, matching `posix/Cargo.toml`.
2. Measure the fallout before fixing any of it. `pedantic` plus the five
   defensive lints across 87 binaries will produce a large number; that number
   is the actual size of this entry and is not yet known.
3. Fix, don't blanket-`allow`. Where a lint is genuinely wrong for this code,
   suppress it *individually with a comment*, per `CLAUDE.md` — a crate-level
   `allow` would re-create exactly the condition this entry describes, and
   would do it in a way that looks deliberate.
4. Delete `lib.rs`'s lone `#![deny(clippy::all)]` once inheritance covers it,
   so there is one place that says what the lint policy is.

**If it is never fixed:** nothing breaks and nothing regresses — the code is
as good as it was. What is lost is the assumption every "clean" in this file
has been written under. Treat past clean results for `coreutils` as "clean at
stock clippy", not "clean at the project standard", until step 2 has a number.
