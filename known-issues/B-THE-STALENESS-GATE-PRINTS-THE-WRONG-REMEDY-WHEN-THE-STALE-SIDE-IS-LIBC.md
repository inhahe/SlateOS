## B-THE-STALENESS-GATE-PRINTS-THE-WRONG-REMEDY-WHEN-THE-STALE-SIDE-IS-LIBC — 2026-08-21 — lane B — OPEN

**In short:** We keep 70-odd compiled test programs in git, and a checker warns
when one has gone out of date. It correctly noticed that nine of them were out
of date — but it told us to rebuild **the wrong thing**. It said "rebuild the
test programs", when what had actually moved was the C library they are built
against. Following that instruction would have rebuilt all nine against a stale
library and recorded the result as *fresh*, which is precisely the accident this
checker exists to prevent, and precisely what happened for real on 2026-08-16.

**Found by:** lane A, 2026-08-21, while unblocking a boot test. Reported to lane
B as `requests/a-b-operator-answered-five-of-your-open-questions.md`. It is
direct evidence in **B-Q5** (`open-questions.md`) and lane A thinks it is a
stronger argument for that question's option C than the reproducibility result
the operator actually asked for.

**Where:** `scripts/ctest-fixtures.py`, the `check` subcommand. The stamp
(`<fixture>.stamp`, format v2) hashes `build.py`, `main.c`, `libc.a` and the
output ELF into **one** value. A mismatch therefore proves only that *something*
in that set moved; the script cannot tell *which*, and its message picks one
answer and states it as fact.

**What actually happened.** `toolchain/sysroot/lib/libc.a` (gitignored) was built
at 08:32. The last `posix/` commit was `4bd151de5` at 10:17; the stamp commit
`a1b26843b` at 10:20; the merge `85955aec7` at 11:17. The sysroot was simply two
`posix/` commits behind, so `libc.a` was the stale input and the nine ELFs were
correct. Rebuilding the sysroot produced an archive hashing to *exactly* the
value already recorded in the stamps, confirming the ELFs had never been wrong.

**Why the safety net does not close the hole.** What caught the misdirection here
was `scripts/create-ext4-rootfs.sh`'s independent **mtime** gate. But mtime is
documented as unusable in a fresh clone — `git clone` stamps every file with the
checkout time, destroying the ordering it depends on, which is the whole reason
the content stamps were introduced. So in CI, or on any freshly-cloned machine,
the mtime gate is silent and **only the wrong advice survives**.

**The fix, within the current design.** Record a committed identity for `libc.a`
itself and split the diagnosis on it:

| `libc.a` vs committed identity | ELF vs stamp | Remedy to print |
|---|---|---|
| differs | — | **rebuild the sysroot** (`toolchain/build-sysroot.ps1`) |
| matches | differs | **rebuild the fixture** (`services/<name>/build.py`) |

That is a strictly smaller change than B-Q5's option C and is worth doing even
if C is chosen later, since C needs the same identity to decide what to rebuild.

**A related, smaller nuisance in the same cascade.** Rebuilding `libc.a` to
*byte-identical* content still moves its mtime, after which
`create-ext4-rootfs.sh` emits nine `WARNING: ctest-*.elf is OLDER than the
sysroot libc.a` lines that are pure noise — every content stamp matches. Observed
directly this session. A gate that cries wolf on a verified no-op trains its
readers to skip it.

**If never fixed:** the checker keeps catching real staleness — it is not
broken, it is *ambiguous* — but roughly half the time it will name the wrong
remedy, and the remedy it names is the one that manufactures a false green. The
2026-08-16 incident is reachable by following the tool's own instructions.
