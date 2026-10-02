### A-LOCKDEP-RECORD-EDGE-WAS-A-LINEAR-SCAN — FOLLOW-UP 2026-08-17 — `page_fault_anonymous` recovered on its own; there was no regression

**In short:** the benchmark went back to normal by itself. Boot #18 measured
`page_fault_anonymous` at **2105ns**, inside the 2057-2158ns band it held for
eight consecutive runs before the excursion — on a kernel whose page-fault code
is **identical** to the one that measured 4138ns a run earlier. Nothing was
fixed in between. The question the CORRECTION above left open ("still unknown,
and it is probably not code") is now answered as far as it can be: **there was
no code regression here.**

**The series, in full.** Each row is one recorded run:

| Commit | `page_fault` | Note |
|---|---|---|
| `602fc62e0` | 2062, 2084 | |
| `86a923fe1` | 2076 | |
| `9ecef3188` | 2057, 2139 | |
| `d542299e2` | 2097, 2158 | |
| `7342d57ea` | **3510** | diff vs previous is *two bench JSONL files* |
| `78affced4` | **4333, 4978** | the +6365-line merge |
| `e3ae7bae1` | **4138** | `record_edge` made O(1) — barely moved |
| `71fa87ab7` | **2105** | recovered; **no kernel file changed** |
| `71fa87ab7` | **2110** | A/A control, byte-identical binary |

**Why the last two rows settle it.** `git diff --stat e3ae7bae1 origin/main --
kernel/` is *empty*: the merge that preceded the recovery touched `gui/`,
`apps/` and shared documents, and not one file under `kernel/`. The only
kernel-side change between 4138ns and 2105ns is inside `lockdep::self_test` —
a test that runs once at boot and cannot be reached from the page-fault path.
It is dead code with respect to what was measured. A benchmark that halves
across a change it cannot observe is not measuring that change.

The A/A control then rules out the other explanation. Re-running the *same*
tree produced 2110ns against 2105ns — **0.2% apart**. `kernel/build.rs` emits
no `cargo:rustc-env`, no git hash and no timestamp, so the rebuild is
reproducible and this really is the same binary twice. Therefore:

| Reading | Predicts | Verdict |
|---|---|---|
| Run-to-run noise | same binary lands in either band | **ruled out** — 2105 vs 2110 |
| Code layout under TCG | the band is a property of the build | consistent with everything |

Layout also matches this harness's own precedent,
`A-CRYPTO-BENCHMARKS-STEPPED-58-PERCENT-WITH-BYTE-IDENTICAL-CRYPTO-SOURCE`
("code layout under TCG. Not a regression."). Under TCG a translation block
ends at a guest page boundary, so a hot loop whose backward branch straddles a
4 KiB boundary pays a dispatcher round-trip per iteration — ~1.7x on this
project, against ~2x here.

**The one loose end.** Layout cannot explain the *first* step
(`d542299e2` 2158 -> `7342d57ea` 3510), whose commit changes only
`bench/boot-history.jsonl` and `bench/history.jsonl`. If the kernel there was
byte-identical then its layout did not move either, so that step is something
else — most likely host interference, which this harness cannot see
(`B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING`). Worth noting that boot #18 was
flagged `RUN CONTAMINATED` on wall time (178s vs a 132s median) while producing
the *fast* number: the contamination flag and the direction of the error are
not correlated the way one would assume.

**Action: none, and none is warranted.** No page-fault code changed in the
window and the value is back to baseline. What did come out of the
investigation is in the entry above and stands on its own: a real class-table
overflow (138 classes against a 128 cap, so 10 lock classes went untracked), an
O(1) `record_edge`, a fixed double-report race, and a complete `has_cycle`.

**The reusable lesson.** The harness printed this metric's own range —
`0-7274ns`, median 2834ns over 8 runs — on every run of the investigation. That
spread is wider than the "regression" that was chased through an entire
implementation. The rule that would have saved the effort: **before attributing
a movement to a change, check whether the metric's own same-binary spread
already covers it.** It is the same test that correctly dismissed
`isr_latency`, applied one metric to the left.
