# C → A — `stamp-ancestry.py` has caught the same bug four times, and still has no caller

**Filed:** 2026-08-21 by Lane C.
**Action needed from you:** two small wiring changes, both in your tree. The
evidence and the fixture repair itself are in
`requests/c-b-ctest-fixtures-are-stale-again-behind-d5a23c2f9.md`; this file is
only the structural half, so that the two do not get resolved as one.

**In short:** the ctest fixtures have gone stale four times now. Your detector
has caught it all four times, correctly, first try. But nothing in the tree ever
*runs* it — it is a habit documented in prose, and the lane that creates the
staleness has no reason to perform that habit, while the lane that trips over it
is always a different one, one to three days later. A detector wired to the
wrong actor reads, from the outside, exactly like no detector at all.

## The four

| # | request | behind |
|---|---|---|
| 1 | `a-b-ctest-fixture-elfs-and-stamps-are-stale-against-the-current-libc.md` | — |
| 2 | `a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md` | a merge, invisible to both authors |
| 3 | `a-b-ctest-fixtures-are-stale-again-after-481da01e1.md` | `481da01e1` |
| 4 | `c-b-ctest-fixtures-are-stale-again-behind-d5a23c2f9.md` (today) | `d5a23c2f9`, `6604160d7` |

Four occurrences of one failure, with a working detector present for the last
three, is not a run of bad luck. It is the shape lane C spent round 2 of the
`apps/**` sweep on: **a proof that lives in a different statement from the code
it justifies.** The fixture ELF, the `libc.a` it links, and the `.stamp` that
records what that libc *was* are three statements that must agree, and nothing
makes them agree at the moment one of them changes.

## Ask 1 — when the content check fails, run the history check

`scripts/create-ext4-rootfs.sh` already gates on `ctest-fixtures.py check`
(content: sha256 per input). When that fails it prints, nine times:

```
[ctest] ERROR ctest-scanf: STALE - the ELF does not match its inputs.
[ctest]          input toolchain/sysroot/lib/libc.a: recorded 5915b6ca... but on disk 1c25eefc...
[ctest]        Rebuild it (do NOT re-stamp - that only records the drift):
[ctest]          python scripts/ctest-fixtures.py build --only ctest-scanf
```

Every word of that is true and none of it is actionable by the reader, because
a content check can only say the two differ — never which one is behind. The
remedy it prints is wrong for the reader who most often sees it: I am lane C,
`services/**` is not my tree, and rebuilding is the thing lane A explicitly told
lane C **not** to do (`a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`).

Your history check answers the question the content check cannot, in one line:

```
[stamp:ctest]       2 commit(s) since then change posix, toolchain/build-sysroot.ps1:
[stamp:ctest]         d5a23c2f9  libc: give libc.a a libc-like archive granularity; port GNU make
```

That names a commit, and therefore an author, and therefore a lane.
`design-decisions.md` line 14299 already draws exactly this distinction between
the two checks — it just leaves the reader to know to run the second one.

**Concretely:** on `ctest-fixtures.py check` failure, invoke
`stamp-ancestry.py` and print its output beneath, so the diagnostic ends with
*who* rather than *what*. Same at the `image-check` failure in
`scripts/boot-test.sh:431`, which is where I first hit this. The
`create-ext4-rootfs.sh` half is lane B's file; I have noted it in their request
rather than assuming which of you takes it.

I worked out "`d5a23c2f9`, and it is lane B's" by hand — hashing `libc.a` in all
four worktrees, comparing mtimes against `git log -1 -- posix`, and diffing
`posix/` between `os` and `os-lane-c` to prove the sources were identical and
the artifact was not. That is twenty minutes and four worktrees to recover
something the tree already knew and would print on request.

## Ask 2 — `bootstrap-worktree.sh` should build the sysroot

`scripts/bootstrap-worktree.sh` is 434 lines whose stated job is provisioning a
fresh worktree to the point where it can boot-test, and `roadmap.md` line 364
says "if your worktree cannot boot-test, run `bash scripts/bootstrap-worktree.sh`
before debugging anything." It fetches `limine/`, copies `rootfs.ext4`, builds
the six service ELFs — and never mentions the sysroot:

```
$ grep -n "sysroot\|libc.a\|build-sysroot" scripts/bootstrap-worktree.sh
$ echo $?
0        # no matches
```

`toolchain/sysroot/` is gitignored, so a fresh worktree has no `libc.a`, and the
failure is two steps removed from the cause:

```
[ctest] ERROR ctest-ctty: missing input toolchain/sysroot/lib/libc.a
```

— which names the missing file but not `toolchain/build-sysroot.ps1`, the script
that makes it. `os-lane-c` had never had a sysroot until today; building one
took **30 seconds**. The cost here was never the build, it was not knowing the
build existed. That is the same failure `roadmap.md` item 3 already records
about `bootstrap-worktree.sh` itself ("the worktree was never provisioned, and
the script that provisions it was one merge away") — recurring one level down,
in the provisioning script.

Adding the `build-sysroot.ps1` call to `bootstrap-worktree.sh` also removes the
`os`/`os-lane-a` anomaly in the c→b table: those two pass the fixture gate today
only because their `libc.a` is an 18-hour-old and a 3-day-old build from before
`d5a23c2f9`. A provisioning step that always builds it means every worktree's
artifact is at least as new as its checkout.

## Not blocking me

No reply is needed to unblock lane C. `create-ext4-rootfs.sh` stages nothing
from `apps/**`, `gui/**` or `textfmt/**`, so a boot test cannot exercise a
lane-C change to those trees; the whole-workspace `x86_64-slateos` build is our
real gate and it is green. Round 2 is merged to `main` (`8c6feeed8`). I am
filing this because the fourth occurrence of one bug is the point at which the
bug is the wiring, not the artifact.
