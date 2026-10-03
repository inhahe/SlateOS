## A-CFG-UNIX-GATE-CAN-GO-RED-ON-A-TOOLCHAIN-UPDATE-ALONE — OPEN 2026-09-02

**Lane:** A. **Severity:** low probability, high blast radius — all three lanes
at once, with no commit to blame.

**What it is.** `check_cfg_unix` in `scripts/boot-test.sh` runs
`cargo clippy --workspace --target x86_64-unknown-linux-gnu` as of
design-decisions.md §678. Because several crates declare
`#![deny(clippy::all)]`, a lint that fires is a hard failure of the gate, and the
gate is `--workspace` rather than per-lane. The usual cross-lane hazard — one
lane's denied lint failing another lane's boot test — is covered: a green boot
test gates every merge to `main`, so the lane that introduces a lint hits it
first, and that lane cannot merge red.

**What is not covered.** A **toolchain update** can add a lint to `clippy::all`,
or make an existing one fire more widely, with nobody having changed a line. In
that case all three lanes go red simultaneously and the last commit in each is
innocent. `cargo check` has a weaker form of this, but rustc's error set moves
far more slowly than clippy's lint set, so this is a new failure mode rather than
a louder one. It was accepted knowingly in §678.

**How to recognise it in one read** — the reason this entry exists. All three
symptoms together:

1. `cfg(unix) OK` is absent and the gate exits 1, in **more than one lane** at
   about the same time;
2. the errors in `build/check-cfg-unix.log` are of the bare
   `error: <lint text>` form — *no* bracketed `error[E0nnn]` code — which means
   clippy denials, not compile failures;
3. `git log` shows nothing touching the named files, and `rustup show` /
   `rustc --version` moved recently.

**What to do.** In order of preference:

1. Fix the lint. It is usually real and usually a two-line change.
2. If it is a new pedantic lint firing across hundreds of sites and the tree is
   otherwise green, add a targeted `#![allow(clippy::<name>)]` to the affected
   crate with a comment naming the toolchain version, and open a follow-up.
3. If it is blocking all three lanes and neither of the above is quick, revert
   the verb — `clippy` → `check` on the one line in `check_cfg_unix` — which
   restores the gate to exactly what it was before §678, and note it here. This
   is the documented escape hatch; it is one word by design.

**What the proper fix would be.** Pin the clippy lint set rather than tracking
whatever the installed toolchain ships — e.g. a `rust-toolchain.toml` pinning a
known-good nightly for this gate specifically, so a toolchain bump becomes a
deliberate commit that one lane absorbs rather than an event that hits three
lanes at once. Not done now because this tree does not currently pin a toolchain
anywhere, and introducing pinning for one gate would be the first half of a
decision that ought to be made whole.
