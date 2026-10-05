## `B-DEV-HOST-IS-WINDOWS-SO-CFG-UNIX-CODE-IS-NEVER-COMPILED` (lane B, 2026-08-26) — **open**, process gap

**Measured 2026-09-12: the remaining gap is latent, not live.** Lane A reports
that `scripts/check-cfg-unix.py` never gained `--all-targets` and does not
accept the flag, so the pre-push gate compiles default targets only while
`boot-test.sh` runs cargo directly with `--all-targets --exclude kernel` over
the whole workspace. That means `#[cfg(unix)]` code inside a `#[cfg(test)]`
module is checked at boot and not at push.

Whether that currently hides anything is a question nobody had asked, so:

| check | result |
|---|---|
| `cargo check --workspace --exclude kernel --target x86_64-unknown-linux-gnu` | 0 errors |
| same, plus `--all-targets` | 0 errors, `Finished`, exit 0 |

So no `cfg(unix)` code in a test target fails to compile for a unix today. The
tooling gap is real and worth closing — it is a push-time blind spot in exactly
the class of code this entry exists for — but it is a ratchet to install, not a
fire.

**2026-09-14: the gap stopped being latent, and cost 2004 seconds.** Lane C's
`1db3c7cc8` put a clippy denial -- `field_reassign_with_default` -- in the test
target of `apps/diskcleanup`. It **passed the push gate** and killed lane A's
boot at gate 86, before QEMU, after 33 minutes of gates. Two witnesses were
unvalidated as a result and had to be re-run.

**The predicate was never the problem; the population is.** This script does run
`cargo clippy` with `--all-targets`, so the ratchet recorded above is real and
still installed. What it does not do is *look at the crate*:
`crates_with_unix_code()` derives the list by scanning for `#[cfg(unix)]` blocks,
and `apps/diskcleanup` contains **zero** of them. The failing test is unix-only
because it pulls in `oswindow`, not because it carries a unix gate -- so the
crate is absent from the list entirely, and `--all-targets` cannot help a crate
that is never named.

That is 937's **population blindness** in its purest form: right predicate, wrong
scope, and widening the scope fixes it mechanically.

**This was predicted, in this script's own docstring, and filed as hypothetical.**
It names "a denial in the test target of `apps/launcher`, a crate with no
`#[cfg(unix)]` code at all" as the concrete case. `apps/launcher` also has zero
`cfg(unix)` blocks, so today's failure is the same shape with a different crate
name -- the prediction was right and sat unacted-on because nothing had paid for
it yet. It has now been paid for once, by the lane that did not write the code.

**The fix is to make the push gate's population the workspace**, the way
`boot-test.sh` already does (`--all-targets --exclude kernel`), rather than
crates-containing-cfg(unix). Not done here for two reasons, both pending:
`scripts/` ownership is **A-Q11**, and the cost of a heavier push gate is the
subject of **A-Q13** -- which this incident is direct evidence for, since the
cost of *not* gating landed on a lane that could not have caused it.

**The ratchet is installed, 2026-09-12.** `check-cfg-unix.py` passes
`--all-targets`, so the push gate now compiles the `#[cfg(unix)]` code inside
`#[cfg(test)]` modules that it previously skipped. Measured after the change:
60 of 412 workspace crates hold unix-gated code and all 60 compile clean for
`x86_64-unknown-linux-gnu` with the flag, exit 0.

**The self-test gained the case that was actually blind.** Its existing fixture
put a unix-only compile error at module top level, which the gate caught before
this change — so it demonstrated the gate working on the half that was never
the problem. The new fixture puts the error inside a `#[cfg(test)] mod` and
asserts *both* directions: the module does not compile without `--test` (so the
flag is buying something) and the error IS caught with it (so the flag reaches
test targets). `rustc --test` is to `rustc` what `cargo check --all-targets` is
to `cargo check`.

**And the summary line now names the flag.** A gate that prints "OK, 60 crates
compile" reads identically whether or not it looked at test targets, which is
how this went unnoticed for as long as it did.

**It runs `clippy` rather than `check` as of the same day, which closes the
other half.** Every crate in this population carries
`#![deny(clippy::all, clippy::pedantic)]`, so a clippy finding **is** a compile
error on the target -- and `cargo check` does not run clippy. The gate was
therefore reporting "the `#[cfg(unix)]` code compiles" without having asked the
question that decides it. Lane A accepted exactly this argument for
`boot-test.sh` on 2026-09-02
(`requests/b-a-cfg-unix-gate-should-lint-as-well-as-compile.md`); the push gate
kept `check` for ten days after, which is the gap this closes.

Clippy **subsumes** check -- it runs the compiler front end and then the lints
-- so this is one pass, not two. Cost, measured after an *identical* cache
invalidation rather than by comparing two runs in different cache states:

| | after touching a shared leaf crate |
|---|---:|
| `cargo check --all-targets` | 11 s |
| `cargo clippy --all-targets` | 16 s |

Five seconds on a hook that already runs for minutes. Two earlier attempts at
this number were unusable and both looked plausible: the first reported "1
error in 1 second" (cargo had rejected a CRLF in a package name and never ran),
and the second had clippy *faster* than check, which is impossible -- clippy's
artifacts were warm from a previous run and check's were not.

**The lint fixture is the point.** A gate switched to clippy whose self-test
only ever exercised plain compile errors would pass forever without
demonstrating that clippy does anything. The fixture is `&Vec<i32>` inside
`#[cfg(unix)]` -- `clippy::ptr_arg`: valid Rust that compiles, and a hard error
under `deny(clippy::all)`. Both halves are asserted, because if the compile
half ever stops holding the fixture has quietly become a plain compile error
and proves nothing about linting. Verified by hand as well as by the suite:
`rustc` exits 0, `clippy-driver` refuses.

**A note on how that was checked, because it nearly was not.** The first run
piped the output through `grep -c '^error'` and printed `0`, which is the same
thing it would print if cargo had failed to start. The second run captured the
exit status and the tail, and the `appearance (lib test)` line in it is the
evidence that `--all-targets` genuinely reached test targets rather than being
silently ignored. **A count of zero findings and a check that did not run print
the same digit** — a lesson this tree learned four separate times today, in
four different tools.

**In short:** everyone develops on a Windows machine, and the routine checks
(`cargo build`, `cargo clippy`, `cargo test`) are run for that machine. Any code
inside `#[cfg(unix)]` is therefore *not compiled at all* by a normal check —
rustc skips it wholesale, so it can contain outright syntax and name errors and
still look green. SlateOS is a unix (`toolchain/x86_64-slateos.json` sets
`"target-family": ["unix"]`), so that is exactly the code that ships. This is not
a hypothetical: it hid a hard compile error in `backup` for nearly three months.

**How it bit.** `0cf670e67` (2026-06-03, "apps: clippy hygiene sweep") answered
an unused-variable warning on

```rust
ManifestEntry::Symlink { target, path } => {
```

by rewriting the binding as `target: _`. On Windows the warning was real: the
only reader of `target` is the `#[cfg(unix)]` arm four lines down calling
`symlink(target, &dst)`, and on Windows the `#[cfg(not(unix))]` arm is compiled
instead. On any unix target the same edit is

```
error[E0425]: cannot find value `target` in this scope
 --> userspace/backup/src/main.rs:862:45
```

so `backup` had not compiled for the machine it ships on since June. It was
found only because a lane-b→main merge was verified with `cargo check --workspace
--target x86_64-unknown-linux-gnu` rather than with the host default. Fixed in
`c9aee2c2c`; the binding is restored and discarded explicitly in the non-unix arm,
with a comment saying why it must not be re-elided.

**Why this class is nastier than it looks.** The failure mode is silent *and*
self-inflicted: a warning-cleanup pass on Windows is precisely the operation that
introduces it, because the warnings it is chasing are the ones that only exist
because the unix arm is invisible. Every future clippy sweep is a fresh chance to
do it again, and nothing in the current workflow would catch it.

**Proper fix.** Make a unix-target check part of the routine, not of the
occasional merge verification: add `cargo check --workspace --target
x86_64-unknown-linux-gnu` (fast — under three minutes warm) to whatever gate
`cargo clippy` is already in, and run it before any `-D warnings` cleanup is
committed. `x86_64-unknown-linux-gnu` rather than `x86_64-slateos` because the
latter needs `-Zbuild-std` and is far slower; for `cfg(unix)` coverage the two
are equivalent. Worth raising with the other lanes — `kernel/**` and `gui/**`
have their own `#[cfg(unix)]` arms and the same blind spot.

**Update 2026-08-26 — adopted by all three lanes, but the coverage is lopsided.**
Lane C put the check in its per-task gate; lane A put it in `scripts/pre-boot.py`
(`ee2503c88`), deliberately *not* in `scripts/boot-test.sh`, because that script
takes the cross-worktree QEMU lock and a `--workspace` compile there would let any
lane's red tree block any other lane's boot test. Lane A's gate also triages
failures by owning lane: only lane A's own files fail it, other lanes' print a
`WARN` naming the paths, and errors inside `~/.cargo/registry` are attributed to
nobody. Measured cost: 5m20s cold, 21s warm. Details:
`requests/a-b-the-cfg-unix-check-is-in-lane-as-gate-with-two-changes.md`.

The lopsided part, which lane A measured and asked to be recorded here:
**lane A owns no `cfg(unix)` code at all** — zero occurrences in `kernel/**` and
`bench/**`, against 515 in the tree, all of them in lane B's `userspace/**` or
lane C's `apps/**`/`gui/**`/`randrange/**`. The kernel is bare-metal `no_std`, so
there is nothing there for `cfg(unix)` to guard. My "worth raising with the other
lanes — `kernel/**` … have their own `#[cfg(unix)]` arms" above was therefore
wrong on the facts for lane A.

The consequence is worth being honest about rather than quietly enjoying: in lane
A this check is today a pure service to the other two trees, so lane A is the lane
most likely to eventually stop paying 21s per task for it. **Do not count on all
three lanes catching lane B's regressions symmetrically.** If lane A drops it, the
detection lane B actually relies on is its own — which argues for lane B running
the linux-target check in its own routine rather than treating the tree-wide gate
as the safety net. (As of `045f603e1` lane B does run it per task, by hand.)

**Update 2026-09-01 (lane B) — the adopted remedy is `cargo check`, and half the
blind spot is a clippy one.** Everything above prescribes `cargo check
--workspace --target x86_64-unknown-linux-gnu`, and all three lanes adopted that
verb. `cargo check` does not run clippy. So the half of this blind spot that is a
*compile error* is now covered, and the half that is a **`#![deny(clippy::all)]`
violation** is not — and in these crates a denied lint is a build failure on the
target, not a warning.

Measured, today: `userspace/coreutils/src/utimecmp.rs:360` truncates with
`nsec % SYSCALL_RESOLUTION`, and that constant is `1`, so it is `% 1` —
`clippy::modulo_one`, `deny`-level under the crate's `#![deny(clippy::all)]`. The
whole module is `cfg(unix)`, so on the Windows host it is not compiled at all and
`cargo clippy` here has never seen the line. It has been there since the module
was written. Found by

```
cargo clippy -p coreutils --target x86_64-unknown-linux-gnu --all-targets
```

which is the same trick this entry already describes, with `clippy` in place of
`check`. Fixed in `f107b77cc` with an `#[allow]` and the reasoning for keeping
the no-op line (it is the line that stops being a no-op the day
`SYSCALL_RESOLUTION` changes, and gnulib has it).

**So: say `clippy`, not `check`.** `cargo clippy` runs everything `cargo check`
runs and then the lints, on the same artifacts, so substituting it costs
essentially nothing and covers both halves. `--all-targets` matters too: without
it the `#[cfg(test)]` modules — which is where most `cfg(unix)` test code lives
in `userspace/**` — go unlinted for the target as well. Neither `cargo check` nor
plain `cargo build` for `x86_64-slateos` would have caught this either, because
`deny(clippy::…)` is inert outside clippy; the only thing that sees it is a
clippy run with a unix-family target.

**Update 2026-09-02 (lane B) — measured, and two of the three sentences above
are wrong. Still open; the fix is lane A's to make and is now requested.**

The prescription stands on its main point and fails on both of its asides.

*Wrong aside 1: `--all-targets`.* It cannot be done workspace-wide at all.
`--all-targets` builds a test harness for every crate including `kernel`, which
is `no_std` and defines its own `#[panic_handler]`, so linking it against a
hosted target's libtest is `E0152: found duplicate lang item panic_impl`.
Measured 2026-09-02: exit 101 at `kernel/src/main.rs:7923` after 218 s, having
linted nothing. That sentence was written from reasoning, not measurement.

*Wrong aside 2: "costs essentially nothing", "on the same artifacts".* Clippy
sets `RUSTC_WORKSPACE_WRAPPER`, which is hashed into every workspace unit's
fingerprint — so a clippy run neither reuses nor invalidates `cargo check`'s
artifacts, and pays a cold population of its own. Measured standalone: 55 s warm,
92 s after touching one file, 155 s for the first run of the shape in a session.
Measured *in situ*, in a boot test, immediately after `check_kernel_clippy`
(whose different wrapper means no reuse): **236 s**. That is four minutes on
every boot test in all three lanes, not "essentially nothing".

*The main point survives, and is verified by mutation rather than by a green
run:* with `utimecmp.rs`'s `#[allow(clippy::modulo_one)]` removed, `cargo clippy
-p coreutils --target x86_64-unknown-linux-gnu` — no `--all-targets` — exits 101
and names `utimecmp.rs:370:32`. So the one instance the project has ever had is
inside a plain `mod unix`, and the workspace sweep does catch it.

**Why this is still open.** `scripts/boot-test.sh` is lane A's by name
(`roadmap.md`: "Also owns the two shared build gates … `scripts/boot-test.sh`").
Lane B briefly landed the change on its own branch and reverted it unmerged on
noticing; the four-minute cost is a budget decision for the gate's owner, not
for a lane passing through. Requested in
`requests/b-a-cfg-unix-gate-should-lint-as-well-as-compile.md`, which carries the
measurements above and the exact diff.

**The residue, if it lands.** `cfg(unix)` code inside `#[cfg(test)]` modules
would still be unlinted for a unix target. Closing *that* needs a per-crate
`--all-targets` sweep excluding the `no_std` crates — a crate list, which drifts,
which is why it is not proposed for the gate. Anyone touching `cfg(unix)` test
code should run `cargo clippy -p <crate> --target x86_64-unknown-linux-gnu
--all-targets` by hand; it works fine for a single userspace crate, and it is
only the workspace sweep the kernel makes impossible.
