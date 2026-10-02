### [A] W-BENCH-THREE-BENCHMARKS-ABOVE-SUITE-DRIFT-WITH-NO-MATCHING-CODE-CHANGE (original entry). firewall_check / shm_create_close / ipc_semaphore — WATCH, needs a third data point

**Where:** benchmarks `firewall_check`, `shm_create_close`, `ipc_semaphore`;
history in `bench/history.jsonl` (host `Logoplex3`).

**What.** After the drift correction above, three benchmarks still sit clear of
the suite: `firewall_check +68%` (270→482ns), `shm_create_close +37%`
(58556→84996ns), `ipc_semaphore +30%` (11676→16112ns). As established above,
none of their source changed between `bf26aabdb` and `17dbde179`.

**Why it is a WATCH and not a bug (yet).** `bench/history.jsonl` holds exactly
**two** runs on this host, so there is no per-benchmark variance estimate — the
drift correction removes the *common* factor but says nothing about how noisy
an individual benchmark is around it. `firewall_check` at 270ns is the prime
suspect for being intrinsically noisy: it is the shortest benchmark in the
suite, and at TCG timer granularity a couple of hundred nanoseconds is very
few ticks, so its relative variance should be the largest by construction.

**How to resolve.** Take a third `--bench` run on an otherwise-idle machine and
compare. If these three land back at the suite median they were noise, and the
proper fix is to give the comparator a per-benchmark variance estimate (flag on
deviation from a benchmark's own historical spread, not a flat percentage)
rather than to keep hand-adjudicating. If they stay high, they are real, and
the next question is whether the `handlers.rs` change shifted code layout
(icache/alignment) — cheap to test by benchmarking `bf26aabdb` again.

**Do not** act on either theory from the current two runs; that is exactly the
inference-from-insufficient-samples mistake the entry above documents.
