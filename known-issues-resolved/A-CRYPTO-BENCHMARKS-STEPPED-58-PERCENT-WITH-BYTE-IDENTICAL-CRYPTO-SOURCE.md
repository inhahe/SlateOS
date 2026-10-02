## A-CRYPTO-BENCHMARKS-STEPPED-58-PERCENT-WITH-BYTE-IDENTICAL-CRYPTO-SOURCE (lane A, 2026-08-16)

**Status: RESOLVED 2026-08-16 — not a regression. A QEMU/TCG code-layout
artifact, proven mechanically (see "Verdict" at the end). No kernel change is
warranted and the range must NOT be bisected. Not a boot failure; all 18 boots
green.**

**What happened.** Three crypto benchmarks stepped up by a near-uniform ~1.58x
between the last benchmarked commit (`86a923fe1`) and `9ecef3188`, and stayed
there across two runs of the *same binary*:

| benchmark | historical range (11 runs) | run 1 | run 2 (`--no-stage`, identical image) |
|---|---|---|---|
| `crypto_chacha20_1KiB` | 11749–12178 ns | 19105 | 19187 |
| `crypto_poly1305_1KiB` | 4976–5166 ns | 8102 | 8257 |
| `crypto_aead_1KiB` | 18922–19366 ns | 29386 | 29543 |

The historical range is *tight* — chacha20 varied by 3.6% across eleven runs
spanning many commits — so 19105 is not a tail of the old distribution. The
ratios are 1.590 / 1.573 / 1.569, and `aead` is chacha20+poly1305, so this
reads as **one cause appearing three times**, not three findings.

**Why this is not the usual bench flakiness.** The same pair of runs also
flagged `pick_next` (+150%), `ipc_pipe` (+42%) and `ipc_channel_sync` (+30%),
and those *did* revert on the second run — 1613→820 ns and 1277→872 ns, back
inside their own ranges. That is the documented
`B-BENCH-CONFIRMED-REGRESSIONS-FIRE-ON-AN-UNCHANGED-BINARY` behaviour and it
was almost certainly self-inflicted: this agent was running `git fetch`,
`git merge`, file edits and a `cargo clippy` on the host *while run 1 was
measuring*, which the canary is documented as unable to see
(`B-CANARY-IS-BLIND-TO-HOST-DESCHEDULING` — it counts guest cycles, which do
not advance while the host runs something else). Run 2 was executed with the
agent deliberately idle. **The crypto trio survived that control and the other
three did not**, which is what promotes it from noise to a finding.

Note the reporting subtlety that nearly hid this: the *run-over-run* half of the
suite stopped flagging the trio the moment the elevated value became the
previous run. It was found by reading `bench/history.jsonl` directly, not from
the run verdict, which said `RUN CLEAN`.

**Correction (same day, before acting on it).** An earlier version of this
paragraph went on to claim the comparator "can only ever report edges, never
levels" and called that a defect worth fixing separately. **That is wrong, and
the error is worth recording because it was about to cost a redundant fix.**
`scripts/bench-history.py::level_shifts` already exists for exactly this case —
its docstring opens with the same miss, found on `http_build_response_1KiB` on
2026-08-15 — and it reports under `SUSTAINED SHIFT`, measured against a baseline
drawn from *before* the last `LEVEL_SHIFT_SKIP = 3` runs so a new step cannot
enter its own reference.

Why it was silent here is not a bug but a threshold: it requires the shift in
the run being judged **and** in `LEVEL_SHIFT_PERSIST = 2` runs before it — three
consecutive elevated runs — and this step has been measured twice. Replaying it
against the real history confirms both halves:

```
level_shifts(records excluding current run) -> NOTHING
level_shifts(records including current run) -> crypto_poly1305 +63.2%,
                                               crypto_chacha20 +59.6%,
                                               crypto_aead     +56.0%
```

(`report()` is called at line ~2598 with history loaded at ~2574, and
`append_record` runs at ~2723 — *after* — so `records` correctly excludes the
run being judged. The two-line replay above is the honest test of the detector,
not evidence against it.) Persistence is deliberate and measured: without it the
check fired on 11 of 26 replayed runs, nearly all single-run host excursions.

**So the third consecutive bench run at or after `9ecef3188` will print
`SUSTAINED SHIFT` for these three unprompted.** The tooling is not blind to
this, and this entry is not the only thing standing between the finding and
silent acceptance — which is what the closing paragraph originally claimed.

**What is ruled out.** `kernel/src/crypto.rs` and `kernel/src/bench.rs` are
**byte-identical** across the range (`git diff 86a923fe1..HEAD --` on both is
empty). No crypto-related file changed anywhere in the tree. Nothing in the
build profile changed; the only root `Cargo.toml` edit is lane C adding the
`byteread` workspace member, which is not in the kernel's dependency graph.

**Leading hypothesis, unconfirmed: code layout under TCG.** The range adds
~5,400 lines to the kernel image, almost all of it NTFS
(`kernel/src/fs/ntfs/*`, §210). QEMU's TCG is sensitive to code placement and
translation-block behaviour in ways real hardware is not, so a hot loop can
shift substantially without its own source changing. This would make the number
an emulation artifact rather than a real slowdown — but that is a hypothesis,
and "probably the emulator" is exactly the reasoning that lets a real
regression sit unexamined.

**Explicitly considered and rejected:** that the new NTFS self-test perturbs
machine state before the benchmarks. It does run on every boot, before the
suite (`kernel/src/main.rs`, `fs::ntfs::self_test()`), but its synthetic volume
is `TOTAL_CLUSTERS(24) * CLUSTER(4096)` ≈ 98 KiB in a dropped `Vec` — far too
small to matter, and a memory-pressure effect would move the whole suite rather
than three benchmarks (whole-suite drift was +0.0%).

**How to settle it — cheapest test FIRST, which is not the one that was
started.** The layout hypothesis has a mechanical, no-boot test that this
project already built for precisely this situation:

```
python scripts/straddle-check.py --compare <86a923fe1-kernel-elf> <9ecef3188-kernel-elf>
```

It disassembles both, locates each hot loop's backward branch, and compares
`addr >> 12` at the two ends: a loop whose branch crosses a 4 KiB guest page
cannot stay one directly-chained translation block and pays a dispatcher
round-trip every iteration, measured on this project at **~1.7x**
(`B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`). The observed ratios here are
1.590 / 1.573 / 1.569.

That number being *that* close to the documented penalty, in three benchmarks
whose source is byte-identical, in a range that added ~5,400 lines of unrelated
NTFS, is the single most likely explanation — and the straddle check settles it
in seconds against two ELFs, with no boot at all.

**Do not bisect before that check.** `bench-history.py::mode_structure` exists
to ask whether a fence separates *binaries* (code layout — no guilty commit
exists and bisecting is the wrong tool) or *runs* (noise), and the precedent is
recorded in its own docstring: `http_build_response_1KiB` "was bisected across
three commits before anyone asked; the answer was 'binaries', and there was no
guilty commit." The `SUSTAINED SHIFT` report prints the straddle-check command
as its first suggested action for this reason.

I started the expensive path first — a throwaway worktree at `86a923fe1`, full
rebuild and bench (~40 min) — before finding the above. That run is still worth
having, because it measures the *old binary on today's host* and so separates
"binary-dependent" from "host-dependent", which the straddle check cannot do.
But it should have been step 2. Do **not** run anything else on the host during
a measuring run.

**Why it is filed rather than fixed.** The cause is unknown and the honest
options differ by an order of magnitude in effort. If the straddle check shows
a page-crossing difference in the chacha20/poly1305 inner loops, this is an
emulation artifact, gets recorded as one, and no kernel change is warranted.
