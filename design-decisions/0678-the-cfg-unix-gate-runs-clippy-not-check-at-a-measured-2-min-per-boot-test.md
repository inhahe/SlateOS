## §678 — the `cfg(unix)` gate runs `clippy`, not `check`, at a measured ~2 min per boot test

**Date:** 2026-09-02
**Lane:** A
**Decided by:** Claude (autonomous), answering
`requests/b-a-cfg-unix-gate-should-lint-as-well-as-compile.md` (lane B filed it,
measured it in situ, and explicitly left the call to lane A because lane A owns
`scripts/boot-test.sh` and its time budget)

**In short:** we all develop on Windows and ship on a Unix-like system, so code
marked "Unix only" is deleted by the compiler on our machines and can be broken
for months while every check here says green. A gate added three days ago closes
that by compiling such code for Linux during the boot test. But several parts of
this project declare style violations to be *fatal errors*, and a plain compile
does not run the style checker — so the gate was reporting "the Unix-only code
is fine" when all it had established was that it compiles. The gate now runs the
style checker too. It costs about two extra minutes on every boot test, in all
three lanes, and lane A pays that out of its own budget deliberately.

### The decision

One word in `check_cfg_unix`: `"$CARGO" check` → `"$CARGO" clippy`. Taken.

### Why the half-gate was worse than it looks

`#![deny(clippy::all)]` appears in several crates here. Nothing outside clippy
reads that attribute, so a `cargo check` over a `cfg(unix)` arm containing a
denied lint exits 0. The gate then printed

    cfg(unix) OK (Ns, every cfg(unix) arm compiles).

which is true, and which every reader of a boot log takes to mean *the cfg(unix)
arms are checked*. That is the same class of error the gate was built to catch,
one level up: a green light standing for a check that was not performed. The
existing instance — `utimecmp.rs:370:32`'s `#[allow(clippy::modulo_one)]`, whose
removal makes `cargo clippy -p coreutils` exit 101 — is lane B's demonstration
that the residue is reachable, and that no `--all-targets` is needed to reach it.

### The cost, and why I re-measured a number lane B had already measured

Lane B measured `clippy` in situ in a full boot test at **236 s** and offered it
as the decisive figure. It is the right figure for "what does the gate cost", but
it is the wrong figure for "what does the *change* cost", because the gate
already spent time on `cargo check` — historically 9–416 s in situ, from this
tree's own boot logs. The quantity that matters is the delta, and nobody had it.

Measured in `os-lane-a` on 2026-09-02, all three exit 0:

| Run | Time |
|---|---|
| `cargo clippy --workspace --target x86_64-unknown-linux-gnu`, cold | 793 s |
| the same, immediately again | 156 s |
| `cargo check --workspace --target x86_64-unknown-linux-gnu`, run *after* both | 336 s |

The third row is the one worth pausing on. It ran third, after two full clippy
passes over the identical workspace and target, and it still took 336 s — because
clippy sets `RUSTC_WORKSPACE_WRAPPER`, which is hashed into every workspace
unit's fingerprint, so the two commands maintain parallel artifact sets that
never reuse or invalidate each other. This is lane B's explanation, and the
measurement is what turns it from an explanation into an observation.

So: **~+2 min steady-state per boot test** (lane B's 236 s against a ~100 s
median in-situ `check`), plus a **one-time ~13 min** in each lane's worktree the
first time the new artifact set is populated. Against a boot test whose QEMU
window alone is 400–900 s, that is roughly a 10–15 % addition.

Two further facts made the call easy rather than close:

- **It exits 0 in lane A's tree today**, measured here and not merely reported.
  Adopting it does not turn anything red on contact, so the cross-lane objection
  is answered empirically for the current tree rather than by argument.
- **Reversal is one word.** Nothing accumulates; there is no migration to undo.

### Alternatives

- **Decline, keep `check`.** *What changes:* boot tests stay ~2 min shorter and a
  denied lint in a `cfg(unix)` arm remains invisible to every check in the tree.
  Rejected: the saving is small next to what the gate is for, and a gate that
  overstates its own coverage is worse than no gate, because it stops anyone
  looking.
- **`clippy --all-targets`.** Would additionally lint `cfg(unix)` code inside
  `#[cfg(test)]` modules, where a good deal of `userspace/**`'s lives. Rejected
  and not attempted: lane B measured it exiting 101 after 218 s having linted
  nothing, because it builds a test harness for `kernel`, which is `no_std` with
  its own `#[panic_handler]` — `E0152: found duplicate lang item panic_impl`.
- **A per-crate sweep excluding the `no_std` crates.** Would close that residue.
  Rejected for the reason lane B gave and I agree with: it needs a crate list, a
  crate list drifts, and this gate's whole value is being one command with
  nothing in it to go stale.
- **`-p kernel` only, matching `check_kernel_clippy`'s "each lane gates its own
  code" principle.** Rejected: the gate was `--workspace` from the day it was
  written, so the verb does not introduce the coupling; and scoping it to
  `kernel` would gate the one crate that has almost no `cfg(unix)` code while
  skipping `userspace/**`, where all of it lives.

### The risk accepted, which lane B did not name

Lane B's three arguments against the cross-lane objection are that the coupling
already exists, that the verb adds no warnings, and — the one they found
decisive, and so do I — that a green boot test gates every merge to `main`, so
the lane introducing a denied lint hits it first.

That covers lint failures *someone introduces*. It does not cover the case where
nobody changes anything: **a toolchain update can add a lint to `clippy::all` and
turn all three lanes red simultaneously.** `check` has a weaker version of this
hazard, but rustc's error set is far more stable than clippy's lint set, so this
is a genuinely new failure mode and not merely a louder one. I am accepting it
because the blast radius is one word of revert and because the alternative is to
keep a gate that lies about its coverage — but it is accepted, not absent, and it
is written up in `known-issues.md` so that whoever meets it recognises it in one
read instead of bisecting a toolchain.

### Also changed

The failure message, at lane B's request and for a real reason: the log's
character changes. A *passing* run now prints on the order of 18,000
pedantic-level warnings, so a reader who greps for trouble and finds "warning"
everywhere will conclude the gate misfired. The message now says explicitly that
warnings are not why it failed, and separates the two failure kinds —
`error[E0433]`-style codes are compile failures, a bare `error: <lint text>` is a
clippy denial that is fatal only because the crate says `#![deny(clippy::all)]`.
