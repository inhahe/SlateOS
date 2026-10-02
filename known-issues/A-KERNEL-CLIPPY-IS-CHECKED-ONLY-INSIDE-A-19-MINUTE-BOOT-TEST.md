## A-KERNEL-CLIPPY-IS-CHECKED-ONLY-INSIDE-A-19-MINUTE-BOOT-TEST (lane A, 2026-09-09)

**In short:** nothing checks that the kernel passes clippy until you are
nineteen minutes into a boot test. `cargo clippy -p kernel` is a *deny*-level
gate — a single lint refuses the build outright — but it runs only inside
`boot-test.sh`, and only after the whole `check-*.py` gate phase has finished.
So a lint can be committed, pushed, merged, and sit in the tree until somebody
starts a boot test and waits a third of an hour to be told about a one-line
fix. That happened today, and the tree had been red at that gate for about a
day before anyone found out.

**What it cost, concretely (2026-09-09).** `kernel/src/selftest.rs:260` failed
`clippy::vec_init_then_push`. I ran `cargo check -p kernel` first — it passed,
because **`cargo check` does not run clippy** — committed, pushed, and started
a boot test. It spent nineteen minutes before the build and then refused. The
fix was mechanical and took two minutes. The `Vec::new()`-then-push shape it
objected to was already in the file before yesterday's §914 commits, so this
was not a fresh mistake being caught promptly; it was an old one finally being
reached.

**`known-issues.md` states the cadence wrongly, and that is the part worth
fixing.** `TD-B-NOTHING-RUNS-CLIPPY-OVER-USERSPACE-OFTEN-ENOUGH-TO-MATTER`
carries a table headed "where the two clippy gates are, and why neither caught
it", whose first row reads:

| gate | scope | when it runs |
|---|---|---|
| `scripts/pre-boot.py` | `cargo clippy -p kernel` | every push (pre-push hook) |

**It does not run on every push, and it is not in the pre-push hook.** Checked
2026-09-09:

- `scripts/hooks/pre-push` is the real hook (`.git/hooks/pre-push` is a 533-byte
  trampoline that `exec`s it). It has fifteen gates. It never mentions
  `pre-boot.py`, and its only compiler gate — gate 12 — is scoped to
  **coreutils**, which is lane B's zone. Nothing there compiles the kernel.
- `scripts/pre-boot.py` is invoked by `boot-test.sh`, and `boot-test.sh:3658`
  describes it in its own words as "a ~40-minute local pre-flight **nobody is
  obliged to run**."

The row matters because the table is the *argument* for how much clippy
coverage exists. Read as written, kernel lints are caught at push time and only
userspace is exposed. Read correctly, **neither** is checked at push time, and
the kernel's gate is a manual tool plus a 19-minute wait. The conclusion drawn
from that table is therefore narrower than the truth.

**Why this is not simply "run `pre-boot.py`".** It is the right advice and I
should have taken it — the tool exists precisely for this, and its docstring
describes the failure I hit almost word for word, down to a previous instance
on 2026-08-26. But a gate whose trigger is *remembering to run a 40-minute
optional tool* is the "periodic needs a trigger nobody has defined" shape the
operator explicitly ruled out when answering Q46: "Make a solution that will
not result in 'never' in practice." Two people have now hit the identical trap
two weeks apart, which is the evidence that the trigger does not work.

**What the pre-build phase actually costs, measured on E: (2026-09-09).** "19
minutes of gates" is how this felt, and it is wrong in a way worth writing
down. Read from the per-gate timing logs that `run_checker` already writes to
`<git-common-dir>/worktrees/os-lane-a/boot-test-gate-timing.*.tsv`:

| | gates measured by `run_checker` | slowest single gate |
|---|---|---|
| `D:` 2026-09-06 | 219 gates, **3717 s (62 min)** | `scan-orphan-modules` 747 s |
| `E:` 2026-09-09 | 220 gates, **440 s (7.3 min)** | `check-live-counter-reads` 50 s |

**8.4x faster after the migration**, with `scan-orphan-modules` alone falling
747 s → 32 s. Any argument that rests on "the gates take an hour" is now false,
and `run-checker.sh`'s own header — "the gates were 10000 s, 86%" — is a
pre-migration figure still stated as current.

The nineteen minutes to reach clippy is real, but only 440 s of it is gates.
The rest is the 34 `scripts/test-*.py` tooling suites, which do not go through
`run_checker` and so appear nowhere in the timing log. Bracketed from the
runner's own heartbeat markers on the 2026-09-09 run: **540–660 s**, against
410 s for every gate combined. The suites, not the gates, are now the
dominant pre-build cost. **Anyone shortening this phase should start with
the tooling suites, not the gates.**

**A warning this is the third instance of today.** The `D:`→`E:` migration
silently invalidated every performance number written in this tree, and the
documents still state them as current. I have now three times quoted a stale
figure as a present fact: the `70 ms` per file open (A-Q7 — it was the disk,
and it is gone; `design-decisions.md` §923), the capability-gate site counts in
§924, and "19 minutes of gates" here. A number in prose carries neither a date
nor a machine, so re-dating a document silently re-dates its measurements.
Prefer citing something *re-runnable* — `bench/file-read-latency.py`, the
timing TSVs — over a figure copied out of another document.

**A pre-push clippy gate was the obvious fix, and the arithmetic rejects it.**
The proposal was to run `cargo clippy -p kernel` in the pre-push hook, scoped
the way gate 12 scopes its compiler — only when the pushed commits touch
`kernel/**`. I wrote the checker before costing it, which was the wrong order.

Costed afterwards, on this tree:

| quantity | measured |
|---|---|
| kernel-touching commits | **138 in 10 days ≈ 14/day** (`git log --since='10 days ago' -- kernel/`) |
| pushes carrying them | ~5–7/day at `CLAUDE.md`'s "every completed task, or every few commits" |
| `cargo clippy -p kernel` after a source edit, on E: | **101 s** (6 s warm with nothing changed) |
| added push latency | **~9–13 min/day, ~150 min/fortnight** |
| what it prevents | a break roughly fortnightly, costing ~19 min to discover |

**Spending 150 minutes to save 19 is a bad trade**, so the gate is not built.
The checker is not in the tree; recreating it is a twenty-minute job if the
numbers ever change, and this table is here so the next person costs it before
writing it rather than after.

Note the 101 s is CPU-bound compilation, so unlike almost everything else
measured this week the SSD did not help — the old D:-era figure of 113 s is
still roughly right, and it is the *only* pre-migration number in this entry
that survived checking.

**What to do instead, in order of value:**

1. **Run `scripts/pre-boot.py` before starting a boot test** — but know what it
   costs. It performs exactly this check and exists for exactly this reason.

   **Correction (2026-09-09, same day):** I first wrote here that the
   "40-minute optional tool" objection was itself a stale measurement, on the
   grounds that the phase it duplicates is 440 s on E:. **That was wrong, and
   it is my own error made in the entry complaining about exactly this.** The
   440 s covers only the gates that go through `run_checker`. It excludes the
   34 `scripts/test-*.py` suites (540–660 s) *and* the `cfg(unix)` workspace
   clippy, which appears nowhere in the timing log either — `boot-test.sh`'s
   own accounting put the gate phase at **1102 s** on a run that stopped
   *before* kernel clippy and `cfg(unix)` had started.

   `pre-boot.py` additionally runs `cargo check --workspace --exclude kernel
   --all-targets`, across 2,700+ crates. That is CPU-bound compilation, so the
   SSD barely helped it — the same result the kernel clippy measurement gave
   (101 s on E: against a D:-era 113 s). Its own docstring's "~6 minutes"
   describes only the `check-*.py` phase, not the tool; `boot-test.sh`'s
   "~40-minute" figure is the honest one for the whole run.

   **Sharper still, and this reverses the advice:** on current hardware
   `pre-boot.py` costs *more* than the run it is meant to protect. Measured
   today, a boot test is ~26 min — gates 1102 s, build 76 s, QEMU 392 s — so
   the most a pre-flight can save you is the **8 minutes** of build and QEMU.
   `pre-boot.py` pays the same ~18 minutes of gates *plus* a
   `cargo check --workspace --all-targets` over 2,700+ crates. **≥18 minutes
   spent to save 8.**

   Its docstring's own rationale — "costs the same ~6 minutes the gate phase
   would have cost, and buys back the ~13-minute run that would have been
   thrown away" — was true when written and has since **inverted**: the gate
   phase is nothing like 6 minutes, and the SSD shrank build+QEMU to under
   eight. The tool is not broken; the ratio it was designed around moved.

   So: it is worth running when you want the checks **without** booting. It
   is *not* worth running before a boot test you intend to run anyway, which
   is exactly what this entry originally recommended. For catching an edit
   mistake, run the single gate that guards the file you touched — that would
   have caught both of today's lost runs in under a second, at a cost of
   nothing.
2. **Move `check_kernel_clippy` earlier in `boot-test.sh`'s sequence.** Free —
   it does not change the total when everything passes, only how soon you learn
   it did not. Costs at most one thing: a cheap text gate's failure is then
   reported ~100 s later than it is today.
3. **Shorten the 34 `scripts/test-*.py` suites** (540–660 s), which are the
   dominant pre-build cost and are not what most changes can break.

**If it is never fixed:** nothing rots on its own, but every kernel lint costs
a 19-minute boot test to find, and the tree can sit red at a deny-level gate
for days — as it just did — because the only thing that looks is the slowest
thing we run.
