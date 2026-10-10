## 979. A gate whose every input is unchanged replays its last pass, and the cache must never be the reason a gate passes or fails

**Date:** 2026-09-27 · **Decided by:** Claude (operator-approved scope: C-Q11's
answer, "do what you want", and lane A's reply taking idea 2) · **Lane:** A

**In short:** the boot test spends most of its three-plus hours re-running
checks on files nobody changed. §974 measured it: 133.9 gate-hours across 395
boots, 55% of it in gates that never once refused. The gate cache watches
what each check reads the first time it passes. On a later boot it replays
that pass instantly, but only if every one of those things is exactly as it
was, and it says out loud that it did. Anything it cannot see, it does not
cache. A random tenth of its would-be replays run for real anyway, as a
continuous check on the cache itself.

**What is built** (`scripts/gate-cache.py`, `gatecache_trace.py`,
`gatecache_tee.py`, `gatecache_site/sitecustomize.py`; 45 tests in
`test-gate-cache.py`):
- `run_checker` routes a Python gate through the driver when the boot test
  turns the cache on. So does the tooling-suite sweep.
- The traced run is watched by Python audit hooks plus patched `stat`-like
  functions, in every Python process it starts.
- What it records:
  - files read, hashed when opened, with git-style size/mtime/id stamps so a
    lookup need not re-read them;
  - directories listed;
  - paths asked about;
  - environment variables read by name;
  - read-only git answers, taken through a tee that sees the exact bytes;
  - git's version and configuration, for git confined to scratch
    repositories.
- A hit replays the stored output byte for byte and ends it with a
  `gate-cache: HIT` line.

**The rules, each one tested:**

| rule | why |
|---|---|
| only passes are stored | a failing gate always runs again |
| any input it cannot see makes the run uncacheable | covers other executables, shell lines, sockets, native code, writes outside run-created directories, directories left behind, untraced children, and walks over the whole environment. A miss costs a normal run; a wrong hit is a gate that did not run |
| every input is re-checked at every lookup, and the miss names the first that moved | a replay rests on evidence taken today, not on a decision made once |
| entries last one UTC day | bounds any verdict that depends on the date, and re-checks every gate daily |
| a random 10% of would-be hits run fresh and are compared | if they disagree, `DISABLED` is written to the shared store and the cache is off for every lane until someone reads it |
| a traced run that fails is re-run untraced, and the untraced verdict stands | the tracer must never be why a gate fails; a disagreement is logged separately, since it means a tracer fault or a flaky gate |
| off for release and bench boots, and with `--no-gate-cache` | the full, uncached check always exists |

**Known limits.** Accepted with eyes open, and each is also where to look first
if a verification ever fails:
- **mtime restored.** A file edited to the same size with its old mtime put
  back (`touch -r`) passes the stamp check unread. git's index makes the same
  bet, and it guards "racy" files the same way: anything modified within 2 s
  of the recording is always re-hashed.
- **Symlink resolution.** `os.path.realpath`'s resolution is not recorded, and
  neither is a listing made through a directory file descriptor (`os.fwalk`).
  No gate here uses either.
- **Time of day.** Logic that depends on the time within a day, rather than
  the date, is bounded only by the verification sample.
- **Store keyed by worktree path.** Lanes do not share entries, which is safe
  but saves less than sharing would.

**Measured on this machine before any boot used it:**
- `check-selftest-skips`: 49.9 s to 0.5 s.
- `check-eol`: 14 s to 0.7 s, after the stamp check. It reads every tracked
  file, so without stamps a hit cost what the gate did.
- `check-text-mode-writes`: 6.5 s to 0.6 s.

The first boot with the cache populates it. Its first real measure is the boot
after that, whose end-of-gates summary counts hits, misses, skips,
verifications and traced-run disagreements.

**Rejected:**
- **A hand-kept map from paths to gates.** It goes stale the day a checker
  learns to read one more directory; a trace follows the code.
- **"Check once a day."** The operator's first idea in C-Q11. It trades
  catching a fault for time, where the cache trades only time.
- **Caching nothing that runs git.** Most checkers list their files through
  `git ls-files`, so almost nothing would be cached.
