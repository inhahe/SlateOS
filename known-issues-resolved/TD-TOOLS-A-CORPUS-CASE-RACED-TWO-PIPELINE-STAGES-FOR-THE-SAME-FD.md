### TD-TOOLS-A-CORPUS-CASE-RACED-TWO-PIPELINE-STAGES-FOR-THE-SAME-FD. `xtrace-pipeline` failed once in a full sweep and passed 5/5 alone — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/tests/corpus/xtrace-pipeline.sh`, the "function and
compound stages trace their bodies" section.

**What:** `( set -x; for i in 1 2; do true; done | true )` has both stages
tracing to the same merged fd. The loop stage writes `+ for i in 1 2` and
`+ true` twice; the other writes `+ true` once, and *where* that single line
lands is a race. On a loaded host it landed third in osh and last in bash, and
the case failed:

```text
bash: + for i in 1 2 / + true / + for i in 1 2 / + true / + true
osh : + for i in 1 2 / + true / + true       / + for i in 1 2 / + true
```

The case's own comment two lines above already stated the rule that makes the
neighbouring four lines safe — "they are spelled the same to keep the bytes
deterministic" — and this line broke it, because `+ for i in 1 2` and `+ true`
are not the same spelling.

**Fixed** by turning xtrace on *inside* the stage
(`f() { set -x; for i in 1 2; do true; done; }; f | true`) rather than around
the pipeline. A pipeline stage is its own process, so the other stage never
traces at all and there is only one writer: the determinism is now structural
rather than probabilistic. It also pins something worth pinning — that `set -x`
in one stage does not leak into another, which is not obvious for osh, whose
builtin stages run on a threaded executor.

**Standing lesson:** a test whose expected output depends on the interleaving
of two concurrent writers is not a flaky test, it is a wrong one. "Passes 5/5
when re-run alone" is the signature, and the fix is to remove the second
writer, not to widen the tolerance or retry.
