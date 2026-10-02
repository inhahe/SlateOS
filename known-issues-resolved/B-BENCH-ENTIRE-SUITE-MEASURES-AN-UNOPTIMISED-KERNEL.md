### [A] B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL. Every recorded benchmark ran at `opt-level = 0` and was scored against optimised-reference targets — **FIXED 2026-08-14, confirmed by measurement**

> **Resolution.** `scripts/boot-test.sh` now builds `--release` and stages from
> `target/x86_64-unknown-none/release/kernel` when `--bench` is passed, and
> `bench-history.py` records/compares a `profile` field so release and debug
> records are never diffed against each other. Confirmed end-to-end by run
> `fcd066231`: the release kernel built clean (0 warnings, 9m25s), booted, and
> **`over_target` fell from 58–59 of 63 on every debug record to 15 of 63** —
> scorecard `48/63 within hardware target`. The comparator correctly refused
> to diff against the six debug records. Quantified per-benchmark evidence is
> in the RESULT section of
> `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` above (e.g. `tcp_checksum_v4`
> 20667 → 1716 ns, `v6` 35048 → 1602 ns).
>
> Two things this did **not** settle, both tracked elsewhere and neither a
> reason to keep this entry open: (a) whether the *non-bench* boot test should
> also build release — that is **Q46**, still with the operator, and the
> default deliberately stays debug meanwhile; (b) the 15 benchmarks still over
> target, which are now genuine findings rather than codegen artefacts and
> need triage on their own merits.

**Where:** `scripts/boot-test.sh:602` (`"$CARGO" build`) and `:218`
(`KERNEL_BIN=".../target/x86_64-unknown-none/debug/kernel"`); `Cargo.toml`
`[profile.dev]` (357–365) vs `[profile.release.package.kernel]` (370–373).

**What.** The boot test builds with a bare `cargo build` — no `--release` — and
stages the artefact out of `target/x86_64-unknown-none/**debug**/kernel`. The
benchmark suite is compiled into the kernel unconditionally; `--bench` only
changes which serial marker the script waits for (`BENCH_OK` instead of
`BOOT_OK`), it does not change the build. `[profile.dev]` sets only
`panic = "abort"`, and there is no `[profile.dev.package.kernel]`, so the
kernel is built at **`opt-level = 0`**.

So every number in `bench/history.jsonl` — all 5 records, all 63 benchmarks —
measures unoptimised codegen, and every one of them is scored against
`baselines.toml` targets taken from *optimised* Linux / Fuchsia / L4 / jemalloc
implementations.

**Evidence.** Disassembling the staged binary shows textbook `opt-level = 0`
output. `tcp_checksum_bench` reloads and re-spills the accumulator to the stack
around every single add:

```
805d7181:  addl  %ecx, %eax
805d7183:  movl  %eax, -0x64(%rbp)
805d7186:  movl  -0x64(%rbp), %eax     # reload of the value just stored
```

That is one store + one load per accumulation in a loop whose entire body is
one accumulation. (`llvm-objdump` ships with the rustup toolchain — see the
path in `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` — so this needs no binutils
install.)

**The irony.** `[profile.release.package.kernel]` already exists and is
deliberately tuned for exactly this — `opt-level = 3`, `codegen-units = 1`,
`strip = "none"` — with a comment explaining the per-package override. The
benchmark path has simply never used it.

**Why it matters.** This invalidates the *absolute* verdicts wholesale, and
they are the ones CLAUDE.md's benchmarking protocol is built on:

- The `over_target` list is not a list of subsystems that are too slow. It is
  mostly a list of subsystems compiled without optimisation. `tcp_checksum_v4`
  at 20000 ns against a 2000 ns target is a 10x miss that says nothing about
  the shipped code.
- "If a change regresses a benchmark by more than 10%, investigate before
  merging" cannot be applied to numbers whose baseline is debug codegen.
- The scale is wrong in the direction that matters: `opt-level = 0` → `3` on
  byte-loop code of this shape is routinely 5–20x. That dwarfs both the ~1.7x
  swing in `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` and the 25% canary
  tolerance, which means the noise work done so far has been tuning the
  measurement of the wrong binary.

*Relative* commit-to-commit comparisons are not destroyed — both sides are
debug — but they are still measuring optimisation-sensitive code paths whose
debug/release ratio is not uniform, so a debug-visible change need not be a
release-visible one.

**Same family as the three before it.** `TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN`
(the suite never ran), `B-BENCH-WATCHLIST-...` (the watch list never looked),
`B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION` (the diff named
innocents) — and now a suite that ran, reported, and was compared against
targets, while measuring a binary nobody intends to ship. A check that measures
the wrong thing is indistinguishable from a check that passes.

**Proposed fix.** Build the kernel `--release` for `--bench` runs, staging from
`target/x86_64-unknown-none/release/kernel`, and add an append-only `profile`
field to each `bench/history.jsonl` record so the comparator only ever compares
like with like. The 5 existing records must keep their meaning: absent
`profile` reads as `"debug"`, and a release record must never be diffed against
a debug one. This is a real cost — a second full kernel build, and a bench
history that restarts from zero same-profile records, which also resets the
≥6-record threshold that `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`
is waiting on. It is still the only honest option: a benchmark that does not
measure the shipped build is not a benchmark.

**Open sub-question:** whether the *non*-bench boot test should stay debug.
Keeping it debug preserves fast iteration and readable panics, at the cost of
two kernel builds in the tree and the risk that release-only miscompiles or
UB-dependent behaviour are only ever exercised on bench runs. Leaning toward
keeping the default boot test debug and making release the `--bench` path, but
this is worth the operator's view — see `open-questions.md`.
