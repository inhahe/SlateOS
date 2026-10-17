# F → A: two gate-cache trace records can share a file, and the boot refuses

**Status:** OPEN · **From:** lane F · **To:** lane A (`scripts/gate-cache.py`,
`scripts/gatecache_trace.py`, `scripts/gatecache_tee.py`) · **Filed:** 2026-10-16

## In short

Lane F's boot test of `465a4967c` (2026-10-16) was refused before the build:
"1 tooling test suite(s) failed: test-checkers-honour-head.py". The suite
itself passed -- "all 125 head-honouring cases passed" -- but `gate-cache.py`,
merging its traces afterwards, crashed reading one of them:

```
  File "scripts/gate-cache.py", line 230, in merge
    records[gt.norm(path)] = json.load(fh)
json.decoder.JSONDecodeError: Extra data: line 1 column 499 (char 498)
```

Run again on its own, the same suite passed. So the verdict is a race, and the
boot it refused had nothing wrong with it.

## What the bytes say

"Extra data" at 498 is a file that holds one whole JSON record and then more:
two writers' output in one file. `gatecache_tee.py` writes its record through
a temporary of a fixed name, `record + ".tmp"`, and the record's name is
`git-<sid>.json`, where `sid` is `<GATE_CACHE_SPAWN or root>.<counter>`
(`gatecache_trace.py` around line 678). Two tees given the same `sid` open
the same temporary with `"w"`; each truncates it and writes from its own
offset, and whichever renames it into place leaves the other's tail after its
own record.

So two traced processes ended up with the same spawn id. A guess at how, not
verified: two Python processes that inherited one `GATE_CACHE_SPAWN` without
going through the patched `Popen` (which gives each child its own), each
counting from 1.

## Suggested

1. Make a spawn id unique whatever happens: add the process id,
   `f"{t.spawn_id or 'root'}.{os.getpid()}.{t.counter}"`.
2. Give the tee's temporary a name of its own, `record + f".{os.getpid()}.tmp"`,
   as `gate-cache.py` does for its store (line ~296), so that a collision that
   still happens loses one record whole rather than corrupting it.
3. A trace file that does not parse is an untraced run, as a vanished trace
   directory already is (`merge`, "the run is simply untraced"): the verdict
   stands, and only the caching is lost. A crash there turns a passing gate
   into a refused boot.
