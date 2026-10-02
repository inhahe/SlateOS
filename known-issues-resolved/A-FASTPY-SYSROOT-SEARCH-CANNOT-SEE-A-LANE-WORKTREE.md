### A-FASTPY-SYSROOT-SEARCH-CANNOT-SEE-A-LANE-WORKTREE. The Path-Z attribution warning fires on every lane boot, always, because the search never looks at the tree being tested — 2026-09-01 — **Status: FIXED 2026-09-10**

**Resolution (lane A, 2026-09-10).** The warning no longer fires, and the reason is
not that the search was repaired -- it is that the hazard it describes was closed by
lane B and this gate had not been told.

`scripts/ctest-fixtures.py` computes the worktree's sysroot in
`_slateos_sysroot_env` and assigns it into the child environment beside that call,
so every fixture build receives `FASTPY_SLATEOS_SYSROOT` and fastpy's
sibling-checkout fallback is never reached. `check_sysroot_identity` now asks whether
that assignment is present before declaring that it is not; when it is absent the
old warning returns verbatim, so the check fails closed.

Repairing the path mirror -- the obvious fix, and the one the request suggested --
was rejected. That function's header says "Mirror `_find_slateos_sysroot_lib`'s
candidate order exactly. If this ever disagrees with fastpy the gate becomes worse
than nothing," and it already disagreed: boot-test.sh computed
`<worktree>/../fastpy/../os/...`, which on `E:` does not exist, while fastpy computes
`<fastpy>/../os/...` from `D:` where it still lives -- and that target is real,
11,857,582 bytes dated 2026-09-03 against this tree's 11,770,764 from 2026-09-10.
Two implementations of one search, drifted. Repairing the mirror would have restored
the second implementation; asserting the invariant that makes the fallback
unreachable needs no mirror.

Still wanted, and filed rather than done: a behavioural query. `_slateos_sysroot_env`
is private and `ctest-fixtures.py` exposes no subcommand for it, so the check is
textual -- it matches the assignment, not the behaviour. A `print-sysroot` subcommand
would let the gate compare bytes again.

**In short:** every boot test in a lane worktree prints `WARNING: fastpy would
not resolve any SlateOS sysroot`, which reads as "the C fixtures in this image
were linked against a libc nobody can identify, so their results cannot be
attributed to any `posix/` revision." The alarming part is true. The reason is
not what the message implies: the lane's `libc.a` exists and is perfectly
identifiable — the search simply never looks in the lane's own tree. It looks
only in a sibling checkout literally named `os`, which in the three-worktree
layout is the *integration* tree, and which has no sysroot at all.

**Lane:** A (`scripts/boot-test.sh`, and fastpy's own
`_find_slateos_sysroot_lib`, which the gate deliberately mirrors).
**Found:** 2026-09-01, in the `bdl8f0gxl` boot run.

**Evidence, on this machine, right now:**

| path | `libc.a` |
|---|---|
| `os-lane-a/toolchain/sysroot/lib/` (the tree under test) | **present**, 12,645,582 bytes, 2026-08-31 |
| `os/toolchain/sysroot/lib/` (the only place searched) | **absent** |

The candidate list in `boot-test.sh` (~:646–658) is, in order:
`$FASTPY_SLATEOS_SYSROOT/lib`, `$FASTPY_SLATEOS_SYSROOT`, then
`${FASTPY_DIR:-$PROJECT_ROOT/../fastpy}/../os/toolchain/sysroot/lib`. Both
environment variables are unset in the lane shells, so only the third is ever
tried, and that third hard-codes the directory name `os`. `$PROJECT_ROOT` — the
tree whose kernel is being booted and whose `libc.a` is sitting right there — is
not a candidate at any point.

**Why this is worse than a stale-artifact warning.** It is unconditional. It
fired on this run, it fires on every lane-A run, and by construction it fires on
lane B's and lane C's too, since none of the three worktrees is named `os`. A
warning that cannot ever *not* fire carries no information, and the cost is
precisely the failure mode `A-GATES-SILENTLY-STOPPED-CHECKING` describes from
the other direction: readers learn to scroll past it, so on the day the sysroot
genuinely is unresolvable — or genuinely is the wrong one — the message that
says so will look exactly like the noise it has been printing for weeks.

Note the irony that makes this worth writing down rather than silencing: the
gate's own comment says it mirrors fastpy's search "so it is written to be read
side-by-side with that function", and warns that if the two ever disagree "the
gate becomes worse than nothing -- it would report on a file fastpy does not
use." The mirroring is faithful. The defect is in the thing being mirrored, and
faithfully copying it is what propagated the defect into the harness.

**What a proper fix looks like.** Two candidates, and they are not exclusive:

1. ~~**Harness-side, small and local to lane A:** `boot-test.sh` exports
   `FASTPY_SLATEOS_SYSROOT="$PROJECT_ROOT/toolchain/sysroot"` before the check
   when that directory holds a `libc.a`. This makes the first candidate hit,
   makes the warning informative again, and is correct on its face — the tree
   under test is the tree whose libc the fixtures should be attributed to.~~
   **← WRONG. Do not do this. Struck 2026-09-02, lane A, before implementing
   it.** It does not make the warning informative; it deletes the gate.

   `check_sysroot_identity` ends in `cmp -s "$resolved/libc.a" "$ours"`. Export
   that variable and `$resolved` *becomes* `$ours`, so the comparison reads our
   own `libc.a` against itself, always matches, and returns silently — forever,
   on every lane, whatever the fixtures were actually linked against.

   The reasoning error was assuming `boot-test.sh` links the fixtures. It does
   not: it runs `ctest-fixtures.py image-check` and `sysroot-check` only, never
   `build`, and its own comment at :460 says the rootfs rebuild is something
   "the boot test does not run". The fixtures in the image were linked by an
   earlier, separate invocation, and no variable set inside `boot-test.sh` can
   retroactively change what they linked against. Setting it at check time
   changes only the *report*, not the fact reported on.

   So the proposal would have converted a warning that is annoying-but-true
   into a gate that passes without checking — `A-GATES-SILENTLY-STOPPED-CHECKING`
   exactly, and self-inflicted by an entry that cites that failure mode two
   paragraphs earlier. The environment variable is the right lever; **the place
   to pull it is the fixture *build*, not the boot test.**
2. **Root-cause, and cross-project:** fastpy's `_find_slateos_sysroot_lib`
   should walk up from the *current* project root before falling back to a
   sibling named `os`. Hard-coding a checkout's directory name is what breaks
   the moment anyone uses `git worktree`, which this project now does by policy
   for all three lanes.

(2) is what actually fixes this, and it **is not lane A's tree** — fastpy lives
at `D:\visual studio projects\fastpy` — so it needs either a request or an
operator decision about who owns it. Anything invoking fastpy against a lane
tree has the same blind spot, with no harness to compensate.

**What lane A did instead, 2026-09-02 (partial — the entry stays OPEN).** The
warning was *misdiagnosing* its own cause, which is separable from the cause
and was fixed. It read:

> Neither `$FASTPY_SLATEOS_SYSROOT` nor the sibling 'os' checkout holds a
> `libc.a`. The fixtures in the image were linked against **something this host
> can no longer name**…

That describes a missing or unidentifiable libc. Ours is present, 12.6 MB, at a
path the function already holds in `$ours` — the early return above the warning
*proves* it exists, since the check bails out when it does not. The failure is
a search that never looks there. A reader told "this host can no longer name
it" goes and rebuilds a sysroot that was never missing.

The message now names the real cause, prints `$ours` as "present, never
searched", gives a repair that sets the variable **at fixture-build time**
(quoted — every checkout of this project is under a path containing a space, so
an unquoted assignment is a repair line that fails when pasted), and states in
the output itself why the script does not set the variable around the check.
That last line exists so the struck proposal above is not re-invented by
someone reading only the script: the refutation lives where the temptation is.

This does not stop the warning firing — it *should* fire, because attribution
genuinely is impossible until (2) lands. It stops it being wrong about why.
`bash -n` and `shellcheck --severity=warning` clean; the branch was exercised
by extracting the real function and running it with both variables unset.

**Not to be confused with** the `[ctest] ERROR: ... libc.a was built from 7
input(s) that have since changed` block printed just above it on the same run.
That one is a genuine staleness report about lane B's `posix/` sources, it is
accurate, and it is separately actionable by rebuilding the sysroot. This entry
is only about the *attribution* warning that follows it.
