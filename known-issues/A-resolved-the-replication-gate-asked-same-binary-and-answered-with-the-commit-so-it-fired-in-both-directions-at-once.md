### [A] RESOLVED — the replication gate asked "same binary?" and answered with the commit, so it fired in both directions at once — 2026-08-19

**Symptom.** A `--bench` run flagged `page_alloc_zeroed_free` at 6532ns, `REGRESSED,
UNREPLICATED`, and printed the advice the gate exists to give: re-run *without
rebuilding* to confirm. That was done — `--no-build --no-stage`, so QEMU booted the
identical bytes — and the re-run measured 3493ns, inside the benchmark's own
3379–3854ns range. It should have been read as a contradiction of the flag and as an
A/A pair besides. Instead the tool treated it as a fresh commit's first measurement
and manufactured four *new* claims out of an image that had not changed by one byte:

| benchmark | flagged run | re-run | reported |
|---|---|---|---|
| `sched_pick_next_d1` | 40ns | 65ns | +62% REGRESSED, UNREPLICATED |
| `vfs_stat_breakdown_ns` | 56ns | 81ns | +45% REGRESSED, UNREPLICATED |
| `http_build_response_gzip_1KiB` | 193176ns | 251656ns | +30% REGRESSED, UNREPLICATED |
| `http_gzip_1KiB` | 192035ns | 245506ns | +28% REGRESSED, UNREPLICATED |

The A/A banner — the one check whose entire job is to say "these two runs are the same
image, nothing below can be code" — stayed silent throughout.

**Cause.** `values_for_commit()`, `replication_verdict()` and the A/A banner all asked
"is this the same *binary*?" — their docstrings said exactly that — and all three
answered by comparing `record["commit"]`. A commit label is neither necessary nor
sufficient for that question, and this run hit both failures at once:

- **Same binary, different commit.** The re-run the tool *itself prescribes* is made
  without rebuilding, so if anything was committed in between — and something was, the
  `MEASURED-AS` fix — the confirming record is filed under a new commit. The gate then
  cannot find the very evidence it asked for. This is not an unlucky case; it is the
  *normal* case for the workflow the tool prescribes.
- **Same commit, different binary.** The flagged run carried `dirty: true`, so its
  `commit` field named a tree that was never built. Two such runs share a label while
  measuring different code, and the gate would have counted each as evidence about the
  other.

**Fix.** Identity now comes from the kernel image itself. `bench-history.py` hashes the
ELF it was handed (`--kernel-elf`, which `boot-test.sh` already passes) and stores
`kernel_sha`; `binary_identity()` prefers that hash, falls back to a *clean* commit,
and otherwise reports "unidentifiable" rather than a sentinel that would make every
unidentifiable run look alike.

The comparison itself is `same_image(a, b)`, a **relation** rather than an equality
between identity strings — because a hashed run and an older unhashed-but-clean run of
the same commit are the same image, yet no two labels drawn from those records compare
equal, and the entire 79-record pre-hash history is on the unhashed side. Both hashed
⇒ the bytes decide; at most one hashed ⇒ fall back to a clean commit; unidentifiable
matches nothing, including another unidentifiable record. The asymmetry is deliberate:
two hashes settle the question between themselves and a matching commit cannot
overrule them (see design-decisions.md §232).

The banner now distinguishes three states instead of two: **A/A** (same image),
**SAME COMMIT / DIFFERENT IMAGE** (both hashed, hashes differ — known-different code
under one label), and **SAME COMMIT / UNKNOWN IMAGE** (at least one side unpinnable —
named rather than left for the reader to fill in from the matching commit in the
header line).

**Verified.** 379 assertions in `scripts/test-bench-history.py` green, including an
end-to-end reconstruction of the failure above (two records of one image under two
commits must read as A/A and must not fail the build) and each of the three banner
states. Live against the real history: a hashed re-read of the 2026-08-19 serial log
against the unhashed clean baseline of the same commit now prints A/A, and the same
run with `--dirty` correctly degrades to UNKNOWN IMAGE.

**What this does not fix.** Every record written before this change is unhashed, so
pairs among them still fall back to the clean-commit rule and a dirty pre-hash record
still identifies nothing. That is not recoverable — the bytes are gone — and it is why
the fallback exists rather than being retired.
