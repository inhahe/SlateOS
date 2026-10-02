### TD-KERNEL-CLIPPY-HAS-8-DENY-LEVEL-ERRORS — `cargo clippy -p kernel` does not pass — ✅ FIXED 2026-08-24 (lane A, `7465db994`; gate added `81a12435c`)

**What.** `CLAUDE.md` → "When You Finish a Task" requires `cargo clippy`
clean. The kernel is not: `cargo clippy -p kernel --release` ends with

```
error: could not compile `kernel` (bin "kernel") due to 8 previous errors;
       18142 warnings emitted
```

The 18142 warnings are the known `pedantic` / `indexing_slicing` /
`arithmetic_side_effects` backlog (same shape as the `apps/` backlog recorded
earlier in this file) and are `warn`-level. The **8 errors** are `clippy::all`
at `deny` and are a different matter — they mean the command exits non-zero, so
nobody can use "clippy is green" as a gate on this crate at all.

| Site | Lint |
|---|---|
| `kernel/src/fs/cas.rs:366` | returning the result of a `let` binding from a block |
| `kernel/src/fs/fileselect.rs:409` | `contains()` instead of `iter().any()` |
| `kernel/src/fs/prefetch.rs:418` | needless borrow — `&format!(…)` |
| `kernel/src/kshell.rs:21280` | redundant closure — use `Path::new` |
| `kernel/src/kshell.rs:21294` | redundant closure — use `Path::new` |
| `kernel/src/kshell.rs:60713` | needless borrow — `&format!(…)` |
| `kernel/src/ksyms.rs:480` | `let…else` that should be `?` |
| `kernel/src/proc/pcb.rs:4643` | very complex type; factor into a `type` alias |

**Reproduce.** `cargo clippy -p kernel --release --message-format=short 2>&1 |
grep "^error"` from `os-lane-a`.

**Proper fix.** Seven are one-line rewrites that clippy dictates verbatim; the
eighth (`pcb.rs:4643`) wants a `type` alias for a nested generic. Fix all eight
rather than `#[allow]` them — none is a case where the lint is wrong. Then
consider whether `scripts/boot-test.sh` should gate on
`cargo clippy -p kernel` exiting 0, which is the thing that would stop this
recurring; today it runs no clippy at all, which is why eight `deny`-level
errors accumulated unnoticed.

**Fixed** in `7465db994`, exactly as clippy dictated in all eight cases. The
`pcb.rs` one gained a named `ReapedProcess` alias, which is an improvement on
its own terms: the tuple exists only to carry state *out* of the
`PROCESS_TABLE` lock so the teardown can run unlocked, and the name now says
so where a bare `Option<(ExitInfo, u64, Vec<(ResourceType, u64)>)>` did not.

**The gate is still not built, and that is the part that matters.**
`boot-test.sh` runs no clippy, so nothing stops a ninth error appearing
tomorrow — the eight above accumulated precisely because the crate declares
`#![deny(clippy::all, clippy::pedantic)]` and nothing ever ran it. Gating on
`cargo clippy -p kernel` exiting 0 is cheap now that it *does* exit 0; the
open question is where to put it, since a clippy run and a `cargo check` run
invalidate each other's fingerprints in a shared `target/`, so naïvely adding
one to `boot-test.sh` doubles every boot's build time. Tracked as the
remaining half of this entry rather than closed outright.

**Gate built 2026-08-24 (`81a12435c`) — this entry is now fully closed.**
`scripts/boot-test.sh` → `check_kernel_clippy`, run with the other pre-build
checks and before `cargo build`.

**The paragraph above is wrong, and the way it is wrong is the more useful
half of this entry.** "A clippy run and a `cargo check` run invalidate each
other's fingerprints" was never measured — it was plausible reasoning written
in the voice of a finding, and it deferred the gate for a day. `cargo clippy`
sets `RUSTC_WORKSPACE_WRAPPER`, which is hashed into the fingerprint of every
workspace unit, so clippy's artifacts occupy their own entries and leave the
build's untouched. Measured:

| Step | Time |
|---|---|
| `cargo build` (warm baseline) | 13.8 s |
| `cargo clippy -p kernel` (cold) | 200 s |
| `cargo build` immediately after | **4.7 s** — not invalidated |
| `cargo clippy` again, no source edit | 5 s |
| `cargo clippy` after touching one file | **113 s** — what a real run pays |
| `cargo build` after touch + clippy | 215 s — an ordinary full kernel codegen |

113 s against a QEMU window of 400–900 s. Not free, and the comment at the
gate says so in numbers rather than calling it cheap.

Four choices, each recorded at the gate rather than here:

* **`-p kernel`, not the workspace** — a workspace-wide clippy would let a red
  crate in lane B's or lane C's tree block lane A's boot test, which is the
  exact coupling the lane split exists to prevent. Each lane gates its own.
* **Same profile as the build** — `cfg(debug_assertions)` selects real code in
  this kernel, so linting debug while shipping release would leave a hole of
  precisely the size of the difference.
* **Output to `build/clippy-kernel.log`, not the boot log** — clippy emits
  18,163 lines here, all `pedantic`-level backlog, and they would bury the
  output the rest of the script greps.
* **Not a pipe.** `cargo … | grep` makes `$?` grep's, and grep's status answers
  "did I match" — which for an *error* filter is inverted: a clean crate would
  report failure and a broken one success. This is the failure mode a gate can
  carry indefinitely, because it only misfires in the direction nobody checks.

**Tested on both paths before shipping**, which for a gate is not optional —
one that has only ever been seen green is indistinguishable from one that
cannot fire. Green: exit 0 and a single summary line. Red: a deliberate
`ptr_arg` violation appended to `ksyms.rs` produced exit 101 and
``kernel\src\ksyms.rs:626:34: error: writing `&Vec` instead of `&[_]` …``
through the failure filter; then reverted.

**Still not gated:** the `bench/**` crates, also lane A's. Same one-line
addition if they turn out to be clippy-clean; unmeasured as of this writing.

**Correction, 2026-08-24 — there are no `bench/**` crates, so the gate is
already complete for lane A's scope.** Checked before extending it: `bench/`
holds three data files (`baselines.toml`, `boot-history.jsonl`,
`history.jsonl`) and no `Cargo.toml`, and the workspace `members` list has no
`bench/*` glob. Benchmarking here is the boot test's own `--bench` mode over
in-kernel code, which `-p kernel` already covers.

The paragraph above was written from `CLAUDE.md`'s instruction to *put*
benchmarks in `bench/<subsystem>/` — a statement about where they should go,
read as one about where they are. Worth noting as a pattern, because it is the
second time in two days that an entry in this file recorded an unverified
premise as fact (the first being the fingerprint claim corrected above). Both
were about a minute's work to check; neither was checked before being written
down, and both were written *confidently enough to act on* — the fingerprint
one deferred this gate by a day, and this one would have sent the next reader
looking for crates that do not exist.

Nothing further to gate. `-p kernel` is the whole of lane A's lintable tree;
the other lanes' crates are theirs to gate, and a workspace-wide run is not
lane A's to impose on them.
