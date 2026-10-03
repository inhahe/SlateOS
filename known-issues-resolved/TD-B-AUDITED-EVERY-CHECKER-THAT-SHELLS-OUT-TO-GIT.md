## TD-B-AUDITED-EVERY-CHECKER-THAT-SHELLS-OUT-TO-GIT (lane B, 2026-09-12) — done, one offender

**Why.** Lane A found that `cwd=`/`git -C` do not anchor the repository —
`GIT_DIR` wins, and git sets it for every hook it runs — and suggested the grep.
It came back positive on `check-stale-blockers.py`, fixed at `364efd92a`.

**The rest of the audit, so nobody repeats it.** 35 scripts under `scripts/`
shell out to git; 18 use `gitenv`, 16 do not. Four of the sixteen that are
wired as gates were run against a throwaway repository with `GIT_DIR` pointed at
it:

| gate | normal | hostile `GIT_DIR` |
|---|---|---|
| `check-eol.py` | 1 | **2 — declines** |
| `check-text-mode-writes.py` | 0 | **2 — declines** |
| `check-release-staleness.py` | 0 | **2 — declines** |
| `merge-readiness.py` | 0 | **2 — declines** |

All four fail CLOSED: they notice the repository is not the one they expected
and refuse a verdict rather than reporting a clean empty scan. So the tree's
convention was already sound and `check-stale-blockers.py` was the exception,
not the rule — which is the opposite of what the grep count suggested, and the
reason a count of `gitenv` mentions is not a count of vulnerable scripts.

**A thing checked and cleared while here.** `check-eol.py` reports 246 files
containing carriage returns, several under `posix/`, which I had edited that
night through Python. Checked rather than assumed: every file I touched is
LF-only *and was already LF-only before my commit* — `posix/src/time.rs` had
zero CRs at `aa3cb1e03~1` and my diff there is eleven lines. So no line-ending
rewrite happened. The 246 are pre-existing and the gate marks them
"reported, not fatal".
