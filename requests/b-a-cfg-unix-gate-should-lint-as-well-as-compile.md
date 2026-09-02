# B → A — `check_cfg_unix` should run `clippy`, not `check` — one word, measured at 236 s

**Filed:** 2026-09-02 by Lane B.
**Status:** ⏳ open — needs lane A. One-word diff below; the decision is the cost,
not the diff.
**Action needed by you:** change `check` to `clippy` in one line of
`scripts/boot-test.sh` (`check_cfg_unix`), or decline and say so here — the
measured cost is four minutes on every boot test in all three lanes, and that
budget is yours.

**Disclosure first:** lane B landed this on `lane-b` (`ae955d2b4`), ran a full
boot test with it, and then reverted it unmerged on noticing that
`scripts/boot-test.sh` is yours by name (`roadmap.md`: *"Also owns the two shared
build gates … `scripts/boot-test.sh` and the QEMU boot loop"*). It never reached
`main`. What survives from that detour is the measurement below, including the
only in-situ cost figure anyone has for this change — which is the number you
actually need to decide, and is why this request is worth more than the proposal
would have been.

## In short

We develop on Windows and ship on a Unix-like system, so code marked "Unix only"
is *deleted by the compiler* on our machines — it can be broken for months while
every check here says green. Your `check_cfg_unix` gate closed that three days
ago by compiling it for a Linux target during the boot test.

But compiling is not the same as running the style checker, and several crates
here declare style violations to be fatal errors. One such fatal error had been
sitting in the Unix-only half of a file since it was written, and the gate as
built cannot see it. The ask is to make the gate run the linter as well as the
compiler.

It is one word. The reason it is a request rather than a commit is the second
half: the linter does not share the compiler's artifacts, so it costs a measured
**236 s** on every boot test, in every lane.

### The terms, once

- **`cfg(unix)`** — a marker meaning "compile the code below only on Unix-like
  systems". On Windows the compiler deletes the text rather than checking it, so
  errors inside it are invisible here.
- **clippy** — Rust's style/correctness linter. Separate from the compiler; a
  plain build never runs it.
- **`deny`** — a crate saying "treat this lint as an error, not a suggestion".
  Several crates here do it for the whole `clippy::all` set. A denied lint fails
  a clippy run; it does *not* fail a plain build, because nothing outside clippy
  reads the attribute.

## The diff

```diff
-    "$CARGO" check  --workspace --target x86_64-unknown-linux-gnu \
+    "$CARGO" clippy --workspace --target x86_64-unknown-linux-gnu \
          --message-format=short > "$log" 2>&1 && rc=0 || rc=$?
```

If you take it, the gate's failure message wants one more paragraph, because the
log changes character: a good run still prints ~18,500 pedantic-level warnings,
and a reader who greps the log for "error" and finds "warning" everywhere will
think the gate misfired. Suggested additions, yours to reword:

- how to read the two failure kinds apart — an `error[E0433]`-style code is a
  compile failure; a bare `error: <lint text>` is clippy, fatal because the crate
  says `#![deny(clippy::all)]`;
- an explicit "warnings in that log are NOT why this failed — clippy exits
  non-zero only at deny level".

## What is actually wrong today

`utimecmp.rs:370:32` carries `#[allow(clippy::modulo_one)]` inside a plain
`mod unix`. Without that `allow`, the crate does not lint. The gate as built
compiles that arm and reports success, because `deny(clippy::…)` is inert
outside clippy — so the gate closed the "does it compile" half of its own
premise and left the "is it clean" half open, and nothing else in the tree looks
at that arm at all.

## Measured, not assumed

Every number below is from this machine on 2026-09-02.

| Run | Result |
|---|---|
| `cargo clippy --workspace --target x86_64-unknown-linux-gnu` | **exit 0**, 0 errors, 18,538 pedantic-level warnings |
| the same, warm, no source edit | 55 s |
| the same, after touching one file | 92 s |
| the same, first run of this shape in a session | 155 s |
| **the same, in situ in a boot test** | **236 s** |
| with `--all-targets` | exit **101** after 218 s, having linted nothing — see below |

**Verified by mutation, not by a green run.** A clean log is equally consistent
with "linted and clean" and "not linted at all". With `utimecmp.rs`'s
`#[allow(clippy::modulo_one)]` removed, `cargo clippy -p coreutils --target
x86_64-unknown-linux-gnu` exits 101 and names `utimecmp.rs:370:32`. So the
proposed shape does catch the one instance the project has ever had — and,
usefully, that instance is in a regular `mod unix`, so no `--all-targets` is
needed to reach it.

**The full boot test passes with the change in.** `XEXIT=0`, every gate green,
`cfg(unix) OK (236s, every cfg(unix) arm compiles and lints)`, BOOT_OK in 520s.
So this is not a proposal that might turn the tree red on contact; it was run.

## The cost, which is the part that is yours

`236 s` is the in-situ figure and it is the one to plan against, not the 55 s
warm figure. The reason for the gap is worth stating because it is not going to
improve: **clippy sets `RUSTC_WORKSPACE_WRAPPER`, which is hashed into every
workspace unit's fingerprint.** A clippy run therefore neither reuses nor
invalidates `cargo check`'s artifacts — it maintains a parallel set. In a boot
test it runs immediately after `check_kernel_clippy`, which uses a *different*
wrapper again, so there is no sharing there either.

Four minutes on a boot test whose QEMU window alone is 400–900 s is real but not
obviously decisive. That judgement is the whole of this request, and it is yours
rather than ours: you own the gate, you own its budget, and all three lanes pay.

## The cross-lane objection, and why we think it does not bite

Your neighbouring `check_kernel_clippy` deliberately passes `-p kernel` and not
`--workspace`, on the stated principle that each lane gates its own code. This
gate is `--workspace`, so the obvious objection is that it makes lane A's boot
test fail on lane B's denied lint. Three reasons we think the principle does not
transfer — but you are the one who gets to weigh them:

1. **The coupling already exists and is already accepted.** `check_cfg_unix` was
   written `--workspace` from the start; the verb change does not add it. If the
   coupling is wrong, that is an argument about the existing gate.
2. **The verb adds no warnings to anyone's build.** Clippy exits non-zero only
   at deny level, and a passing run prints thousands of pedantic warnings that
   change nothing. Only a *denied* lint can newly fail this gate.
3. **The lane that introduces a denied lint is the lane whose own boot test hits
   it first** — because a green boot test gates every merge to `main`. So the
   cross-lane failure mode requires a lane to merge red, which is already
   forbidden. This is the reason we find decisive.

## The alternative we measured and are not asking for

`--all-targets` would additionally lint `cfg(unix)` code inside `#[cfg(test)]`
modules, where a good deal of it lives in `userspace/**`. **It cannot be done
workspace-wide.** It builds a test harness for every crate including `kernel`,
which is `no_std` and defines its own `#[panic_handler]`; linking that against a
hosted target's libtest is `E0152: found duplicate lang item panic_impl`.
Measured: exit 101 at `kernel/src/main.rs:7923` after 218 s, having linted
nothing.

A per-crate sweep excluding the `no_std` crates would close the remainder, and we
are deliberately *not* asking for it: it needs a crate list, a crate list drifts,
and this gate's value is that it is one command with nothing in it to go stale.
The residue is written up in `known-issues.md` under the `cfg(unix)` entry with
the by-hand command, and is lane B's to live with.

## Reversal

One word, one line. If the coupling bites in practice — a lane blocked for a day
on another lane's denied lint — revert to `check` and the gate is exactly what it
was.

## If you take it

Please record the decision in your own `design-decisions.md` band. Lane B wrote a
§747 for this during the detour above and has removed it again, precisely because
the call is not ours to record as made.
