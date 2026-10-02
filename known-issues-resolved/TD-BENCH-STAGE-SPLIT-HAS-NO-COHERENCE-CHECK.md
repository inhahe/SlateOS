### TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK — 2026-08-14 (`kernel/src/bench.rs`)

Two byte-identical benchmarks, in the same boot, disagreed by 1.67x:

```
SCORE vfs_stat_root 2971 ...
[bench] vfs_stat_breakdown_full: min=25808 cycles (4976ns) ...
```

Both are `run(..., 500, || black_box(Vfs::stat("/")))`. Nothing distinguishes
them but *when in the boot they ran*. In the next boot the same pair came out
4394 and 4306 — coherent. So the harness's min-of-500 is sometimes accurate and
sometimes 1.7x off, and **nothing in the output says which kind of run you are
reading.**

The consequences are not hypothetical; they are the two runs above:

* Run A attributed `stat_resolved` 2531 → 4109 ns, a 62% "regression" caused by
  a change that cannot touch it.
* Run B printed `full 4306ns = resolve_follow ~0ns + stat_resolved 5762ns` — the
  subtraction saturated at zero because a *part* measured larger than the
  *whole*. That is arithmetically impossible and it was printed without comment.
* Run A's parts summed to 133% of its whole. Also printed without comment.

This is the project's recurring defect class in its purest form: the check was
*there* — the code deliberately measures `resolve_follow` both directly and by
subtraction, with a comment explaining that a disagreement would indict the
subtraction — and then prints both numbers side by side and says nothing when
they disagree by 2.9x. **A check whose failure is not distinguishable from its
success is not a check, it is a decoration.**

> **Resolution (same change).** Two gates, both of which say the word WARNING:
>
> * **Drift gate.** The first measurement (`vfs_stat_breakdown_full`) is repeated
>   verbatim at the *end* of the block as `..._full2`. The two are the same code
>   over the same input, so any difference is pure measurement drift across the
>   width of the block, and it bounds how much of every stage difference is real.
>   Over 25% and the run is declared not internally coherent and unusable for
>   attribution.
> * **Parts/whole gate.** `resolve_direct + stat_resolved` must land within
>   75–125% of `full`, or the stage attribution is declared "not arithmetic, it
>   is noise".
>
> The same discipline is applied to the new lock benchmark, which prints
> `unexplained` as an explicit residual and warns when the components exceed the
> total they were subtracted from.

**Not fixed:** the harness still reports a single `min` with no confidence
interval, so a *single* benchmark with no in-block replicate (i.e. all the
others) remains ungraded for coherence. The proper fix is for `run()` itself to
take two interleaved sample sets and report their disagreement, making every
benchmark self-checking rather than just this one. Tracked here; not blocking.
