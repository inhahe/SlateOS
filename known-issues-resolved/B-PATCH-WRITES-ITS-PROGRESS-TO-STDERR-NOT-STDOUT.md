## B-PATCH-WRITES-ITS-PROGRESS-TO-STDERR-NOT-STDOUT — **FIXED 2026-09-12 in both halves** (lane B, 2026-09-11)

**Measured against GNU patch 2.7.6 both ways round**, rather than assumed:

    patch f < u.patch 2>/dev/null      ->  patching file f.txt
    patch f < u.patch 2>&1 >/dev/null  ->  (nothing)

and the same for `--dry-run`'s `checking file …`. Both of ours used stderr —
`diag!` in the coreutils bin, `eprintln!` in the standalone.

**What it was worth, which is more than it looks:**

| half | before | after |
|---|---|---|
| `coreutils` `patch` | 3 passed, 62 differed | **14 passed, 51 differed** |
| `userspace/patch` | 3 passed, 62 differed | **17 passed, 48 differed** |

**RETIRED 2026-09-12 (lane B) -- `userspace/patch` is deleted.**
design-decisions.md 1005: coreutils is the one home, the better half of each
duplicate pair survives inside it, and the duplicate crate is deleted. Measured
the same day on the same 67 cases against the same GNU 2.7.6 reference, so the
comparison is like-for-like rather than two runs of different vintages:

| half | 2026-09-12 |
|---|---|
| `coreutils` `patch` | **64 passed, 0 differed**, 3 differ on purpose |
| `userspace/patch` | **17 passed, 47 differed**, 3 differ on purpose |

Checked before deleting rather than after, because a pair's row names one
program and the crate behind it may be six (`stat-diff.sh`'s multicall hazard):
no Cargo.toml anywhere depends on it, `multicall-aliases.py` finds it dispatches
on no other name, and `create-ext4-rootfs.sh` stages neither half -- /bin holds
hand-compiled C fixtures, not these crates. It WAS a workspace member: the
`userspace/*` glob covers it, which a grep for the literal path does not show,
and concluding orphan from that grep was a mistake caught before acting on it.

**The one thing the loser had that the winner lacked was carried across first**:
`--input` / `--input=FILE`, the long spelling of `-i`. Deleting the worse half
means the better half ends up with everything, so a survey column reading "1
only in the standalone" is a list of one to go and check, not a rounding error.

Stale baseline entries removed through each gate's own `--update-baseline` /
`--write-baseline` rather than by hand: `argv-utf8-baseline.txt` and
`workspace-lints-baseline.txt` each lost exactly one line. A deleted crate whose
baseline lines survive is a declaration that has stopped being true.

Almost every case in `patch-diff.sh` produces one of these lines, so **the
stream alone was deciding the verdict and nothing about the patching was being
compared at all.** Eleven and fourteen cases respectively were hidden behind it.

**The identical 3/62 was the tell, and I nearly misread it.** Two separate
implementations — 892 lines against 2183, no shared module — scoring byte-for-
byte the same is not a coincidence, it is a sign that something upstream of both
is deciding the answer. My first hypothesis was that `DIFF_PKG=patch` had not
switched halves at all, which the preamble's own probe disproved:
`DIFF_PKG=no-such-package-at-all` fails loudly, so the knob was working.

**The pair is still NOT decided, and 17-to-14 is not a decision.** Three cases
apart out of sixty-five, with both halves failing about three quarters of them.
`dup-bins-survey.py`'s own warning is against exactly this — five pairs ranked
on thin evidence were later measured and the ranking was wrong every time. What
the fix bought is a comparison that measures `patch` instead of measuring a file
descriptor; what it did not buy is a winner. **Neither half is close to GNU**,
and whichever survives needs real work rather than a retirement decision.

**Both halves of the `patch` pair write `patching file X` to stderr. GNU writes
it to stdout.** Verified directly rather than inferred from a harness column:

    $ /usr/bin/patch -p1 -i p.patch </dev/null 2>/dev/null
    patching file a/base.txt          <- stdout
    $ our patch    -p1 -i p.patch </dev/null 2>/dev/null
                                      <- nothing
    $ our patch    -p1 -i p.patch </dev/null 2>&1 >/dev/null
    patching file a/base.txt          <- stderr

Both exit 0 and both apply the patch correctly, so this is not a failure — it is
the same information on the wrong channel. It matters twice: anything capturing
`patch`'s stdout (a build log, a CI transcript) records nothing from ours, and
anything treating stderr as the error channel sees noise on every successful
run. It is also the single largest source of difference in
`scripts/patch-diff.sh`, touching nearly every case, which is how it was found.
