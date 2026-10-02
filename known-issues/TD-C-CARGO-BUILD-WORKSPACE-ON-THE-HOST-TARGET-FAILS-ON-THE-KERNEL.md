## TD-C-CARGO-BUILD-WORKSPACE-ON-THE-HOST-TARGET-FAILS-ON-THE-KERNEL

**Date:** 2026-09-09. **Lane:** C.
**Where:** not a code defect — a trap in the prescribed workflow.

**In short:** running the whole-project build the way the instructions describe
fails with a wall of linker errors that look alarming and have nothing to do
with whatever you just changed. It is trying to build the kernel — a bare-metal
binary with its own linker script — as an ordinary Windows program, which
cannot work. Everything else builds fine.

**The command and the result:**

```
cargo build --workspace --target x86_64-pc-windows-gnu
  → error: linking with `x86_64-w64-mingw32-gcc` failed
    relocation truncated to fit: IMAGE_REL_AMD64_ADDR32NB
    ... `-T kernel/linker.ld`
  → error: could not compile `kernel` (bin "kernel")
```

**Why it is reachable by accident.** Both halves are what an agent is told to
do: `CLAUDE.md` says to run the workspace build/test before merging, and every
`cargo` invocation in this tree needs `--target x86_64-pc-windows-gnu` because
the host is Windows. Put together they ask for the kernel to be linked for the
host, with `kernel/linker.ld`, against mingw's CRT.

**`cargo check` is unaffected — measured, not assumed.**
`cargo check --workspace --target x86_64-pc-windows-gnu` exits **0** on the same
tree. `check` type-checks without linking, and the kernel's failure is entirely
at the link step, so the whole workspace checks cleanly. That matters because it
is `cargo check --workspace` that C-Q11 proposes as the pre-merge gate: the gate
being considered works today, and only the `build` spelling of it does not.

**What to run instead**, when the point is "did I break anything outside my own
crate":

```
cargo check --workspace --target x86_64-pc-windows-gnu            # works
cargo build --workspace --exclude kernel --target …               # if you need artifacts
```

**Why this is worth writing down rather than just knowing.** The failure names
`libmsvcrt.a`, relocations and a linker script — none of which appear in a GUI
change — so the natural first reaction is that something is badly wrong, and the
natural second is to start bisecting a change that is innocent. It cost lane C a
detour today on the way to merging a caret-width change.

**~~Proper fix (not done)~~: the kernel package could carry**
**`forced-target`. Measured 2026-09-13: it cannot, on this toolchain.**
The suggestion above was written from the cargo documentation without
checking which channel the feature is on, and it is wrong. Kept rather than
deleted because the wrong fix is the part that would have cost somebody an
afternoon, and because a lane that must not edit `kernel/Cargo.toml` filing
a request for an impossible change is worse than filing nothing.

Three things were measured, in a scratch workspace outside the tree rather
than by editing lane A's manifest:

| mechanism | on stable 1.95 | verdict |
|---|---|---|
| `forced-target` (`cargo-features = ["per-package-target"]`) | *"requires a nightly version of Cargo, but this is the `stable` channel"* | **unavailable** |
| the same on `+nightly` | works — the bare crate lands in `target/x86_64-unknown-none/` while the rest lands in `target/x86_64-pc-windows-gnu/` | works, but would put the whole tree on nightly |
| `required-features` on the `[[bin]]` | cargo **silently skips** the binary. No warning, no note, `Finished` as if nothing were missing | works, and is worse |

That last row is the one that settles it. `required-features` would make
`cargo build --workspace` clean on stable today, and the price is that a
kernel build which forgot the feature flag would exit **0 having built no
kernel**. Trading a loud linker error for a silent omission is the opposite
of what this file exists to prevent — it is the same shape as a test runner
reporting PASS over zero targets, which cost this lane a real defect
earlier the same day.

**And the problem is smaller than this entry implies, because half of it is
already solved and the entry did not say so.** `kernel/Cargo.toml` carries

```toml
[[bin]]
name = "kernel"
test = false   # "so `cargo test --workspace` is clean on a normal dev host"
```

so `cargo test --workspace --target x86_64-pc-windows-gnu` **works**, and is
what `scripts/workspace-test.py` runs — 581 targets, repeatedly, all day.
Together with the `cargo check` result already recorded above, that means:

| spelling | state |
|---|---|
| `cargo check --workspace --target …` | works |
| `cargo test --workspace --target …` | works (this is the pre-merge run) |
| `cargo build --workspace --target …` | fails at the kernel's link step |

Neither gate anyone has proposed uses the `build` spelling: C-Q11's option A
is `cargo check`, and the actual pre-merge run is `cargo test`. So what is
left is a trap in a *sentence*, not in the build — someone reading
"`cargo build --workspace`" in `CLAUDE.md` literally, hitting a wall of
mingw relocation errors, and bisecting an innocent change.

**So the fix is not in `kernel/Cargo.toml` at all**, and no request has been
filed against lane A. It is either a line in `CLAUDE.md` naming the
spellings that work, which only the operator may add, or nothing — this
entry, findable by anyone who hits the wall, may be the whole of the answer
a once-a-month trap deserves.
